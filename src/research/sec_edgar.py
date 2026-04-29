"""
SEC EDGAR XBRL API client.
Direct HTTP access — no browser needed. Much faster, zero bot detection risk.

API: data.sec.gov/api/xbrl/companyfacts/CIK{cik}.json
Legacy: sec.gov/cgi-bin/browse-edgar (for filing index navigation)

Rate limit: 10 requests/second. Requires User-Agent header.
"""

import json
import time
import logging
from dataclasses import dataclass, field
from datetime import date, datetime
from typing import Optional
from urllib.parse import urljoin

import requests

from src.models.company import Company, Financials, Filing, ListingType

logger = logging.getLogger(__name__)

SEC_BASE = "https://data.sec.gov"
SEC_LEGACY = "https://www.sec.gov"
CIK_LOOKUP_URL = "https://www.sec.gov/files/company_tickers.json"
HEADERS = {"User-Agent": "Virtual-Financial-Analyst/1.0 (contact@example.com)"}

# Common XBRL concept mappings for financial extraction
XBRL_CONCEPTS = {
    "revenue": ["Revenues", "RevenueFromContractWithCustomerExcludingAssessedTax",
                 "SalesRevenueNet", "SalesRevenueGoodsNet"],
    "ebit": ["OperatingIncomeLoss"],
    "net_income": ["NetIncomeLoss", "ProfitLoss"],
    "eps_basic": ["EarningsPerShareBasic"],
    "eps_diluted": ["EarningsPerShareDiluted"],
    "total_assets": ["Assets"],
    "total_equity": ["StockholdersEquity", "EquityAttributableToParent"],
    "total_debt": ["LongTermDebtAndCapitalLeaseObligations"],
    "current_debt": ["LongTermDebtCurrent"],
    "long_term_debt": ["LongTermDebtNoncurrent"],
    "cash": ["CashAndCashEquivalentsAtCarryingValue"],
    "short_term_investments": ["ShortTermInvestments"],
    "goodwill": ["Goodwill"],
    "intangibles": ["IntangibleAssetsNetExcludingGoodwill"],
    "shares_outstanding": ["CommonStockSharesOutstanding"],
    "cfo": ["NetCashProvidedByUsedInOperatingActivities"],
    "capex": ["PaymentsToAcquirePropertyPlantAndEquipment"],
}


