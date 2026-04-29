"""
M&A Merger Model Rules ported from ma-deals-merger-models.json.
20 rules (MA-001 to MA-016), 12 formulas (MA-F-001 to MA-F-012).
"""

from dataclasses import dataclass
from typing import Optional


@dataclass
class MARule:
    id: str
    topic: str
    description: str


RULES = {
    "MA-001": MARule(id="MA-001", topic="Strategic rationale",
        description="Acquire if target worth > purchase price. IRR must exceed hurdle rate (WACC + risk premium)."),
    "MA-002": MARule(id="MA-002", topic="Deal motivations (9 types)",
        description="1. Consolidation (scale). 2. Geographic expansion. 3. Market share. 4. Customer acquisition. "
                    "5. Product expansion. 6. IP/technology. 7. Defensive (buy or be bought). 8. Acqui-hire. 9. Ego/empire building."),
    "MA-003": MARule(id="MA-003", topic="Sell-side M&A process",
        description="1. Preparation (CIM, management presentation). 2. Buyer outreach. 3. Indicative bids. "
                    "4. Confirmatory DD. 5. Final bids + signing."),
    "MA-004": MARule(id="MA-004", topic="Buy-side M&A process",
        description="1. Target identification. 2. Initial approach. 3. Preliminary DD. 4. LOI. 5. Confirmatory DD + definitive agreement."),
    "MA-005": MARule(id="MA-005", topic="Financing methods (cheapest to most expensive)",
        description="Cash > Debt > Stock. Cash: cheapest, no dilution. Debt: interest tax shield, adds leverage. "
                    "Stock: most expensive (EPS dilution), but preserves cash/debt capacity."),
    "MA-006": MARule(id="MA-006", topic="EPS accretion/dilution",
        description="Accretive: Combined EPS > Acquirer EPS. Dilutive: Combined EPS < Acquirer EPS. "
                    "Depends on: P/E of target vs acquirer, financing cost, synergy assumptions."),
    "MA-007": MARule(id="MA-007", topic="Accretion/dilution calculation steps",
        description="1. Combined Pre-Tax Income = Acquirer PTI + Target PTI +/- Synergies - Incremental Interest. "
                    "2. Combined NI = Combined PTI x (1-t). "
                    "3. New Share Count = Acquirer shares + Shares issued for stock consideration. "
                    "4. Combined EPS = Combined NI / New Shares. "
                    "5. Accretion % = (Combined EPS - Acquirer EPS) / Acquirer EPS."),
    "MA-008": MARule(id="MA-008", topic="Financing limits",
        description="Max Cash = Acquirer Cash - Minimum Operating Cash. "
                    "Max Debt = (Max D/EBITDA x Combined EBITDA) - Existing Debt. "
                    "Stock: limited by accretion/dilution threshold."),
    "MA-009": MARule(id="MA-009", topic="True purchase price (PEV)",
        description="Purchase Enterprise Value = Offer Price x Shares + Target Net Debt. "
                    "Cash-free, debt-free: seller keeps cash, repays debt at close."),
    "MA-010": MARule(id="MA-010", topic="Cash-free debt-free deals",
        description="Seller retains all cash and repays all debt at close. Buyer pays equity value only. "
                    "Most common in private M&A. Working capital peg adjustment often included."),
    "MA-011": MARule(id="MA-011", topic="EV changes in M&A",
        description="Acquirer EV post-deal = Pre-deal EV + Target EV. Target shareholders get eqv + cash + debt assumed. "
                    "Consolidation: balance sheets combined, intercompany eliminated."),
    "MA-012": MARule(id="MA-012", topic="Merger model walkthrough (8 steps)",
        description="1. Determine offer price + form of consideration. 2. Calculate goodwill + intangibles created. "
                    "3. Combine IS line by line. 4. Add incremental D&A from PPA. 5. Add incremental interest. "
                    "6. Calculate combined NI. 7. Calculate new share count. 8. Combined EPS -> accretion/dilution."),
    "MA-013": MARule(id="MA-013", topic="Value creation sources",
        description="1. Synergies (cost + revenue). 2. Multiple expansion (buy at lower multiple than sell/exit). "
                    "3. EBITDA growth (organic + acquired). 4. Financial engineering (optimal capital structure, tax)."),
    "MA-014": MARule(id="MA-014", topic="Contribution analysis",
        description="Post-merger ownership split: % = Party A EBITDA / Combined EBITDA (or Revenue/EBIT). "
                    "Determines control premium, board seats, governance."),
    "MA-015": MARule(id="MA-015", topic="Synergy types",
        description="Cost synergies: headcount, procurement, facility consolidation, systems. "
                    "Revenue synergies: cross-selling, pricing power, geographic reach, product bundling. "
                    "Cost synergies are more reliable (60-80% realized). Revenue synergies are riskier (30-50% realized)."),
    "MA-016": MARule(id="MA-016", topic="Goodwill and PPA in M&A",
        description="Purchase Price - Fair Value of Net Assets = Goodwill. "
                    "PPA identifies: identifiable intangibles (customer lists, tech, brand), PPE step-up, above/below market contracts. "
                    "D&A from stepped-up assets reduces combined NI post-close."),
}


