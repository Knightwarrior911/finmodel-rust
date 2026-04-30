"""
EV Bridge rules ported from equity-enterprise-value.json (Breaking Into Wall Street).
Rules R-001 through R-018, formulas F-001 through F-028.

CRITICAL: These are non-negotiable. Apply exactly as stated.
"""

from dataclasses import dataclass, field
from typing import Optional


@dataclass
class Formula:
    id: str
    name: str
    formula: str
    conditions: list[str] = field(default_factory=list)
    application: str = ""
    practical_note: str = ""


@dataclass
class Rule:
    id: str
    topic: str
    description: str
    enforcement: str = ""  # How to enforce/verify


# --- FORMULAS ---

FORMULAS = {
    "F-001": Formula(
        id="F-001",
        name="Core company value formula",
        formula="Company Value = Cash Flow / (Discount Rate - Cash Flow Growth Rate)",
        conditions=["Growth rate must be less than discount rate"],
        application="Conceptual foundation for implied equity value and enterprise value.",
        practical_note=(
            "Shares = latest filing weighted average basic shares, NOT period-end count "
            "from annual report. For non-calendar-year companies, use most recent filing."
        ),
    ),
    "F-002": Formula(
        id="F-002",
        name="Current Equity Value (public company)",
        formula="Equity Value = Current Share Price x Basic or Diluted Shares Outstanding",
        application="Used for public companies when market pricing exists.",
        practical_note=(
            "ALWAYS use PRIMARY LISTING price in home currency. European companies on "
            "Euronext Paris/LSE: use EUR/GBP price, NOT USD ADR. Sanity-check: "
            "market cap / LTM Revenue in sector-normal range."
        ),
    ),
    "F-004": Formula(
        id="F-004",
        name="Enterprise Value bridge",
        formula=(
            "Enterprise Value = Equity Value - Non-Operating Assets "
            "+ Liability and Equity Items Representing Other Investor Groups"
        ),
        application="Standard bridge from equity value to enterprise value.",
        practical_note=(
            "EV bridge is a CHECKLIST not a template. Only include items that are "
            "(a) present, (b) material, and (c) disclosed. Do not force zeros."
        ),
    ),
    "F-018": Formula(
        id="F-018",
        name="Unfunded pension adjustment",
        formula="max(0, Pension Liabilities - Pension Assets)",
        application="Only defined-benefit pension plans. Do NOT add overfunded pensions.",
        practical_note="ALWAYS source from 10-K NOTES section, NOT balance sheet XBRL tag.",
    ),
    "F-010": Formula(
        id="F-010",
        name="EBITDA",
        formula="EBITDA = EBIT + Depreciation & Amortization",
        application="Standard EBITDA calculation from EBIT.",
    ),
    "F-022": Formula(
        id="F-022", name="P/E multiple",
        formula="P/E = Equity Value / Net Income = Price / EPS",
    ),
    "F-023": Formula(
        id="F-023", name="TEV/EBITDA",
        formula="Enterprise Value / EBITDA",
    ),
    "F-024": Formula(
        id="F-024", name="TEV/EBIT",
        formula="Enterprise Value / EBIT",
    ),
    "F-025": Formula(
        id="F-025", name="TEV/Revenue",
        formula="Enterprise Value / Revenue",
    ),
    "F-006": Formula(
        id="F-006",
        name="Treasury Stock Method dilution",
        formula="Net New Shares = ITM Options - ((Exercise Price x ITM Options) / Share Price)",
        application="Calculates dilution from options and warrants.",
    ),
    "F-012": Formula(
        id="F-012",
        name="Free Cash Flow",
        formula="FCF = CFO - CapEx",
    ),
    "F-014": Formula(
        id="F-014",
        name="Unlevered Free Cash Flow (FCFF)",
        formula="UFCF = EBIT x (1-t) + D&A + Non-Cash Adj +/- Change in WC - CapEx",
    ),
}


# --- RULES ---

