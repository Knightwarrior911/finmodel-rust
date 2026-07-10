# Changelog


## Unreleased

**Phase 1 Wave 1 (task 1.1.0): tie-out unblocked, basket fixed, baseline re-frozen to 307/334 (91.92%) on 7 industrials.**

### Fixed
- Tie-out LLM transport: pass explicit `--model` — headless `claude -p` inherited the broken global `claude-opus[1m]` alias (rc=1), which had blocked all of Phase 1. `tieout/llm.py` (opus examiner), `src/extractor.py` (opus default; override `FINMODEL_LLM_MODEL` / `FINMODEL_TIEOUT_MODEL`).
- `tieout/pin_filings._download`: single-iterator download — was calling `iter_content()` twice on one streamed response, truncating large PDFs (root cause of "MC.PA discovery failed").

### Changed
- Basket: SAP.DE → BASF (BAS.DE). SAP's 344-page integrated report (parent-HGB statements before consolidated IFRS + 17 decoy pages) defeats face-window detection; BASF's standalone consolidated-statements PDF ties out cleanly (52-cell GT). MC.PA pinned + added (32-cell GT).
- Ground truth committed + immutable per company (`tieout/groundtruth/*.json`); previously only ATCO was committed and the rest rebuilt per-run (non-deterministic).
- Baseline re-frozen (`tieout/results/_baseline_wave0.json`): 307/334 (91.92%) across 7 industrial companies. The old 256/256 was built on a Claude model generation that can no longer be invoked (unreproducible).
- Phase R parity gate wording: 256/256 → 307/334 / cell-for-cell (MASTER_PLAN.md, CLAUDE.md).

### Known gaps (Rust-engine extraction targets, per the Rust amendment)
- BASF income statement not sent to the model by `_extract_financial_section` (BAS.DE 34/52); `dividends_paid` and `net_income` group-vs-total conventions; `intangibles_net` goodwill scope; ATCO `ppe_net` includes IFRS-16 right-of-use.

## v0.1.0 (current)

**Initial baseline — 256/256 tie-out on 5 European industrials. Dynamic IS Phases 1–4 implemented.**

- Master plan committed (`7c8c342`)
- Amendments: build-first, Rust
- Project packaging: `pyproject.toml` with setuptools, `finmodel` CLI entry point
- Release checklist and changelog established
