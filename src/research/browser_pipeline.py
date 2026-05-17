"""
Automated browser research pipeline.
Google search -> find annual report PDF -> download -> extract financial data.

Handles non-US companies (Euronext, LSE, BSE/NSE, etc.) via real Chrome browser.
US companies prefer SEC EDGAR API (faster), browser as fallback.
"""

import asyncio
import json
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
from src.browser.llm_navigator import LLMNavigator

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
    ".ST": ("Sweden", ["com", "se"], "",
            "annual report OR årsredovisning OR financial statements"),
    ".HE": ("Finland", ["com", "fi"], "",
            "annual report OR vuosikertomus OR financial statements"),
    ".OL": ("Norway", ["com", "no"], "",
            "annual report OR årsrapport OR financial statements"),
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


# Exchange/regulator databases by ticker suffix.
# These are authoritative sources — annual reports are legally required to be filed here.
REGULATOR_SITES = {
    '.NS': 'bseindia.com',
    '.BO': 'bseindia.com',
    '.HK': 'hkexnews.hk',
    '.AX': 'asx.com.au',
    '.T':  'edinet-fsa.go.jp',
    '.CO': 'sgx.com',
}

# Link text keywords that unambiguously mean "annual report" in various languages
ANNUAL_KEYWORDS = [
    'annual report', 'annual financial report', 'annual review',
    'integrated report', 'report and accounts',
    'jaarverslag', 'geschäftsbericht', 'rapport annuel',
    'relazione annuale', 'informe anual', 'годовой отчет',
]

# Navigation section names that lead to annual reports (used for sub-nav following, not PDF text matching)
SUBNAV_KEYWORDS = [
    'annual report', 'annual reports', 'annual review', 'annual results',
    'publications', 'publications and reports', 'publications & ad hoc',
    'reports and publications', 'financial reports', 'financial publications',
    'investor publications', 'results centre', 'results center',
    'document library', 'regulatory disclosures', 'financial calendar',
    # German
    'geschäftsbericht', 'berichte', 'veröffentlichungen', 'publikationen', 'finanzberichte',
    # French
    'rapports financiers', 'publications financières', 'documents financiers',
    # Dutch
    'jaarverslag', 'publicaties', 'financiële publicaties',
    # Italian/Spanish
    'bilancio', 'relazioni', 'publicaciones', 'informes',
]

# Domains known to aggregate/republish filings — skip these when looking for IR pages
BLOCKED_DOMAINS = [
    # Search engines / social
    'google.com', 'google.co', 'about.google', 'bing.com',
    'linkedin.com', 'twitter.com', 'facebook.com',
    'reddit.com', 'youtube.com',
    # Document aggregators — NEVER the company's own filing
    'scribd.com', 'slideshare.net', 'issuu.com', 'academia.edu',
    'annualreports.com', 'annualreportservice.com', 'reportlinker.com',
    'slideboxx.com', 'yumpu.com', 'docplayer.net', 'calameo.com',
    # Financial data vendors (not IR pages)
    'seekingalpha.com', 'yahoo.com', 'reuters.com', 'bloomberg.com',
    'wsj.com', 'ft.com', 'macrotrends.net', 'marketwatch.com',
    'investing.com', 'simplywallst.com', 'wisesheets.io', 'stockanalysis.com',
    'marketscreener.com', 'zonebourse.com', 'boerse.de', 'finanzen.net',
    'spglobal.com', 'moodys.com', 'fitchratings.com',
    # Nordic/European document aggregators (not the company's own IR)
    'millistream.com', 'cision.com', 'mb.cision.com',
    'huginonline.com', 'newswire.ca', 'accesswire.com',
    # General encyclopedias / news
    'wikipedia.org', 'businesswire.com', 'prnewswire.com', 'globenewswire.com',
]


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

    # EBITDA / EBITA hierarchy (preference order)
    adjusted_ebitda: Optional[float] = None   # Tier 1: Company-reported adjusted (one-offs removed)
    reported_ebitda: Optional[float] = None   # Tier 2: Company-reported EBITDA
    ebita: Optional[float] = None             # EBIT + amortisation of intangibles (IFRS KPI)
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

    # Debt components (used to sum total_debt when no explicit total in filing)
    current_borrowings: Optional[float] = None
    noncurrent_borrowings: Optional[float] = None

    # Metadata
    currency: str = ""
    accounting_standard: str = ""  # IFRS or US GAAP
    source_sections: dict = field(default_factory=dict)
    extraction_confidence: dict = field(default_factory=dict)
    raw_snippets: dict = field(default_factory=dict)
    # Maps field_name → PDF URL that was the source of that value
    field_sources: dict = field(default_factory=dict)


