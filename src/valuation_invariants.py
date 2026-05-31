"""Structural invariants for a WACC/DCF valuation.

A DCF has no single 'correct' answer, so this does not compare to a ground
truth. It asserts properties that MUST hold for any sound valuation regardless
of assumptions (WACC > terminal g, beta >= 0, weights sum to 1, EV > 0, the EV
bridge identity, no NaN/inf, etc.). A CRITICAL violation means a real modeling
bug; WARNINGs are suspicious-but-legal. Reads attributes via getattr so it
works on the real WACCOutput/DCFOutput dataclasses and on lightweight test
stand-ins; a missing input skips its check.
"""
from __future__ import annotations

import math
from dataclasses import dataclass


@dataclass
class ValuationCheckReport:
    passed: bool
    critical: list
    warnings: list


def _num(obj, name):
    v = getattr(obj, name, None) if obj is not None else None
    if v is None:
        return None
    try:
        return float(v)
    except (TypeError, ValueError):
        return None


def check_valuation(wacc_output, dcf_output, *, sector: str = "standard") -> ValuationCheckReport:
    w, d = wacc_output, dcf_output
    crit: list[str] = []
    warn: list[str] = []

    for src, obj in (("wacc", w), ("dcf", d)):
        t = _num(obj, "tax_rate")
        if t is not None and not (0.0 <= t <= 0.5):
            crit.append(f"tax_rate out of [0,0.5] on {src}: {t}")

    wacc = _num(d, "wacc")
    if wacc is None:
        wacc = _num(w, "wacc")
    if wacc is not None and not (0.02 <= wacc <= 0.40):
        crit.append(f"wacc out of [0.02,0.40]: {wacc}")

    wmg = _num(d, "wacc_minus_g")
    g = _num(d, "tv_growth_rate")
    if wmg is None and wacc is not None and g is not None:
        wmg = wacc - g
    if wmg is not None and wmg <= 0:
        crit.append(f"WACC <= terminal growth (wacc_minus_g={wmg})")

    ke, rf = _num(w, "cost_of_equity"), _num(w, "risk_free_rate")
    if ke is not None and rf is not None and ke < rf - 1e-9:
        crit.append(f"cost_of_equity {ke} < risk_free_rate {rf} (implies negative beta)")

    katp, kat = _num(w, "cost_of_debt_pretax"), _num(w, "after_tax_cost_of_debt")
    if katp is not None and kat is not None and kat > katp + 1e-9:
        crit.append(f"after_tax_cost_of_debt {kat} > pretax {katp}")

    for obj in (w, d):
        ew, dw = _num(obj, "equity_weight"), _num(obj, "debt_weight")
        if ew is not None and dw is not None and abs(ew + dw - 1.0) > 1e-4:
            crit.append(f"capital weights sum != 1: {ew}+{dw}")
            break

    dfs = getattr(d, "discount_factors", None) if d is not None else None
    if dfs:
        prev = None
        for x in dfs:
            try:
                xf = float(x)
            except (TypeError, ValueError):
                continue
            if not math.isfinite(xf) or not (0 < xf <= 1.0):
                crit.append(f"discount factor out of (0,1]: {x}")
                break
            if prev is not None and xf >= prev:
                crit.append("discount factors not strictly decreasing")
                break
            prev = xf

    ev = _num(d, "enterprise_value")
    if ev is not None and ev <= 0:
        crit.append(f"enterprise_value <= 0: {ev}")

    eq, nd = _num(d, "equity_value"), _num(d, "net_debt")
    if ev is not None and eq is not None and nd is not None:
        tol = max(1.0, 0.001 * abs(ev))
        if abs(eq - (ev - nd)) > tol:
            crit.append(f"equity_value {eq} != EV {ev} - net_debt {nd}")

    sh, px = _num(d, "shares_diluted"), _num(d, "implied_price")
    if sh is not None and sh > 0 and px is not None and px <= 0:
        crit.append(f"implied_price <= 0 with shares {sh}")

    for name in ("wacc", "cost_of_equity", "enterprise_value", "equity_value",
                 "implied_price", "pv_tv", "pv_fcfs"):
        for obj in (w, d):
            raw = getattr(obj, name, None) if obj is not None else None
            if raw is None:
                continue
            try:
                f = float(raw)
            except (TypeError, ValueError):
                continue
            if not math.isfinite(f):
                crit.append(f"non-finite {name}: {raw}")
    fcff = getattr(d, "fcff_proj", None) if d is not None else None
    if fcff:
        for x in fcff:
            try:
                f = float(x)
            except (TypeError, ValueError):
                continue
            if not math.isfinite(f):
                crit.append("non-finite value in fcff_proj")
                break

    tvp = _num(d, "tv_pct_of_ev")
    if tvp is not None and not (0.40 <= tvp <= 0.95):
        warn.append(f"TV is {tvp:.0%} of EV (outside 40-95%)")
    if wmg is not None and 0 < wmg < 0.01:
        warn.append(f"thin WACC-g margin: {wmg}")
    beta = _num(w, "target_levered_beta")
    if beta is not None and not (0.1 <= beta <= 3.0):
        warn.append(f"levered beta unusual: {beta}")
    if ke is not None and not (0.03 <= ke <= 0.30):
        warn.append(f"cost_of_equity unusual: {ke}")
    cpx, ud = _num(d, "current_share_price"), _num(d, "upside_downside_pct")
    if cpx is not None and cpx > 0 and ud is not None and abs(ud) >= 2.0:
        warn.append(f"extreme upside/downside: {ud:.0%}")

    return ValuationCheckReport(passed=(len(crit) == 0), critical=crit, warnings=warn)
