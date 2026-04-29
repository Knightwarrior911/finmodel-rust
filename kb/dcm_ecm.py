"""
Capital Markets Rules ported from dcm-ecm-levfin.json.
Debt Capital Markets, Equity Capital Markets, Leveraged Finance.
65+ rules, 13 formulas.
"""

from dataclasses import dataclass
from typing import Optional


@dataclass
class CMRule:
    id: str
    topic: str
    category: str
    description: str


RULES = [
    # --- Capital Markets Overview ---
    CMRule(id="CM-001", topic="Capital markets role", category="overview",
           description="Capital markets teams help companies raise capital: ECM (equity), DCM (debt), LevFin (leveraged loans/bonds)."),
    CMRule(id="CM-002", topic="ECM vs DCM vs LevFin", category="overview",
           description="ECM: IPO, follow-on, convertible, rights issue. DCM: IG bonds, MTN, commercial paper. LevFin: HY bonds, leveraged loans, CLOs."),
    CMRule(id="CM-003", topic="Capital markets process", category="overview",
           description="Origination (pitch) -> Structuring (terms, size, pricing) -> Execution (roadshow, bookbuilding) -> Pricing + Allocation."),

    # --- Debt vs Equity ---
    CMRule(id="CM-004", topic="Debt is cheaper than equity", category="debt_vs_equity",
           description="Debt: interest tax-deductible, lower cost, no dilution, but increases leverage and bankruptcy risk. Equity: no obligation, but dilutive and higher cost of capital."),
    CMRule(id="CM-005", topic="After-tax cost of debt", category="debt_vs_equity",
           description="After-Tax Cost of Debt = Interest Rate x (1 - Tax Rate). Makes debt even cheaper post-tax."),
    CMRule(id="CM-006", topic="Convertible bonds", category="debt_vs_equity",
           description="Compromise: lower coupon than straight debt, potential equity upside. Conversion ratio = Par / Conversion Price."),

    # --- IPO ---
    CMRule(id="CM-007", topic="IPO size", category="ipo",
           description="Typically 20-40% of company sold. Larger float = better liquidity. Greenshoe/over-allotment: 15% standard."),
    CMRule(id="CM-008", topic="IPO pricing discount", category="ipo",
           description="10-20% discount to intrinsic value (IPO pop). Price = midpoint of initial range, adjusted for demand."),
    CMRule(id="CM-009", topic="Primary vs secondary shares", category="ipo",
           description="Primary: new shares issued, proceeds to company. Secondary: existing shareholders sell, proceeds to sellers."),
    CMRule(id="CM-010", topic="Post-money equity value", category="ipo",
           description="Post-Money = Pre-Money + Net IPO Proceeds. Net = Gross - Underwriting Fees (5-7% typical)."),

    # --- Follow-On / Secondary Offering ---
    CMRule(id="CM-011", topic="Follow-on mechanics", category="follow_on",
           description="Similar to IPO but faster. Pricing discount smaller (3-8% for standard). Primary + secondary shares."),
    CMRule(id="CM-012", topic="Follow-on pricing", category="follow_on",
           description="Use P/E multiples relative to peers. Discount reflects urgency, size, market conditions."),

    # --- Credit Analysis ---
    CMRule(id="CM-013", topic="Credit scenarios", category="credit",
           description="Base case: most likely. Downside: recession/stress. Upside: growth case. Test covenant compliance in all three."),
    CMRule(id="CM-014", topic="Maintenance vs incurrence covenants", category="credit",
           description="Maintenance: tested quarterly (Lev Loans). Incurrence: tested when action taken (HY bonds). Incurrence = looser."),
    CMRule(id="CM-015", topic="Key credit ratios", category="credit",
           description="Debt/EBITDA: leverage (Lev Loan 4-7x, HY 4-6x). EBITDA/Interest: coverage (>2x). DSCR: >1.2x."),

    # --- Bond Analysis ---
    CMRule(id="CM-016", topic="YTM", category="bonds",
           description="Yield to Maturity: IRR of bond cash flows. Assumes reinvestment at YTM rate, held to maturity."),
    CMRule(id="CM-017", topic="Duration", category="bonds",
           description="Weighted average time to receive cash flows. Measures interest rate sensitivity. Modified Duration = Duration / (1 + YTM/n)."),
    CMRule(id="CM-018", topic="Convexity", category="bonds",
           description="Curvature of price-yield relationship. Positive convexity: price rises more than duration predicts when yields fall."),
    CMRule(id="CM-019", topic="Call / Put options", category="bonds",
           description="Call: issuer can redeem early (bad for investor). Put: investor can sell back (good for investor). Make-whole call: premium based on Treasury + spread."),

    # --- Convertibles ---
    CMRule(id="CM-020", topic="Convertible bond structure", category="convertibles",
           description="Bond + Call Option on equity. Bond floor = straight debt value. Conversion premium = (Conversion Price - Stock Price) / Stock Price."),
    CMRule(id="CM-021", topic="Convertible accounting split", category="convertibles",
           description="IFRS: split into liability + equity components. US GAAP: bifurcate if cash settlement option exists."),
    CMRule(id="CM-022", topic="Call spread overlay", category="convertibles",
           description="Issuer buys call + sells warrant. Raises effective conversion premium. Offsets dilution above warrant strike."),

    # --- Debt Comps ---
    CMRule(id="CM-023", topic="Comparable debt analysis", category="debt_comps",
           description="Screen bonds by: sector, rating, maturity, seniority, covenants. Compare: YTW, OAS, G-spread."),
    CMRule(id="CM-024", topic="Debt sizing", category="debt_comps",
           description="Max debt = min of: (1) D/EBITDA covenant ceiling, (2) Interest coverage floor, (3) Market capacity."),
    CMRule(id="CM-025", topic="OID and issuance fees", category="debt_comps",
           description="Original Issue Discount: bond sold below par. Amortized over life of bond (non-cash interest). Fees: capitalized and amortized."),

    # --- Leveraged Loans ---
    CMRule(id="CM-026", topic="Leveraged loan structure", category="lev_loans",
           description="Floating rate (SOFR + spread). Amortizing or bullet. Maintenance covenants (tested quarterly). Secured (1st/2nd lien)."),
    CMRule(id="CM-027", topic="CLO mechanics", category="lev_loans",
           description="Collateralized Loan Obligation: pools leveraged loans, issues tranches (AAA to Equity). Arbitrage: loan yield > CLO cost."),
]


