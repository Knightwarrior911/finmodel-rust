"""
Automated browser research pipeline.
Google search -> find annual report PDF -> download -> extract financial data.

Handles non-US companies (Euronext, LSE, BSE/NSE, etc.) via real Chrome browser.
US companies prefer SEC EDGAR API (faster), browser as fallback.
"""

import asyncio
import logging
import os
import random
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

# Jurisdiction → (TLDs, regulator site, filing name patterns)
JURISDICTION_PATTERNS = {
    ".DE": ("Germany", ["de", "com"], "site:bundesanzeiger.de",
            "geschäftsbericht OR annual report OR financial statements"),
    ".PA": ("France", ["fr", "com"], "",
            "rapport financier OR annual report OR financial statements"),
    ".AS": ("Netherlands", ["nl", "com"], "",
            "jaarverslag OR annual report OR financial statements"),
    ".L": ("UK", ["co.uk", "com"], "site:companieshouse.gov.uk",
           "annual report OR annual financial report OR financial statements"),
    ".SW": ("Switzerland", ["ch", "com"], "",
            "geschäftsbericht OR annual report OR financial statements"),
    ".MI": ("Italy", ["it", "com"], "",
            "bilancio OR relazione finanziaria OR annual report"),
    ".MC": ("Spain", ["es", "com"], "",
            "informe anual OR cuentas anuales OR annual report"),
    ".NS": ("India", ["co.in", "com"], "site:bseindia.com OR site:nseindia.com",
            "annual report OR financial results"),
    ".BO": ("India", ["co.in", "com"], "site:bseindia.com OR site:nseindia.com",
            "annual report OR financial results"),
    ".T": ("Japan", ["co.jp", "com"], "site:edinet-fsa.go.jp",
            "annual report OR financial statements OR yukashoken hokokusho"),
    ".HK": ("Hong Kong", ["hk", "com"], "site:hkexnews.hk",
            "annual report OR announcement OR circular"),
    ".CO": ("Singapore", ["com.sg", "com"], "site:sgx.com",
            "annual report"),
    ".IR": ("Other", ["com"], "", "annual report"),
    ".AX": ("Australia", ["com.au", "com"], "site:asx.com.au",
            "annual report OR announcement"),
}


def _detect_jurisdiction(ticker: str, company: str = "") -> dict:
    """Detect jurisdiction from ticker suffix or company name country hints."""
    ticker_upper = ticker.upper() if ticker else ""
    company_lower = (company or "").lower()

    for suffix, (country, tlds, reg_site, query) in JURISDICTION_PATTERNS.items():
        if ticker_upper.endswith(suffix):
            return {"country": country, "tlds": tlds, "regulator_site": reg_site,
                    "local_query": query, "ticker_suffix": suffix}

    # Country name in company name fallback
    country_hints = {
        "germany": "DE", "deutschland": "DE", "france": "FR", "netherlands": "NL",
        "italy": "IT", "spain": "ES", "españa": "ES", "switzerland": "CH",
        "schweiz": "CH", "japan": "JP", "india": "IN", "china": "CN",
        "brazil": "BR", "brasil": "BR", "australia": "AU", "canada": "CA",
        "singapore": "SG", "hong kong": "HK", "uk": "UK", "united kingdom": "UK",
    }
    for hint, cc in country_hints.items():
        if hint in company_lower:
            return {"country": cc, "tlds": ["com"], "regulator_site": "",
                    "local_query": "annual report", "ticker_suffix": ""}

    return {"country": "Unknown", "tlds": ["com"], "regulator_site": "",
            "local_query": "annual report", "ticker_suffix": ""}


def _human_delay(min_s: float = 1.0, max_s: float = 6.0):
    """Random human-like delay between actions."""
    return random.uniform(min_s, max_s)


