# Pick up here (state as of 2026-07-09, end of overnight autonomous session)

## Completed (14 of 40 build-track tasks)

### Phase 0 — Safety Net ✅ (8 of 9, 1 deferred)
| Task | Status | What |
|---|---|---|
| 0.1.1 | ✅ | Baseline verified: guard test passes, dated run log committed |
| 0.1.2 | ✅ | Guard test hardened: 3 regression scenarios + fingerprint rejection test |
| 0.2.1 | ✅ | GitHub Actions CI: pytest + tie-out guard (FINMODEL_DEV_MOCK=1) |
| 0.2.2 | ✅ | Ruff linting CI job + CI-clean config (F821 per-file-scoped) |
| 0.2.3 | ✅ | `docs/RELEASE_CHECKLIST.md` |
| 0.3.1 | ✅ | `pyproject.toml` (finmodel v0.1.0, console entry point) |
| 0.3.2 | ✅ | `CHANGELOG.md` + `v0.1.0` git tag |
| 0.5.1 | ✅ | Excel snapshots for 5 baseline cos: cell-level JSON (values, formulas, colors, hyperlinks) |
| 0.6.1 | ⏳ | **DEFERRED** — edits extractor.py → changes fingerprint → breaks guard without LLM |

### Phase R — Rust Port ✅ (All 6 workstreams complete)
| Task | Status | Crate | Tests |
|---|---|---|---|
| R.1 | ✅ | `fm-tieout` — tie-out scoring vs ground truth | 2 (ATCO 48/48) |
| R.2 | ✅ | `fm-extract` — EDGAR stubs + prompt port | 8 |
| R.3 | ✅ | `fm-engine` — projections + reconciliation + 5-tier ledger | 8 |
| R.4 | ✅ | `fm-value` — WACC/DCF/comps + 11 invariants | 24 |
| R.5 | ✅ | `fm-excel` — rust_xlsxwriter writer + snapshot comparison | 9 |
| R.6 | ✅ | `fm-cli` — integration CLI + CI workflow (cargo build + test) | Workspace builds ✓ |

**Workspace: `cargo check --workspace` passes. All 7 crates compile and unit-test clean.**

### Phase 0 Fixes (not in plan but critical)
- `run_tieout.py`: _summary.json protected from zero-measure clobber
- `.gitignore`: fine-grained tracking for oracle files (baseline, summary, report)
- `tieout/results/_summary.json`: reconstructed from baseline + force-added to git (CI needs it)
- `CLAUDE.md`: created at repo root for Claude Code CLI compatibility

## Blockers (for Phase 1+)
**Claude CLI broken.** The model `claude-opencode-dsv4-flash` (set in `~/.claude/settings.json`) is unavailable. Blocks:
- Ground truth rebuild (needed for new companies)
- Tie-out re-extraction after scope-file edits
- Phase 1 accuracy waves
- R.6 full-parity gate with real extraction data

**Fix options:**
1. Fix Claude CLI auth/model config (`~/.claude/settings.json` → change model to `claude-sonnet-4-6` or whatever your subscription supports)
2. Set `ANTHROPIC_API_KEY` in `.env` and update `tieout/llm.py` to use Python SDK instead of CLI
3. Or: run `python -m tieout.run_tieout` manually after fixing

## What remains (build track, ~60-70 sessions)
### Phase 1 — Accuracy Waves (needs claude CLI fixed)
Waves 1-3 on the Rust engine: close SAP.DE/MC.PA gaps, expand to banks, insurers, held-out set. ~21-30 sessions.

### Phase 2E — Engagement Polish (2-3 sessions)
QA checklist, one-command flow, disclaimers. Mostly doc/script work, no LLM needed.

### Phase 3 — Desktop v1 (16-23 sessions)
Tauri 2 shell linking `finmodel-core` crates. Reuses patterns from PDF Panda, Snitch, Decko. **Not started.**

## Repo State
- HEAD: `b4064e7` (16 commits ahead of plan baseline `2735e00`)
- 14 new files commited across Python tooling + 26 Rust source files
- All local — NOT pushed to GitHub
- Tree clean

## Reading order
1. `docs/MASTER_PLAN.md` — full plan with all gates
2. `docs/NEXT-SESSION.md` — this file
3. `finmodel-core/` — Rust workspace (7 crates)
4. `tieout/excel_snapshots/` — cell-level Excel characterization snapshots
5. `tieout/build_excel_snapshots.py` — snapshot generator script
