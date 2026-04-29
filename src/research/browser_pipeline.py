"""
Automated browser research pipeline.
Google search -> find annual report PDF -> download -> extract financial data.

Handles non-US companies (Euronext, LSE, BSE/NSE, etc.) via real Chrome browser.
US companies prefer SEC EDGAR API (faster), browser as fallback.
"""

import asyncio
import logging
import os
import re
import tempfile
from dataclasses import dataclass, field
from typing import Optional

import fitz  # PyMuPDF
import requests

from src.browser.session import BrowserSession
from src.browser.navigation import BrowserNav
from src.browser.extraction import BrowserExtract

logger = logging.getLogger(__name__)


@dataclass
class FilingDocument:
    company: str
    year: str
    pdf_url: str
    pdf_path: str
    total_pages: int = 0
    total_chars: int = 0
    source: str = ""  # "ir_website", "google_search", "direct"


@dataclass
class ExtractedFinancials:
    """Financial data extracted from annual report text."""
    company: str = ""
    year: str = ""
    revenue: Optional[float] = None
    operating_income: Optional[float] = None  # EBIT / Operating Result
    net_income: Optional[float] = None
    total_assets: Optional[float] = None
    total_equity: Optional[float] = None
    total_debt: Optional[float] = None
    cash: Optional[float] = None

    # EBITDA hierarchy (preference order)
    adjusted_ebitda: Optional[float] = None   # Tier 1: Company-reported adjusted (one-offs removed)
    reported_ebitda: Optional[float] = None   # Tier 2: Company-reported EBITDA
    # If neither, compute: EBIT + D&A

    # IFRS 16 lease data
    rou_depreciation: Optional[float] = None
    lease_interest: Optional[float] = None
    short_term_rent: Optional[float] = None
    lease_liabilities_current: Optional[float] = None
    lease_liabilities_noncurrent: Optional[float] = None
    rou_assets: Optional[float] = None

    # D&A breakdown
    depreciation_total: Optional[float] = None
    amortisation_total: Optional[float] = None

    # Metadata
    currency: str = ""
    accounting_standard: str = ""  # IFRS or US GAAP
    source_sections: dict = field(default_factory=dict)
    extraction_confidence: dict = field(default_factory=dict)
    raw_snippets: dict = field(default_factory=dict)


