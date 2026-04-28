"""
Public trading comparables analysis (per public_comps/SPEC_methodology).

Workflow:
  1. LLM proposes 8-15 candidate peers (same as DCF peer-set logic)
  2. Filter to size band 0.3x-3x target market cap
  3. For each peer: fetch market data (yfinance), LTM operating stats (EDGAR XBRL)
  4. Compute LTM multiples: EV/Rev, EV/EBITDA, EV/EBIT, P/E
  5. Summary stats (min/p25/median/mean/p75/max), excluding NMs
  6. Apply target's metrics × peer median to derive implied per-share valuation
"""
import logging
import statistics
from datetime import date

logger = logging.getLogger(__name__)


# ── Helpers ──────────────────────────────────────────────────────────────────

def _yf_market_data(ticker: str) -> dict:
    """Pull share price, mkt cap, debt, cash, 52w hi/lo from yfinance."""
    out = {"share_price": 0.0, "shares_diluted": 0.0, "market_cap": 0.0,
           "total_debt": 0.0, "cash": 0.0, "week52_high": 0.0, "week52_low": 0.0}
    try:
        import yfinance as yf
        t = yf.Ticker(ticker)
        fi = t.fast_info
        for key, attr in [("lastPrice", "share_price"),
                          ("marketCap", "market_cap"),
                          ("yearHigh", "week52_high"),
                          ("yearLow", "week52_low")]:
            try:
                v = fi[key]
                if v is not None:
                    out[attr] = float(v)
            except (KeyError, Exception):
                pass
        # Convert market cap from $ → $M
        if out["market_cap"]:
            out["market_cap"] = out["market_cap"] / 1e6
        # Debt + cash from .info
        try:
            info = t.info
            out["total_debt"] = float(info.get("totalDebt") or 0) / 1e6
            out["cash"]       = float(info.get("totalCash") or 0) / 1e6
            out["shares_diluted"] = float(info.get("sharesOutstanding") or 0) / 1e6
        except Exception:
            pass
    except Exception as e:
        logger.warning("yfinance market data failed for %s: %s", ticker, e)
    return out


def _edgar_ltm_stats(ticker: str) -> dict:
    """Pull LTM revenue, EBITDA, EBIT, NI, EPS from EDGAR XBRL (last reported FY)."""
    out = {"ltm_revenue": 0.0, "ltm_ebitda": 0.0, "ltm_ebit": 0.0,
           "ltm_net_income": 0.0, "ltm_eps_diluted": 0.0}
    try:
        from src.fetcher import get_cik, fetch_xbrl_facts
        cik = get_cik(ticker)
        facts = fetch_xbrl_facts(cik)
        gaap = facts.get("facts", {}).get("us-gaap", {})

        def _last_annual(tags: list[str]) -> float:
            for tag in tags:
                if tag not in gaap:
                    continue
                entries = gaap[tag].get("units", {}).get("USD", [])
                annual = sorted(
                    [e for e in entries if e.get("form") == "10-K" and e.get("fp") == "FY"],
                    key=lambda e: e["end"]
                )
                if annual:
                    return float(annual[-1]["val"]) / 1e6
            return 0.0

        out["ltm_revenue"] = _last_annual([
            "Revenues", "RevenueFromContractWithCustomerExcludingAssessedTax",
            "RevenueFromContractWithCustomerIncludingAssessedTax",
            "SalesRevenueNet",
        ])
        out["ltm_ebit"] = _last_annual(["OperatingIncomeLoss"])
        da = _last_annual(["DepreciationDepreciationAndAmortization",
                           "DepreciationAndAmortization", "Depreciation"])
        out["ltm_ebitda"] = out["ltm_ebit"] + da
        out["ltm_net_income"] = _last_annual(["NetIncomeLoss"])
        # EPS not divided by 1e6
        for tag in ["EarningsPerShareDiluted", "EarningsPerShareBasicAndDiluted"]:
            if tag in gaap:
                annual = sorted(
                    [e for e in gaap[tag].get("units", {}).get("USD/shares", [])
                     if e.get("form") == "10-K" and e.get("fp") == "FY"],
                    key=lambda e: e["end"]
                )
                if annual:
                    out["ltm_eps_diluted"] = float(annual[-1]["val"])
                    break
    except Exception as e:
        logger.warning("EDGAR LTM stats failed for %s: %s", ticker, e)
    return out


