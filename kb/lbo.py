"""
LBO Model Rules ported from lbo-models.json.
16 rules (lbo-001 to lbo-016), 12 formulas (f-lbo-001 to f-lbo-012).
"""

from dataclasses import dataclass
from typing import Optional


@dataclass
class LBORule:
    id: str
    topic: str
    description: str


@dataclass
class LBOFormula:
    id: str
    name: str
    formula: str
    description: str = ""


RULES = {
    "lbo-001": LBORule(id="lbo-001", topic="LBO structure",
        description="Use Debt + Equity to acquire target, operate 3-7 years, exit via sale/IPO for IRR."),
    "lbo-002": LBORule(id="lbo-002", topic="Leverage amplifies returns",
        description="Leverage amplifies returns in both directions. Higher debt = higher IRR if exit EV grows, lower if it shrinks."),
    "lbo-003": LBORule(id="lbo-003", topic="HoldCo structure",
        description="Acquired company borrows the debt, not the PE firm. HoldCo -> OpCo. Debt sits at OpCo level."),
    "lbo-004": LBORule(id="lbo-004", topic="Ideal LBO candidates",
        description="Stable cash flows, low CapEx requirements, strong management, identifiable cost savings, non-cyclical."),
    "lbo-005": LBORule(id="lbo-005", topic="Purchase price",
        description="Private company: EBITDA x EBITDA Multiple. Public company: share price x shares + premium."),
    "lbo-006": LBORule(id="lbo-006", topic="Sources & Uses",
        description="Sources (Debt + Equity) = Uses (Purchase Price + Fees + Refinancing). Must balance."),
    "lbo-007": LBORule(id="lbo-007", topic="FCF in LBO",
        description="FCF = NI + D&A +/- Change in WC - CapEx. Used to repay debt (cash sweep)."),
    "lbo-008": LBORule(id="lbo-008", topic="CFADS",
        description="Cash Flow Available for Debt Service = Beginning Cash + FCF - Min Cash - Mandatory Repayments."),
    "lbo-009": LBORule(id="lbo-009", topic="Interest linkage",
        description="Interest changes as debt balance changes (circular reference). Model must iterate: debt -> interest -> NI -> FCF -> debt repayment."),
    "lbo-010": LBORule(id="lbo-010", topic="Exit",
        description="Exit EV = Exit Multiple x Exit EBITDA. Exit Equity = Exit EV - Net Debt + Cash."),
    "lbo-011": LBORule(id="lbo-011", topic="IRR calculation",
        description="IRR = (Exit Equity / Investor Equity)^(1/years) - 1. XIRR for irregular cash flows."),
    "lbo-012": LBORule(id="lbo-012", topic="IRR rules of thumb",
        description="2x/3yr = ~26% IRR. 2x/5yr = ~15% IRR. 3x/3yr = ~44% IRR. 3x/5yr = ~25% IRR."),
    "lbo-013": LBORule(id="lbo-013", topic="Public company LBO",
        description="Purchase price based on share price + premium (20-40%). Higher entry multiple. Premium reduces returns."),
    "lbo-014": LBORule(id="lbo-014", topic="LBO sets floor valuation",
        description="Max purchase price = EBITDA x Multiple that still delivers target IRR given exit assumptions."),
    "lbo-015": LBORule(id="lbo-015", topic="LBO vs M&A",
        description="LBO: only Debt + Equity as funding. Focus on IRR not EPS. M&A: Cash/Debt/Stock funding. Focus on accretion/dilution."),
    "lbo-016": LBORule(id="lbo-016", topic="Three return drivers",
        description="1. Multiple Expansion (buy low, sell high). 2. EBITDA Growth (operational improvement). 3. Debt Paydown (FCF sweep)."),
}