RULES = {
    "R-014": Rule(
        id="R-014",
        topic="Equity investments and goodwill",
        description=(
            "Subtract equity-method investments (20-50% stakes) from EV bridge. "
            "Add Non-Controlling Interest. Do NOT subtract goodwill — the acquired "
            "business is still operating."
        ),
        enforcement="Check if equity investments on balance sheet. If consolidated (>50%), use NCI.",
    ),
    "R-015": Rule(
        id="R-015",
        topic="Pension — NOTES SECTION ONLY",
        description=(
            "The pension liability on the balance sheet may include other provisions "
            "(warranty, restructuring, OPEB) and is NOT the true net defined-benefit "
            "pension liability. ALWAYS source pension from the NOTES section (pension "
            "footnote: 'Defined benefit plans', 'Pension commitments'). "
            "Find PBO, plan assets, and net funded status. "
            "Formula: max(0, PBO − Plan Assets). Skip if overfunded. "
            "Use net aggregate (all plans combined), NOT the subset table."
        ),
        enforcement=(
            "CONFIRM: number came from 10-K NOTES section (pension footnote table). "
            "NOT from balance sheet XBRL tag. "
            "Use max(0, Total Pension Liability − Total Plan Assets). "
            "If overfunded (assets > liability), add $0."
        ),
    ),
    "R-016": Rule(
        id="R-016",
        topic="Operating Leases — ASC 842 / IFRS 16 Note",
        description=(
            "Use the ASC 842/IFRS 16 lease footnote (not the balance sheet operating "
            "lease liability line). Operating lease liability from the note = EV bridge "
            "addition. Finance lease liabilities also go in debt."
        ),
        enforcement="Source from lease footnote, not BS line item.",
    ),
    "R-009": Rule(
        id="R-009",
        topic="Equity Value to Enterprise Value bridge",
        description=(
            "EV Bridge checklist (conditional — only include if present, material, disclosed):\n"
            "ADD: +Total Debt, +Capital Leases (IFRS 16/ASC 842), +Underfunded Pensions "
            "(from NOTES), +Minority Interest, +Preferred Stock\n"
            "SUBTRACT: -Cash & Equivalents, -Short-term Investments, "
            "-Equity Method Investments/Associates (non-operating), "
            "-Financial Investments, -Assets Held for Sale, -Discontinued Ops Assets, "
            "-NOL Deferred Tax Assets"
        ),
    ),
    "R-012": Rule(
        id="R-012",
        topic="Pairing multiples correctly",
        description=(
            "If denominator deducts interest (Net Income, FCFE), use Equity Value. "
            "If denominator does NOT deduct interest (EBIT, EBITDA, Revenue, FCFF), "
            "use Enterprise Value."
        ),
    ),
}


# --- EV BRIDGE BUILDER ---

EV_BRIDGE_ADD_ITEMS = [
    ("total_debt", "Total Debt", "Balance sheet"),
    ("capital_leases", "Capital/Finance Leases", "ASC 842 / IFRS 16 note"),
    ("operating_leases", "Operating Leases", "ASC 842 / IFRS 16 note (R-016)"),
    ("underfunded_pension", "Underfunded Pension", "10-K pension footnote ONLY (R-015)"),
    ("minority_interest", "Minority Interest (NCI)", "Balance sheet"),
    ("preferred_stock", "Preferred Stock", "Balance sheet"),
]

EV_BRIDGE_SUBTRACT_ITEMS = [
    ("cash", "Cash & Equivalents", "Balance sheet"),
    ("short_term_investments", "Short-term Investments", "Balance sheet"),
    ("equity_investments", "Equity Investments/Associates", "Balance sheet (R-014)"),
    ("financial_investments", "Financial Investments (non-operating)", "Balance sheet"),
    ("assets_held_for_sale", "Assets Held for Sale", "Balance sheet"),
    ("discontinued_ops", "Discontinued Operations Assets", "Balance sheet"),
    ("nol_dta", "NOL Deferred Tax Assets", "Balance sheet"),
]

EV_BRIDGE_DO_NOT_TOUCH = [
    "goodwill",
    "ordinary_intangibles",
    "ordinary_dtl",
    "most_provisions",
    "industry_specific_operating_assets",
]


