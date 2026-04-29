"""
Query type router ported from analyst-research-protocol.json.
Detects query type from trigger phrases and returns source priority chains.
"""

import re
from dataclasses import dataclass, field
from typing import Optional

from src.models.research import QueryType, Source, SourceTier, AccessMethod


# --- TRIGGER PHRASES ---

TRIGGER_MAP: dict[QueryType, list[str]] = {
    QueryType.EARNINGS_ANALYSIS: [
        "Q1 20", "Q2 20", "Q3 20", "Q4 20",
        "FY20", "financial results", "earnings", "quarterly results",
        "bank results", "earnings release",
    ],
    QueryType.SYNERGY_REALIZATION: [
        "synerg", "integration", "run-rate", "cost saving", "dis-synerg",
        "realized vs expected", "synergy target",
    ],
    QueryType.BENEFICIAL_OWNERSHIP: [
        "owns", "ownership", "shareholder", "stake", "beneficial", "holder",
        "investor >5%", "major shareholder", "promoter", "who owns",
    ],
    QueryType.DEBT_MATURITY_SCHEDULE: [
        "debt maturity", "debt schedule", "borrowing", "leverage",
        "covenant", "credit facility", "notes outstanding", "term loan",
        "revolver", "debt structure",
    ],
    QueryType.REGULATORY_APPROVAL_STATUS: [
        "regulatory approval", "antitrust", "merger control",
        "CCI", "DOJ", "cleared", "condition", "regulatory condition",
    ],
    QueryType.EARNINGS_ESTIMATE_CONSENSUS: [
        "estimate", "consensus", "earnings forecast", "revenue forecast",
        "EPS forecast", "analyst estimate", "sell-side",
    ],
    QueryType.TRANSACTION_TERMS: [
        "deal terms", "acquisition terms", "purchase price",
        "consideration", "earnout", "valuation multiple",
        "M&A deal", "transaction details", "deal announced",
    ],
    QueryType.GENERAL_COMPANY_INTELLIGENCE: [
        "about", "profile", "business description", "competitors",
        "market position", "product portfolio", "customer base",
        "company overview", "what does", "who is",
    ],
}


# --- SOURCE CHAINS ---

@dataclass
class SourceChain:
    query_type: QueryType
    sources: list[Source] = field(default_factory=list)
    irrelevant_sources: list[str] = field(default_factory=list)


def _source(name: str, url_pattern: str = "", tier: SourceTier = SourceTier.TIER_1_FILINGS,
            method: AccessMethod = AccessMethod.DIRECT_API, priority: int = 1,
            reason: str = "") -> Source:
    return Source(
        name=name, url=url_pattern, source_type="filing",
        tier=tier, access_method=method, priority=priority, reason=reason,
    )


