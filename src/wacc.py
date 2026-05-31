"""
WACC builder.

Per dcf/SPEC_methodology section 5:
  1. Unlever each peer beta:  Bu = Be / (1 + (1 - t) × D/E)
  2. Take median (or mean) unlevered beta of peer set
  3. Re-lever to target capital structure:  Be_target = Bu_med × (1 + (1 - t) × D/E_target)
  4. CAPM cost of equity:  Ke = Rf + Be × ERP
  5. After-tax cost of debt:  Kd_at = Kd × (1 - t)
  6. WACC = We × Ke + Wd × Kd_at
"""
import logging
import statistics

logger = logging.getLogger(__name__)


def _unlever_beta(levered_beta: float, de_ratio: float, tax_rate: float) -> float:
    """Bu = Be / (1 + (1 - t) × D/E)"""
    denom = 1 + (1 - tax_rate) * de_ratio
    return levered_beta / denom if denom else levered_beta


def _relever_beta(unlevered_beta: float, target_de: float, target_tax: float) -> float:
    """Be_target = Bu × (1 + (1 - t) × D/E_target)"""
    return unlevered_beta * (1 + (1 - target_tax) * target_de)


def compute_wacc(
    peer_set,                                 # PeerSet
    target_market_cap: float,                 # $M
    target_debt: float,                       # $M (long-term debt last historical)
    risk_free_rate: float,
    equity_risk_premium: float,
    cost_of_debt_pretax: float,
    target_tax_rate: float = 0.21,
    target_de_ratio: float | None = None,
    fallback_beta: float = 1.0,
    sector: str = "standard",
    ledger=None,
):
    """
    Returns WACCOutput with full peer-set unlever/relever + CAPM build-up.

    Falls back to single-ticker beta when peer_set has no peers (LLM unavailable).
    """
    from schemas.financial_data import WACCOutput

    from src.assumption_registry import resolve as _resolve

    if peer_set.peers:
        unlevered_betas = [
            _unlever_beta(p.levered_beta, p.de_ratio, p.tax_rate)
            for p in peer_set.peers
        ]
        median_bu = statistics.median(unlevered_betas)
        if ledger is not None:
            ledger.record_derived(
                "wacc", "median_unlevered_beta", None, value=round(median_bu, 4),
                formula="median(unlever(peer.beta))",
                inputs=[("peers", p.ticker, None) for p in peer_set.peers],
            )
    else:
        a = _resolve("sector_beta", sector=sector)
        median_bu = a.value if a else fallback_beta
        logger.warning("No peers in set — using sector-median beta %.2f", median_bu)
        if ledger is not None:
            if a:
                ledger.record_assumption("wacc", "median_unlevered_beta", None,
                                         value=round(median_bu, 4),
                                         rationale=a.rationale, basis=a.basis)
            else:
                ledger.record_unverified("wacc", "median_unlevered_beta", None,
                                         value=round(median_bu, 4),
                                         reason="no peers and no sector beta declared")

    de = target_de_ratio if target_de_ratio is not None else peer_set.target_de_ratio
    target_levered_beta = _relever_beta(median_bu, de, target_tax_rate)

    # CAPM
    cost_of_equity = risk_free_rate + target_levered_beta * equity_risk_premium

    # After-tax cost of debt
    after_tax_kd = cost_of_debt_pretax * (1 - target_tax_rate)

    # Capital structure weights — use market values
    total_capital = target_market_cap + target_debt
    if total_capital <= 0:
        equity_weight = 1.0
        debt_weight = 0.0
    else:
        equity_weight = target_market_cap / total_capital
        debt_weight   = target_debt / total_capital

    # WACC
    wacc = equity_weight * cost_of_equity + debt_weight * after_tax_kd
    # Sanity clamp
    wacc = max(0.05, min(wacc, 0.30))

    if ledger is not None:
        ledger.record_derived(
            "wacc", "wacc", None, value=round(wacc, 4),
            formula="We*Ke + Wd*Kd*(1-t)",
            inputs=[("wacc", "cost_of_equity", None), ("wacc", "after_tax_cost_of_debt", None)],
        )

    return WACCOutput(
        peers=peer_set.peers,
        median_unlevered_beta=round(median_bu, 4),
        target_levered_beta=round(target_levered_beta, 4),
        target_de_ratio=de,
        risk_free_rate=risk_free_rate,
        equity_risk_premium=equity_risk_premium,
        cost_of_equity=round(cost_of_equity, 4),
        cost_of_debt_pretax=cost_of_debt_pretax,
        tax_rate=target_tax_rate,
        after_tax_cost_of_debt=round(after_tax_kd, 4),
        target_market_cap=target_market_cap,
        target_debt=target_debt,
        target_total_capital=total_capital,
        equity_weight=round(equity_weight, 4),
        debt_weight=round(debt_weight, 4),
        wacc=round(wacc, 4),
    )