class BrowserPipeline:
    """Automated browser research pipeline for company filings."""

    def __init__(self):
        self._session: Optional[BrowserSession] = None
        self._llm = LLMNavigator()
        self._last_doc = None   # best-available doc from failed validations
        self._last_text = None  # corresponding text

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

    async def _dismiss_cookie_popup(self, session=None) -> bool:
        """
        Dismiss cookie consent popups.
        Takes before/after screenshots for visibility.
        Handles: OneTrust, TrustArc, Cookiebot (iframe), generic banners.
        """
        s = session or self.session
        tab = s.default_page
        if not tab:
            return False

        await self._take_screenshot("cookie-before", session=s)

        dismissed = False

        # Pass 1: main document — OneTrust, TrustArc, generic buttons
        try:
            result = await tab.evaluate("""(() => {
                const labels = [
                    'Accept All', 'Accept All Cookies', 'Accept Cookies',
                    'Accept and Continue', 'Accept & Continue', 'I Accept',
                    'Allow All', 'Allow Cookies', 'Accept', 'Agree', 'OK',
                    'Akzeptieren', 'Akzeptieren Alle', 'Zustimmen',
                    'Accepter', 'Tout accepter', 'Aceptar', 'Accetta', 'Accetta tutti'
                ];
                // OneTrust (most common on corporate IR sites)
                const ot = document.getElementById('onetrust-accept-btn-handler');
                if (ot && ot.offsetParent !== null) { ot.click(); return 'onetrust'; }

                // TrustArc
                const ta = document.querySelector(
                    '#truste-consent-button, .truste-button.pdynamicbutton, ' +
                    '[id*="truste"] button, [class*="trustarc"] button'
                );
                if (ta && ta.offsetParent !== null) { ta.click(); return 'trustarc'; }

                // CookieYes / Complianz
                const cy = document.querySelector(
                    '.cky-btn-accept, .cmplz-btn.cmplz-accept, ' +
                    '[data-cky-tag="accept-button"], [class*="cookie-accept"]'
                );
                if (cy && cy.offsetParent !== null) { cy.click(); return 'cookieyes'; }

                // Generic: button or link whose text matches accept labels
                const els = Array.from(document.querySelectorAll(
                    'button, a[role="button"], a[class*="accept"], a[class*="cookie"], ' +
                    '[class*="consent"] button, [id*="cookie"] button, [id*="consent"] button, ' +
                    '[class*="gdpr"] button, [id*="gdpr"] button'
                ));
                for (const el of els) {
                    const t = el.textContent.trim();
                    if (labels.some(l => t === l || t.startsWith(l))) {
                        if (el.offsetParent !== null) { el.click(); return t; }
                    }
                }
                return null;
            })()""")
            if result:
                logger.info(f"Cookie dismissed (main doc): {result}")
                dismissed = True
        except Exception:
            pass

        # Pass 2: iframe-based banners (Cookiebot, TrustArc iframe, IAB)
        # Same-origin iframes only — cross-origin iframes raise security error
        if not dismissed:
            try:
                iframe_result = await tab.evaluate("""(() => {
                    const frameSelectors = [
                        'iframe[src*="cookiebot"]', 'iframe[src*="consent"]',
                        'iframe[src*="trustarc"]', 'iframe[src*="quantcast"]',
                        'iframe[id*="cookie"]',    'iframe[id*="consent"]',
                        'iframe[name*="cookie"]',  'iframe[title*="cookie" i]',
                        'iframe[title*="consent" i]', 'iframe[title*="privacy" i]'
                    ];
                    const labels = [
                        'Accept', 'Accept All', 'Allow', 'Allow All', 'OK',
                        'Agree', 'I Accept', 'Continue', 'Akzeptieren', 'Accepter'
                    ];
                    for (const sel of frameSelectors) {
                        const frame = document.querySelector(sel);
                        if (!frame) continue;
                        try {
                            const doc = frame.contentDocument || frame.contentWindow.document;
                            if (!doc) continue;
                            const btns = Array.from(doc.querySelectorAll(
                                'button, a[role="button"], [class*="accept"]'
                            ));
                            for (const btn of btns) {
                                const t = btn.textContent.trim();
                                if (labels.some(l => t === l || t.startsWith(l))) {
                                    btn.click();
                                    return 'iframe:' + t;
                                }
                            }
                        } catch(e) { /* cross-origin — skip */ }
                    }
                    return null;
                })()""")
                if iframe_result:
                    logger.info(f"Cookie dismissed (iframe): {iframe_result}")
                    dismissed = True
            except Exception:
                pass

        if dismissed:
            await asyncio.sleep(random.uniform(0.5, 1.2))
            await self._take_screenshot("cookie-after", session=s)
            return dismissed

        # Pass 3: LLM visual fallback — screenshot → Claude identifies exact button text
        snap = await self._take_screenshot("cookie-llm-check", session=s)
        if snap:
            btn_text = await self._llm.decide_cookie_action(snap)
            if btn_text:
                try:
                    escaped = btn_text.replace("'", "\\'")
                    result = await tab.evaluate(f"""(() => {{
                        const els = Array.from(document.querySelectorAll(
                            'button, a[role="button"], [class*="accept"], [class*="cookie"]'
                        ));
                        for (const el of els) {{
                            if (el.textContent.trim() === '{escaped}' && el.offsetParent !== null) {{
                                el.click(); return '{escaped}';
                            }}
                        }}
                        // partial match fallback
                        for (const el of els) {{
                            if (el.textContent.trim().includes('{escaped}') && el.offsetParent !== null) {{
                                el.click(); return el.textContent.trim();
                            }}
                        }}
                        return null;
                    }})()""")
                    if result:
                        logger.info(f"Cookie dismissed (LLM visual): {result}")
                        dismissed = True
                        await asyncio.sleep(random.uniform(0.5, 1.0))
                        await self._take_screenshot("cookie-after", session=s)
                except Exception:
                    pass

        return dismissed

    async def _human_scroll(self, session=None):
        """Simulate human reading: scroll down in segments with pauses."""
        s = session or self.session
        tab = s.default_page
        if not tab:
            return
        try:
            segments = random.randint(3, 5)
            for _ in range(segments):
                px = random.randint(200, 700)
                await tab.scroll_down(px)
                await asyncio.sleep(random.uniform(0.5, 2.0))
            if random.random() < 0.3:
                await tab.scroll_up(random.randint(100, 300))
                await asyncio.sleep(random.uniform(0.5, 1.5))
        except Exception:
            pass

    async def _take_screenshot(self, label: str = "", session=None) -> Optional[str]:
        """Capture page screenshot for debug visibility. Returns saved path."""
        s = session or self.session
        tab = s.default_page
        if not tab:
            return None
        try:
            import time
            screenshots_dir = os.path.join(
                os.path.dirname(os.path.dirname(os.path.dirname(
                    os.path.abspath(__file__)))),
                "screenshots"
            )
            os.makedirs(screenshots_dir, exist_ok=True)
            fname = f"{label}_{int(time.time())}.png" if label else f"snap_{int(time.time())}.png"
            path = os.path.join(screenshots_dir, fname)
            await tab.save_screenshot(path)
            logger.info(f"Screenshot saved: {path}")
            return path
        except Exception as e:
            logger.debug(f"Screenshot failed: {e}")
            return None

    async def _wait_for_js_render(self, min_links: int = 8, timeout: float = 10.0, session=None):
        """Wait for JS to render page by polling anchor count until stable."""
        s = session or self.session
        tab = s.default_page
        if not tab:
            return
        loop = asyncio.get_event_loop()
        deadline = loop.time() + timeout
        prev_count = 0
        stable_ticks = 0
        while loop.time() < deadline:
            try:
                raw = await tab.evaluate("document.querySelectorAll('a[href]').length")
                count = int(raw) if raw is not None else 0
                if count >= min_links:
                    if count == prev_count:
                        stable_ticks += 1
                        if stable_ticks >= 2:
                            return
                    else:
                        stable_ticks = 0
                prev_count = count
            except Exception:
                pass
            await asyncio.sleep(0.8)

    # --- STEP 0: Latest interim/quarterly report ---

    async def _find_latest_interim(
        self, company: str, year: str, jurisdiction: dict, session=None
    ) -> Optional[str]:
        """
        Search for the most recent quarterly or semi-annual report.
        Returns PDF URL or None. Used to get fresh balance sheet data.
        """
        s = session or self.session
        nav = BrowserNav(s)
        # Derive the current and prior year for interim search
        try:
            yr = int(year)
        except ValueError:
            yr = 2025
        next_yr = yr + 1

        # Build query targeting interim/quarterly reports
        local_terms = jurisdiction.get('local_query', 'annual report')
        interim_terms = (
            f'("{company}" "interim report" OR "quarterly report" OR "half-year report" '
            f'OR "Q1" OR "Q3" OR "six months" filetype:pdf) '
            f'({next_yr} OR {yr})'
        )
        logger.info(f"Interim search: {company} latest quarterly/interim")
        try:
            await nav.google_search(interim_terms)
            await asyncio.sleep(_human_delay(1, 3))
            tab = s.default_page
            if not tab:
                return None
            blocked = json.dumps(BLOCKED_DOMAINS)
            raw = await tab.evaluate(f"""JSON.stringify((() => {{
                const blocked = {blocked};
                return Array.from(document.querySelectorAll('a[href]'))
                    .filter(a => {{
                        const h = a.href.toLowerCase();
                        return h.includes('.pdf') &&
                               !blocked.some(b => h.includes(b));
                    }})
                    .map(a => ({{ href: a.href, text: a.textContent.trim() }}))
                    .slice(0, 15);
            }})())""")
            candidates = json.loads(raw) if raw else []
            if candidates:
                href = await self._llm.select_latest_filing(candidates, company, yr)
                if href:
                    logger.info(f"Interim candidate: {href[:100]}")
                    return href
        except Exception as e:
            logger.debug(f"Interim search failed: {e}")
        return None

    def _overlay_interim_bs(
        self, annual: "ExtractedFinancials", interim: "ExtractedFinancials"
    ) -> "ExtractedFinancials":
        """
        Override balance sheet items in annual financials with fresher interim data.
        Income statement items (revenue, EBITDA) stay from the annual report.
        """
        bs_fields = [
            'total_assets', 'total_equity', 'total_debt', 'cash',
            'goodwill', 'short_term_investments',
            'minority_interest', 'preferred_stock',
            'equity_investments', 'financial_investments',
            'assets_held_for_sale', 'discontinued_ops_assets', 'nol_dta',
            'pension_pbo', 'pension_plan_assets',
            'operating_lease_liabilities', 'finance_lease_liabilities',
            'rou_assets', 'lease_liabilities_current', 'lease_liabilities_noncurrent',
            'rou_depreciation', 'lease_interest',
        ]
        for f in bs_fields:
            interim_val = getattr(interim, f, None)
            if interim_val is not None:
                setattr(annual, f, interim_val)
                # Track that this field's source is the interim filing, not the annual
                if interim.field_sources.get(f):
                    annual.field_sources[f] = interim.field_sources[f]
        # Preserve income statement from annual
        return annual

    # --- STEP 1: Find annual report ---

    async def _find_via_regulator(self, company: str, year: str,
                                  jurisdiction: dict, session=None) -> Optional[str]:
        """
        Search the official exchange/regulator filing database.
        Only applies to jurisdictions with well-indexed public filing databases.
        Returns a PDF URL directly from the authoritative source.
        """
        s = session or self.session
        suffix = jurisdiction.get('ticker_suffix', '')
        regulator = REGULATOR_SITES.get(suffix)
        if not regulator:
            return None

        nav = BrowserNav(s)
        query = f'"{company}" "annual report" {year} site:{regulator}'
        logger.info(f"Regulator search [{regulator}]: {query}")
        await nav.google_search(query)
        await asyncio.sleep(_human_delay(2, 4))

        tab = s.default_page
        if not tab:
            return None

        raw = await tab.evaluate(f"""JSON.stringify((() => {{
            const reg = '{regulator}';
            return Array.from(document.querySelectorAll('a[href]'))
                .map(a => a.href)
                .filter(href => {{
                    const h = href.toLowerCase();
                    return h.includes(reg) &&
                           (h.includes('.pdf') || h.includes('download') ||
                            h.includes('annual') || h.includes('annualreport'));
                }})
                .slice(0, 5);
        }})())""")
        candidates = json.loads(raw) if raw else []

        if candidates:
            logger.info(f"Regulator candidate: {candidates[0][:100]}")
            return candidates[0]
        return None

    async def _find_via_ir_page(self, company: str, year: str, session=None) -> Optional[str]:
        """
        Find the company's official IR page via Google → navigate → text-match PDF.

        Google returns the company's own investor relations page as the top result.
        We navigate there and find the link explicitly labelled 'Annual Report {year}'.
        No URL guessing — the IR page itself tells us which document is which.
        """
        s = session or self.session
        nav = BrowserNav(s)

        # Strip legal form suffixes that over-constrain Google results
        import re as _re2
        search_name = _re2.sub(
            r'\s*\((?:publ|plc|ag|sa|nv|bv|ab|oy|asa|as|se|inc|ltd|llc|corp|gmbh|kg)\)\.?$',
            '', company, flags=_re2.IGNORECASE
        ).strip()
        query = f'"{search_name}" "investor relations" "annual report" {year}'
        logger.info(f"IR page search: {query}")
        await nav.google_search(query)
        await asyncio.sleep(_human_delay(2, 4))

        tab = s.default_page
        if not tab:
            return None

        blocked = json.dumps(BLOCKED_DOMAINS)
        raw = await tab.evaluate(f"""JSON.stringify((() => {{
            const blocked = {blocked};
            // Paths that strongly indicate an annual reports section or IR landing page
            const goodPaths = ['annual-report', 'annual-reports', 'publications-and-reports',
                               'reports-and-publications', 'investor-publications',
                               'financial-reports', 'yearly-report',
                               'investor-relations', 'investors/reports', 'ir/reports'];
            // Paths that indicate wrong page type or document-server paths (not IR pages)
            const badPaths = ['ad-hoc', 'press-release', 'press-releases', 'news',
                              'media', 'regulatory', 'agm', 'governance',
                              'sustainability', 'esg', 'csr',
                              'content/dam', '/dam/', 'cdn.', 'assets.'];

            const links = Array.from(document.querySelectorAll('a[href]'))
                .map(a => a.href)
                .filter(href => {{
                    const h = href.toLowerCase().split('#')[0].split('?')[0];
                    return href.startsWith('http') &&
                        !blocked.some(b => h.includes(b)) &&
                        !h.endsWith('.pdf') &&
                        !h.includes('.pdf/');
                }});

            // Score and sort: prefer annual-report paths, penalise bad paths
            const scored = links.map(href => {{
                const h = href.toLowerCase();
                let score = 0;
                if (goodPaths.some(p => h.includes(p))) score += 10;
                if (badPaths.some(p => h.includes(p))) score -= 10;
                if (h.includes('investor')) score += 2;
                return {{ href, score }};
            }});
            scored.sort((a, b) => b.score - a.score);
            return scored.slice(0, 8).map(x => x.href);
        }})())""")
        ir_candidates = json.loads(raw) if raw else []
        logger.info(f"IR page candidates: {[u[:80] for u in ir_candidates[:4]]}")

        for ir_url in ir_candidates[:5]:
            pdf_url = await self._extract_pdf_from_ir_page(ir_url, year, session=s)
            if pdf_url:
                return pdf_url

        return None

    async def _extract_pdf_from_ir_page(self, ir_url: str, year: str, session=None) -> Optional[str]:
        """
        Navigate to an IR page and find the annual report PDF by text matching.

        Two-pass:
        Pass 1 — look for a direct PDF link on the current page where the LINK TEXT
                  contains both the year AND an annual report keyword.
        Pass 2 — if nothing found, follow any navigation link whose text says
                  "Annual Reports" / "Publications" one level deeper, then repeat.

        Requiring the year + annual keyword in the link text itself (not parent)
        prevents matching "Annual Financial Statements" (standalone HGB/statutory)
        or "Half-Year Report".
        """
        s = session or self.session
        try:
            logger.info(f"Navigating IR page: {ir_url[:100]}")
            await asyncio.wait_for(s.goto(ir_url), timeout=30)
            await self._wait_for_js_render(min_links=30, session=s)  # AEM sites load in phases
            await asyncio.sleep(_human_delay(3, 5))       # extra wait for PDF section
            await self._dismiss_cookie_popup(session=s)
            await self._take_screenshot("ir-page", session=s)
            await self._human_scroll(session=s)

            # Pass 1: look for PDF directly on this page
            pdf = await self._scan_page_for_annual_report_pdf(year, session=s)
            if pdf:
                return pdf

            # Pass 2: follow annual-reports section navigation link (one level deeper)
            tab = s.default_page
            if not tab:
                return None

            subnav_kw = json.dumps(SUBNAV_KEYWORDS)
            nav_raw = await tab.evaluate(f"""JSON.stringify((() => {{
                const subnavKw = {subnav_kw};
                const navBad = ['half-year', 'interim', 'quarterly', 'sustainability',
                                'esg', 'governance', 'agm', 'shareholder-meeting'];
                return Array.from(document.querySelectorAll('a[href]'))
                    .filter(a => {{
                        const t = a.textContent.trim().toLowerCase();
                        const h = a.href.toLowerCase();
                        return subnavKw.some(k => t.includes(k)) &&
                               !h.endsWith('.pdf') &&
                               !navBad.some(b => h.includes(b) || t.includes(b));
                    }})
                    .map(a => ({{ href: a.href, text: a.textContent.trim() }}))
                    .slice(0, 6);
            }})())""")
            nav_links = json.loads(nav_raw) if nav_raw else []
            logger.info(f"Sub-nav candidates: {[(r.get('text','')[:40], r.get('href','')[:60]) for r in nav_links]}")

            # LLM fallback: if keyword scan found nothing, collect all links → LLM picks section
            if not nav_links:
                all_raw = await tab.evaluate("""JSON.stringify(
                    Array.from(document.querySelectorAll('a[href]'))
                        .filter(a => a.href.startsWith('http') && !a.href.endsWith('.pdf'))
                        .map(a => ({href: a.href, text: a.textContent.trim()}))
                        .slice(0, 40)
                )""")
                all_links = json.loads(all_raw) if all_raw else []
                company_hint = getattr(self, '_current_company', '')
                llm_url = await self._llm.find_reports_section(all_links, company_hint, year)
                if llm_url:
                    nav_links = [{'href': llm_url, 'text': 'LLM-selected section'}]

            visited = {ir_url}
            for nav in nav_links[:3]:
                sub_url = nav.get('href', '')
                if not sub_url or sub_url in visited:
                    continue
                visited.add(sub_url)
                try:
                    logger.info(f"Following sub-nav: '{nav.get('text','')[:50]}' → {sub_url[:80]}")
                    await asyncio.wait_for(s.goto(sub_url), timeout=30)
                    await self._wait_for_js_render(min_links=30, session=s)
                    await asyncio.sleep(_human_delay(3, 5))
                    await self._dismiss_cookie_popup(session=s)
                    await self._take_screenshot(f"subnav-{len(visited)}", session=s)
                    pdf = await self._scan_page_for_annual_report_pdf(year, session=s)
                    if pdf:
                        return pdf

                    # Pass 3: one level deeper from sub-nav (e.g. legal notice redirect)
                    tab2 = s.default_page
                    if tab2:
                        deep_raw = await tab2.evaluate(f"""JSON.stringify((() => {{
                            const subnavKw = {subnav_kw};
                            const navBad = ['half-year', 'interim', 'quarterly',
                                            'sustainability', 'esg', 'governance'];
                            return Array.from(document.querySelectorAll('a[href]'))
                                .filter(a => {{
                                    const t = a.textContent.trim().toLowerCase();
                                    const h = a.href.toLowerCase();
                                    return subnavKw.some(k => t.includes(k)) &&
                                           !h.endsWith('.pdf') &&
                                           !navBad.some(b => h.includes(b) || t.includes(b));
                                }})
                                .map(a => ({{ href: a.href, text: a.textContent.trim() }}))
                                .slice(0, 4);
                        }})())""")
                        deep_links = json.loads(deep_raw) if deep_raw else []
                        for deep in deep_links[:2]:
                            deep_url = deep.get('href', '')
                            if not deep_url or deep_url in visited:
                                continue
                            visited.add(deep_url)
                            try:
                                logger.info(f"Deep nav: '{deep.get('text','')[:40]}' → {deep_url[:80]}")
                                await asyncio.wait_for(s.goto(deep_url), timeout=30)
                                await self._wait_for_js_render(min_links=30, session=s)
                                await asyncio.sleep(_human_delay(3, 5))
                                await self._take_screenshot(f"deepnav-{len(visited)}", session=s)
                                pdf = await self._scan_page_for_annual_report_pdf(year, session=s)
                                if pdf:
                                    return pdf
                            except Exception as e2:
                                logger.info(f"Deep nav failed {deep_url[:60]}: {e2}")
                except Exception as e:
                    logger.info(f"Sub-nav failed {sub_url[:60]}: {e}")

        except Exception as e:
            logger.info(f"IR page failed {ir_url[:80]}: {e}")

        return None

    async def _scan_page_for_annual_report_pdf(self, year: str, session=None) -> Optional[str]:
        """
        Scan current page for a link whose text says '{year} Annual Report'.
        Link text must contain BOTH year AND annual keyword — no parent context.
        Excludes half-year reports, statutory accounts, sustainability reports.
        """
        s = session or self.session
        tab = s.default_page
        if not tab:
            return None

        annual_kw = json.dumps(ANNUAL_KEYWORDS)
        raw = await tab.evaluate(f"""JSON.stringify((() => {{
            const year = '{year}';
            const annualKw = {annual_kw};
            const exclude = [
                'half-year', 'half year', 'interim', 'quarterly',
                'q1 ', 'q2 ', 'q3 ', 'q4 ',
                'financial statements', 'statutory accounts',
                'sustainability', 'esg report', 'proxy',
                'ad hoc', 'ad-hoc', 'press release', 'remuneration'
            ];

            return Array.from(document.querySelectorAll('a[href]'))
                .filter(a => {{
                    const t = a.textContent.trim().toLowerCase();
                    const h = a.href.toLowerCase();
                    const hasYear = t.includes(year) || h.includes(year);
                    const isAnnual = annualKw.some(k => t.includes(k));
                    const isExcluded = exclude.some(e => t.includes(e));
                    const isPdf = h.includes('.pdf') || h.includes('download') ||
                                  !!a.getAttribute('download');
                    return hasYear && isAnnual && !isExcluded && isPdf;
                }})
                .map(a => ({{ href: a.href, text: a.textContent.trim() }}))
                .slice(0, 5);
        }})())""")

        results = json.loads(raw) if raw else []

        # Fast path: single result — return it directly (no LLM needed)
        if len(results) == 1:
            href = results[0].get('href', '')
            logger.info(f"PDF found (sole match): '{results[0].get('text','')[:70]}' → {href[:100]}")
            return href

        # Multiple results — LLM picks the right one
        if len(results) > 1:
            company_hint = getattr(self, '_current_company', '')
            href = await self._llm.select_annual_report_link(results, company_hint, year)
            if href:
                return href
            # LLM returned None — fall back to first .pdf link
            for r in results:
                if '.pdf' in r.get('href', '').lower():
                    return r.get('href', '')

        # No results from strict filter — broaden search and ask LLM
        broad_raw = await tab.evaluate(f"""JSON.stringify((() => {{
            return Array.from(document.querySelectorAll('a[href]'))
                .filter(a => {{
                    const h = a.href.toLowerCase();
                    return h.includes('.pdf') || h.includes('download') ||
                           !!a.getAttribute('download');
                }})
                .map(a => ({{ href: a.href, text: a.textContent.trim() }}))
                .slice(0, 20);
        }})())""")
        broad = json.loads(broad_raw) if broad_raw else []
        if broad:
            company_hint = getattr(self, '_current_company', '')
            href = await self._llm.select_annual_report_link(broad, company_hint, year)
            if href:
                logger.info(f"PDF found via LLM broad scan → {href[:100]}")
                return href

        return None


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
        # Normalize thousand separators so regex patterns work across locales:
        #     = narrow no-break space  (Swedish, French: "202 454")
        #     = non-breaking space     (some PDFs)
        #     = thin space             (some typeset PDFs)
        # Replace numeric-context spaces with comma so "202 454" → "202,454"
        import re as _re
        text = _re.sub(r'(\d)[     ](\d{3})(?!\d)', r'\1,\2', text)
        text = _re.sub(r'(\d)[     ](\d{3})(?!\d)', r'\1,\2', text)  # 2nd pass for 1 234 567
        # Also normalise em-dash used as minus in some Nordic reports
        text = text.replace('−', '-')
        doc.total_chars = len(text)
        return text

    def is_annual_report(self, doc: FilingDocument, text: str = None) -> bool:
        """Alias kept for backwards compatibility — calls is_valid_filing."""
        return self.is_valid_filing(doc, text)

    def is_valid_filing(self, doc: FilingDocument, text: str = None) -> bool:
        """
        Validate the PDF contains a balance sheet (annual OR quarterly/interim).
        Annual reports: 40+ pages.
        Quarterly/interim reports: 8+ pages (earnings releases are short but have BS).
        Rejects: press releases with no financials, marketing PDFs, sustainability-only.
        """
        if text is None:
            text = self.extract_text(doc)
        tl = text.lower()

        # Must have a balance sheet — the minimum for EV bridge
        has_balance_sheet = any(kw in tl for kw in [
            'balance sheet', 'statement of financial position',
            'total assets', 'total equity', 'balansräkning',   # Swedish
            'bilanz', 'bilanzsumme',                            # German
            'bilan',                                            # French
        ])
        if not has_balance_sheet:
            return False

        # Annual / full-year reports need more pages
        is_quarterly_or_interim = any(kw in tl for kw in [
            'interim report', 'quarterly report', 'half-year report',
            'half year report', 'q1 ', 'q2 ', 'q3 ', 'first quarter',
            'second quarter', 'third quarter', 'six months',
            'delårsrapport', 'kvartalsrapport',    # Swedish
            'zwischenbericht', 'quartalsbericht',  # German
        ])

        min_pages = 8 if is_quarterly_or_interim else 40
        if doc.total_pages < min_pages:
            return False

        return True

    def filing_type(self, text: str) -> str:
        """Classify filing as 'annual', 'quarterly', 'semi-annual', or 'unknown'."""
        tl = text.lower()
        if any(k in tl for k in ['annual report', 'full year', 'full-year', 'geschäftsbericht',
                                  'årsredovisning', 'jaarverslag', 'rapport annuel']):
            return 'annual'
        if any(k in tl for k in ['q1 ', 'q3 ', 'first quarter', 'third quarter',
                                  'nine months', 'kvartalsrapport', 'quartalsbericht']):
            return 'quarterly'
        if any(k in tl for k in ['half-year', 'half year', 'six months', 'interim report',
                                  'h1 ', 'delårsrapport', 'zwischenbericht']):
            return 'semi-annual'
        return 'unknown'

    # --- STEP 4: Extract financial data ---

    def extract_financials(self, text: str, company: str = "",
                           year: str = "", pdf_url: str = "") -> ExtractedFinancials:
        """Extract structured financial data from annual report text."""
        fin = ExtractedFinancials(company=company, year=year)

        # Detect accounting standard
        if 'IFRS' in text[:50000] or 'ifrs' in text[:50000].lower():
            fin.accounting_standard = "IFRS"
        elif 'US GAAP' in text[:50000] or 'GAAP' in text[:50000]:
            fin.accounting_standard = "US GAAP"

        # Detect currency
        for curr, symbols in [("SEK", ["MSEK", "BSEK", "SEK", "Swedish krona", "kronor"]),
                              ("EUR", ["€", "EUR", "euro", "MEUR"]),
                              ("USD", ["$", "USD", "dollar"]),
                              ("GBP", ["£", "GBP", "sterling"]),
                              ("DKK", ["MDKK", "DKK", "Danish krone"]),
                              ("NOK", ["MNOK", "NOK", "Norwegian krone"]),
                              ("CHF", ["MCHF", "CHF", "Swiss franc"]),
                              ("INR", ["₹", "INR", "rupee"])]:
            if any(s in text[:10000] for s in symbols):
                fin.currency = curr
                break

        # --- Extract financial statement line items ---
        # Strategy: find the consolidated income statement and balance sheet,
        # then pull numbers using regex patterns


        # Find section anchors — search within bounded sections to avoid parent-company contamination
        # Use "TOTAL ASSETS" as the anchor for the balance sheet — it's never in the TOC
        # then search BACKWARDS for the section header from that point.
        _ta_pos = max(text.find("TOTAL ASSETS"), text.find("Total assets"))
        if _ta_pos > 0:
            bs_start = max(
                text.rfind("Consolidated balance sheet", 0, _ta_pos),
                text.rfind("Balance sheet", 0, _ta_pos),
            )
        else:
            bs_start = max(
                text.find("Consolidated balance sheet"),
                text.find("Balance sheet"),
            )

        # Income statement anchor: "Revenues\n<note#>\n<value>" distinguishes actual IS
        # from summary tables which have "Revenues\n<value>" directly (no note#).
        _rev_note_m = re.search(r'Revenues?\n\d{1,3}\n\d{1,3}(?:,\d{3})+', text)
        if _rev_note_m:
            _anchor = _rev_note_m.start()
            is_start = max(
                text.rfind("Consolidated income statement", 0, _anchor + 500),
                text.rfind("Income statement", 0, _anchor + 500),
            )
            if is_start < 0:
                is_start = max(0, _anchor - 3000)
        else:
            # Fallback: inline format "Revenue  note#  value  prior" — find the rightmost match
            # (latest in doc = actual IS, not TOC/summary)
            _rev_inline = list(re.finditer(r'Revenue\s+\d+\s+\d{1,3}(?:,\d{3})+', text))
            if _rev_inline:
                _anchor = _rev_inline[-1].start()
                is_start = max(
                    text.rfind("Consolidated income statement", 0, _anchor + 500),
                    text.rfind("Income statement", 0, _anchor + 500),
                )
                if is_start < 0:
                    is_start = max(0, _anchor - 3000)
            else:
                is_start = max(
                    text.find("Consolidated income statement"),
                    text.find("Income statement"),
                    0,
                )
        if is_start < 0:
            is_start = 0

        is_end = bs_start if (bs_start > is_start > 0) else is_start + 30000
        bs_end_markers = ["Consolidated statement of changes in equity", "Statement of changes in equity",
                          "Parent company", "PARENT COMPANY", "Financial statements (Parent)"]
        bs_end = len(text)
        for m in bs_end_markers:
            pos = text.find(m, bs_start + 100) if bs_start > 0 else -1
            if 0 < pos < bs_end:
                bs_end = pos + 5000  # slight buffer to capture trailing totals

        fs_text = text[is_start:is_end] if is_start > 0 else text[:30000]
        bs_text = text[bs_start:bs_end] if bs_start > 0 else text


        # Revenue — patterns handle both note-format and newline-format
        # "Revenues\n3\n168,343" or "Revenue  6  7,039,900"
        fin.revenue = self._extract_amount(fs_text, [
            r'Revenues?\n\d+\n(\d{1,3}(?:,\d{3})+)',                        # Nordic/newline format
            r'Revenue\s+\d+\s+(\d{1,3}(?:,\d{3})+)',                        # Inline with note
            r'Revenues?\s+(\d{1,3}(?:,\d{3}){2,})\s+\d{1,3}(?:,\d{3}){2,}',  # No note
            r'(?:Net\s+)?[Rr]evenue[s]?\s+(\d{1,3}(?:,\d{3}){2,})',        # Simple line
        ], 'income_statement')
        if not fin.revenue:
            fin.revenue = self._extract_amount(text, [
                r'Revenues?\n\d+\n(\d{1,3}(?:,\d{3})+)',
                r'Revenue\s+\d+\s+(\d{1,3}(?:,\d{3})+)',
            ], 'income_statement')

        # Operating income / EBIT
        fin.operating_income = self._extract_amount(fs_text, [
            r'Operating\s+profit\n[^\n]+\n(\d{1,3}(?:,\d{3})+)',            # Nordic: note line then value
            r'Operating\s+profit\n\s*\n(\d{1,3}(?:,\d{3})+)',               # Nordic: blank then value
            r'Operating\s+profit\s{2,}[\d,\s]+?(\d{1,3}(?:,\d{3})+)',       # Inline, ≥2 spaces separator
            r'(?:Operating|Trading)\s+result\n[^\n]+\n(\d{1,3}(?:,\d{3})+)',
            r'(?:Operating|Trading)\s+result\s+.*?(\d{1,3}(?:,\d{3})+)',
            r'Operating\s+income\n[^\n]+\n(\d{1,3}(?:,\d{3})+)',
            r'Operating\s+income\s+.*?(\d{1,3}(?:,\d{3})+)',
            r'Result\s+from\s+operations?\s+.*?(\d{1,3}(?:,\d{3})+)',
            r'EBIT\s+.*?(\d{1,3}(?:,\d{3})+)',
        ], 'income_statement')

        # EBITA (EBIT + amortisation of acquired intangibles — reported IFRS KPI)
        fin.ebita = self._extract_amount(fs_text, [
            r'EBITA\s+\n?\s*(\d{1,3}(?:,\d{3})+)',
            r'EBITA\s{2,}.*?(\d{1,3}(?:,\d{3})+)',
            r'Adjusted\s+EBITA\s+\n?\s*(\d{1,3}(?:,\d{3})+)',
        ], 'income_statement')

        # Net income
        fin.net_income = self._extract_amount(fs_text, [
            r'Profit\s+for\s+the\s+year\n\s*\n(\d{1,3}(?:,\d{3})+)',        # Nordic newline
            r'Profit\s+for\s+the\s+(?:financial\s+)?year\s+.*?(\d{1,3}(?:,\d{3})+)',
            r'Net\s+result\s+.*?(\d{1,3}(?:,\d{3})+)',
            r'Net\s+income\s+.*?(\d{1,3}(?:,\d{3})+)',
            r'Net\s+profit\s+(?:for\s+the\s+year\s+)?.*?(\d{1,3}(?:,\d{3})+)',
        ], 'income_statement')

        # Total assets — from consolidated BS
        fin.total_assets = self._extract_amount(bs_text, [
            r'TOTAL\s+ASSETS\s+\n?\s*(\d{1,3}(?:,\d{3})+)',
            r'Total\s+assets\s+\n?\s*(\d{1,3}(?:,\d{3})+)',
        ], 'balance_sheet')

        # Total equity — from consolidated BS (matches "TOTAL EQUITY" not parent lines)
        fin.total_equity = self._extract_amount(bs_text, [
            r'TOTAL\s+EQUITY\s+\n?\s*(\d{1,3}(?:,\d{3})+)',
            r'Total\s+equity\s+\n?\s*(\d{1,3}(?:,\d{3})+)',
            r'(?:Group\s+)?(?:Total\s+)?[Ee]quity\s+\n?\s*(\d{1,3}(?:,\d{3})+)',
        ], 'balance_sheet')

        # Cash — require commas to avoid matching small note numbers
        # "Cash and cash equivalents\n18\n15,523" — note number is pure digits, value has commas
        fin.cash = self._extract_amount(bs_text, [
            r'Cash\s+and\s+cash\s+equivalents\n\d*\n(\d{1,3}(?:,\d{3})+)',  # Nordic newline
            r'Cash\s+and\s+cash\s+equivalents\s+\d*\s+(\d{1,3}(?:,\d{3})+)',  # Inline note
            r'Cash\s+and\s+cash\s+equivalents\s+(\d{1,3}(?:,\d{3})+)',      # No note
        ], 'balance_sheet')

        # Total debt — prefer explicit aggregate lines; otherwise sum all Borrowings entries
        # (Nordic IFRS has two "Borrowings" lines: non-current + current, both must be summed)
        fin.total_debt = self._extract_amount(bs_text, [
            r'(?:Total\s+)?[Ff]inancial\s+(?:debt|indebtedness)\s+\n?\s*(\d{1,3}(?:,\d{3})+)',
            r'[Tt]otal\s+(?:interest[\s-]bearing\s+)?[Bb]orrowings\s+\n?\s*(\d{1,3}(?:,\d{3})+)',
            r'[Tt]otal\s+[Dd]ebt\s+\n?\s*(\d{1,3}(?:,\d{3})+)',
            r'[Ll]oans\s+and\s+borrowings\s+.*?(\d{1,3}(?:,\d{3})+)',
            r'[Ii]nterest[-\s]bearing\s+(?:debt|liabilities)\s+.*?(\d{1,3}(?:,\d{3})+)',
        ], 'balance_sheet')

        if fin.total_debt is None:
            # Find ALL "Borrowings" entries (Nordic newline format: label\nnote\nvalue)
            all_borrowings_nc = re.findall(r'[Bb]orrowings\n\s*\d+\s*\n(\d{1,3}(?:,\d{3})+)', bs_text)
            all_borrowings_inline = re.findall(r'[Bb]orrowings\s+\d+\s+(\d{1,3}(?:,\d{3})+)', bs_text)
            all_vals = all_borrowings_nc or all_borrowings_inline
            if len(all_vals) >= 2:
                # Sum first two occurrences — typically non-current + current
                fin.total_debt = sum(float(v.replace(',', '')) for v in all_vals[:2])
                logger.debug(f"total_debt summed from {len(all_vals[:2])} Borrowings lines: {fin.total_debt:,.0f}")
            elif all_vals:
                fin.total_debt = float(all_vals[0].replace(',', ''))
            if fin.total_debt is None:
                fin.total_debt = self._extract_amount(bs_text, [
                    r'[Ll]ong[-\s]term\s+debt.{0,40}?(\d{1,3}(?:,\d{3})+)',
                ], 'balance_sheet')

        # Goodwill — from consolidated BS (avoid matching notes/TOC)
        fin.goodwill = self._extract_amount(bs_text, [
            r'Goodwill\n\d+\n(\d{1,3}(?:,\d{3})+)',          # Nordic newline
            r'Goodwill\s+\d+\s+(\d{1,3}(?:,\d{3})+)',        # Inline note
            r'Goodwill\s+(\d{1,3}(?:,\d{3})+)',              # No note
        ], 'balance_sheet')

        # Short-term investments
        fin.short_term_investments = self._extract_amount(bs_text, [
            r'[Ss]hort[-\s]term\s+investments\s+.*?(\d{1,3}(?:,\d{3})+)',
            r'[Mm]arketable\s+securities\s+.*?(\d{1,3}(?:,\d{3})+)',
        ], 'balance_sheet')

        # Adjusted EBITDA (Tier 1 — company-reported, one-offs removed)
        # "Total Group" segment table: "Total Group  7,040  400.3  6,455  333.3"
        # Only valid for Siemens Energy format: decimal margin column (e.g. 400.3) between revenue and prior
        # Group 1=revenue(EUR M), Group 2=adj-EBITDA-margin-or-value(decimal), Group 3=prior-revenue
        tg_match = re.search(
            r'Total\s+Group[\s\S]{0,200}?(\d{1,3}(?:,\d{3})+)\s+(\d{1,3}(?:,\d{3})*\.\d+)\s+(\d{1,3}(?:,\d{3})+)',
            text[:200000], re.IGNORECASE
        )
        if tg_match:
            tg_adj_ebitda = float(tg_match.group(2).replace(',', ''))
            # Siemens Energy Total Group table: col2 is Adj.EBITDA value in EUR millions
            # Only accept plausible EBITDA range (100M–10,000M) for this heuristic
            if 100 < tg_adj_ebitda < 10000:
                fin.adjusted_ebitda = tg_adj_ebitda * 1_000  # EUR millions -> thousands
    

        if not fin.adjusted_ebitda:
            fin.adjusted_ebitda = self._extract_amount(text, [
                r'[Aa]djusted\s+EBITDA.{0,60}?(?:EUR|EUR|of\s+)?\s*(\d+\.?\d*)\s*(?:million|mln)',
                r'[Aa]djusted\s+EBITDA\s+was.{0,40}?(?:EUR|EUR)?\s*(\d+\.?\d*)\s*(?:million|mln)',
                r'[Uu]nderlying\s+EBITDA.{0,30}?(?:EUR|EUR)?\s*(\d+\.?\d*)\s*(?:million|mln)',
            ], 'adjusted_ebitda')

        # Reported EBITDA (Tier 2 — company-reported)
        # Use [^\n] not .* to avoid spanning pages with re.DOTALL
        fin.reported_ebitda = self._extract_amount(text, [
            r'(?:^|\n)\s*EBITDA\s+[^\n]*?(\d{1,3}(?:,\d{3})+)',
            r'[Rr]eported\s+EBITDA[^\n]{0,30}?(\d{1,3}(?:,\d{3})+)',
            r'EBITDA\n[^\n]+\n(\d{1,3}(?:,\d{3})+)',                        # Nordic newline
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

        # D&A total — from cash flow statement (most reliable source)
        # Formats:
        #   "Depreciation, amortization and impairment\n11, 12, 22\n9,529"  (Nordic)
        #   "Depreciation and amortisation  (157,791)"
        fin.depreciation_total = self._extract_amount(text, [
            r'Depreciation,\s*amorti[sz]ation\s+and\s+impairment\n[^\n]+\n(\d{1,3}(?:,\d{3})+)',
            r'Depreciation\s+and\s+amorti[sz]ation\n[^\n]*\n(\d{1,3}(?:,\d{3})+)',
            r'Depreciation\s+and\s+amorti[sz]ation\s+\(?\s*(\d{1,3}(?:,\d{3})+)\s*\)?',
            r'Depreciation,\s*amorti[sz]ation\s+\(?\s*(\d{1,3}(?:,\d{3})+)\s*\)?',
        ], 'income_statement')

        # --- EV bridge balance sheet items ---

        # Minority interest / Non-controlling interest (NCI can be < 1000 — use 'nci' section)
        fin.minority_interest = self._extract_amount(bs_text, [
            r'[Nn]on[-\s]controlling\s+interests?\n\s*\n(\d{1,3}(?:,\d{3})*)',  # Nordic newline
            r'[Nn]on[-\s]controlling\s+interests?\s+\n?\s*(\d{1,3}(?:,\d{3})*)',
            r'[Mm]inority\s+(?:interest|equity)\s+.*?(\d{1,3}(?:,\d{3})*(?:\.\d+)?)',
            r'[Nn]on[-\s]controlling\s+interests?\s+.*?(\d{1,3}(?:,\d{3})*(?:\.\d+)?)',
        ], 'nci')

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
            # Nordic newline format: label then note number then value
            r'[Dd]epreciation.{0,50}right[-\s]of[-\s]use.{0,100}\n[^\n\d]*(\d{1,3}(?:,\d{3})+)',
            # Alternative: "ROU assets" depreciation line
            r'[Rr]ight[-\s]of[-\s]use\s+assets?\s+(?:depreciation|amortisation).{0,80}?(\d{1,3}(?:,\d{3})+)',
            # Nordic: "Depreciation, right-of-use assets" with surrounding whitespace
            r'[Dd]epreciation,?\s+right[-\s]of[-\s]use\s+assets?\s+(\d{1,3}(?:,\d{3})+)',
            r'[Dd]epreciation\s+of\s+right[-\s]of[-\s]use\s+assets?\s+(\d{1,3}(?:,\d{3})+)',
        ], 'lease_note')

        fin.lease_interest = self._extract_amount(text, [
            # Match next non-negative number on same/next line (values may lack comma, e.g. "263")
            r'Interest\s+expense\s+on\s+lease\s+liabilities[^\n]*\n[^\n\d]*(\d{1,3}(?:,\d{3})*)',
            r'Interest\s+expense\s+on\s+lease\s+liabilities[^\n]*?(\d{1,3}(?:,\d{3})+)',
            r'[Ii]nterest.{0,30}lease\s+liabilit[^\n]*\n[^\n\d]*(\d{1,3}(?:,\d{3})*)',
        ], 'finance_note')

        fin.short_term_rent = self._extract_amount(text, [
            r'[Rr]ent\s+expenses?\s+.*?short[-\s]term\s+leases?\s+.*?(\d{1,3}(?:,\d{3})+)',
            r'[Ss]hort[-\s]term\s+lease.{0,80}?(\d{1,3}(?:,\d{3}){2,})',
        ], 'lease_note')

        fin.lease_liabilities_current = self._extract_amount(bs_text, [
            r'[Ll]ease\s+liabilities?\n\d*\n(\d{1,3}(?:,\d{3})+)',           # BS current section
            r'[Cc]urrent\s+lease\s+liabilities?\s+\n?\s*(\d{1,3}(?:,\d{3})+)',
        ], 'balance_sheet')

        fin.lease_liabilities_noncurrent = self._extract_amount(bs_text, [
            r'[Nn]on[-\s]current\s+lease\s+liabilities?\s+\n?\s*(\d{1,3}(?:,\d{3})+)',
        ], 'balance_sheet')

        fin.rou_assets = self._extract_amount(bs_text, [
            r'Right[-\s]of[-\s]use\s+assets?\n\d+\s*\n(\d{1,3}(?:,\d{3})+)',   # Nordic (note may have trailing space)
            r'Right[-\s]of[-\s]use\s+assets?\s+\d+\s+(\d{1,3}(?:,\d{3})+)', # Inline note
            r'Right[-\s]of[-\s]use\s+assets?\s+(\d{1,3}(?:,\d{3})+)',
        ], 'balance_sheet')

        # Tag every extracted field with its source PDF URL for audit trail
        if pdf_url:
            _skip = {'company', 'year', 'currency', 'accounting_standard',
                     'source_sections', 'extraction_confidence', 'raw_snippets', 'field_sources'}
            for fname, fval in fin.__dict__.items():
                if fname not in _skip and fval is not None:
                    fin.field_sources[fname] = pdf_url

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
                    elif section == 'nci':
                        if val >= 0:  # NCI can be zero or very small (0-1B MSEK)
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

    async def _try_pdf(self, pdf_url: str, company: str, year: str) -> Optional[tuple]:
        """Download, validate, and extract a PDF. Stores best-available on rejection."""
        if not pdf_url:
            return None
        logger.info(f"  PDF URL: {pdf_url[:100]}")
        try:
            doc = self.download_pdf(pdf_url, company, year)
            text = self.extract_text(doc)
            logger.info(f"Downloaded: {doc.total_pages}p, {doc.total_chars:,}c")

            if not self.is_valid_filing(doc, text):
                self._last_doc, self._last_text = doc, text
                logger.warning(f"Heuristic rejected ({doc.total_pages}p)")
                return None

            correct, reason = await self._llm.verify_document(text[:700], company, year)
            if not correct:
                self._last_doc, self._last_text = doc, text
                logger.warning(f"LLM rejected: {reason}")
                return None

            doc.source = self.filing_type(text)
            return doc, self.extract_financials(text, company, year, pdf_url=doc.pdf_url)
        except Exception as e:
            logger.warning(f"Download failed: {e}")
        return None

    async def _resolve_results(self, interim_url: Optional[str],
                               annual_urls: list, company: str, year: str) -> Optional[tuple]:
        """Try annual URLs; overlay interim BS if found."""
        annual_result = None
        for url in filter(None, annual_urls):
            annual_result = await self._try_pdf(url, company, year)
            if annual_result:
                break

        if annual_result:
            doc, fin = annual_result
            if interim_url:
                interim_result = await self._try_pdf(interim_url, company, year)
                if interim_result:
                    fin = self._overlay_interim_bs(fin, interim_result[1])
                    logger.info("BS overlaid with latest interim data")
            return doc, fin

        if interim_url:
            return await self._try_pdf(interim_url, company, year)

        return None

    async def _phase1_nodriver_parallel(self, company: str, year: str,
                                        jurisdiction: dict) -> list:
        """
        Phase 1: 3 concurrent nodriver tabs — fastest path, one Chrome process.
        Returns [interim_url, regulator_url, ir_url] (any may be None).
        """
        await self._ensure_browser()
        ts1 = await self.session.get_isolated_tab()
        ts2 = await self.session.get_isolated_tab()
        ts3 = await self.session.get_isolated_tab()

        results = await asyncio.gather(
            self._find_latest_interim(company, year, jurisdiction, session=ts1),
            self._find_via_regulator(company, year, jurisdiction, session=ts2),
            self._find_via_ir_page(company, year, session=ts3),
            return_exceptions=True,
        )
        return [r if not isinstance(r, Exception) else None for r in results]

    async def _phase2_actionbook_parallel(self, company: str, year: str,
                                          jurisdiction: dict) -> Optional[tuple]:
        """
        Phase 2: Actionbook extension — real Chrome profile, anti-bot bypass.
        Fires 3 Google searches into 3 tabs concurrently, extracts PDF candidates
        from snapshots (which preserve href attributes unlike text output).
        """
        from urllib.parse import quote as _q
        import time as _t

        session_id = f"fm_{int(_t.time())}"

        import shutil as _shutil
        _ab_bin = (
            _shutil.which("actionbook")
            or _shutil.which("actionbook.cmd")
            or r"C:\Users\vinit\AppData\Roaming\npm\actionbook.cmd"
        )

        async def ab(*args) -> str:
            proc = await asyncio.create_subprocess_exec(
                _ab_bin, *args,
                stdout=asyncio.subprocess.PIPE,
                stderr=asyncio.subprocess.PIPE,
            )
            out, _ = await proc.communicate()
            return out.decode(errors="replace")

        yr = int(year)
        queries = {
            "t1": f'"{company}" interim quarterly "half-year" report {yr} OR {yr + 1} filetype:pdf',
            "t2": f'"{company}" annual report {year} filetype:pdf',
            "t3": f'"{company}" investor relations annual report {year}',
        }
        search_url = lambda q: f"https://www.google.com/search?q={_q(q)}"

        try:
            # extension mode requires --open-url to create the first tab (t1)
            await ab("browser", "start", "--set-session-id", session_id,
                     "--mode", "extension", "--open-url", search_url(queries["t1"]))

            # Open t2 and t3 concurrently (goto to a new tab ID creates it)
            await asyncio.gather(
                ab("browser", "goto", search_url(queries["t2"]),
                   "--session", session_id, "--tab", "t2"),
                ab("browser", "goto", search_url(queries["t3"]),
                   "--session", session_id, "--tab", "t3"),
            )
            await asyncio.sleep(3)

            # Snapshot all 3 tabs concurrently — snapshots include href attributes
            snaps = await asyncio.gather(*[
                ab("browser", "snapshot", "--session", session_id, "--tab", tab)
                for tab in ("t1", "t2", "t3")
            ])

            # href: pattern in snapshot YAML captures all link targets including PDFs
            href_re = re.compile(r'href:\s*(https?://[^\s]+)', re.IGNORECASE)
            pdf_re  = re.compile(r'https?://[^\s"\'<>]+\.pdf(?:\?[^\s"\'<>]*)?', re.IGNORECASE)

            for snap in snaps:
                # Collect all PDF URLs from hrefs in snapshot
                hrefs = href_re.findall(snap)
                pdf_urls = [u for u in hrefs if pdf_re.match(u)]
                # Also catch any bare PDF URLs in the snapshot text
                pdf_urls += pdf_re.findall(snap)
                pdf_urls = list(dict.fromkeys(pdf_urls))[:10]  # dedupe, cap at 10

                if not pdf_urls:
                    continue
                candidates = [{"href": u, "text": u} for u in pdf_urls]
                url = await self._llm.select_annual_report_link(candidates, company, yr)
                if url:
                    result = await self._try_pdf(url, company, year)
                    if result:
                        return result

        except Exception as e:
            logger.warning(f"Actionbook phase 2 failed: {e}")
        finally:
            try:
                await ab("browser", "close", "--session", session_id)
            except Exception:
                pass
        return None

    async def run_full_pipeline(self, company: str, year: str = "2025",
                                country: str = "", ticker: str = "") -> tuple[FilingDocument, ExtractedFinancials]:
        """
        3-phase pipeline: nodriver parallel → Actionbook anti-bot → fallback.
        """
        await self._ensure_browser()
        jurisdiction = _detect_jurisdiction(ticker, company)
        logger.info(f"Pipeline: {company} {year} | jurisdiction: {jurisdiction['country']}")
        self._current_company = company

        # Phase 0: DDG direct PDF search (no browser needed, fastest)
        logger.info("Phase 0: DDG PDF search (fetcher)")
        try:
            from src.fetcher import _find_annual_report_pdf_url
            pdf_url = _find_annual_report_pdf_url(company, ticker or "", int(year))
            if pdf_url:
                logger.info(f"Phase 0 found: {pdf_url[:80]}")
                result = await self._try_pdf(pdf_url, company, year)
                if result:
                    return result
        except Exception as e:
            logger.warning(f"Phase 0 failed: {e}")

        # Phase 1: parallel nodriver — 3 concurrent tabs (fastest)
        logger.info("Phase 1: parallel nodriver (3 tabs)")
        interim_url, regulator_url, ir_url = await self._phase1_nodriver_parallel(
            company, year, jurisdiction
        )
        result = await self._resolve_results(interim_url, [regulator_url, ir_url], company, year)
        if result:
            return result

        # Phase 2: Actionbook extension — real Chrome, anti-bot bypass
        logger.info("Phase 2: Actionbook parallel (real Chrome)")
        result = await self._phase2_actionbook_parallel(company, year, jurisdiction)
        if result:
            return result

        # Best-available fallback
        if self._last_doc and self._last_text:
            logger.warning("All phases failed validation. Returning best available.")
            return self._last_doc, self.extract_financials(
                self._last_text, company, year, pdf_url=self._last_doc.pdf_url
            )

        raise FileNotFoundError(f"Could not find any financial filing for {company} {year}")


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
