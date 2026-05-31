# Valuation-Sanity Gate Implementation Plan

> **For agentic workers:** Use superpowers:subagent-driven-development. TDD, checkbox steps.

**Goal:** Enforce structural invariants on WACC/DCF outputs so a broken valuation can't ship silently.

**Architecture:** A pure `check_valuation(wacc_output, dcf_output)` returns critical/warning violations (duck-typed via getattr so it works on real dataclasses and test stand-ins). Wired into cli (print-only) and enforced by an exhaustive unit suite.

**Tech Stack:** Python 3.11, dataclasses, pytest. Spec: `docs/superpowers/specs/2026-05-31-valuation-sanity-gate-design.md`. Branch `feat/valuation-sanity`.

**Baseline:** 219 passed, 6 skipped. Gate: full pytest green; tie-out guard green.

---

## Task 1: valuation invariants checker

**Files:** Create `src/valuation_invariants.py`; Create `tests/test_valuation_invariants.py`.

- [ ] **Step 1: Write `tests/test_valuation_invariants.py`**

```python
import math
from types import SimpleNamespace
from src.valuation_invariants import check_valuation


def _good_wacc(**over):
    d = dict(tax_rate=0.25, cost_of_equity=0.095, risk_free_rate=0.04,
             cost_of_debt_pretax=0.05, after_tax_cost_of_debt=0.0375,
             equity_weight=0.8, debt_weight=0.2, wacc=0.085,
             target_levered_beta=1.1)
    d.update(over)
    return SimpleNamespace(**d)


def _good_dcf(**over):
    d = dict(tax_rate=0.25, wacc=0.085, wacc_minus_g=0.06, tv_growth_rate=0.025,
             equity_weight=0.8, debt_weight=0.2,
             discount_factors=[0.96, 0.88, 0.81, 0.74, 0.68],
             fcff_proj=[100.0, 110.0, 121.0, 133.0, 146.0],
             enterprise_value=5000.0, net_debt=1000.0, equity_value=4000.0,
             shares_diluted=100.0, implied_price=40.0,
             tv_pct_of_ev=0.7, current_share_price=35.0, upside_downside_pct=0.14,
             pv_tv=3000.0, pv_fcfs=2000.0)
    d.update(over)
    return SimpleNamespace(**d)


def test_good_model_passes():
    r = check_valuation(_good_wacc(), _good_dcf())
    assert r.passed is True
    assert r.critical == []


def test_wacc_below_growth_is_critical():
    r = check_valuation(_good_wacc(), _good_dcf(wacc_minus_g=-0.01, tv_growth_rate=0.10, wacc=0.085))
    assert r.passed is False
    assert any("terminal growth" in c for c in r.critical)


def test_negative_beta_implied_by_ke_below_rf():
    r = check_valuation(_good_wacc(cost_of_equity=0.03, risk_free_rate=0.04), _good_dcf())
    assert any("risk_free_rate" in c for c in r.critical)


def test_weights_dont_sum_to_one():
    r = check_valuation(_good_wacc(equity_weight=0.9, debt_weight=0.2), _good_dcf())
    assert any("weights" in c for c in r.critical)


def test_nonpositive_ev():
    r = check_valuation(_good_wacc(), _good_dcf(enterprise_value=-10.0))
    assert any("enterprise_value" in c for c in r.critical)


def test_ev_bridge_identity_violation():
    r = check_valuation(_good_wacc(), _good_dcf(equity_value=999.0))  # EV-netdebt=4000
    assert any("equity_value" in c for c in r.critical)


def test_tax_shield_negative():
    r = check_valuation(_good_wacc(after_tax_cost_of_debt=0.06, cost_of_debt_pretax=0.05), _good_dcf())
    assert any("after_tax_cost_of_debt" in c for c in r.critical)


def test_discount_factors_not_decreasing():
    r = check_valuation(_good_wacc(), _good_dcf(discount_factors=[0.9, 0.95, 0.8]))
    assert any("discount factors" in c for c in r.critical)


def test_nan_in_fcff():
    r = check_valuation(_good_wacc(), _good_dcf(fcff_proj=[100.0, float("nan")]))
    assert any("fcff_proj" in c for c in r.critical)


def test_tv_share_warning():
    r = check_valuation(_good_wacc(), _good_dcf(tv_pct_of_ev=0.99))
    assert r.passed is True
    assert any("TV is" in w for w in r.warnings)


def test_extreme_upside_warning():
    r = check_valuation(_good_wacc(), _good_dcf(upside_downside_pct=3.5))
    assert any("upside" in w.lower() for w in r.warnings)


def test_missing_attrs_does_not_raise():
    r = check_valuation(SimpleNamespace(), SimpleNamespace())
    assert isinstance(r.critical, list)  # no crash on empty objects
```

- [ ] **Step 2: Run `python -m pytest tests/test_valuation_invariants.py -q` — confirm FAIL (ModuleNotFoundError).**

- [ ] **Step 3: Create `src/valuation_invariants.py`:**