def build_ev_bridge(financials) -> dict:
    """
    Build EV bridge from Financials dataclass.
    Returns dict with each line item, amount, and whether it was included.
    Only includes items that are present, material, and disclosed.
    """
    bridge = {"market_cap": None, "additions": [], "subtractions": [], "total_ev": None}

    # Market cap is required
    bridge["market_cap"] = financials.market_cap

    ev = financials.market_cap or 0

    # Add items
    for attr, label, source in EV_BRIDGE_ADD_ITEMS:
        val = getattr(financials, attr, None)
        if val and val != 0:
            bridge["additions"].append({"item": label, "amount": val, "source": source})
            ev += val

    # Subtract items
    for attr, label, source in EV_BRIDGE_SUBTRACT_ITEMS:
        val = getattr(financials, attr, None)
        if val and val != 0:
            bridge["subtractions"].append({"item": label, "amount": val, "source": source})
            ev -= val

    bridge["total_ev"] = ev
    return bridge


def compute_unfunded_pension(pbo: Optional[float], plan_assets: Optional[float],
                             tax_rate: float = 0.0, tax_adjusted: bool = False) -> float:
    """
    R-015 + F-018 + F-019: Compute underfunded pension for EV bridge.
    ALWAYS use PBO + plan assets from pension footnote (NOT balance sheet).
    Returns 0 if overfunded.
    """
    if pbo is None or plan_assets is None:
        return 0.0

    net = max(0, pbo - plan_assets)
    if tax_adjusted:
        net = net * (1 - tax_rate)
    return net


# --- EV BRIDGE FORMATTER ---

@dataclass
class EVBridgeInput:
    """Inputs for EV bridge calculation and formatting."""
    company: str = ""
    period: str = ""
    currency: str = "USD"

    # Market cap components
    share_price: Optional[float] = None
    shares_outstanding: Optional[float] = None  # Weighted avg basic, latest filing (F-001)
    market_cap: Optional[float] = None          # Computed if price + shares given

    # ADD items (checklist — only if present, material, disclosed)
    total_debt: Optional[float] = None
    finance_leases: Optional[float] = None       # ASC 842 / IFRS 16 finance/capital lease liability
    operating_leases: Optional[float] = None     # ASC 842 / IFRS 16 operating lease liability (R-016)
    underfunded_pension: Optional[float] = None  # R-015: from pension footnote ONLY, NOT BS tag
    minority_interest: Optional[float] = None
    preferred_stock: Optional[float] = None

    # SUBTRACT items
    cash: Optional[float] = None
    short_term_investments: Optional[float] = None
    equity_investments: Optional[float] = None   # R-014: equity method investments (non-operating)
    financial_investments: Optional[float] = None
    assets_held_for_sale: Optional[float] = None
    discontinued_ops_assets: Optional[float] = None
    nol_dta: Optional[float] = None

    # DO NOT TOUCH (explicitly excluded per rules)
    goodwill: Optional[float] = None             # R-014: NOT subtracted
    ordinary_intangibles: Optional[float] = None
    pension_bs_tag: Optional[float] = None       # R-015: BS tag is WRONG, shows this as warning

    # For multiples
    ltm_revenue: Optional[float] = None
    ltm_ebitda: Optional[float] = None
    ltm_ebit: Optional[float] = None

    # Source references — text labels for cell comments
    notes_ref: dict = field(default_factory=dict)
    # PDF URLs per field — used for clickable hyperlinks in Excel source column
    field_urls: dict = field(default_factory=dict)

    @property
    def computed_market_cap(self) -> Optional[float]:
        if self.market_cap:
            return self.market_cap
        if self.share_price and self.shares_outstanding:
            return self.share_price * self.shares_outstanding
        return None


def _fmt(val: Optional[float]) -> str:
    """Format large numbers with B/M suffix for readability."""
    if val is None:
        return "N/A"
    v = abs(val)
    if v >= 1e12:
        return f"{val/1e12:,.2f}T"
    elif v >= 1e9:
        return f"{val/1e9:,.1f}B"
    elif v >= 1e6:
        return f"{val/1e6:,.1f}M"
    elif v >= 1e3:
        return f"{val/1e3:,.1f}K"
    return f"{val:,.0f}"


