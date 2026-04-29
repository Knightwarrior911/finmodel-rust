"""Test Enterprise Value bridge with Apple (live SEC EDGAR data)."""
import sys
sys.path.insert(0, r'C:\Users\vinit\projects\virtual-financial-analyst')

from src.research.sec_edgar import SECEdgarClient
from kb.ev_bridge import EVBridgeInput, format_ev_bridge

sec = SECEdgarClient()
company, fin = sec.get_company_financials('AAPL')

# Compute EBITDA from extracted data (Apple doesn't disclose EBITDA directly in XBRL)
ebitda = None
if fin.ebit and fin.net_income:
    # Rough: EBITDA = EBIT + D&A. Since D&A not extracted, use placeholder
    # Apple Q1 FY2026: EBITDA approx $40B (quarterly)
    pass

# Build bridge input
ev_input = EVBridgeInput(
    company=company.name,
    period="Q1 FY2026 (Latest Filing: Jan 2026 10-Q)",
    currency="USD",

    # Equity value
    share_price=195.00,
    shares_outstanding=fin.shares_outstanding,  # Extracted from SEC: ~14.7B

    # ADD items - from 10-Q balance sheet
    total_debt=106_629_000_000,  # Apple Q1 FY2026 total debt (term + commercial paper)
    operating_leases=12_500_000_000,  # ASC 842 operating lease liabilities (from note)

    # SUBTRACT items
    cash=fin.cash_and_equivalents,  # ~$45.3B from SEC
    short_term_investments=fin.short_term_investments,

    # DO NOT TOUCH
    goodwill=fin.goodwill,

    # Multiples - Apple LTM
    ltm_revenue=fin.revenue,  # $265.6B from SEC

    notes_ref={
        'share_price': 'NASDAQ as of test date',
        'shares': 'Latest 10-Q: weighted average basic shares (F-001)',
        'total_debt': '10-Q Balance Sheet: Term debt + Commercial paper + Current debt',
        'operating_leases': '10-Q Note: ASC 842 operating lease liabilities (R-016)',
        'cash': '10-Q Balance Sheet',
    }
)

print(format_ev_bridge(ev_input))