class SECEdgarClient:
    """Direct HTTP client for SEC EDGAR XBRL API."""

    def __init__(self, rate_limit: float = 0.1):
        self.session = requests.Session()
        self.session.headers.update(HEADERS)
        self.rate_limit = rate_limit
        self._last_request = 0.0
        self._ticker_map: dict[str, dict] = {}

    def _rate_limit(self):
        """Enforce SEC rate limit (10 req/s)."""
        elapsed = time.time() - self._last_request
        if elapsed < self.rate_limit:
            time.sleep(self.rate_limit - elapsed)
        self._last_request = time.time()

    def _get_json(self, url: str) -> dict:
        """GET JSON with rate limiting."""
        self._rate_limit()
        resp = self.session.get(url)
        resp.raise_for_status()
        return resp.json()

    # --- CIK resolution ---

    def _load_ticker_map(self):
        """Load SEC company tickers JSON into memory."""
        if not self._ticker_map:
            data = self._get_json(CIK_LOOKUP_URL)
            self._ticker_map = {v["ticker"]: v for v in data.values()}

    def ticker_to_cik(self, ticker: str) -> str:
        """Convert ticker to zero-padded 10-digit CIK."""
        self._load_ticker_map()
        ticker_upper = ticker.upper().strip()
        if ticker_upper not in self._ticker_map:
            raise ValueError(f"Ticker '{ticker}' not found in SEC company list")
        cik_raw = str(self._ticker_map[ticker_upper]["cik_str"])
        return cik_raw.zfill(10)

    def cik_to_ticker(self, cik: str) -> Optional[str]:
        """Convert CIK to ticker (reverse lookup)."""
        self._load_ticker_map()
        cik_int = int(cik.lstrip("0"))
        for ticker, info in self._ticker_map.items():
            if info["cik_str"] == cik_int:
                return ticker
        return None

    def get_company_name(self, ticker: str) -> str:
        """Get company name from SEC ticker map."""
        self._load_ticker_map()
        ticker_upper = ticker.upper().strip()
        if ticker_upper in self._ticker_map:
            return self._ticker_map[ticker_upper]["title"]
        raise ValueError(f"Ticker '{ticker}' not found")

    # --- Company Facts (XBRL API) ---

    def get_company_facts(self, cik: str) -> dict:
        """
        Fetch ALL XBRL-tagged company facts.
        Returns dict with taxonomy concepts, units, and filing periods.
        """
        url = urljoin(SEC_BASE, f"/api/xbrl/companyfacts/CIK{cik}.json")
        return self._get_json(url)

    def get_submissions(self, cik: str) -> dict:
        """Fetch company filing history (submissions)."""
        url = urljoin(SEC_BASE, f"/submissions/CIK{cik}.json")
        return self._get_json(url)

    # --- Financial Extraction ---

    def _extract_concept(self, facts: dict, concept_names: list[str],
                         unit: str = "USD") -> Optional[float]:
        """Extract most recent value for any matching XBRL concept."""
        for name in concept_names:
            if name in facts:
                data = facts[name]["units"].get(unit, [])
                if data:
                    # Sort by end date descending, take most recent
                    sorted_data = sorted(data, key=lambda x: x.get("end", ""), reverse=True)
                    return sorted_data[0].get("val")
        return None

    def _extract_shares(self, facts: dict) -> Optional[float]:
        """Extract weighted average basic shares from latest filing."""
        concept_names = XBRL_CONCEPTS["shares_outstanding"]
        for name in concept_names:
            if name in facts:
                data = facts[name]["units"].get("shares", [])
                if data:
                    sorted_data = sorted(data, key=lambda x: x.get("end", ""), reverse=True)
                    return sorted_data[0].get("val")
        return None

    def extract_financials(self, facts: dict) -> Financials:
        """Extract core financial data from company facts XBRL JSON."""
        f = Financials()

        # Income statement
        f.revenue = self._extract_concept(facts, XBRL_CONCEPTS["revenue"])
        f.ebit = self._extract_concept(facts, XBRL_CONCEPTS["ebit"])
        f.net_income = self._extract_concept(facts, XBRL_CONCEPTS["net_income"])
        f.eps_basic = self._extract_concept(facts, XBRL_CONCEPTS["eps_basic"], "USD/shares")
        f.eps_diluted = self._extract_concept(facts, XBRL_CONCEPTS["eps_diluted"], "USD/shares")

        # Balance sheet
        f.total_assets = self._extract_concept(facts, XBRL_CONCEPTS["total_assets"])
        f.total_equity = self._extract_concept(facts, XBRL_CONCEPTS["total_equity"])
        f.total_debt = self._extract_concept(facts, XBRL_CONCEPTS["total_debt"])
        f.current_debt = self._extract_concept(facts, XBRL_CONCEPTS["current_debt"])
        f.long_term_debt = self._extract_concept(facts, XBRL_CONCEPTS["long_term_debt"])
        f.cash_and_equivalents = self._extract_concept(facts, XBRL_CONCEPTS["cash"])
        f.short_term_investments = self._extract_concept(facts, XBRL_CONCEPTS["short_term_investments"])
        f.goodwill = self._extract_concept(facts, XBRL_CONCEPTS["goodwill"])
        f.intangibles = self._extract_concept(facts, XBRL_CONCEPTS["intangibles"])

        # Shares
        f.shares_outstanding = self._extract_shares(facts)

        # Cash flow
        f.cfo = self._extract_concept(facts, XBRL_CONCEPTS["cfo"])
        f.capex = self._extract_concept(facts, XBRL_CONCEPTS["capex"])

        # Compute derived values
        if f.ebit is not None and f.capex is not None:
            pass  # EBITDA requires D&A which is not reliably in XBRL tags

        return f

    def get_company_financials(self, ticker: str) -> tuple[Company, Financials]:
        """One-shot: ticker -> Company + Financials from SEC EDGAR."""
        cik = self.ticker_to_cik(ticker)
        name = self.get_company_name(ticker)
        facts = self.get_company_facts(cik)

        company = Company(
            name=name,
            ticker=ticker.upper(),
            cik=cik,
            listing_type=ListingType.US_EXCHANGE,
            currency="USD",
        )

        financials = self.extract_financials(facts.get('facts', {}).get('us-gaap', {}))
        financials.source_url = f"{SEC_BASE}/api/xbrl/companyfacts/CIK{cik}.json"

        return company, financials

    # --- Filing search ---

    def get_recent_filings(self, cik: str, form_type: str = "10-K",
                           limit: int = 5) -> list[Filing]:
        """Get recent filings of a specific type from submissions."""
        subs = self.get_submissions(cik)
        filings = []
        recent = subs.get("filings", {}).get("recent", {})

        if not recent:
            return filings

        forms = recent.get("form", [])
        dates = recent.get("filingDate", [])
        periods = recent.get("reportDate", [])
        accessions = recent.get("accessionNumber", [])
        primary_docs = recent.get("primaryDocument", [])

        count = 0
        for i, form in enumerate(forms):
            if form == form_type and count < limit:
                acc = accessions[i] if i < len(accessions) else ""
                acc_formatted = acc.replace("-", "")
                doc = primary_docs[i] if i < len(primary_docs) else ""
                filing_url = (
                    f"{SEC_LEGACY}/Archives/edgar/data/"
                    f"{int(cik.lstrip('0'))}/{acc_formatted}/{doc}"
                )

                filings.append(Filing(
                    company="",
                    form_type=form,
                    filing_date=date.fromisoformat(dates[i]) if i < len(dates) else date.today(),
                    fiscal_period_end=date.fromisoformat(periods[i]) if i < len(periods) else date.today(),
                    url=filing_url,
                    cik=cik,
                    accession_number=acc,
                ))
                count += 1

        return filings

    def search_filings(self, cik: str, form_types: list[str] = None,
                       limit: int = 20) -> list[Filing]:
        """Search for filings by form types."""
        if form_types is None:
            form_types = ["10-K", "10-Q", "8-K", "20-F", "6-K"]
        subs = self.get_submissions(cik)
        filings = []
        recent = subs.get("filings", {}).get("recent", {})

        if not recent:
            return filings

        forms = recent.get("form", [])
        dates = recent.get("filingDate", [])
        periods = recent.get("reportDate", [])
        accessions = recent.get("accessionNumber", [])
        primary_docs = recent.get("primaryDocument", [])

        count = 0
        for i, form in enumerate(forms):
            if form in form_types and count < limit:
                acc = accessions[i] if i < len(accessions) else ""
                acc_formatted = acc.replace("-", "")
                doc = primary_docs[i] if i < len(primary_docs) else ""
                filing_url = (
                    f"{SEC_LEGACY}/Archives/edgar/data/"
                    f"{int(cik.lstrip('0'))}/{acc_formatted}/{doc}"
                )

                filings.append(Filing(
                    company="",
                    form_type=form,
                    filing_date=date.fromisoformat(dates[i]) if i < len(dates) else date.today(),
                    fiscal_period_end=date.fromisoformat(periods[i]) if i < len(periods) else date.today(),
                    url=filing_url,
                    cik=cik,
                    accession_number=acc,
                ))
                count += 1

        return filings


def get_company_fast(ticker: str) -> tuple[Company, Financials]:
    """Convenience function: ticker -> Company + Financials."""
    client = SECEdgarClient()
    return client.get_company_financials(ticker)