class BrowserPipeline:
    """Automated browser research pipeline for company filings."""

    def __init__(self):
        self._session: Optional[BrowserSession] = None

    @property
    def session(self) -> BrowserSession:
        if self._session is None:
            self._session = BrowserSession()
        return self._session

    async def _ensure_browser(self):
        if self._session is None or not self._session.is_connected:
            self._session = BrowserSession()
            await self._session.start()

    async def close(self):
        if self._session:
            await self._session.close()
            self._session = None

    # --- STEP 1: Find annual report ---

    async def find_annual_report(self, company: str, year: str = "2025",
                                 country: str = "") -> Optional[str]:
        """
        Find company annual report PDF URL.
        Strategy: Try IR URL patterns first (most reliable), Google fallback.
        """
        await self._ensure_browser()

        # Strategy 1: Try common IR URL patterns directly
        common_patterns = self._get_ir_patterns(company)
        consecutive_failures = 0
        max_consecutive = 8  # Stop after this many failures in a row

        for url in common_patterns:
            if consecutive_failures >= max_consecutive:
                logger.info(f"Too many failures ({consecutive_failures}), switching to Google search")
                break

            try:
                logger.info(f"Trying IR URL: {url}")
                await self.session.goto(url)
                await asyncio.sleep(1.5)
                pdf_url = await self._find_pdf_link_on_page(year)
                if pdf_url:
                    logger.info(f"Found PDF on IR page: {pdf_url}")
                    return pdf_url
                consecutive_failures = 0  # Reset: page loaded, just no PDF found
            except Exception as e:
                consecutive_failures += 1
                logger.info(f"IR URL failed ({consecutive_failures}/{max_consecutive}): {url[:80]} - {type(e).__name__}")
                continue

        # Strategy 2: Google search (may be blocked by anti-bot)
        try:
            nav = BrowserNav(self.session)
            query = f'"{company}" annual report {year}'
            await nav.google_search(query)
            pdf_url = await self._find_pdf_in_search_results(company, year)
            if pdf_url:
                return pdf_url
        except Exception as e:
            logger.warning(f"Google search failed: {e}")

        return None

    async def _find_pdf_in_search_results(self, company: str, year: str) -> Optional[str]:
        """Extract PDF URL from Google search results page."""
        page = self.session.default_page
        if not page:
            return None

        try:
            links = await page.evaluate("""() => {
                const links = document.querySelectorAll('a[href]');
                return Array.from(links)
                    .filter(a => a.href.toLowerCase().includes('.pdf'))
                    .map(a => ({
                        href: a.href,
                        text: a.textContent.toLowerCase()
                    }));
            }""")

            # Exclude press releases, trading updates, quarterly results
            exclude_words = ['press release', 'results', 'trading update',
                           'quarterly', 'q1', 'q2', 'q3', 'q4', 'interim',
                           'half.year', 'hy', 'earnings release']

            candidates = []
            for link in (links or []):
                href = link.get('href', '')
                text = link.get('text', '')

                if year not in href and year not in text:
                    continue

                # Skip exclusions
                if any(ex in text for ex in exclude_words):
                    continue
                if any(ex in href.lower() for ex in exclude_words):
                    continue

                # Score: annual report = highest
                score = 0
                if any(w in text for w in ['annual report', 'jaarverslag', 'geschäftsbericht',
                                           'annual review', 'integrated report']):
                    score = 100
                elif any(w in text for w in ['annual', 'report', 'jaar']):
                    score = 50
                elif company.lower().split()[0] in text:
                    score = 20

                if score > 0:
                    candidates.append((score, href))
                    logger.info(f"  PDF candidate (score={score}): {href[:100]}")

            if candidates:
                candidates.sort(key=lambda x: x[0], reverse=True)
                best = candidates[0][1]
                logger.info(f"Selected PDF: {best}")
                return best
        except Exception as e:
            logger.warning(f"PDF link search failed: {e}")

        return None

    async def _find_pdf_link_on_page(self, year: str) -> Optional[str]:
        """Find annual report PDF link on current page.
        Uses text matching + file size heuristics (annual reports > 2MB typical)."""
        page = self.session.default_page
        if not page:
            return None

        try:
            raw = await page.evaluate("""() => {
                return Array.from(document.querySelectorAll('a[href]'))
                    .filter(a => a.href.toLowerCase().includes('.pdf') ||
                                 a.href.toLowerCase().includes('annual') ||
                                 a.href.toLowerCase().includes('report'))
                    .map(a => ({
                        href: a.href,
                        text: a.textContent.trim(),
                        className: a.className,
                        parentText: a.parentElement ? a.parentElement.textContent.trim().substring(0, 200) : ''
                    }));
            }""")

            exclude_words = [
                'press release', 'results', 'trading update', 'quarterly',
                'q1', 'q2', 'q3', 'q4', 'interim', 'half.year', 'hy ',
                'earnings release', 'invitation', 'agenda', 'notice',
                'transcript', 'webcast', 'registration', 'tax report',
                'remuneration', 'governance', 'esg report', 'csr report',
                'sustainability report', 'proxy', 'circular', 'form 20-f',
            ]
            # Words that indicate this IS the annual report
            include_words = [
                'annual report', 'jaarverslag', 'geschäftsbericht',
                'annual review', 'integrated report', 'annual financial',
                'annual accounts', 'report and accounts', 'year in review',
            ]

            candidates = []
            for link in (raw or []):
                href = link.get('href', '')
                text = (link.get('text', '') + ' ' + link.get('parentText', '')).lower()

                # Must contain year
                if year not in href and year not in text:
                    continue

                # Must not be an excluded type
                if any(ex in text for ex in exclude_words):
                    continue
                if any(ex in href.lower() for ex in ['press-release', 'trading-update',
                                                       'quarterly', 'interim', 'esef']):
                    continue

                # Score: higher = more likely the annual report
                score = 0
                if any(w in text for w in include_words):
                    score = 100
                elif any(w in text for w in ['annual', 'report', 'jaar', 'geschäfts']):
                    score = 50
                elif 'report' in text and year in text:
                    score = 30
                elif year in href:
                    score = 10

                # PDF files preferred
                if href.lower().endswith('.pdf'):
                    score += 20
                # ESEF packages are NOT annual reports (machine-readable XBRL)
                if 'esef' in href.lower() or 'esef' in text:
                    score -= 90

                if score > 0:
                    candidates.append((score, href))

            candidates.sort(key=lambda x: x[0], reverse=True)
            if candidates:
                best = candidates[0]
                logger.info(f"Best PDF candidate (score={best[0]}): {best[1][:120]}")
                return best[1]

        except Exception as e:
            logger.warning(f"PDF link search failed: {e}")

        return None

    def _get_ir_patterns(self, company: str) -> list[str]:
        """Generate IR URL patterns. Combined name first (hdfcbank.com),
        then recognizable parts (bam.com for 'Royal BAM Group')."""
        raw = company.lower().replace("'", "").replace(".", "").replace(",", "")
        words = raw.split()

        # Combined slug (hdfcbank, royalbam)
        combined = "".join(words)
        combined_hyphen = "-".join(words)

        skip = {'royal', 'group', 'n.v.', 'nv', 'plc', 'ltd', 'limited',
                'inc', 'sa', 'ag', 'se', 'corporation', 'corp', 'co.',
                'holding', 'holdings', 'international', 'intl', 'the',
                'industries', 'limited.', 'private', 'pvt'}
        key_words = [w for w in words if w not in skip]

        bases = []
        # 1. Key recognizable word FIRST (bam from Royal BAM Group)
        #    Short brand names are the most common domain pattern
        if key_words:
            hint = key_words[-1]
            if hint not in bases:
                bases.append(hint)
            if len(key_words) >= 2 and key_words[0] not in bases:
                bases.append(key_words[0])
        # 2. Combined name SECOND (hdfcbank, royabamgroup)
        if combined not in bases:
            bases.append(combined)
        # 3. First word last
        if words[0] not in bases:
            bases.append(words[0])

        tlds = ['com', 'co.in', 'nl', 'de', 'fr', 'co.uk', 'eu', 'be', 'ch']

        ir_paths = [
            '/investors/annual-reports',
            '/en/investors/annual-reports',
            '/investors/annual-report',
            '/investor-relations/annual-reports',
            '/investors',
            '/en/investors',
        ]

        patterns = []
        for base in bases:
            # Direct combined domain first
            for tld in tlds[:5]:
                patterns.append(f"https://www.{base}.{tld}{ir_paths[0]}")
                patterns.append(f"https://www.{base}.{tld}{ir_paths[5]}")
                patterns.append(f"https://{base}.{tld}{ir_paths[0]}")
            # ir subdomain
            patterns.append(f"https://ir.{base}.com")
            patterns.append(f"https://investors.{base}.com")

        return patterns

    # --- STEP 2: Download PDF ---

    def download_pdf(self, pdf_url: str, company: str = "", year: str = "") -> FilingDocument:
        """Download annual report PDF. Returns FilingDocument with path."""
        logger.info(f"Downloading: {pdf_url}")
        resp = requests.get(pdf_url, timeout=120, headers={
            'User-Agent': 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36'
        })
        resp.raise_for_status()

        tmp_path = os.path.join(
            tempfile.gettempdir(),
            f"annual_report_{company.replace(' ', '_')}_{year}.pdf"
        )
        with open(tmp_path, 'wb') as f:
            f.write(resp.content)

        return FilingDocument(
            company=company,
            year=year,
            pdf_url=pdf_url,
            pdf_path=tmp_path,
            source="ir_website",
        )

    # --- STEP 3: Extract text ---

    def extract_text(self, doc: FilingDocument) -> str:
        """Extract full text from PDF. Updates doc with stats."""
        pdf = fitz.open(doc.pdf_path)
        doc.total_pages = pdf.page_count
        text = ""
        for page in pdf:
            text += page.get_text()
        pdf.close()
        doc.total_chars = len(text)
        return text

    def is_annual_report(self, doc: FilingDocument, text: str = None) -> bool:
        """
        Validate that the downloaded PDF is actually an annual report.
        Checks: page count, presence of financial statement keywords, IFRS/GAAP indicators.
        Returns True if it looks like an annual report, False if it's a press release/small doc.
        """
        # Annual reports are typically 50+ pages
        if doc.total_pages < 40:
            return False

        if text is None:
            text = self.extract_text(doc)

        # Must contain multiple financial statement indicators
        indicators = [
            'balance sheet', 'income statement', 'cash flow',
            'statement of financial position', 'profit or loss',
            'consolidated financial', 'notes to the financial',
            'auditor', 'independent auditor',
            'annual report', 'jaarverslag', 'geschäftsbericht',
        ]
        matches = sum(1 for ind in indicators if ind in text.lower())
        return matches >= 2  # At least 2 indicators = likely annual report

    # --- STEP 4: Extract financial data ---

    def extract_financials(self, text: str, company: str = "",
                           year: str = "") -> ExtractedFinancials:
        """Extract structured financial data from annual report text."""
        fin = ExtractedFinancials(company=company, year=year)

        # Detect accounting standard
        if 'IFRS' in text[:50000] or 'ifrs' in text[:50000].lower():
            fin.accounting_standard = "IFRS"
        elif 'US GAAP' in text[:50000] or 'GAAP' in text[:50000]:
            fin.accounting_standard = "US GAAP"

        # Detect currency
        for curr, symbols in [("EUR", ["€", "EUR", "euro"]),
                              ("USD", ["$", "USD", "dollar"]),
                              ("GBP", ["£", "GBP", "sterling"]),
                              ("INR", ["₹", "INR", "rupee"])]:
            if any(s in text[:10000] for s in symbols):
                fin.currency = curr
                break

        # --- Extract financial statement line items ---
        # Strategy: find the consolidated income statement and balance sheet,
        # then pull numbers using regex patterns

        # Revenue — matches formats like "Revenue  6  7,039,900" or "Revenue  7,039,900"
        fin.revenue = self._extract_amount(text, [
            r'Revenue\s+.*?(\d{1,3}(?:,\d{3}){2,})',
            r'Revenue\s+\d+\s+(\d{1,3}(?:,\d{3})+)',
            r'Total\s+revenue\s+.*?(\d{1,3}(?:,\d{3}){2,})',
        ], 'income_statement')

        # Operating income / EBIT
        fin.operating_income = self._extract_amount(text, [
            r'(?:Operating|Trading)\s+result\s+.*?(\d{1,3}(?:,\d{3})+)',
            r'Operating\s+income\s+.*?(\d{1,3}(?:,\d{3})+)',
            r'Result\s+from\s+operations?\s+.*?(\d{1,3}(?:,\d{3})+)',
            r'EBIT\s+.*?(\d{1,3}(?:,\d{3})+)',
        ], 'income_statement')

        # Net income
        fin.net_income = self._extract_amount(text, [
            r'Net\s+result\s+.*?(\d{1,3}(?:,\d{3})+)',
            r'Net\s+income\s+.*?(\d{1,3}(?:,\d{3})+)',
            r'Profit\s+for\s+the\s+(?:financial\s+)?year\s+.*?(\d{1,3}(?:,\d{3})+)',
        ], 'income_statement')

        # Total assets
        fin.total_assets = self._extract_amount(text, [
            r'Total\s+assets\s+.*?(\d{1,3}(?:,\d{3})*(?:\.\d+)?)',
        ], 'balance_sheet')

        # Total equity
        fin.total_equity = self._extract_amount(text, [
            r'(?:Group\s+)?(?:Total\s+)?[Ee]quity\s+.*?(\d{1,3}(?:,\d{3})*(?:\.\d+)?)',
        ], 'balance_sheet')

        # Cash
        fin.cash = self._extract_amount(text, [
            r'Cash\s+and\s+cash\s+equivalents\s+.*?(\d{1,3}(?:,\d{3})*(?:\.\d+)?)',
        ], 'balance_sheet')

        # Adjusted EBITDA (Tier 1 — company-reported, one-offs removed)
        # Look for patterns like "Adjusted EBITDA of EUR 400 million" or "Adjusted EBITDA 400.3"
        fin.adjusted_ebitda = self._extract_amount(text, [
            r'[Aa]djusted\s+EBITDA.{0,30}?(?:EUR|EUR)?\s*(\d{1,3}(?:,\d{3})*(?:\.\d+)?)\s*(?:million|mln|billion)?',
            r'[Uu]nderlying\s+EBITDA.{0,30}?(?:EUR|EUR)?\s*(\d{1,3}(?:,\d{3})*(?:\.\d+)?)\s*(?:million|mln|billion)?',
        ], 'adjusted_ebitda')

        # Reported EBITDA (Tier 2 — company-reported)
        # Look for standalone EBITDA line in financial tables
        fin.reported_ebitda = self._extract_amount(text, [
            r'(?:^|\n)\s*EBITDA\s+.*?(\d{1,3}(?:,\d{3})+)',
            r'[Rr]eported\s+EBITDA.{0,30}?(\d{1,3}(?:,\d{3})+)',
        ], 'reported_ebitda')

        # Sanity checks: if extracted EBITDA is way off expected range (based on EBIT),
        # mark it as unreliable
        if fin.adjusted_ebitda and fin.operating_income:
            # Adjusted EBITDA should be >= EBIT and typically within 1-3x EBIT
            if fin.adjusted_ebitda < fin.operating_income * 0.5:
                fin.adjusted_ebitda = None  # Too low, unreliable
            elif fin.adjusted_ebitda > fin.operating_income * 5:
                fin.adjusted_ebitda = None  # Too high, unreliable

        if fin.reported_ebitda and fin.operating_income:
            if fin.reported_ebitda < fin.operating_income * 0.5:
                fin.reported_ebitda = None
            elif fin.reported_ebitda > fin.operating_income * 5:
                fin.reported_ebitda = None

        # D&A total — matches "Depreciation and amortisation  (157,791)" format
        fin.depreciation_total = self._extract_amount(text, [
            r'Depreciation\s+and\s+amorti[sz]ation\s*\(?\s*(\d{1,3}(?:,\d{3})+)',
            r'Depreciation,?\s*amorti[sz]ation\s*\(?\s*(\d{1,3}(?:,\d{3})+)',
            r'Depreciation\s+and\s+amorti[sz]ation\s+.*?(\d{1,3}(?:,\d{3}){2,})',
        ], 'income_statement')

        # --- IFRS 16 lease data ---
        fin.rou_depreciation = self._extract_amount(text, [
            r'Depreciation\s+expense\s+of\s+right[-\s]of[-\s]use\s+assets?\s+.*?(\d{1,3}(?:,\d{3})+)',
            r'[Dd]epreciation.{0,30}right[-\s]of[-\s]use.{0,30}?(\d{1,3}(?:,\d{3})+)',
        ], 'lease_note')

        fin.lease_interest = self._extract_amount(text, [
            r'Interest\s+expense\s+on\s+lease\s+liabilities\s+.*?(\d{1,3}(?:,\d{3})+)',
            r'[Ii]nterest.{0,30}lease\s+liabilit.{0,30}?(\d{1,3}(?:,\d{3})+)',
        ], 'finance_note')

        fin.short_term_rent = self._extract_amount(text, [
            r'[Rr]ent\s+expenses?\s+.*?short[-\s]term\s+leases?\s+.*?(\d{1,3}(?:,\d{3})+)',
            r'[Ss]hort[-\s]term\s+lease.{0,80}?(\d{1,3}(?:,\d{3}){2,})',
        ], 'lease_note')

        fin.lease_liabilities_current = self._extract_amount(text, [
            r'[Ll]ease\s+liabilities?\s+.*?(?:[Cc]urrent|short[-\s]term).{0,50}?(\d{1,3}(?:,\d{3})*(?:\.\d+)?)',
        ], 'balance_sheet')

        fin.lease_liabilities_noncurrent = self._extract_amount(text, [
            r'[Ll]ease\s+liabilities?\s+.*?(?:[Nn]on[-\s]current|long[-\s]term).{0,50}?(\d{1,3}(?:,\d{3})*(?:\.\d+)?)',
        ], 'balance_sheet')

        fin.rou_assets = self._extract_amount(text, [
            r'Right[-\s]of[-\s]use\s+assets?\s+.*?(\d{1,3}(?:,\d{3})*(?:\.\d+)?)',
        ], 'balance_sheet')

        return fin

    def _extract_amount(self, text: str, patterns: list[str],
                        section: str = "") -> Optional[float]:
        """Extract a financial amount from text using regex patterns.
        Returns amount in the reported unit (thousands, millions as-is).
        Looks for large numbers (5-10 digits) that represent financial amounts."""
        for pattern in patterns:
            matches = re.findall(pattern, text, re.IGNORECASE | re.DOTALL)
            for raw in matches:
                if isinstance(raw, tuple):
                    raw = raw[0]
                try:
                    val = float(raw.replace(',', '').replace(' ', ''))
                    # Sanity: financial line items should be large numbers (100K+)
                    # This filters out small numbers that happen to match
                    if val > 1000:
                        return val
                except ValueError:
                    continue
        return None

    # --- FULL PIPELINE ---

    async def run_full_pipeline(self, company: str, year: str = "2025",
                                country: str = "") -> tuple[FilingDocument, ExtractedFinancials]:
        """Run full automated pipeline: find -> download -> validate -> extract.
        Retries if the downloaded file is not an actual annual report."""
        logger.info(f"Finding annual report for {company} {year}...")

        max_attempts = 3
        tried_urls = set()
        last_doc = None
        last_text = None

        for attempt in range(max_attempts):
            # Step 1: Find annual report
            pdf_url = await self.find_annual_report(company, year, country)
            if not pdf_url:
                raise FileNotFoundError(f"Could not find annual report for {company} {year}")

            if pdf_url in tried_urls:
                logger.warning(f"Already tried URL: {pdf_url}, skipping")
                continue
            tried_urls.add(pdf_url)

            # Step 2: Download
            try:
                doc = self.download_pdf(pdf_url, company, year)
            except Exception as e:
                logger.warning(f"Download failed for {pdf_url}: {e}")
                continue

            # Step 3: Extract text
            text = self.extract_text(doc)
            logger.info(f"Attempt {attempt+1}: {doc.total_pages} pages, {doc.total_chars:,} chars")

            # Step 4: Validate it's an annual report
            if not self.is_annual_report(doc, text):
                logger.warning(
                    f"URL returned {doc.total_pages}-page document — "
                    f"likely press release, not annual report. Retrying..."
                )
                last_doc = doc
                last_text = text
                continue

            # Success — looks like a real annual report
            logger.info(f"Valid annual report: {doc.total_pages} pages")
            fin = self.extract_financials(text, company, year)
            logger.info(f"Extracted financials for {company}")
            return doc, fin

        # All attempts exhausted — return best available
        if last_doc and last_text:
            logger.warning("All attempts returned non-annual-report files. Using best available.")
            fin = self.extract_financials(last_text, company, year)
            return last_doc, fin

        raise FileNotFoundError(f"Could not find valid annual report for {company} {year}")


# --- Convenience function ---

async def research_non_us_company(company: str, year: str = "2025",
                                  country: str = "") -> ExtractedFinancials:
    """One-shot: research a non-US company from annual report."""
    pipeline = BrowserPipeline()
    try:
        doc, fin = await pipeline.run_full_pipeline(company, year, country)
        return fin
    finally:
        await pipeline.close()