SOURCE_CHAINS: dict[QueryType, SourceChain] = {
    QueryType.EARNINGS_ANALYSIS: SourceChain(
        query_type=QueryType.EARNINGS_ANALYSIS,
        sources=[
            _source("Company IR Earnings Press Release", tier=SourceTier.TIER_2_COMPANY_IR,
                    method=AccessMethod.BROWSER_CDP, priority=1,
                    reason="Primary source for earnings data"),
            _source("Company IR Earnings Supplement", tier=SourceTier.TIER_2_COMPANY_IR,
                    method=AccessMethod.BROWSER_CDP, priority=2,
                    reason="Detailed financial supplement with segment data"),
            _source("SEC 10-Q / 10-K", tier=SourceTier.TIER_1_FILINGS,
                    method=AccessMethod.DIRECT_API, priority=3,
                    reason="Official filing — use if IR sources fail"),
        ],
        irrelevant_sources=["13D/13G", "UK PSC", "India SAST", "Form 4"],
    ),
    QueryType.SYNERGY_REALIZATION: SourceChain(
        query_type=QueryType.SYNERGY_REALIZATION,
        sources=[
            _source("Company Press Release (deal announcement)", tier=SourceTier.TIER_2_COMPANY_IR,
                    method=AccessMethod.BROWSER_CDP, priority=1),
            _source("SEC S-4 / F-4 (registration)", tier=SourceTier.TIER_1_FILINGS,
                    method=AccessMethod.DIRECT_API, priority=2),
            _source("SEC 10-K (annual)", tier=SourceTier.TIER_1_FILINGS,
                    method=AccessMethod.DIRECT_API, priority=3),
            _source("SEC 10-Q (quarterly)", tier=SourceTier.TIER_1_FILINGS,
                    method=AccessMethod.DIRECT_API, priority=4),
            _source("Earnings Call Transcript", tier=SourceTier.TIER_2_COMPANY_IR,
                    method=AccessMethod.BROWSER_CDP, priority=5),
        ],
        irrelevant_sources=["PSC register", "Insider trading", "13D/13G"],
    ),
    QueryType.BENEFICIAL_OWNERSHIP: SourceChain(
        query_type=QueryType.BENEFICIAL_OWNERSHIP,
        sources=[
            _source("SEC Form 13D/13G", tier=SourceTier.TIER_4_OWNERSHIP,
                    method=AccessMethod.DIRECT_API, priority=1),
            _source("UK PSC Register", tier=SourceTier.TIER_4_OWNERSHIP,
                    method=AccessMethod.BROWSER_CDP, priority=1),
            _source("India SAST / BSE NSE Shareholding", tier=SourceTier.TIER_4_OWNERSHIP,
                    method=AccessMethod.BROWSER_CDP, priority=1),
        ],
        irrelevant_sources=["10-K debt footnotes", "8-K approvals", "EPS estimates"],
    ),
    QueryType.DEBT_MATURITY_SCHEDULE: SourceChain(
        query_type=QueryType.DEBT_MATURITY_SCHEDULE,
        sources=[
            _source("10-K Debt Footnote", tier=SourceTier.TIER_1_FILINGS,
                    method=AccessMethod.DIRECT_API, priority=1),
            _source("10-Q Quarterly Update", tier=SourceTier.TIER_1_FILINGS,
                    method=AccessMethod.DIRECT_API, priority=2),
            _source("8-K Debt Issuance", tier=SourceTier.TIER_1_FILINGS,
                    method=AccessMethod.DIRECT_API, priority=2),
        ],
        irrelevant_sources=["13D/13G", "PSC register"],
    ),
    QueryType.REGULATORY_APPROVAL_STATUS: SourceChain(
        query_type=QueryType.REGULATORY_APPROVAL_STATUS,
        sources=[
            _source("8-K Closing Conditions", tier=SourceTier.TIER_1_FILINGS,
                    method=AccessMethod.DIRECT_API, priority=1),
            _source("DOJ/FTC Press Releases", tier=SourceTier.TIER_3_NEWS,
                    method=AccessMethod.BROWSER_CDP, priority=2),
            _source("EU DG COMP", tier=SourceTier.TIER_3_NEWS,
                    method=AccessMethod.BROWSER_CDP, priority=3),
            _source("India CCI", tier=SourceTier.TIER_3_NEWS,
                    method=AccessMethod.BROWSER_CDP, priority=4),
        ],
        irrelevant_sources=["13D/13G", "PSC register"],
    ),
    QueryType.EARNINGS_ESTIMATE_CONSENSUS: SourceChain(
        query_type=QueryType.EARNINGS_ESTIMATE_CONSENSUS,
        sources=[
            _source("Company Earnings Release", tier=SourceTier.TIER_2_COMPANY_IR,
                    method=AccessMethod.BROWSER_CDP, priority=1),
            _source("10-Q Management Guidance", tier=SourceTier.TIER_1_FILINGS,
                    method=AccessMethod.DIRECT_API, priority=2),
            _source("Earnings Call Transcript", tier=SourceTier.TIER_2_COMPANY_IR,
                    method=AccessMethod.BROWSER_CDP, priority=3),
        ],
        irrelevant_sources=["13D/13G", "PSC register", "Debt maturity"],
    ),
    QueryType.TRANSACTION_TERMS: SourceChain(
        query_type=QueryType.TRANSACTION_TERMS,
        sources=[
            _source("8-K Deal Announcement", tier=SourceTier.TIER_1_FILINGS,
                    method=AccessMethod.DIRECT_API, priority=1),
            _source("S-4 / F-4 Merger Proxy", tier=SourceTier.TIER_1_FILINGS,
                    method=AccessMethod.DIRECT_API, priority=1),
            _source("Company Press Release", tier=SourceTier.TIER_2_COMPANY_IR,
                    method=AccessMethod.BROWSER_CDP, priority=2),
            _source("Investor Presentation", tier=SourceTier.TIER_2_COMPANY_IR,
                    method=AccessMethod.BROWSER_CDP, priority=3),
        ],
        irrelevant_sources=["13D/13G", "PSC register"],
    ),
    QueryType.GENERAL_COMPANY_INTELLIGENCE: SourceChain(
        query_type=QueryType.GENERAL_COMPANY_INTELLIGENCE,
        sources=[
            _source("10-K Item 1 (Business)", tier=SourceTier.TIER_1_FILINGS,
                    method=AccessMethod.DIRECT_API, priority=1),
            _source("Company IR Website", tier=SourceTier.TIER_2_COMPANY_IR,
                    method=AccessMethod.BROWSER_CDP, priority=2),
            _source("Investor Presentation", tier=SourceTier.TIER_2_COMPANY_IR,
                    method=AccessMethod.BROWSER_CDP, priority=3),
        ],
        irrelevant_sources=["13D/13G", "Synergy analysis", "Debt maturity"],
    ),
}