FORMULAS = {
    "MA-F-001": {
        "name": "Purchase Equity Value",
        "formula": "PEV = Offer Price per Share x Target Shares Outstanding",
    },
    "MA-F-002": {
        "name": "Purchase Enterprise Value",
        "formula": "PEV = Purchase Equity Value + Target Net Debt",
    },
    "MA-F-003": {
        "name": "Accretion / Dilution %",
        "formula": "(Combined EPS - Acquirer Standalone EPS) / Acquirer Standalone EPS",
    },
    "MA-F-004": {
        "name": "Combined EPS",
        "formula": "Combined Net Income / New Share Count",
    },
    "MA-F-005": {
        "name": "Max New Debt",
        "formula": "Max D/EBITDA x Combined EBITDA - Existing Debt",
    },
    "MA-F-006": {
        "name": "Max Cash Available",
        "formula": "Acquirer Cash - Minimum Operating Cash",
    },
    "MA-F-007": {
        "name": "After-Tax Cost of Financing",
        "formula": "Pre-Tax Cost x (1 - Tax Rate)",
    },
    "MA-F-008": {
        "name": "IRR (M&A investment)",
        "formula": "IRR where NPV of (Cash Inflows - Outflows) = 0",
    },
    "MA-F-009": {
        "name": "Synergy Break-Even",
        "formula": "Deal Cost x (WACC - Growth) / (WACC x (1 - t))",
    },
    "MA-F-010": {
        "name": "Combined EBITDA",
        "formula": "Buyer EBITDA + Seller EBITDA +/- Synergies",
    },
    "MA-F-011": {
        "name": "Contribution %",
        "formula": "Party A EBITDA / Combined EBITDA",
    },
    "MA-F-012": {
        "name": "New Shares Issued",
        "formula": "Stock Portion of Consideration / Buyer Share Price",
    },
}


def accretion_dilution(acquirer_ni: float, target_ni: float,
                       acquirer_shares: float, target_shares: float,
                       offer_price: float, cash_portion: float = 0.0,
                       stock_portion: float = 0.0,
                       synergies: float = 0.0,
                       incremental_interest: float = 0.0,
                       tax_rate: float = 0.25) -> dict:
    """
    Calculate M&A EPS accretion/dilution.

    Args:
        acquirer_ni: Acquirer standalone net income
        target_ni: Target standalone net income
        acquirer_shares: Acquirer diluted shares
        target_shares: Target diluted shares outstanding
        offer_price: Offer price per target share
        cash_portion: Cash used to fund deal
        stock_portion: Stock value used to fund deal
        synergies: Annual pre-tax synergies
        incremental_interest: Incremental interest on new debt (after-tax)
        tax_rate: Effective tax rate

    Returns dict with combined EPS, acquirer EPS, accretion/dilution %.
    """
    # After-tax synergies
    synergies_after_tax = synergies * (1 - tax_rate)

    # Combined net income
    combined_ni = acquirer_ni + target_ni + synergies_after_tax - incremental_interest

    # New shares issued for stock consideration
    acquirer_share_price = (offer_price * target_shares - cash_portion) / target_shares if target_shares > 0 else 0
    if acquirer_share_price > 0 and stock_portion > 0:
        new_shares = stock_portion / acquirer_share_price
    else:
        new_shares = 0

    combined_shares = acquirer_shares + new_shares

    # Per-share metrics
    acquirer_eps = acquirer_ni / acquirer_shares if acquirer_shares > 0 else 0
    combined_eps = combined_ni / combined_shares if combined_shares > 0 else 0

    accretion_pct = (combined_eps - acquirer_eps) / acquirer_eps * 100 if acquirer_eps > 0 else 0

    return {
        "acquirer_eps": round(acquirer_eps, 4),
        "combined_eps": round(combined_eps, 4),
        "accretion_dilution_pct": round(accretion_pct, 1),
        "accretive": accretion_pct > 0,
        "combined_ni": round(combined_ni, 2),
        "new_shares_issued": round(new_shares, 2),
        "combined_shares": round(combined_shares, 2),
    }
