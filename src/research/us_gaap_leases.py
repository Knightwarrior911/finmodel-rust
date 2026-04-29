"""
US GAAP ASC 842 lease data extraction from SEC 10-K filings.
Direct HTTP download (no browser). Parses HTML with BeautifulSoup.

Extracts: discount rate, lease term, operating lease cost, ROU assets, lease liabilities.
Computes: ROU depreciation, lease interest for IFRS 16 conversion.
"""

import logging
import re
from dataclasses import dataclass
from typing import Optional

import requests
from bs4 import BeautifulSoup

from src.research.sec_edgar import SECEdgarClient

logger = logging.getLogger(__name__)
SEC_HEADERS = {"User-Agent": "FinancialModelBot vinit.paul@gmail.com"}


@dataclass
class ASC842LeaseData:
    """Lease data extracted from 10-K ASC 842 note."""
    ticker: str = ""
    company: str = ""
    fiscal_year: str = ""

    # Extracted from 10-K notes
    operating_lease_cost: Optional[float] = None    # Fixed lease cost (straight-line rent)
    finance_lease_cost: Optional[float] = None       # Finance lease total cost
    variable_lease_cost: Optional[float] = None      # Variable payments (NOT adjusted)
    short_term_lease_cost: Optional[float] = None    # Short-term (NOT adjusted)

    # ROU assets (from note table)
    operating_rou_assets: Optional[float] = None
    finance_rou_assets: Optional[float] = None

    # Lease liabilities (from note table)
    operating_lease_liability: Optional[float] = None
    finance_lease_liability: Optional[float] = None

    # Key parameters
    weighted_avg_discount_rate: Optional[float] = None   # e.g., 3.4 -> 3.4%
    weighted_avg_lease_term: Optional[float] = None      # e.g., 9.8 -> 9.8 years

    # Computed (US GAAP -> IFRS 16 conversion inputs)
    estimated_rou_depreciation: Optional[float] = None
    estimated_lease_interest: Optional[float] = None

    # Metadata
    filing_url: str = ""
    extraction_confidence: str = ""  # "high", "medium", "low"

    def compute_ifrs_adjustments(self):
        """
        Estimate ROU depreciation and lease interest for IFRS 16 conversion.
        Under ASC 842, operating leases show single lease cost (not split).
        We estimate: Lease Interest = Lease Liability x Discount Rate
                     ROU Depreciation = Operating Lease Cost - Lease Interest
        """
        if self.operating_lease_liability and self.weighted_avg_discount_rate:
            self.estimated_lease_interest = (
                self.operating_lease_liability * self.weighted_avg_discount_rate / 100
            )

        if self.operating_lease_cost and self.estimated_lease_interest:
            self.estimated_rou_depreciation = (
                self.operating_lease_cost - self.estimated_lease_interest
            )
            # Sanity: ROU depreciation should be positive
            if self.estimated_rou_depreciation < 0:
                self.estimated_rou_depreciation = 0

        # Alternative: if we have ROU assets and lease term
        if not self.estimated_rou_depreciation and self.operating_rou_assets and self.weighted_avg_lease_term:
            self.estimated_rou_depreciation = self.operating_rou_assets / self.weighted_avg_lease_term

        # If still no estimate, use XBRL approximation
        if not self.estimated_lease_interest and self.operating_lease_liability:
            self.estimated_lease_interest = self.operating_lease_liability * 0.035  # assume 3.5%

        if not self.estimated_rou_depreciation and self.operating_lease_cost:
            self.estimated_rou_depreciation = self.operating_lease_cost * 0.75  # assume 75%