# --- QUERY TYPE DETECTION ---

def detect_query_type(user_query: str) -> QueryType:
    """Detect query type from natural language query using trigger phrases."""
    query_lower = user_query.lower()
    scores: dict[QueryType, int] = {}

    for qtype, triggers in TRIGGER_MAP.items():
        score = 0
        for trigger in triggers:
            if trigger.lower() in query_lower:
                score += 1
        if score > 0:
            scores[qtype] = score

    if not scores:
        return QueryType.GENERAL_COMPANY_INTELLIGENCE

    # Return highest-scoring match
    return max(scores, key=scores.get)


def get_source_chain(query_type: QueryType) -> SourceChain:
    """Get priority-ordered source chain for a query type."""
    return SOURCE_CHAINS.get(query_type, SOURCE_CHAINS[QueryType.GENERAL_COMPANY_INTELLIGENCE])


def classify_query(user_query: str) -> tuple[QueryType, SourceChain]:
    """Detect query type and return its source chain in one call."""
    qtype = detect_query_type(user_query)
    chain = get_source_chain(qtype)
    return qtype, chain


# --- LISTING TYPE DETECTION ---

def detect_listing_type(company_name: str) -> str:
    """
    Determine where company files based on name patterns and known exchanges.
    Returns: 'us', 'uk', 'eu', 'india', 'hk', 'japan', 'other'
    """
    # Known Indian companies
    indian_patterns = [
        "HDFC", "ICICI", "SBI", "Kotak", "Axis", "Reliance", "TCS", "Infosys",
        "Wipro", "HCL Tech", "Asian Paints", "Berger", "Tata", "Mahindra",
        "Bharti", "Adani", "Hindustan Unilever", "ITC", "NTPC", "ONGC",
    ]
    for pattern in indian_patterns:
        if pattern.lower() in company_name.lower():
            return "india"

    # Known UK companies
    uk_patterns = ["HSBC", "Barclays", "Lloyds", "BP", "Shell", "GSK", "AstraZeneca",
                   "Unilever", "Diageo", "BAE Systems", "Rolls-Royce", "Vodafone"]
    for pattern in uk_patterns:
        if pattern.lower() in company_name.lower():
            return "uk"

    # Check if likely US
    us_indicators = ["Inc.", "Corp.", "Corporation", "NYSE", "NASDAQ"]
    if any(ind in company_name for ind in us_indicators):
        return "us"

    return "other"