def _build_google_query(company: str, year: str, jurisdiction: dict) -> str:
    """Build jurisdiction-aware Google search query with operators."""
    country = jurisdiction["country"]
    reg_site = jurisdiction["regulator_site"]
    local_q = jurisdiction["local_query"]
    tlds = jurisdiction["tlds"]

    # Derive company domain hint for site: operator
    skip = {'royal', 'group', 'n.v.', 'nv', 'plc', 'ltd', 'limited',
            'inc', 'sa', 'ag', 'se', 'corporation', 'corp', 'co.',
            'holding', 'holdings', 'international', 'intl', 'the',
            'industries', 'limited.', 'private', 'pvt', 'gmbh'}
    key_words = [w for w in company.lower().replace(',', '').split() if w not in skip]
    domain_hint = "-".join(key_words) if len(key_words) >= 2 else (key_words[0] if key_words else "")

    # Base: exact company name + annual report + year
    base = f'"{company}" ({local_q}) {year} filetype:pdf'

    # Prefer company's own domain
    if domain_hint:
        base += f" OR site:{domain_hint}.com"

    # Jurisdiction-aware regulator site
    if reg_site:
        base += f" OR {reg_site}"

    # Exclude noise domains
    base += ' -inurl:(news OR press OR blog OR yahoo OR seekingalpha OR simplywallst OR macrotrends)'

    return base


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
    goodwill: Optional[float] = None
    short_term_investments: Optional[float] = None

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

    # EV bridge items (balance sheet / notes)
    minority_interest: Optional[float] = None        # Non-controlling interest
    preferred_stock: Optional[float] = None
    equity_investments: Optional[float] = None       # Equity method investments / associates
    financial_investments: Optional[float] = None    # Non-operating financial investments
    assets_held_for_sale: Optional[float] = None
    discontinued_ops_assets: Optional[float] = None
    nol_dta: Optional[float] = None                  # NOL / Deferred Tax Assets (non-operating)
    pension_pbo: Optional[float] = None               # Projected Benefit Obligation (R-015)
    pension_plan_assets: Optional[float] = None       # Plan assets at fair value (R-015)
    operating_lease_liabilities: Optional[float] = None  # Operating leases per note (R-016)
    finance_lease_liabilities: Optional[float] = None    # Finance/capital leases per note

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

    async def _dismiss_cookie_popup(self):
        """Dismiss common cookie consent popups that block page content."""
        page = self.session.default_page
        if not page:
            return
        try:
            # OneTrust (most common)
            btn = page.locator("#onetrust-accept-btn-handler")
            if await btn.count() > 0 and await btn.is_visible():
                await btn.click()
                await asyncio.sleep(random.uniform(0.3, 0.8))
                return
            # Common button texts
            for label in ["Accept All", "Accept All Cookies", "Accept Cookies",
                          "Accept", "Allow All", "Allow Cookies",
                          "I Accept", "Agree", "OK", "Got it",
                          "Accept and Continue", "Accept & Continue",
                          "Akzeptieren", "Akzeptieren Alle", "Zustimmen",
                          "Accepter", "Aceptar", "Accetta"]:
                btn = page.locator(f"button:has-text('{label}')")
                if await btn.count() > 0 and await btn.first.is_visible():
                    await btn.first.click()
                    await asyncio.sleep(random.uniform(0.3, 0.8))
                    return
            # Cookie consent banner links
            for label in ["Accept", "Agree", "Allow"]:
                link = page.locator(f"a:has-text('{label}')")
                if await link.count() > 0 and await link.first.is_visible():
                    await link.first.click()
                    await asyncio.sleep(random.uniform(0.3, 0.8))
                    return
        except Exception:
            pass  # Cookie dismissal is best-effort

    async def _human_scroll(self):
        """Simulate human reading: scroll down in segments with pauses."""
        page = self.session.default_page
        if not page:
            return
        try:
            # Scroll in 3-5 segments with pauses like actual reading
            segments = random.randint(3, 5)
            for _ in range(segments):
                px = random.randint(200, 700)
                await page.evaluate(f"window.scrollBy(0, {px})")
                await asyncio.sleep(random.uniform(0.5, 2.0))
            # Sometimes scroll back up a bit (re-reading)
            if random.random() < 0.3:
                await page.evaluate(f"window.scrollBy(0, -{random.randint(100, 300)})")
                await asyncio.sleep(random.uniform(0.5, 1.5))
        except Exception:
            pass

    # --- STEP 1: Find annual report ---

    async def find_annual_report(self, company: str, year: str = "2025",
                                 country: str = "", ticker: str = "") -> Optional[str]:
        """
        Find company annual report PDF URL.
        Strategy: Jurisdiction-aware Google search → IR URL patterns fallback.
        """
        await self._ensure_browser()

        # Detect jurisdiction for targeted search
        jurisdiction = _detect_jurisdiction(ticker, company)
        logger.info(f"Jurisdiction: {jurisdiction['country']} (ticker={ticker}, company={company})")

        # Strategy 1: Jurisdiction-aware Google search
        google_candidates = []
        try:
            nav = BrowserNav(self.session)
            query = _build_google_query(company, year, jurisdiction)
            logger.info(f"Google query: {query[:150]}...")
            await nav.google_search(query)
            # Human-like pause — scan search results
            await asyncio.sleep(_human_delay(3, 6))
            google_candidates = await self._find_pdf_in_search_results(company, year)
            if google_candidates:
                self._google_candidates = google_candidates
                logger.info(f"Found {len(google_candidates)} PDF candidates via Google, top: {google_candidates[0][:120]}")
                return google_candidates[0]
        except Exception as e:
            logger.warning(f"Google search failed: {e}")

        # Strategy 2: Try common IR URL patterns (fallback)
        common_patterns = self._get_ir_patterns(company)
        consecutive_failures = 0
        max_consecutive = 5

        for url in common_patterns:
            if consecutive_failures >= max_consecutive:
                logger.info(f"Too many IR failures, giving up")
                break
            try:
                # Human-like pause between URL attempts
                await asyncio.sleep(_human_delay(2, 5))
                logger.info(f"Trying IR URL: {url}")
                await self.session.goto(url)
                await asyncio.sleep(_human_delay(3, 8))
                await self._dismiss_cookie_popup()
                await self._human_scroll()
                pdf_url = await self._find_pdf_link_on_page(year)
                if pdf_url:
                    logger.info(f"Found PDF on IR page: {pdf_url}")
                    return pdf_url
                consecutive_failures = 0
            except Exception as e:
                consecutive_failures += 1
                logger.info(f"IR URL failed ({consecutive_failures}/{max_consecutive}): {url[:80]} - {type(e).__name__}")
                continue

        return None

    async def _find_pdf_in_search_results(self, company: str, year: str) -> list[str]:
        """Extract PDF URLs from Google search results page.
        Returns list sorted by relevance (best first)."""
        page = self.session.default_page
        if not page:
            return []

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

            exclude_words = ['press release', 'results', 'trading update',
                           'quarterly', 'q1', 'q2', 'q3', 'q4', 'interim',
                           'half.year', 'hy', 'earnings release']

            candidates = []
            for link in (links or []):
                href = link.get('href', '')
                text = link.get('text', '')
                href_lower = href.lower()

                if year not in href and year not in text:
                    continue

                if any(ex in text for ex in exclude_words):
                    continue
                if any(ex in href_lower for ex in exclude_words):
                    continue

                # Score: full annual report > summary > other
                score = 0
                # Full annual report keywords
                if any(w in text for w in ['annual report', 'jaarverslag', 'geschäftsbericht',
                                           'annual review', 'integrated report', 'annual financial']):
                    score = 100
                # Summary / abbreviated versions
                elif any(w in text for w in ['annual summary', 'annual-summary']):
                    score = 30
                elif any(w in text for w in ['annual', 'report', 'jaar']):
                    score = 50
                elif company.lower().split()[0] in text:
                    score = 20

                # Penalize annual summaries / abridged versions in filename
                if '-as-' in href_lower or '-as.' in href_lower or href_lower.endswith('-as'):
                    score -= 70
                if 'summary' in href_lower or 'annual-summary' in href_lower:
                    score -= 70
                if 'abridged' in href_lower or 'short' in href_lower:
                    score -= 50

                # Penalize India subsidiary (not the parent company report)
                if 'india' in href_lower:
                    score -= 60

                # Domain relevance: prefer URLs matching company name
                company_slug = re.sub(r'[^a-z0-9]', '-', company.lower().strip())
                company_domain_hint = company_slug.replace('--', '-').strip('-')
                try:
                    from urllib.parse import urlparse
                    domain = urlparse(href).netloc.lower()
                    # Exact domain match (e.g., siemens-energy.com = "siemens energy ag")
                    if company_domain_hint in domain or domain in company_domain_hint:
                        score += 30
                    # Partial: first key word in domain
                    first_word = company.lower().split()[0] if company.split() else ''
                    if first_word and first_word in domain:
                        score += 10
                    # Penalize clearly different parent company domains
                    # "new.siemens.com" vs expected "siemens-energy.com"
                    if first_word and first_word in domain and company_domain_hint not in domain:
                        # Check: is there another keyword that matches better?
                        other_words = [w for w in company.lower().split() if w != first_word
                                      and w not in ('ag', 'se', 'sa', 'plc', 'ltd', 'limited', 'inc', 'corp',
                                                     'group', 'holding', 'holdings', 'n.v.', 'nv')]
                        if other_words and not any(w in domain for w in other_words):
                            score -= 20  # Only first word matches, not full company name
                except Exception:
                    pass

                # Penalize parent/holding company domains when searching for subsidiary
                # e.g., "Siemens Energy AG" → should NOT get "new.siemens.com" (Siemens AG parent)
                company_words = set(company.lower().split()) - {'ag', 'se', 'sa', 'plc', 'ltd', 'limited',
                                     'inc', 'corp', 'group', 'holding', 'holdings', 'n.v.', 'nv', 'gmbh'}
                # If URL domain contains first word but NONE of the other key words, penalize
                if len(company_words) >= 2:
                    first = list(company_words)[0]
                    others = company_words - {first}
                    if first in domain and not any(w in domain for w in others):
                        score -= 40  # Only parent company name matches, not subsidiary

                if score > 0:
                    candidates.append((score, href))
                    logger.info(f"  PDF candidate (score={score}): {href[:120]}")

            candidates.sort(key=lambda x: x[0], reverse=True)
            if candidates:
                logger.info(f"Top candidate (score={candidates[0][0]}): {candidates[0][1][:120]}")
            return [url for _, url in candidates]
        except Exception as e:
            logger.warning(f"PDF link search failed: {e}")
            return []

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

        skip = {'royal', 'group', 'n.v.', 'nv', 'plc', 'ltd', 'limited',
                'inc', 'sa', 'ag', 'se', 'corporation', 'corp', 'co.',
                'holding', 'holdings', 'international', 'intl', 'the',
                'industries', 'limited.', 'private', 'pvt'}
        key_words = [w for w in words if w not in skip]
        key_hyphenated = "-".join(key_words)  # "siemens-energy" not "siemens-energy-ag"

        bases = []
        # 1. Hyphenated key-words domain FIRST (siemens-energy.com)
        #    Most common for multi-word company names
        if key_hyphenated not in bases and len(key_words) >= 2:
            bases.append(key_hyphenated)
        # 2. Combined name SECOND (siemensenergyag)
        if combined not in bases:
            bases.append(combined)
        # 3. Single key words LAST (siemens, energy)
        if key_words:
            for kw in key_words:
                if kw not in bases:
                    bases.append(kw)
        # 4. First word fallback
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
            # European company IR patterns
            '/global/en/company/investor-relations',
            '/global/en/company/investor-relations.html',
            '/company/investor-relations',
            '/en/company/investor-relations',
        ]

        patterns = []
        for base in bases:
            # .com TLD with ALL path patterns (most likely to work)
            for path in ir_paths:
                patterns.append(f"https://www.{base}.com{path}")
                patterns.append(f"https://{base}.com{path}")
            # Other TLDs with top 2 most common paths
            for tld in [t for t in tlds[:5] if t != 'com']:
                patterns.append(f"https://www.{base}.{tld}{ir_paths[0]}")
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

        # Revenue — income statement format: "Revenue  6  7,039,900  6,454,951" (label note# value prior_year)
        # AFTER the income statement header to avoid matching other "revenue" mentions
        is_start = text.find("Consolidated income statement")
        if is_start == -1:
            is_start = text.find("Income statement")
        fs_text = text[is_start:] if is_start > 0 else text

        fin.revenue = self._extract_amount(fs_text[:50000], [
            r'Revenue\s+\d+\s+(\d{1,3}(?:,\d{3})+)\s+\d{1,3}(?:,\d{3})+',  # Note format
            r'Revenue\s+(\d{1,3}(?:,\d{3}){2,})\s+\d{1,3}(?:,\d{3}){2,}',   # No note, has prior year
        ], 'income_statement')
        # Fallback: search full text
        if not fin.revenue:
            fin.revenue = self._extract_amount(text, [
                r'Revenue\s+\d+\s+(\d{1,3}(?:,\d{3})+)\s+\d{1,3}(?:,\d{3})+',
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

        # Total debt — from balance sheet or debt footnote
        fin.total_debt = self._extract_amount(text, [
            r'(?:Total\s+)?[Ff]inancial\s+(?:debt|liabilities|indebtedness)\s+.*?(\d{1,3}(?:,\d{3})*(?:\.\d+)?)',
            r'(?:Total\s+)?[Bb]orrowings\s+.*?(\d{1,3}(?:,\d{3})*(?:\.\d+)?)',
            r'[Ll]oans\s+and\s+borrowings\s+.*?(\d{1,3}(?:,\d{3})*(?:\.\d+)?)',
            r'[Nn]otes\s+(?:payable|outstanding)\s+.*?(\d{1,3}(?:,\d{3})*(?:\.\d+)?)',
            r'[Ll]ong[-\s]term\s+debt.{0,40}?(\d{1,3}(?:,\d{3})*(?:\.\d+)?)',
            r'(?:^|\n)\s*(?:Total\s+)?[Dd]ebt\s+.*?(\d{1,3}(?:,\d{3})*(?:\.\d+)?)',
        ], 'balance_sheet')

        # Goodwill — from balance sheet
        fin.goodwill = self._extract_amount(text, [
            r'Goodwill\s+.*?(\d{1,3}(?:,\d{3})*(?:\.\d+)?)',
        ], 'balance_sheet')

        # Short-term investments
        fin.short_term_investments = self._extract_amount(text, [
            r'[Ss]hort[-\s]term\s+investments\s+.*?(\d{1,3}(?:,\d{3})*(?:\.\d+)?)',
            r'[Mm]arketable\s+securities\s+.*?(\d{1,3}(?:,\d{3})*(?:\.\d+)?)',
        ], 'balance_sheet')

        # Adjusted EBITDA (Tier 1 — company-reported, one-offs removed)
        # Try "Total Group" segment table first: "Total Group  7,040  400.3  6,455  333.3"
        tg_match = re.search(
            r'Total\s+Group[\s\S]{0,200}?(\d{1,3}(?:,\d{3})*(?:\.\d+)?)\s+(\d+\.?\d*)\s+(\d{1,3}(?:,\d{3})*(?:\.\d+)?)',
            text[:200000], re.IGNORECASE
        )
        if tg_match:
            tg_adj_ebitda = float(tg_match.group(2).replace(',', ''))
            # Total Group table in EUR millions, convert to thousands
            if tg_adj_ebitda > 10 and tg_adj_ebitda < 100000:
                fin.adjusted_ebitda = tg_adj_ebitda * 1_000  # millions -> thousands

        if not fin.adjusted_ebitda:
            fin.adjusted_ebitda = self._extract_amount(text, [
                r'[Aa]djusted\s+EBITDA.{0,60}?(?:EUR|EUR|of\s+)?\s*(\d+\.?\d*)\s*(?:million|mln)',
                r'[Aa]djusted\s+EBITDA\s+was.{0,40}?(?:EUR|EUR)?\s*(\d+\.?\d*)\s*(?:million|mln)',
                r'[Uu]nderlying\s+EBITDA.{0,30}?(?:EUR|EUR)?\s*(\d+\.?\d*)\s*(?:million|mln)',
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

        # --- EV bridge balance sheet items ---

        # Minority interest / Non-controlling interest
        fin.minority_interest = self._extract_amount(text, [
            r'[Nn]on[-\s]controlling\s+(?:interest|equity)\s+.*?(\d{1,3}(?:,\d{3})*(?:\.\d+)?)',
            r'[Mm]inority\s+(?:interest|equity)\s+.*?(\d{1,3}(?:,\d{3})*(?:\.\d+)?)',
            r'[Nn]on[-\s]controlling\s+.*?interest.*?(\d{1,3}(?:,\d{3})*(?:\.\d+)?)',
        ], 'balance_sheet')

        # Preferred stock
        fin.preferred_stock = self._extract_amount(text, [
            r'[Pp]referred\s+(?:stock|shares|equity)\s+.*?(\d{1,3}(?:,\d{3})*(?:\.\d+)?)',
            r'[Pp]reference\s+shares?\s+.*?(\d{1,3}(?:,\d{3})*(?:\.\d+)?)',
        ], 'balance_sheet')

        # Equity investments / Associates (equity method)
        fin.equity_investments = self._extract_amount(text, [
            r'[Ii]nvestments?\s+(?:in|accounted\s+for\s+using\s+the\s+)?(?:equity[-\s]?(?:accounted|method)\s+)?(?:associates?|joint\s+ventures?)\s+.*?(\d{1,3}(?:,\d{3})*(?:\.\d+)?)',
            r'[Ee]quity[-\s]?(?:accounted|method)\s+invest(?:ments|ees)\s+.*?(\d{1,3}(?:,\d{3})*(?:\.\d+)?)',
            r'[Ii]nvestments?\s+in\s+associates?\s+.*?(\d{1,3}(?:,\d{3})*(?:\.\d+)?)',
            r'[Ii]nterests?\s+in\s+associates?\s+.*?(\d{1,3}(?:,\d{3})*(?:\.\d+)?)',
        ], 'balance_sheet')

        # Financial investments (non-operating)
        fin.financial_investments = self._extract_amount(text, [
            r'[Ff]inancial\s+(?:assets?\s+at\s+fair\s+value\s+through\s+(?:profit|OCI|other)|investments?\s+\(?non[-\s]?(?:operating|current)\)?)\s+.*?(\d{1,3}(?:,\d{3})*(?:\.\d+)?)',
            r'[Oo]ther\s+(?:long[-\s]term\s+)?(?:financial\s+)?investments?\s+.*?(\d{1,3}(?:,\d{3})*(?:\.\d+)?)',
            r'[Nn]on[-\s]current\s+financial\s+assets\s+.*?(\d{1,3}(?:,\d{3})*(?:\.\d+)?)',
            r'[Ll]ong[-\s]term\s+financial\s+investments?\s+.*?(\d{1,3}(?:,\d{3})*(?:\.\d+)?)',
        ], 'balance_sheet')

        # Assets held for sale
        fin.assets_held_for_sale = self._extract_amount(text, [
            r'[Aa]ssets?\s+(?:classified\s+as\s+)?held\s+for\s+sale\s+.*?(\d{1,3}(?:,\d{3})*(?:\.\d+)?)',
            r'[Nn]on[-\s]current\s+assets?\s+held\s+for\s+sale\s+.*?(\d{1,3}(?:,\d{3})*(?:\.\d+)?)',
        ], 'balance_sheet')

        # Discontinued operations assets
        fin.discontinued_ops_assets = self._extract_amount(text, [
            r'[Dd]iscontinued\s+operations?\s+(?:assets?|net\s+assets?)\s+.*?(\d{1,3}(?:,\d{3})*(?:\.\d+)?)',
            r'[Aa]ssets?\s+(?:of|from)\s+discontinued\s+operations?\s+.*?(\d{1,3}(?:,\d{3})*(?:\.\d+)?)',
        ], 'balance_sheet')

        # NOL / Deferred Tax Assets
        fin.nol_dta = self._extract_amount(text, [
            r'[Dd]eferred\s+tax\s+assets?\s+.*?(\d{1,3}(?:,\d{3})*(?:\.\d+)?)',
            r'[Nn]et\s+operating\s+loss\s+(?:carry[-\s]?forwards?|DTA)\s+.*?(\d{1,3}(?:,\d{3})*(?:\.\d+)?)',
            r'[Tt]ax\s+loss\s+carry[-\s]?forwards?\s+.*?(\d{1,3}(?:,\d{3})*(?:\.\d+)?)',
        ], 'balance_sheet')

        # --- Pension footnote data (R-015: ALWAYS from notes, NOT balance sheet) ---
        # Search for the pension note section first
        pension_section = ""
        for marker in ["Defined benefit", "Pension commitment", "Post-employment benefit",
                       "Pension obligations", "Employee benefit obligations",
                       "Retirement benefit obligation", "Pension plans",
                       "defined benefit obligation", "pension liability"]:
            idx = text.lower().find(marker.lower())
            if idx > 0:
                pension_section = text[idx:idx + 15000]
                break

        if pension_section:
            # PBO / DBO (Defined Benefit Obligation)
            fin.pension_pbo = self._extract_amount(pension_section, [
                r'[Pp]resent\s+value\s+of\s+(?:the\s+)?(?:defined\s+benefit\s+)?obligations?\s+.*?(\d{1,3}(?:,\d{3})*(?:\.\d+)?)',
                r'[Dd]efined\s+benefit\s+obligations?\s+.*?(\d{1,3}(?:,\d{3})*(?:\.\d+)?)',
                r'[Pp]rojected\s+benefit\s+obligations?\s+.*?(\d{1,3}(?:,\d{3})*(?:\.\d+)?)',
                r'[Bb]enefit\s+obligations?(?:\s+at\s+(?:fair\s+value|present\s+value))?\s+.*?(\d{1,3}(?:,\d{3})*(?:\.\d+)?)',
                r'[Pp]ension\s+(?:obligations?|liability)\s+.*?(\d{1,3}(?:,\d{3})*(?:\.\d+)?)',
            ], 'pension_note')

            # Plan assets
            fin.pension_plan_assets = self._extract_amount(pension_section, [
                r'[Ff]air\s+value\s+of\s+(?:plan\s+)?assets?\s+.*?(\d{1,3}(?:,\d{3})*(?:\.\d+)?)',
                r'[Pp]lan\s+assets?\s+(?:at\s+fair\s+value\s+)?.*?(\d{1,3}(?:,\d{3})*(?:\.\d+)?)',
                r'[Aa]ssets?\s+of\s+(?:the\s+)?(?:defined\s+benefit\s+)?plans?\s+.*?(\d{1,3}(?:,\d{3})*(?:\.\d+)?)',
            ], 'pension_note')

            # If PBO/DBO was found but plan assets not yet, try wider search in pension section
            if fin.pension_pbo and not fin.pension_plan_assets:
                fin.pension_plan_assets = self._extract_amount(pension_section, [
                    r'(?:^|\n)\s*(?:Plan\s+)?[Aa]ssets?\s+.*?(\d{1,3}(?:,\d{3})*(?:\.\d+)?)',
                ], 'pension_note')

        # --- Operating vs Finance lease liabilities (R-016: from lease footnote) ---
        lease_note_section = ""
        for marker in ["Lease liabilities", "Lease commitments", "Right-of-use",
                       "IFRS 16", "ASC 842", "Leases (Note", "Note 15",
                       "lease liability maturity"]:
            idx = text.find(marker)
            if idx > 0:
                lease_note_section = text[idx:idx + 10000]
                break

        if lease_note_section:
            # Operating lease liabilities (non-current portion per note)
            fin.operating_lease_liabilities = self._extract_amount(lease_note_section, [
                r'[Oo]perating\s+lease\s+(?:liabilit|obligation).{0,40}?(\d{1,3}(?:,\d{3})*(?:\.\d+)?)',
                r'[Nn]on[-\s]current\s+lease\s+liabilit.{0,40}?(\d{1,3}(?:,\d{3})*(?:\.\d+)?)',
            ], 'lease_note')
            # Fallback: use balance sheet non-current lease liabilities as operating leases
            if not fin.operating_lease_liabilities and fin.lease_liabilities_noncurrent:
                fin.operating_lease_liabilities = fin.lease_liabilities_noncurrent

            # Finance/capital lease liabilities
            fin.finance_lease_liabilities = self._extract_amount(lease_note_section, [
                r'[Ff]inance\s+lease\s+(?:liabilit|obligation).{0,40}?(\d{1,3}(?:,\d{3})*(?:\.\d+)?)',
                r'[Cc]apital\s+lease\s+(?:liabilit|obligation).{0,40}?(\d{1,3}(?:,\d{3})*(?:\.\d+)?)',
            ], 'lease_note')

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
                        section: str = "", scale: str = "auto") -> Optional[float]:
        """Extract a financial amount from text using regex patterns.
        Returns amount in the reported unit (thousands, millions as-is).
        scale: 'auto' detects from context, 'k' = thousands, 'm' = millions.

        Looks for numbers with commas (1,234,567) or decimals (400.3).
        Sane range check filters out small/irrelevant matches.
        """
        for pattern in patterns:
            matches = re.findall(pattern, text, re.IGNORECASE | re.DOTALL)
            for raw in matches:
                if isinstance(raw, tuple):
                    raw = raw[0]
                try:
                    val = float(raw.replace(',', '').replace(' ', ''))
                    # Sanity check based on context
                    if section in ('income_statement', 'adjusted_ebitda'):
                        if val > 10:  # Income statement items in thousands
                            # If it's a decimal like 400.3, it's in millions -> convert to thousands
                            if '.' in str(raw) and val < 10000:
                                return val * 1_000  # millions -> thousands
                            # If it's a comma number like 7,039,900 it's already in thousands
                            if ',' in str(raw):
                                return val
                            # Clean integer in millions (like 7040) -> convert to thousands
                            if val >= 100 and '.' not in str(raw):
                                return val * 1_000
                            return val
                    elif section in ('lease_note', 'finance_note'):
                        if val > 10:  # Lease note items can be smaller
                            return val
                    elif val > 1000:  # Generic: must be reasonably large
                        return val
                except ValueError:
                    continue
        return None

    # --- FULL PIPELINE ---

    async def run_full_pipeline(self, company: str, year: str = "2025",
                                country: str = "", ticker: str = "") -> tuple[FilingDocument, ExtractedFinancials]:
        """Run full automated pipeline: find -> download -> validate -> extract.
        Retries with next Google candidate if validation fails."""
        logger.info(f"Finding annual report for {company} {year}...")

        tried_urls = set()
        last_doc = None
        last_text = None
        google_idx = 0
        ir_fallback_urls = []
        ir_fallback_idx = 0

        # Step 1: Google search to discover candidates
        await self.find_annual_report(company, year, country, ticker=ticker)
        google_candidates = getattr(self, '_google_candidates', [])
        if not google_candidates:
            logger.info("No Google candidates, going straight to IR URL patterns")
            ir_fallback_urls = self._get_ir_patterns(company)
        logger.info(f"Pipeline: {len(google_candidates)} Google candidates + IR fallback available")

        while True:
            pdf_url = None

            # Phase 1: Try Google candidates
            if google_idx < len(google_candidates):
                pdf_url = google_candidates[google_idx]
                google_idx += 1
            elif ir_fallback_idx == 0:
                # Phase 2: Google exhausted — initialize IR URL pattern fallback
                logger.info(f"All {len(google_candidates)} Google candidates tried. Starting IR URL pattern fallback.")
                ir_fallback_urls = self._get_ir_patterns(company)

            if ir_fallback_idx < len(ir_fallback_urls) and not pdf_url:
                # Phase 2: Try next IR pattern
                url = ir_fallback_urls[ir_fallback_idx]
                ir_fallback_idx += 1
                if url not in tried_urls:
                    # Limit IR patterns tried (first 20 most likely)
                    if ir_fallback_idx > 20:
                        logger.info(f"IR pattern limit reached, stopping fallback")
                        break
                    try:
                        logger.info(f"IR fallback: {url[:100]}")
                        await self.session.goto(url)
                        await asyncio.sleep(_human_delay(1.5, 3))
                        await self._dismiss_cookie_popup()
                        found = await self._find_pdf_link_on_page(year)
                        if found:
                            pdf_url = found
                            logger.info(f"IR pattern found PDF: {found[:120]}")
                    except Exception:
                        continue

            if not pdf_url:
                if ir_fallback_idx >= len(ir_fallback_urls) and google_idx >= len(google_candidates):
                    break
                continue

            if pdf_url in tried_urls:
                logger.warning(f"Already tried URL, trying next candidate")
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
            attempt_num = len(tried_urls)
            logger.info(f"Attempt {attempt_num}: {doc.total_pages} pages, {doc.total_chars:,} chars")

            # Step 4: Validate it's an annual report
            if not self.is_annual_report(doc, text):
                logger.warning(
                    f"URL returned {doc.total_pages}-page document — "
                    f"likely not full annual report. Trying next candidate..."
                )
                last_doc = doc
                last_text = text
                continue

            # Success
            logger.info(f"Valid annual report: {doc.total_pages} pages")
            self._google_candidates = []  # Clear for next run
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
                                  country: str = "", ticker: str = "") -> ExtractedFinancials:
    """One-shot: research a non-US company from annual report."""
    pipeline = BrowserPipeline()
    try:
        doc, fin = await pipeline.run_full_pipeline(company, year, country, ticker=ticker)
        return fin
    finally:
        await pipeline.close()
