# Valuation-Sanity Gate — Design Spec

**Date:** 2026-05-31
**Status:** Approved-to-build (trust-wedge roadmap continuation, standing pre-approval)
**Branch:** `feat/valuation-sanity`

## Purpose

The tie-out instrument proves **extraction** accuracy (extracted number == filing). Nothing checks the **valuation** is internally sane. Under the "provable accuracy" wedge, a correct extraction paired with a broken WACC/DCF (WACC < terminal g → negative TV, beta < 0, weights not summing to 1, TV = 99% of value) still ships silently.

A DCF has **no single ground-truth answer** (it is assumption-dependent), so valuation "accuracy" cannot be exact-match. Instead we enforce **invariants** — properties that MUST hold for ANY correct valuation regardless of assumptions. Violations = a real modeling bug.

## Scope decisions

- **Invariant-based, not ground-truth.** No "correct implied price" exists; we assert structural soundness.
- **Pure checker** over WACC/DCF outputs, duck-typed (`getattr`) so it works on the real dataclasses AND lightweight test objects; a missing input skips its check (never crashes).
- **Two enforcement points:** (1) unit tests (the offline regression gate — good model passes all, crafted-bad models trip each invariant); (2) cli runtime — print violations on every real build.
- **Offline boundary (honest):** a live-basket valuation pass (build WACC/DCF for the 5-7 tie-out companies and aggregate invariant pass-rate) needs network + LLM, so it is a **follow-up**, not v1. v1's gate is the test suite + cli wiring.
- **Out of scope:** ground-truth price comparison; live-basket runner; changing any valuation math.

## Module: `src/valuation_invariants.py`

```python
@dataclass
class ValuationCheckReport:
    passed: bool                 # True iff no critical violations
    critical: list[str]
    warnings: list[str]

def check_valuation(wacc_output, dcf_output, *, sector="standard") -> ValuationCheckReport
```

Reads attributes defensively (`getattr(obj, name, None)`); skips a check whose inputs are None.

### CRITICAL invariants (a violation = broken model)
1. Tax rate sane: `0 <= tax_rate <= 0.5` (check on both wacc & dcf if present).
2. WACC in band: `0.02 <= wacc <= 0.40`.
3. WACC > terminal growth: `wacc - tv_growth_rate > 0` (else Gordon TV invalid). Prefer `wacc_minus_g` field if present.
4. Cost of equity ≥ risk-free rate (implies beta·ERP ≥ 0): `cost_of_equity >= risk_free_rate - 1e-9`.
5. Tax shield non-negative: `after_tax_cost_of_debt <= cost_of_debt_pretax + 1e-9`.
6. Capital weights sum to 1: `abs(equity_weight + debt_weight - 1.0) < 1e-4`.
7. Discount factors valid: each in `(0, 1]` and strictly decreasing.
8. Enterprise value positive: `enterprise_value > 0`.
9. EV bridge identity: `abs(equity_value - (enterprise_value - net_debt)) <= max(1.0, 0.001*abs(enterprise_value))`.
10. Implied price positive when shares present: `shares_diluted > 0 => implied_price > 0`.
11. No NaN/inf in any checked scalar or in `fcff_proj`/`discount_factors`.

### WARNING invariants (suspicious, not fatal)
12. TV share of EV in `[0.40, 0.95]` (TV neither ~all value nor tiny/negative).
13. Gordon stability margin: `wacc_minus_g >= 0.01`.
14. Levered beta in `[0.1, 3.0]`.
15. Cost of equity in `[0.03, 0.30]`.
16. Upside/downside sane: `current_share_price > 0 => abs(upside_downside_pct) < 2.0` (>200% swing ⇒ check assumptions).

(Sector param reserved for future per-sector bands; v1 uses the same bands for all.)

## Wiring: `src/cli.py`

After `compute_dcf` succeeds (the `dcf_output` block, ~line 346), call:
```python
        from src.valuation_invariants import check_valuation
        vc = check_valuation(wacc_output, dcf_output, sector=cfg.sector)
        if vc.critical:
            print(f"      ⚠ Valuation invariants: {len(vc.critical)} CRITICAL")
            for v in vc.critical:
                print(f"          ✗ {v}")
        elif vc.warnings:
            print(f"      → Valuation invariants OK ({len(vc.warnings)} warnings)")
        else:
            print("      → Valuation invariants: all pass")
```
Guarded in try/except so a checker error never breaks the build. Non-breaking (print-only).

## Testing

- `tests/test_valuation_invariants.py`:
  - A `_good()` helper builds a `SimpleNamespace` WACC+DCF that satisfies every invariant → `check_valuation(...).passed is True` and `critical == []`.
  - One crafted-bad case per CRITICAL invariant (e.g. WACC < g, beta-implied Ke < Rf, weights 0.9/0.2, EV ≤ 0, equity ≠ EV−netdebt, NaN in fcff) → asserts the specific violation appears and `passed is False`.
  - A couple WARNING cases (TV% = 0.99, upside 350%).
  - None-tolerance: passing objects missing some attrs does not raise.
- Regression: full `pytest` green (currently 219 passed, 6 skipped); tie-out guard green.

## Follow-ups (out of v1)
- Live-basket valuation pass in the tie-out instrument (needs network/LLM).
- Per-sector invariant bands (banks/utilities have different WACC/TV norms).
- Feed critical violations into the ledger/verifier so the Excel + answer surfaces flag them too.
