# Release Checklist

> A manual pre-release ritual for finmodel. Run through each step in order before
> cutting a new tag.

---

## 1. Full live tie-out run (API keys required)

The tie-out harness verifies every historical cell of the extraction pipeline against
the source filing PDFs. This requires a working `claude` CLI or Anthropic API key.

```bash
# From the repo root:
python -m tieout.run_tieout
```

**Acceptance:** matches the committed baseline — 339/350 (96.86%) across the 7-company basket, with no *new* mismatches vs `_baseline_wave0.json` (the pytest guard enforces this). The 11 known mismatches are documented extraction-convention targets, not regressions.
The report is written to `tieout/results/_report.md` — scan it for any `FAIL` or `MISMATCH`
lines.

If any cell falls below 100%, investigate before proceeding. Known-skip companies
(SAP.DE, MC.PA) are excluded from the count — they are documented as unsupported
(integrated-report layouts).

```bash
# Single-company fast check when only one changed:
python -m tieout.run_tieout --only ATCO-B.ST
```

---

## 2. Invariant spot-check

Beyond filing tie-out, run the internal verifier and invariant checks:

```bash
python -m pytest tests/ -q -x --tb=short
```

Also run the tie-out regression guard explicitly:

```bash
python -m pytest tests/test_tieout_no_regression.py -q -x --tb=short
```

**Acceptance:** all tests pass. The tie-out guard test compares the current run
against `tieout/results/_baseline_wave0.json` — a regression is an automatic
`FAIL`.

---

## 3. Version bump

Update the version in `pyproject.toml`:

| File | Field |
|------|-------|
| `pyproject.toml` | `version = "X.Y.Z"` |

Update `CHANGELOG.md` with the new version heading and entries.

---

## 4. Commit and push

```bash
git add -A
git commit -m "release: vX.Y.Z"
git tag vX.Y.Z
git push origin main --tags
```

---

## 5. Verify CI

After push, confirm the GitHub Actions CI run completes:

- **pytest** job: all tests pass
- **ruff** job: no lint warnings
- **tie-out guard** job: no regression

If CI fails, fix the issue, bump the patch version, and repeat from step 1.

---

## Reference: CI guard

The CI pipeline (`.github/workflows/ci.yml`) runs on every push and pull request
to `main`. It executes:

1. `pytest tests/` (fast quality gate)
2. `ruff check src/` (lint)
3. `tests/test_tieout_no_regression.py` (baseline comparison without API keys,
   using `FINMODEL_DEV_MOCK=1` to bypass LLM calls)

A red CI is a release blocker.
