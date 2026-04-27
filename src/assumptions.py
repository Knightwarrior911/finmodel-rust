"""
Builds AssumptionsBlock (toggle + Base/Upside/Downside scenarios + shared valuation inputs)
from a ModelOutput's historical-derived assumptions plus standard scenario deltas.

Base: historical 3yr-avg drivers (same as engine).
Upside: +200bp revenue growth, +100bp gross margin, -100bp capex pct.
Downside: -200bp revenue growth, -100bp gross margin, +100bp capex pct.
"""
from schemas.financial_data import (
    AssumptionsBlock, ScenarioInputs, ModelOutput
)


def _flat(value: float, n: int) -> list[float]:
    return [round(value, 6)] * n


def _build_scenario(name: str, base_assumptions: dict, n_proj: int,
                    rev_g_delta: float = 0.0, gm_delta: float = 0.0,
                    capex_delta: float = 0.0,
                    terminal_g: float = 0.025,
                    exit_mult: float = 12.0) -> ScenarioInputs:
    a = base_assumptions
    return ScenarioInputs(
        name=name,
        revenue_growth_pct=_flat(a.get("revenue_growth_pct", 0.05) + rev_g_delta, n_proj),
        gross_margin_pct=_flat(a.get("gross_margin_pct", 0.30) + gm_delta, n_proj),
        sga_pct_rev=_flat(a.get("sga_pct_rev", 0.10), n_proj),
        rd_pct_rev=_flat(a.get("rd_pct_rev", 0.05), n_proj),
        da_pct_rev=_flat(a.get("da_pct_rev", 0.04), n_proj),
        capex_pct_rev=_flat(a.get("capex_pct_rev", 0.05) + capex_delta, n_proj),
        tax_rate_pct=_flat(a.get("tax_rate_pct", 0.21), n_proj),
        interest_rate_pct=_flat(a.get("interest_rate_pct", 0.035), n_proj),
        dso_days=_flat(a.get("dso_days", 45.0), n_proj),
        dio_days=_flat(a.get("dio_days", 60.0), n_proj),
        dpo_days=_flat(a.get("dpo_days", 50.0), n_proj),
        dividend_per_share=_flat(a.get("dividend_per_share", 0.0), n_proj),
        terminal_growth_rate=terminal_g,
        exit_ebitda_multiple=exit_mult,
    )


def _fetch_market_inputs(ticker: str) -> dict:
    """Pull current share price + risk-free rate from yfinance. Falls back on failure."""
    out = {"current_share_price": 0.0, "risk_free_rate": 0.045}
    try:
        import yfinance as yf
        fi = yf.Ticker(ticker).fast_info
        # fast_info uses camelCase dict keys; the `.get()` method maps differently
        for key in ("lastPrice", "regularMarketPreviousClose", "previousClose"):
            try:
                price = fi[key]
            except (KeyError, Exception):
                continue
            if price:
                out["current_share_price"] = float(price)
                break
        hist = yf.Ticker("^TNX").history(period="5d")
        if not hist.empty:
            out["risk_free_rate"] = float(hist["Close"].iloc[-1]) / 100
    except Exception:
        pass
    return out


def build_assumptions_block(
    model_output: ModelOutput,
    ticker: str,
    active_case: int = 1,
    equity_risk_premium: float = 0.055,
    target_de_ratio: float = 0.30,
    sector: str = "standard",
) -> AssumptionsBlock:
    proj_periods = [p for p in model_output.periods if p.endswith("E")]
    n_proj = len(proj_periods)
    a = model_output.assumptions

    # For utilities/banks/REITs the gross_margin_pct slot holds EBIT margin.
    # Upside/downside deltas are applied to whatever is in that slot (EBIT margin ± 100bp).
    is_utility = sector in ('utility', 'bank', 'reit', 'insurance')
    gm_up   = +0.01 if not is_utility else +0.01   # same magnitude; semantic differs
    gm_down = -0.01 if not is_utility else -0.01

    base = _build_scenario("Base", a, n_proj)
    upside = _build_scenario(
        "Upside", a, n_proj,
        rev_g_delta=+0.02, gm_delta=gm_up, capex_delta=-0.01,
        terminal_g=0.030, exit_mult=14.0,
    )
    downside = _build_scenario(
        "Downside", a, n_proj,
        rev_g_delta=-0.02, gm_delta=gm_down, capex_delta=+0.01,
        terminal_g=0.020, exit_mult=10.0,
    )

    market = _fetch_market_inputs(ticker)

    return AssumptionsBlock(
        proj_periods=proj_periods,
        active_case=active_case,
        base=base,
        upside=upside,
        downside=downside,
        risk_free_rate=market["risk_free_rate"],
        equity_risk_premium=equity_risk_premium,
        target_de_ratio=target_de_ratio,
        cost_of_debt_pretax=a.get("interest_rate_pct", 0.035),
        current_share_price=market["current_share_price"],
        shares_diluted=a.get("shares_diluted", 0.0),
        mid_year_convention=True,
    )