def _fmt_num(val: Optional[float]) -> str:
    """Format number with full commas for the bridge (raw amounts)."""
    if val is None:
        return "N/A"
    return f"{val:>12,.0f}"


def format_ev_bridge(ev_input: EVBridgeInput) -> str:
    """
    Format a full Enterprise Value bridge with every line item, value, source, and rule reference.
    Only includes items that are present, material, and disclosed.
    Items not present are explicitly listed as excluded with reasons.
    """
    notes = ev_input.notes_ref or {}
    curr = ev_input.currency

    lines = []
    lines.append("=" * 70)
    header = f"{ev_input.company} {ev_input.period} -- Enterprise Value Bridge"
    lines.append(header)
    lines.append("=" * 70)
    lines.append("Rules applied: R-009, R-014, R-015, R-016, F-001, F-002, F-004")
    lines.append("")

    mc = ev_input.computed_market_cap
    if mc is None:
        lines.append("ERROR: Market cap required.")
        return "\n".join(lines)

    mc_fmt = _fmt(mc)

    # --- SECTION 1: EQUITY VALUE ---
    lines.append("EQUITY VALUE")
    lines.append("-" * 50)
    if ev_input.share_price and ev_input.shares_outstanding:
        shares_fmt = _fmt(ev_input.shares_outstanding)
        lines.append(f"  Share Price                           {ev_input.share_price:>12,.2f} {curr}")
        lines.append(f"    Source: {notes.get('share_price', 'Primary exchange')}")
        lines.append(f"  x Shares Outstanding (wtd avg basic)  {shares_fmt:>12}")
        lines.append(f"    Source: {notes.get('shares', 'Latest filing (F-001)')}")
        lines.append(f"  {'-' * 50}")
    lines.append(f"  = Market Cap (Equity Value)            {mc_fmt:>12} {curr}")
    lines.append("")

    # --- SECTION 2: ENTERPRISE VALUE BRIDGE ---
    lines.append("ENTERPRISE VALUE BRIDGE")
    lines.append("-" * 50)
    lines.append(f"  Market Cap                            {mc_fmt:>12}")
    ev = mc

    # ADD items
    add_items = [
        (ev_input.total_debt, "Total Debt",
         notes.get('total_debt', 'Balance sheet')),
        (ev_input.finance_leases, "Finance/Capital Lease Liabilities",
         notes.get('finance_leases', 'ASC 842 / IFRS 16 note')),
        (ev_input.operating_leases, "Operating Lease Liabilities (R-016)",
         notes.get('operating_leases', 'ASC 842 / IFRS 16 note (R-016)')),
        (ev_input.underfunded_pension, "Underfunded Pension (R-015)",
         notes.get('pension', 'Pension footnote ONLY - NOT balance sheet (R-015)')),
        (ev_input.minority_interest, "Minority Interest (NCI)",
         notes.get('nci', 'Balance sheet')),
        (ev_input.preferred_stock, "Preferred Stock",
         notes.get('preferred', 'Balance sheet')),
    ]

    for val, label, source in add_items:
        if val and val > 0:
            lines.append(f"  + {label:<36} {_fmt(val):>12}")
            lines.append(f"    Source: {source}")
            ev += val

    # SUBTRACT items
    sub_items = [
        (ev_input.cash, "Cash & Cash Equivalents",
         notes.get('cash', 'Balance sheet')),
        (ev_input.short_term_investments, "Short-term Investments",
         notes.get('st_inv', 'Balance sheet')),
        (ev_input.equity_investments, "Equity Method Investments (R-014)",
         notes.get('equity_inv', 'Balance sheet - non-operating (R-014)')),
        (ev_input.financial_investments, "Financial Investments (non-operating)",
         notes.get('fin_inv', 'Balance sheet')),
        (ev_input.assets_held_for_sale, "Assets Held for Sale",
         notes.get('held_sale', 'Balance sheet')),
        (ev_input.discontinued_ops_assets, "Discontinued Operations Assets",
         notes.get('disc_ops', 'Balance sheet')),
        (ev_input.nol_dta, "NOL Deferred Tax Assets",
         notes.get('nol', 'Balance sheet')),
    ]

    for val, label, source in sub_items:
        if val and val > 0:
            lines.append(f"  - {label:<36} {_fmt(val):>12}")
            lines.append(f"    Source: {source}")
            ev -= val

    lines.append(f"  {'-' * 50}")
    lines.append(f"  = ENTERPRISE VALUE                    {_fmt(ev):>12} {curr}")
    lines.append("")

    # --- SECTION 3: MULTIPLES ---
    if any([ev_input.ltm_revenue, ev_input.ltm_ebitda, ev_input.ltm_ebit]):
        lines.append("VALUATION MULTIPLES")
        lines.append("-" * 50)
        if ev_input.ltm_revenue and ev_input.ltm_revenue > 0:
            lines.append(f"  EV / LTM Revenue                      {ev / ev_input.ltm_revenue:>12,.1f}x")
        if ev_input.ltm_ebitda and ev_input.ltm_ebitda > 0:
            lines.append(f"  EV / LTM EBITDA                       {ev / ev_input.ltm_ebitda:>12,.1f}x")
        if ev_input.ltm_ebit and ev_input.ltm_ebit > 0:
            lines.append(f"  EV / LTM EBIT                         {ev / ev_input.ltm_ebit:>12,.1f}x")
        if mc and ev_input.ltm_revenue and ev_input.ltm_revenue > 0:
            lines.append(f"  Market Cap / LTM Revenue               {mc / ev_input.ltm_revenue:>12,.1f}x")
        lines.append("")

    # --- SECTION 4: ITEMS EXCLUDED ---
    lines.append("ITEMS EXCLUDED FROM BRIDGE")
    lines.append("-" * 50)

    excluded = []

    add_checks = [
        (ev_input.total_debt, "Total Debt", "Not present / immaterial"),
        (ev_input.finance_leases, "Finance Leases", "Not disclosed / immaterial"),
        (ev_input.operating_leases, "Operating Leases", "Not disclosed / immaterial"),
        (ev_input.underfunded_pension, "Underfunded Pension", "Fully funded or no DB plan (R-015)"),
        (ev_input.minority_interest, "Minority Interest", "No consolidated subs with NCI"),
        (ev_input.preferred_stock, "Preferred Stock", "Not issued / immaterial"),
    ]
    for val, label, reason in add_checks:
        if not val or val == 0:
            excluded.append((label, reason))

    sub_checks = [
        (ev_input.cash, "Cash & Equivalents", "Zero / immaterial"),
        (ev_input.short_term_investments, "Short-term Investments", "Not disclosed / immaterial"),
        (ev_input.equity_investments, "Equity Method Investments", "No equity method stakes (R-014)"),
        (ev_input.financial_investments, "Financial Investments", "Not material non-operating"),
        (ev_input.assets_held_for_sale, "Assets Held for Sale", "None classified"),
        (ev_input.discontinued_ops_assets, "Discontinued Ops Assets", "No discontinued ops"),
        (ev_input.nol_dta, "NOL Deferred Tax Assets", "No NOLs or immaterial"),
    ]
    for val, label, reason in sub_checks:
        if not val or val == 0:
            excluded.append((label, reason))

    excluded.append(("Goodwill (R-014)", "Operating asset - NOT subtracted from EV"))
    if ev_input.pension_bs_tag and ev_input.pension_bs_tag > 0:
        excluded.append((
            f"Pension BS tag ({_fmt(ev_input.pension_bs_tag)})",
            "Balance sheet XBRL tag is WRONG - use pension footnote only (R-015)"
        ))

    for label, reason in excluded:
        lines.append(f"  X  {label:<45} - {reason}")

    lines.append("")
    lines.append("=" * 70)
    lines.append("EV bridge is a CHECKLIST, not a template.")
    lines.append("Only items present, material, and disclosed are included.")
    lines.append("=" * 70)
    return "\n".join(lines)
