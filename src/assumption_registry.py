"""The single declared home for every forward-looking default in finmodel.

No module may invent a default inline (`x.get(k, 0.21)`); instead it asks the
registry, which returns an Assumption carrying the value AND its written
rationale + basis. A key the registry does not know returns None, and the
caller must then record UNVERIFIED rather than silently substitute.
"""
from __future__ import annotations

from dataclasses import dataclass
from typing import Optional


@dataclass
class Assumption:
    key: str
    value: float
    rationale: str
    basis: str


# Global (non-sector) declared assumptions.
_GLOBAL: dict[str, Assumption] = {
    "equity_risk_premium": Assumption(
        "equity_risk_premium", 0.055,
        "Long-run US equity risk premium", "Damodaran historical ERP"),
    "target_de_ratio": Assumption(
        "target_de_ratio", 0.30,
        "Sector-typical target capital structure", "house default"),
    "terminal_growth_rate": Assumption(
        "terminal_growth_rate", 0.025,
        "Long-run nominal GDP / inflation proxy", "house default"),
    "risk_free_rate": Assumption(
        "risk_free_rate", 0.045,
        "10Y Treasury proxy when live fetch unavailable", "fallback"),
    "revenue_growth_pct": Assumption(
        "revenue_growth_pct", 0.05, "Generic forward growth when no history", "fallback"),
    "gross_margin_pct": Assumption(
        "gross_margin_pct", 0.30, "Generic margin when no history", "fallback"),
    "sga_pct_rev": Assumption("sga_pct_rev", 0.10, "Generic SG&A% when no history", "fallback"),
    "rd_pct_rev": Assumption("rd_pct_rev", 0.05, "Generic R&D% when no history", "fallback"),
    "da_pct_rev": Assumption("da_pct_rev", 0.04, "Generic D&A% when no history", "fallback"),
    "capex_pct_rev": Assumption("capex_pct_rev", 0.05, "Generic capex% when no history", "fallback"),
    "tax_rate_pct": Assumption(
        "tax_rate_pct", 0.21, "US statutory corporate rate", "fallback when not derivable"),
    "interest_rate_pct": Assumption(
        "interest_rate_pct", 0.035, "Generic pre-tax cost of debt", "fallback when not derivable"),
    "dso_days": Assumption("dso_days", 45.0, "Generic receivable days", "fallback"),
    "dio_days": Assumption("dio_days", 60.0, "Generic inventory days", "fallback"),
    "dpo_days": Assumption("dpo_days", 50.0, "Generic payable days", "fallback"),
    "dividend_per_share": Assumption("dividend_per_share", 0.0, "No dividend assumed", "house default"),
}

# Sector-median levered beta (kills the 1.0 fallback in wacc.py / peers.py).
_SECTOR_BETA: dict[str, float] = {
    "standard": 1.10, "utility": 0.60, "bank": 1.15,
    "insurance": 0.95, "reit": 0.85,
}

# Mirrors assumptions.py _SECTOR_MULTIPLES base column (sector -> base exit mult).
_SECTOR_EXIT_MULT: dict[str, float] = {
    "standard": 16.0, "utility": 14.0, "bank": 12.0,
    "insurance": 12.0, "reit": 16.0,
}


def resolve(key: str, sector: Optional[str] = None) -> Optional[Assumption]:
    if key == "sector_beta":
        s = sector or "standard"
        if s in _SECTOR_BETA:
            return Assumption("sector_beta", _SECTOR_BETA[s],
                              f"Sector-median levered beta ({s})", "house sector table")
        return None
    if key == "exit_ebitda_multiple":
        s = sector or "standard"
        if s in _SECTOR_EXIT_MULT:
            return Assumption("exit_ebitda_multiple", _SECTOR_EXIT_MULT[s],
                              f"Sector-typical exit EBITDA multiple ({s})", "house sector table")
        return None
    return _GLOBAL.get(key)
