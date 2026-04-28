from dataclasses import dataclass, field


@dataclass
class ModelConfig:
    ticker: str
    company_name: str
    domicile: str                    # "US" | "non-US"
    currency: str                    # "USD", "EUR", "JPY", etc.
    fiscal_year_end: str             # "Dec", "Sep", "Mar", etc.
    periods_historical: int = 3
    periods_projected: int = 5
    filing_override: str | None = None
    force: bool = False
    sic: int = 0                     # EDGAR SIC code (0 = unknown)
    sector: str = "standard"         # "standard" | "utility" | "bank" | "insurance" | "reit"


@dataclass
class SourceCitation:
    filing: str                      # e.g. "10-K FY2023"
    confidence: float                # 1.0 = XBRL; <1.0 = PDF extraction
    page: int | None = None
    xbrl_tag: str | None = None

    def __post_init__(self):
        if not (0.0 <= self.confidence <= 1.0):
            raise ValueError(f"confidence must be in [0.0, 1.0], got {self.confidence}")


@dataclass
class ReconciledFinancialData:
    ticker: str
    company_name: str
    currency: str
    fiscal_year_end: str
    periods: list[str]               # ["2019A", "2020A", ...]
    income_statement: dict           # line_item → [value per period]
    balance_sheet: dict
    cash_flow_statement: dict
    notes: dict                      # note_type → structured extract
    sources: dict                    # line_item → list[SourceCitation]
    flags: list[str]                 # unresolved discrepancies


@dataclass
class DiscrepancyReport:
    items: list[str]                 # human-readable discrepancy descriptions


@dataclass
class ModelOutput:
    periods: list[str]               # ["2019A", ..., "2024E", ...]
    income_statement: dict
    balance_sheet: dict
    cash_flow_statement: dict
    schedules: dict
    assumptions: dict
    converged: bool
    plug_used: bool


@dataclass
class VerificationReport:
    passed: bool
    critical_failures: list[str]
    warnings: list[str]
    notes: list[str]
    period_checks: dict              # period → {check_name: bool}


@dataclass
class ScenarioInputs:
    """One scenario's projection drivers — one value per projected period."""
    name: str                            # "Base" | "Upside" | "Downside"
    revenue_growth_pct: list[float]
    gross_margin_pct: list[float]
    sga_pct_rev: list[float]
    rd_pct_rev: list[float]
    da_pct_rev: list[float]
    capex_pct_rev: list[float]
    tax_rate_pct: list[float]
    interest_rate_pct: list[float]
    dso_days: list[float]
    dio_days: list[float]
    dpo_days: list[float]
    dividend_per_share: list[float]
    # Valuation drivers (scenario-specific scalars)
    terminal_growth_rate: float
    exit_ebitda_multiple: float


@dataclass
class AssumptionsBlock:
    """Toggle + three scenarios + shared (non-scenario) valuation inputs."""
    proj_periods: list[str]              # ["2026E", ..., "2030E"]
    active_case: int                     # 1=Base, 2=Upside, 3=Downside
    base: ScenarioInputs
    upside: ScenarioInputs
    downside: ScenarioInputs
    # Shared valuation inputs (non-scenario)
    risk_free_rate: float
    equity_risk_premium: float
    target_de_ratio: float               # D/E used to relever peer beta
    cost_of_debt_pretax: float
    current_share_price: float           # for upside/downside cross-check
    shares_diluted: float                # held flat from last historical
    mid_year_convention: bool = True


@dataclass
class Peer:
    ticker: str
    name: str
    market_cap: float            # $M
    enterprise_value: float      # $M
    levered_beta: float
    de_ratio: float              # debt/equity
    tax_rate: float              # effective
    rationale: str = ""          # why included


@dataclass
class PeerSet:
    target_ticker: str
    target_market_cap: float
    target_de_ratio: float
    peers: list[Peer]
    excluded: list[tuple[str, str]] = None   # (ticker, reason)
    source: str = "llm"          # "llm" | "fallback"

    def __post_init__(self):
        if self.excluded is None:
            self.excluded = []


@dataclass
class PublicCompPeer:
    """One peer with market data + LTM operating stats + LTM & forward multiples."""
    ticker: str
    name: str
    country: str = "US"
    currency: str = "USD"
    tier: int = 1                         # 1 = primary, 2 = broader
    # Market data
    share_price: float = 0.0
    shares_diluted: float = 0.0           # millions
    market_cap: float = 0.0               # $M
    total_debt: float = 0.0               # $M
    cash: float = 0.0                     # $M
    enterprise_value: float = 0.0         # $M
    week52_high: float = 0.0
    week52_low: float = 0.0
    # LTM operating stats ($M except EPS)
    ltm_revenue: float = 0.0
    ltm_ebitda: float = 0.0
    ltm_ebit: float = 0.0
    ltm_net_income: float = 0.0
    ltm_eps_diluted: float = 0.0
    # Forward estimates ($M) — from consensus where available
    ntm_revenue: float = 0.0
    ntm_ebitda: float = 0.0
    fy1_revenue: float = 0.0
    fy1_ebitda: float = 0.0
    fy2_revenue: float = 0.0
    fy2_ebitda: float = 0.0
    ntm_eps: float = 0.0
    fy1_eps: float = 0.0
    # LTM Multiples
    ev_rev_ltm: float | None = None
    ev_ebitda_ltm: float | None = None
    ev_ebit_ltm: float | None = None
    pe_ltm: float | None = None
    # Forward Multiples (NTM / FY+1 / FY+2)
    ev_rev_ntm: float | None = None
    ev_ebitda_ntm: float | None = None
    ev_rev_fy1: float | None = None
    ev_ebitda_fy1: float | None = None
    ev_rev_fy2: float | None = None
    ev_ebitda_fy2: float | None = None
    pe_ntm: float | None = None
    pe_fy1: float | None = None
    rationale: str = ""