```python
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

    # 1 — tax rate sane
    for src, obj in (("wacc", w), ("dcf", d)):
        t = _num(obj, "tax_rate")
        if t is not None and not (0.0 <= t <= 0.5):
            crit.append(f"tax_rate out of [0,0.5] on {src}: {t}")

    # 2 — WACC band
    wacc = _num(d, "wacc")
    if wacc is None:
        wacc = _num(w, "wacc")
    if wacc is not None and not (0.02 <= wacc <= 0.40):
        crit.append(f"wacc out of [0.02,0.40]: {wacc}")

    # 3 — WACC > terminal growth
    wmg = _num(d, "wacc_minus_g")
    g = _num(d, "tv_growth_rate")
    if wmg is None and wacc is not None and g is not None:
        wmg = wacc - g
    if wmg is not None and wmg <= 0:
        crit.append(f"WACC <= terminal growth (wacc_minus_g={wmg})")

    # 4 — Ke >= Rf (non-negative beta)
    ke, rf = _num(w, "cost_of_equity"), _num(w, "risk_free_rate")
    if ke is not None and rf is not None and ke < rf - 1e-9:
        crit.append(f"cost_of_equity {ke} < risk_free_rate {rf} (implies negative beta)")

    # 5 — tax shield non-negative
    katp, kat = _num(w, "cost_of_debt_pretax"), _num(w, "after_tax_cost_of_debt")
    if katp is not None and kat is not None and kat > katp + 1e-9:
        crit.append(f"after_tax_cost_of_debt {kat} > pretax {katp}")

    # 6 — capital weights sum to 1
    for obj in (w, d):
        ew, dw = _num(obj, "equity_weight"), _num(obj, "debt_weight")
        if ew is not None and dw is not None and abs(ew + dw - 1.0) > 1e-4:
            crit.append(f"capital weights sum != 1: {ew}+{dw}")
            break

    # 7 — discount factors in (0,1], strictly decreasing
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

    # 8 — EV positive
    ev = _num(d, "enterprise_value")
    if ev is not None and ev <= 0:
        crit.append(f"enterprise_value <= 0: {ev}")

    # 9 — EV bridge identity
    eq, nd = _num(d, "equity_value"), _num(d, "net_debt")
    if ev is not None and eq is not None and nd is not None:
        tol = max(1.0, 0.001 * abs(ev))
        if abs(eq - (ev - nd)) > tol:
            crit.append(f"equity_value {eq} != EV {ev} - net_debt {nd}")

    # 10 — implied price positive when shares present
    sh, px = _num(d, "shares_diluted"), _num(d, "implied_price")
    if sh is not None and sh > 0 and px is not None and px <= 0:
        crit.append(f"implied_price <= 0 with shares {sh}")

    # 11 — no NaN/inf
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

    # WARNINGS
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
```

- [ ] **Step 4: Run `python -m pytest tests/test_valuation_invariants.py -q` — expect 12 passed.**

- [ ] **Step 5: Commit**

```bash
git add src/valuation_invariants.py tests/test_valuation_invariants.py
git commit -m "feat(valuation): structural invariant checker for WACC/DCF"
```

---

## Task 2: wire the checker into the cli build

**Files:** Modify `src/cli.py` (the `compute_dcf` block, ~line 343-351).

**Context:** cli computes `dcf_output = compute_dcf(model_output, cfg.ticker, wacc_output, assumptions, ledger=ledger)` inside `if wacc_output is not None:` then prints a summary. `wacc_output` and `cfg.sector` are in scope.

- [ ] **Step 1: READ `src/cli.py` lines 342-352 to confirm `wacc_output`, `dcf_output`, `cfg.sector` are in scope.**

- [ ] **Step 2: Add the invariant print after the DCF summary print (inside the same `try`, after the `print(f"      → Implied Price: ...")` line):**

```python
                from src.valuation_invariants import check_valuation
                try:
                    vc = check_valuation(wacc_output, dcf_output, sector=cfg.sector)
                    if vc.critical:
                        print(f"      ⚠ Valuation invariants: {len(vc.critical)} CRITICAL")
                        for _v in vc.critical:
                            print(f"          ✗ {_v}")
                    elif vc.warnings:
                        print(f"      → Valuation invariants OK ({len(vc.warnings)} warning(s))")
                    else:
                        print("      → Valuation invariants: all pass")
                except Exception as _e:
                    print(f"      ⚠ Valuation invariant check skipped: {_e}")
```

Print-only; do not change valuation behavior or fail the build.

- [ ] **Step 3: Sanity-import + full suite.**

Run: `python -c "import src.cli"` (expect no error), then `python -m pytest -q` (expect 231 passed: 219 + 12 new, 6 skipped, 0 failed).

- [ ] **Step 4: Commit**

```bash
git add src/cli.py
git commit -m "feat(valuation): run invariant checker in cli build (print-only)"
```

---

## Task 3: Regression + PR

- [ ] **Step 1:** `python -m pytest -q` → 231 passed, 6 skipped, 0 failed.
- [ ] **Step 2:** `python -m pytest tests/test_tieout_no_regression.py tests/test_tieout_sector.py -q` → green.
- [ ] **Step 3:** Push + PR

```bash
git push -u origin feat/valuation-sanity
gh pr create --title "Valuation-sanity gate: structural invariants on WACC/DCF" \
  --body "Implements docs/superpowers/specs/2026-05-31-valuation-sanity-gate-design.md. Pure check_valuation() enforces invariants any sound valuation must satisfy (WACC>g, beta>=0, weights sum to 1, EV>0, EV bridge identity, decreasing discount factors, no NaN/inf) as CRITICAL, plus WARNINGs (TV%, extreme upside). Wired print-only into the cli build. 12 new tests; full suite + tieout guard green. Follow-up: live-basket valuation pass (needs network)."
```

---

## Self-review notes (author)
- Each CRITICAL invariant in the spec has a dedicated test (Task 1). WARNINGs covered (TV%, upside). None-tolerance tested.
- Duck-typed getattr means the real `WACCOutput`/`DCFOutput` dataclasses (which carry these exact field names per `schemas/financial_data.py`) work unchanged, and tests use `SimpleNamespace`.
- cli wiring is print-only + try/except → cannot break a build. No valuation math changed.