def extract_asc842_from_10k(ticker: str) -> ASC842LeaseData:
    """
    Download latest 10-K HTML from SEC EDGAR and extract ASC 842 lease data.
    Returns ASC842LeaseData with all extracted fields.
    """
    sec = SECEdgarClient()
    try:
        cik = sec.ticker_to_cik(ticker)
        company = sec.get_company_name(ticker)
    except Exception as e:
        logger.error(f"Ticker lookup failed: {e}")
        return ASC842LeaseData(ticker=ticker, extraction_confidence="low")

    # Get latest 10-K URL
    filings = sec.get_recent_filings(cik, "10-K", limit=1)
    if not filings:
        logger.error(f"No 10-K found for {ticker}")
        return ASC842LeaseData(ticker=ticker, company=company, extraction_confidence="low")

    filing = filings[0]
    url = filing.url

    # Download and parse
    try:
        resp = requests.get(url, headers=SEC_HEADERS, timeout=60)
        resp.raise_for_status()
    except Exception as e:
        logger.error(f"10-K download failed: {e}")
        return ASC842LeaseData(ticker=ticker, company=company, filing_url=url, extraction_confidence="low")

    soup = BeautifulSoup(resp.text, "html.parser")
    text = soup.get_text(separator=" ")
    text = re.sub(r"\s+", " ", text)

    data = ASC842LeaseData(
        ticker=ticker,
        company=company,
        fiscal_year=str(filing.fiscal_period_end),
        filing_url=url,
    )

    # Extract weighted average discount rate
    rate_match = re.search(
        r"discount\s+rate.{0,100}?(?:was|of)\s+(\d+\.?\d*)\s*%",
        text, re.IGNORECASE
    )
    if rate_match:
        data.weighted_avg_discount_rate = float(rate_match.group(1))

    # Extract weighted average remaining lease term
    term_match = re.search(
        r"weighted.average\s+remaining\s+lease\s+term.{0,50}?(?:was\s+)?(\d+\.?\d*)\s*years?",
        text, re.IGNORECASE
    )
    if term_match:
        data.weighted_avg_lease_term = float(term_match.group(1))

    # Extract operating lease cost (fixed payments)
    # Apple example: "fixed payments on the Company's operating leases were $2.1 billion"
    cost_patterns = [
        (r"fixed\s+payments?\s+on\s+(?:the\s+)?(?:Company.{1,3}s?\s+)?operating\s+leases?\s+were\s+\$\s*(\d+\.?\d*)\s*(billion|million|B|M)?", 1e9),
        (r"(?:fixed\s+)?lease\s+costs?.{0,50}?operating\s+leases?\s+were\s+\$\s*(\d+\.?\d*)\s*(billion|million|B|M)", 1e9),
        (r"operating\s+lease\s+cost\s+.*?\$\s*(\d+\.?\d*)\s*(billion|million)", 1e9),
        (r"Operating\s+lease\s+cost\s+.*?\$\s*(\d{1,3}(?:,\d{3})*\.?\d*)", 1e6),
    ]
    for pattern, default_mult in cost_patterns:
        m = re.search(pattern, text, re.IGNORECASE)
        if m:
            raw = m.group(1).replace(",", "")
            val = float(raw)
            unit = m.group(2) if m.lastindex and m.lastindex >= 2 else None
            ctx = text[m.start():m.end()]
            if unit and ('billion' in (unit or '').lower() or unit == 'B'):
                val *= 1_000_000_000
            elif unit and ('million' in (unit or '').lower() or unit == 'M'):
                val *= 1_000_000
            elif 'billion' in ctx.lower():
                val *= 1_000_000_000
            elif 'million' in ctx.lower():
                val *= 1_000_000
            else:
                val *= default_mult
            data.operating_lease_cost = val
            logger.info(f"Operating lease cost: \${val:,.0f}")
            break

    # Fix: if we have ROU assets and discount rate but no lease cost,
    # estimate lease cost from liability + depreciation
    if not data.operating_lease_cost and data.operating_rou_assets and data.weighted_avg_discount_rate:
        # Rough: annual cost = ROU asset / avg life + liability * rate
        if data.weighted_avg_lease_term:
            data.estimated_rou_depreciation = data.operating_rou_assets / data.weighted_avg_lease_term
        else:
            data.estimated_rou_depreciation = data.operating_rou_assets / 10  # assume 10 years
        if data.operating_lease_liability and data.weighted_avg_discount_rate:
            data.estimated_lease_interest = data.operating_lease_liability * data.weighted_avg_discount_rate / 100
        data.operating_lease_cost = (data.estimated_rou_depreciation or 0) + (data.estimated_lease_interest or 0)

    # Extract ROU assets and lease liabilities from note table
    # Operating lease ROU assets
    rou_match = re.search(
        r"Operating\s+leases?\s+.*?(?:Other\s+non-current\s+assets).*?\$\s*(\d{1,3}(?:,\d{3})*(?:\.\d+)?)",
        text, re.IGNORECASE
    )
    if rou_match:
        data.operating_rou_assets = float(rou_match.group(1).replace(",", "")) * 1_000_000

    # Total operating lease liabilities
    liab_match = re.search(
        r"Total\s+(?:operating\s+)?lease\s+liabilities?\s+\$\s*(\d{1,3}(?:,\d{3})*(?:\.\d+)?)",
        text, re.IGNORECASE
    )
    if liab_match:
        data.operating_lease_liability = float(liab_match.group(1).replace(",", "")) * 1_000_000

    # If we didn't find operating lease liability, try XBRL
    if not data.operating_lease_liability:
        try:
            facts = sec.get_company_facts(cik)
            usgaap = facts.get("facts", {}).get("us-gaap", {})
            ocl = usgaap.get("OperatingLeaseLiability", {})
            usd_data = ocl.get("units", {}).get("USD", [])
            if usd_data:
                sorted_data = sorted(usd_data, key=lambda x: x.get("end", ""), reverse=True)
                data.operating_lease_liability = sorted_data[0].get("val")
            # Also get ROU assets from XBRL
            rou = usgaap.get("OperatingLeaseRightOfUseAsset", {})
            rou_data = rou.get("units", {}).get("USD", [])
            if rou_data:
                sorted_rou = sorted(rou_data, key=lambda x: x.get("end", ""), reverse=True)
                data.operating_rou_assets = sorted_rou[0].get("val")
        except Exception as e:
            logger.warning(f"XBRL fallback failed: {e}")

    # Compute IFRS 16 adjustments
    data.compute_ifrs_adjustments()

    # Set confidence
    has_critical = all([
        data.operating_lease_cost,
        data.weighted_avg_discount_rate,
        data.operating_lease_liability,
    ])
    data.extraction_confidence = "high" if has_critical else "medium" if data.operating_lease_cost else "low"

    return data
