from dataclasses import dataclass, field
from enum import Enum
from datetime import datetime
from typing import Optional


class QueryType(Enum):
    EARNINGS_ANALYSIS = "earnings_analysis"
    SYNERGY_REALIZATION = "synergy_realization"
    BENEFICIAL_OWNERSHIP = "beneficial_ownership"
    DEBT_MATURITY_SCHEDULE = "debt_maturity_schedule"
    REGULATORY_APPROVAL_STATUS = "regulatory_approval_status"
    EARNINGS_ESTIMATE_CONSENSUS = "earnings_estimate_consensus"
    TRANSACTION_TERMS = "transaction_terms"
    GENERAL_COMPANY_INTELLIGENCE = "general_company_intelligence"


class SourceTier(Enum):
    TIER_1_FILINGS = 1   # SEC EDGAR, BSE/NSE, Companies House
    TIER_2_COMPANY_IR = 2  # Company IR website, earnings transcripts
    TIER_3_NEWS = 3       # Google, Reuters, Bloomberg, FT, WSJ
    TIER_4_OWNERSHIP = 4  # SEC Form 4, 13D/13G, UK PSC


class AccessMethod(Enum):
    DIRECT_API = "direct_api"           # HTTP request, no browser
    BROWSER_CDP = "browser_cdp"         # Browser via CDP (user's Chrome)
    BROWSER_HEADED = "browser_headed"   # Browser headed mode (anti-bot sites)


@dataclass
class Source:
    name: str
    url: str
    source_type: str  # "filing", "press_release", "news", "ir_website", "exchange"
    tier: SourceTier
    access_method: AccessMethod
    priority: int = 1  # Lower = check first
    reason: str = ""
    result: Optional[str] = None  # "found", "404", "blocked", "not_checked"


@dataclass
class ResearchQuery:
    user_query: str
    query_type: QueryType
    company_name: str
    company: Optional[object] = None  # Company dataclass
    items_requested: list[str] = field(default_factory=list)
    ltm_reference: Optional[str] = None
    time_period: Optional[str] = None
    sector: Optional[str] = None
    additional_context: dict = field(default_factory=dict)


@dataclass
class VerificationResult:
    passed: bool
    errors: list[str] = field(default_factory=list)
    warnings: list[str] = field(default_factory=list)
    gaps: list[str] = field(default_factory=list)
    sanity_checks: dict = field(default_factory=dict)


@dataclass
class ResearchResult:
    query: ResearchQuery
    findings: dict = field(default_factory=dict)  # item_name -> value + sources
    sources_checked: list[Source] = field(default_factory=list)
    sources_failed: list[Source] = field(default_factory=list)
    research_log: list[dict] = field(default_factory=list)
    verification: Optional[VerificationResult] = None
    status: str = "pending"  # pending, in_progress, complete, gaps_found, failed
    started_at: Optional[datetime] = None
    completed_at: Optional[datetime] = None