def _yf_forward_estimates(ticker: str) -> dict:
    """Pull forward consensus estimates from yfinance (NTM, FY+1, FY+2)."""
    out = {"rev_growth_ntm": 0.0, "rev_growth_fy1": 0.0, "rev_growth_fy2": 0.0,
           "eps_ntm": 0.0, "eps_fy1": 0.0}
    try:
        import yfinance as yf
        t = yf.Ticker(ticker)
        try:
            growth = t.growth_estimates
            if isinstance(growth, dict):
                out["rev_growth_ntm"] = float(growth.get("revenueGrowthYOY", {}).get("+0Q", 0) or 0)
                out["rev_growth_fy1"] = float(growth.get("revenueGrowthYOY", {}).get("+1Y", 0) or 0)
                out["rev_growth_fy2"] = float(growth.get("revenueGrowthYOY", {}).get("+2Y", 0) or 0)
        except Exception:
            pass
        try:
            info = t.info
            out["eps_ntm"] = float(info.get("forwardEps") or 0)
        except Exception:
            pass
    except Exception as e:
        logger.warning("yfinance forward estimates failed for %s: %s", ticker, e)
    return out


def _build_peer(ticker: str, tier: int = 1) -> "PublicCompPeer":
    from schemas.financial_data import PublicCompPeer
    md = _yf_market_data(ticker)
    op = _edgar_ltm_stats(ticker)
    fwd = _yf_forward_estimates(ticker)
    ev = md["market_cap"] + md["total_debt"] - md["cash"]
    p = PublicCompPeer(
        ticker=ticker,
        name=ticker,
        tier=tier,
        share_price=md["share_price"],
        shares_diluted=md["shares_diluted"],
        market_cap=md["market_cap"],
        total_debt=md["total_debt"],
        cash=md["cash"],
        enterprise_value=ev,
        week52_high=md["week52_high"],
        week52_low=md["week52_low"],
        ltm_revenue=op["ltm_revenue"],
        ltm_ebitda=op["ltm_ebitda"],
        ltm_ebit=op["ltm_ebit"],
        ltm_net_income=op["ltm_net_income"],
        ltm_eps_diluted=op["ltm_eps_diluted"],
    )
    # Forward estimates: apply consensus growth to LTM
    if p.ltm_revenue > 0:
        p.ntm_revenue = round(p.ltm_revenue * (1 + fwd["rev_growth_ntm"]), 2)
        p.fy1_revenue = round(p.ltm_revenue * (1 + fwd["rev_growth_fy1"]), 2)
        p.fy2_revenue = round(p.ltm_revenue * (1 + fwd["rev_growth_fy2"]), 2)
    if p.ltm_ebitda > 0 and fwd["rev_growth_ntm"]:
        p.ntm_ebitda = round(p.ltm_ebitda * (1 + fwd["rev_growth_ntm"]), 2)
        p.fy1_ebitda = round(p.ltm_ebitda * (1 + fwd["rev_growth_fy1"]), 2)
        p.fy2_ebitda = round(p.ltm_ebitda * (1 + fwd["rev_growth_fy2"]), 2)
    p.ntm_eps = fwd.get("eps_ntm", 0.0)
    p.fy1_eps = fwd.get("eps_fy1", 0.0)
    # Compute LTM multiples
    p.ev_rev_ltm    = round(ev / p.ltm_revenue,  2) if p.ltm_revenue  > 0 else None
    p.ev_ebitda_ltm = round(ev / p.ltm_ebitda,   2) if p.ltm_ebitda   > 0 else None
    p.ev_ebit_ltm   = round(ev / p.ltm_ebit,     2) if p.ltm_ebit     > 0 else None
    p.pe_ltm        = round(p.share_price / p.ltm_eps_diluted, 2)                       if p.ltm_eps_diluted > 0 else None
    # Compute forward multiples
    p.ev_rev_ntm    = round(ev / p.ntm_revenue,   2) if p.ntm_revenue   > 0 else None
    p.ev_ebitda_ntm = round(ev / p.ntm_ebitda,    2) if p.ntm_ebitda    > 0 else None
    p.ev_rev_fy1    = round(ev / p.fy1_revenue,   2) if p.fy1_revenue   > 0 else None
    p.ev_ebitda_fy1 = round(ev / p.fy1_ebitda,    2) if p.fy1_ebitda    > 0 else None
    p.ev_rev_fy2    = round(ev / p.fy2_revenue,   2) if p.fy2_revenue   > 0 else None
    p.ev_ebitda_fy2 = round(ev / p.fy2_ebitda,    2) if p.fy2_ebitda    > 0 else None
    p.pe_ntm        = round(p.share_price / p.ntm_eps, 2) if p.ntm_eps > 0 else None
    p.pe_fy1        = round(p.share_price / p.fy1_eps, 2) if p.fy1_eps > 0 else None
    return p

