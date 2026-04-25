from dataclasses import dataclass


@dataclass
class ModelConfig:
    ticker: str
    company_name: str
    domicile: str                    # "US" | "non-US"
    currency: str                    # "USD", "EUR", "JPY", etc.
    fiscal_year_end: str             # "Dec", "Sep", "Mar", etc.
    periods_historical: int = 5
    periods_projected: int = 5
    filing_override: str | None = None
    force: bool = False


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
