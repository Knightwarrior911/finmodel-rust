from dataclasses import dataclass, field
from enum import Enum
from typing import Optional
from datetime import date


class ListingType(Enum):
    US_EXCHANGE = "us_exchange"       # NYSE/NASDAQ -> SEC EDGAR
    US_ADR = "us_adr"                 # ADR -> SEC EDGAR (20-F, 6-K)
    UK_LSE = "uk_lse"                 # Companies House + LSE RNS
    EU_EURONEXT = "eu_euronext"       # AMF + company IR
    GERMANY = "germany"               # Bundesanzeiger + company IR
    INDIA_BSE_NSE = "india_bse_nse"   # BSE/NSE filings + company IR
    HKEX = "hkex"                     # HKEX disclosure
    JAPAN = "japan"                   # TDnet/EDINET
    OTHER = "other"                   # Home market regulator + company IR


class Sector(Enum):
    TMT = "tmt"
    CONSUMER_RETAIL = "consumer_retail"
    HEALTHCARE = "healthcare"
    INDUSTRIALS = "industrials"
    OIL_GAS = "oil_gas"
    METALS_MINING = "metals_mining"
    POWER_UTILITIES = "power_utilities"
    FIG = "fig"
    REAL_ESTATE = "real_estate"
    INFRASTRUCTURE = "infrastructure"
    LEVFIN = "levfin"
    DISTRESSED = "distressed"
    FSG = "fsg"
    ECM = "ecm"
    PRIVATE_CAP = "private_cap"
    PRIVATE_CO = "private_co"
    RENEWABLES = "renewables"


@dataclass
class Company:
    name: str
    ticker: Optional[str] = None
    cik: Optional[str] = None
    exchange: Optional[str] = None
    listing_type: ListingType = ListingType.OTHER
    sector: Optional[Sector] = None
    currency: str = "USD"
    fiscal_year_end: Optional[date] = None
    ir_website: Optional[str] = None


@dataclass
class Financials:
    """LTM financial data extracted from filings."""
    fiscal_period_end: Optional[date] = None
    filing_type: Optional[str] = None  # 10-K, 10-Q, 20-F, etc.

    # Income Statement
    revenue: Optional[float] = None
    ebit: Optional[float] = None
    ebitda: Optional[float] = None
    depreciation_amortization: Optional[float] = None
    interest_expense: Optional[float] = None
    pretax_income: Optional[float] = None
    net_income: Optional[float] = None
    eps_basic: Optional[float] = None
    eps_diluted: Optional[float] = None

    # Balance Sheet
    total_assets: Optional[float] = None
    total_equity: Optional[float] = None
    total_debt: Optional[float] = None
    current_debt: Optional[float] = None
    long_term_debt: Optional[float] = None
    cash_and_equivalents: Optional[float] = None
    short_term_investments: Optional[float] = None
    goodwill: Optional[float] = None
    intangibles: Optional[float] = None
    lease_liabilities_current: Optional[float] = None
    lease_liabilities_noncurrent: Optional[float] = None
    rou_assets: Optional[float] = None
    minority_interest: Optional[float] = None
    preferred_stock: Optional[float] = None
    pension_liability: Optional[float] = None  # From balance sheet (NOT reliable per R-015)
    pension_pbo: Optional[float] = None        # From notes section (correct per R-015)
    pension_plan_assets: Optional[float] = None # From notes section

    # Shares
    shares_outstanding: Optional[float] = None       # Weighted average basic, latest filing
    shares_diluted: Optional[float] = None

    # Cash Flow
    cfo: Optional[float] = None
    cff: Optional[float] = None
    capex: Optional[float] = None
    lease_principal_repayment: Optional[float] = None
    lease_depreciation_addback: Optional[float] = None

    # IFRS-specific
    ifrs_rou_depreciation: Optional[float] = None
    ifrs_lease_interest: Optional[float] = None
    ifrs_short_term_rent: Optional[float] = None
    ifrs_weighted_discount_rate: Optional[float] = None

    # Valuation (computed)
    market_cap: Optional[float] = None
    enterprise_value: Optional[float] = None
    ev_ebitda: Optional[float] = None
    pe_ratio: Optional[float] = None
    pb_ratio: Optional[float] = None

    # Metadata
    source_url: Optional[str] = None
    price_date: Optional[date] = None
    price_source: Optional[str] = None


@dataclass
class Filing:
    company: str
    form_type: str
    filing_date: date
    fiscal_period_end: date
    url: str
    cik: Optional[str] = None
    accession_number: Optional[str] = None
    is_amended: bool = False