def _summary_stats(values: list[float], name: str) -> "CompMultipleStats":
    from schemas.financial_data import CompMultipleStats
    vs = [v for v in values if v is not None and v > 0]
    if not vs:
        return CompMultipleStats(multiple_name=name, values=[], min=0, p25=0,
                                 median=0, mean=0, p75=0, max=0, count=0)
    vs_sorted = sorted(vs)
    return CompMultipleStats(
        multiple_name=name,
        values=vs,
        min=round(min(vs), 2),
        p25=round(statistics.quantiles(vs, n=4)[0], 2) if len(vs) >= 4 else round(vs_sorted[0], 2),
        median=round(statistics.median(vs), 2),
        mean=round(statistics.mean(vs), 2),
        p75=round(statistics.quantiles(vs, n=4)[2], 2) if len(vs) >= 4 else round(vs_sorted[-1], 2),
        max=round(max(vs), 2),
        count=len(vs),
    )


def build_public_comps(
    target_ticker: str,
    target_company_name: str,
    target_revenue: float,
    target_ebitda: float,
    target_ebit: float,
    target_net_income: float,
    target_total_debt: float,
    target_cash: float,
    target_shares_diluted: float,
    sector: str | None = None,
) -> "PublicCompsOutput":
    """Build full public comps analysis. LLM-driven peer selection + filter."""
    from schemas.financial_data import PublicCompsOutput
    from src.peers import _llm_propose_peers, _filter_peers, _market_cap

    target_mc = _market_cap(target_ticker)

    try:
        candidates, _ = _llm_propose_peers(target_ticker, target_company_name, sector)
        kept, excluded = _filter_peers(target_mc, candidates, target_ticker)
        source = "llm"
    except Exception as e:
        logger.warning("LLM peer proposal failed (%s) — public comps will be empty", e)
        kept, excluded, source = [], [], "fallback"

    peers = [_build_peer(tk, tier=1) for tk in kept[:8]]

    # Summary stats per multiple — LTM + forward
    stats = {
        "ev_rev_ltm":    _summary_stats([p.ev_rev_ltm    for p in peers], "EV / Revenue (LTM)"),
        "ev_ebitda_ltm": _summary_stats([p.ev_ebitda_ltm for p in peers], "EV / EBITDA (LTM)"),
        "ev_ebit_ltm":   _summary_stats([p.ev_ebit_ltm   for p in peers], "EV / EBIT (LTM)"),
        "pe_ltm":        _summary_stats([p.pe_ltm        for p in peers], "P/E (LTM)"),
        "ev_rev_ntm":    _summary_stats([p.ev_rev_ntm    for p in peers], "EV / Revenue (NTM)"),
        "ev_ebitda_ntm": _summary_stats([p.ev_ebitda_ntm for p in peers], "EV / EBITDA (NTM)"),
        "ev_rev_fy1":    _summary_stats([p.ev_rev_fy1    for p in peers], "EV / Revenue (FY+1)"),
        "ev_ebitda_fy1": _summary_stats([p.ev_ebitda_fy1 for p in peers], "EV / EBITDA (FY+1)"),
        "pe_ntm":        _summary_stats([p.pe_ntm        for p in peers], "P/E (NTM)"),
        "pe_fy1":        _summary_stats([p.pe_fy1        for p in peers], "P/E (FY+1)"),
    }

    # Implied valuation: median EV/EBITDA × target_ebitda → equity bridge → /shares
    def _implied(stat: "CompMultipleStats") -> float:
        if stat.count == 0:
            return 0.0
        ev_implied = stat.median * target_ebitda
        eq = ev_implied - target_total_debt + target_cash
        return round(eq / target_shares_diluted, 2) if target_shares_diluted else 0.0

    s = stats["ev_ebitda_ltm"]
    if s.count > 0:
        eq_low  = s.p25    * target_ebitda - target_total_debt + target_cash
        eq_med  = s.median * target_ebitda - target_total_debt + target_cash
        eq_high = s.p75    * target_ebitda - target_total_debt + target_cash
        implied_low  = round(eq_low  / target_shares_diluted, 2) if target_shares_diluted else 0
        implied_med  = round(eq_med  / target_shares_diluted, 2) if target_shares_diluted else 0
        implied_high = round(eq_high / target_shares_diluted, 2) if target_shares_diluted else 0
    else:
        implied_low = implied_med = implied_high = 0.0

    return PublicCompsOutput(
        target_ticker=target_ticker,
        target_company_name=target_company_name,
        as_of_date=date.today().isoformat(),
        target_revenue=target_revenue,
        target_ebitda=target_ebitda,
        target_ebit=target_ebit,
        target_net_income=target_net_income,
        target_total_debt=target_total_debt,
        target_cash=target_cash,
        target_shares_diluted=target_shares_diluted,
        peers=peers,
        excluded=excluded,
        stats=stats,
        implied_price_low=implied_low,
        implied_price_median=implied_med,
        implied_price_high=implied_high,
        source=source,
    )