FORMULAS = {
    "f-lbo-001": LBOFormula(id="f-lbo-001", name="Purchase EV",
        formula="EV = EBITDA x Entry Multiple",
        description="Enterprise value at acquisition."),
    "f-lbo-002": LBOFormula(id="f-lbo-002", name="Investor Equity",
        formula="Investor Equity = Purchase EV - New Debt + Fees",
        description="Equity check size from PE fund."),
    "f-lbo-003": LBOFormula(id="f-lbo-003", name="FCF from NI",
        formula="FCF = NI + D&A +/- Change in WC - CapEx",
        description="Free cash flow for debt repayment."),
    "f-lbo-004": LBOFormula(id="f-lbo-004", name="FCF from EBITDA",
        formula="FCF = EBITDA - Net Interest - Cash Taxes +/- Change WC - CapEx",
        description="Alternative FCF calculation from EBITDA."),
    "f-lbo-005": LBOFormula(id="f-lbo-005", name="CFADS",
        formula="CFADS = Beginning Cash + FCF - Min Cash - Mandatory Repayments",
        description="Cash available for optional debt repayment."),
    "f-lbo-006": LBOFormula(id="f-lbo-006", name="Debt Repaid",
        formula="Debt Repaid = MIN(CFADS, Remaining Debt)",
        description="Cash sweep: repay as much debt as possible."),
    "f-lbo-007": LBOFormula(id="f-lbo-007", name="Exit EV",
        formula="Exit EV = Exit Multiple x Exit EBITDA",
        description="Enterprise value at exit (typically year 5)."),
    "f-lbo-008": LBOFormula(id="f-lbo-008", name="Exit Equity Proceeds",
        formula="Exit Equity = Exit EV - Net Debt + Cash",
        description="What the PE firm receives at exit."),
    "f-lbo-009": LBOFormula(id="f-lbo-009", name="MoM",
        formula="MoM = Exit Equity / Investor Equity",
        description="Multiple of Money. Target: 2.0-3.0x."),
    "f-lbo-010": LBOFormula(id="f-lbo-010", name="IRR",
        formula="IRR = (Exit Equity / Investor Equity)^(1/Years) - 1",
        description="Annualized return. Target: 20-25%+."),
    "f-lbo-011": LBOFormula(id="f-lbo-011", name="ROIC",
        formula="ROIC = NOPAT / Average Invested Capital",
        description="Return on capital employed in the business."),
    "f-lbo-012": LBOFormula(id="f-lbo-012", name="Effective Purchase Price (public)",
        formula="Purchase EV = Equity Value + Debt - Excess Cash + Fees",
        description="True cost for public company LBO including assumed debt."),
}


# LBO Metrics
KEY_METRICS = [
    "IRR (target 20-25%+)",
    "MoM / Multiple of Money (target 2.0-3.0x)",
    "Entry EV/EBITDA",
    "Exit EV/EBITDA",
    "Debt/EBITDA at entry (5-7x typical)",
    "Debt/EBITDA at exit (2-4x target)",
    "FCF Yield (FCF/EV)",
    "Equity contribution % (30-50% typical)",
]


def quick_lbo_math(entry_ebitda: float, entry_multiple: float,
                   exit_multiple: float, debt_financing: float,
                   years: int = 5, ebitda_growth: float = 0.05) -> dict:
    """
    Quick LBO return estimation.
    Assumes no debt paydown (simplified).
    """
    purchase_ev = entry_ebitda * entry_multiple
    investor_equity = purchase_ev - debt_financing
    exit_ebitda = entry_ebitda * (1 + ebitda_growth) ** years
    exit_ev = exit_ebitda * exit_multiple
    exit_equity = exit_ev - debt_financing  # Simplified: no paydown
    mom = exit_equity / investor_equity if investor_equity > 0 else 0
    irr = (mom ** (1 / years) - 1) if mom > 0 else 0

    return {
        "purchase_ev": purchase_ev,
        "investor_equity": investor_equity,
        "exit_ev": exit_ev,
        "exit_equity": exit_equity,
        "mom": round(mom, 2),
        "irr": round(irr * 100, 1),
        "entry_ebitda": entry_ebitda,
        "exit_ebitda": exit_ebitda,
    }