@dataclass
class CompMultipleStats:
    """Summary statistics for one multiple across the peer set."""
    multiple_name: str
    values: list[float]                   # non-NM values
    min: float
    p25: float
    median: float
    mean: float
    p75: float
    max: float
    count: int                            # non-NM count


@dataclass
class PublicCompsOutput:
    target_ticker: str
    target_company_name: str
    as_of_date: str                       # ISO date
    # Target LTM metrics
    target_revenue: float = 0.0
    target_ebitda: float = 0.0
    target_ebit: float = 0.0
    target_net_income: float = 0.0
    target_total_debt: float = 0.0
    target_cash: float = 0.0
    target_shares_diluted: float = 0.0
    # Peers (pre-tier; tier accessible via peer.tier)
    peers: list[PublicCompPeer] = field(default_factory=list)
    excluded: list[tuple[str, str]] = field(default_factory=list)
    # Summary stats per multiple
    stats: dict[str, "CompMultipleStats"] = field(default_factory=dict)
    # Implied valuation for target (per-share)
    implied_price_low: float = 0.0
    implied_price_median: float = 0.0
    implied_price_high: float = 0.0
    # Source meta
    source: str = "llm"                   # "llm" | "fallback"


@dataclass
class WACCOutput:
    # Peer-set unlever/relever
    peers: list[Peer]
    median_unlevered_beta: float
    target_levered_beta: float
    target_de_ratio: float
    # CAPM
    risk_free_rate: float
    equity_risk_premium: float
    cost_of_equity: float
    # Cost of debt
    cost_of_debt_pretax: float
    tax_rate: float
    after_tax_cost_of_debt: float
    # Capital structure weights (from target market cap + debt)
    target_market_cap: float
    target_debt: float
    target_total_capital: float
    equity_weight: float
    debt_weight: float
    # WACC
    wacc: float


@dataclass
class DCFOutput:
    ticker: str
    mid_year_convention: bool         # True = 1/(1+WACC)^(t-0.5)
    # WACC inputs (now sourced from WACCOutput)
    beta: float                       # target levered beta
    risk_free_rate: float
    equity_risk_premium: float
    cost_of_equity: float
    cost_of_debt_pretax: float
    tax_rate: float
    after_tax_cost_of_debt: float
    equity_weight: float
    debt_weight: float
    wacc: float
    # FCF projection (projected periods only)
    proj_periods: list[str]
    fcff_proj: list[float]            # unlevered FCF per projected period
    dwc_proj: list[float]
    discount_factors: list[float]
    pv_fcfs_per_period: list[float]
    pv_fcfs: float
    # Terminal value — BOTH methods computed
    terminal_ebitda: float
    tv_ebitda_multiple: float
    tv_ebitda: float                  # Method 1: EBITDA mult
    tv_ebitda_pv: float
    tv_growth_rate: float
    tv_gordon: float                  # Method 2: Gordon Growth
    tv_gordon_pv: float
    tv_method: int                    # 1=EBITDA mult primary, 2=Gordon primary
    tv_selected: float                # selected primary
    pv_tv: float                      # PV of selected
    # EV bridge — full items
    enterprise_value: float
    total_debt: float
    preferred_stock: float
    noncontrolling_interest: float
    cash: float
    investments: float
    net_debt: float
    equity_value: float
    shares_diluted: float
    implied_price: float
    # Cross-checks
    current_share_price: float
    upside_downside_pct: float        # (implied / current) - 1
    tv_pct_of_ev: float               # PV(TV) / EV
    wacc_minus_g: float               # WACC - terminal_g
    implied_exit_mult_from_gordon: float  # Gordon-implied exit multiple
    implied_g_from_exit_mult: float       # exit-implied perpetuity g
    # Sensitivity analysis grids (for legacy single-tab DCF)
    wacc_range: list[float]
    ebitda_multiple_range: list[float]
    gordon_growth_range: list[float]
    sensitivity_ebitda: list[list[float]]
    sensitivity_gordon: list[list[float]]


@dataclass
class ISRow:
    """One row in the dynamic Income Statement structure."""
    key: str                    # data key (maps to IS dict), '' for non-data rows
    label: str                  # display label in Excel
    row_type: str               # 'section_header'|'line_item'|'subtotal'|'driver'|'memo'|'spacer'
    bold: bool = False
    italic: bool = False
    indent: int = 0
    # for 'driver' rows: which assumption this row links to
    driver_key: str = ""        # e.g. "revenue_growth_pct"
    driver_format: str = "pct"  # "pct" | "num"
    # for 'driver' and 'memo' rows: how to compute implied ratio from IS data
    hist_numer_key: str = ""    # IS dict key for numerator; "__growth" = YoY growth rate
    hist_denom_key: str = ""    # IS dict key for denominator