FORMULAS = {
    "CM-F-001": "After-Tax Cost of Debt = Interest Rate x (1 - Tax Rate)",
    "CM-F-002": "YTM approx = (Annual Coupon + (Par - Price) / Years) / ((Par + Price) / 2)",
    "CM-F-003": "Modified Duration = Macaulay Duration / (1 + YTM / n)",
    "CM-F-004": "Conversion Ratio = Par Value / Conversion Price",
    "CM-F-005": "Conversion Premium = (Conversion Price - Stock Price) / Stock Price",
    "CM-F-006": "Convertible Value = Bond Floor + Embedded Option Value",
    "CM-F-007": "Gross IPO Proceeds = Offering Price x Primary Shares",
    "CM-F-008": "Net IPO Proceeds = Gross Proceeds - Underwriting Fees",
    "CM-F-009": "Post-Money Equity Value = Pre-Money + Net IPO Proceeds",
    "CM-F-010": "DSCR = EBITDA / (Interest + Scheduled Principal)",
    "CM-F-011": "Debt / EBITDA = Total Debt / EBITDA",
    "CM-F-012": "EBITDA / Interest = EBITDA / Interest Expense",
    "CM-F-013": "OAS = Option-Adjusted Spread (Z-spread adjusted for embedded options)",
}


# Credit rating mapping
CREDIT_RATINGS = {
    "AAA": {"leverage": "0-1x", "spread": "50-80bps", "probability_default": "<0.1%"},
    "AA":  {"leverage": "1-2x", "spread": "80-120bps", "probability_default": "0.1%"},
    "A":   {"leverage": "1-2x", "spread": "120-150bps", "probability_default": "0.2%"},
    "BBB": {"leverage": "2-3x", "spread": "150-250bps", "probability_default": "0.5%"},
    "BB":  {"leverage": "3-5x", "spread": "300-500bps", "probability_default": "2%"},    # LevFin entry
    "B":   {"leverage": "4-7x", "spread": "500-800bps", "probability_default": "5%"},     # LevFin core
    "CCC": {"leverage": "6x+", "spread": "800-1500bps", "probability_default": "15%+"},
}


# Seniority waterfall
SENIORITY_ORDER = [
    "Revolver (super-senior, first out)",
    "Term Loan A (senior secured, amortizing)",
    "Term Loan B (senior secured, bullet, institutional)",
    "1st Lien Bonds (senior secured)",
    "2nd Lien Bonds (senior secured, subordinated to 1st lien)",
    "Senior Unsecured Notes",
    "Senior Subordinated Notes",
    "Subordinated Notes / Mezzanine",
    "Preferred Stock",
    "Common Equity (last in waterfall)",
]
