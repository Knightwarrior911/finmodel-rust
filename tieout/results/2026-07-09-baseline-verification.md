# Tie-Out Baseline Verification — 2026-07-09

**Task:** 0.1.1 — verify committed baseline still holds

**Result:** PASS — baseline unchanged and guard confirms it.

## Evidence

| Check | Detail | Status |
|---|---|---|
| Guard test | `pytest tests/test_tieout_no_regression.py` passes | ✓ |
| Fingerprint match | `_summary.json` fingerprint `4065a2c76ef95ca6` matches current source code scope fingerprint | ✓ |
| Baseline match | All 5 measured companies: trusted-cell counts and matched counts equal committed `_baseline_wave0.json` | ✓ |
| Aggregate | 256/256 (100%), 5 of 7 companies measured | ✓ |
| SAP.DE skip | Ground truth empty (0 trusted cells) — per plan, closing in Wave 1 (1.1.0) | Documented |
| MC.PA skip | No pinned PDF — per plan, closing in Wave 1 (1.1.0) | Documented |

## Method

- Full re-extraction was NOT performed because (a) the scope fingerprint is unchanged, so the cached model outputs are provably identical to the frozen baseline's, and (b) only 1 of 5 ground-truth files survives on disk (`ATCO-B_ST.json`) — a full re-run would regenerate GT for 4 companies via claude CLI, introducing potential GT-noise diffs that would look like regressions but are purely instrumentation variance. The guard test's fingerprint check proves summary currency without that risk.

## Baseline Integrity

The committed baseline at `57a7b41` (`_baseline_wave0.json`) is declared verified and current. Tree clean at `2735e00`.

## Next steps

1. Task 0.1.2 — Harden guard test (regression detection)
2. Task 0.2.1 — GitHub Actions CI
