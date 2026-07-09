# Final State — 2026-07-10 (end of autonomous overnight execution)

## What was completed

### Phase 0 — Safety Net (8/9, 1 deferred)
| Task | Status | Deliverable |
|---|---|---|
| 0.1.1 | ✅ | Baseline verified, run log committed |
| 0.1.2 | ✅ | Guard test hardened (3 regression categories + fingerprint rejection) |
| 0.2.1 | ✅ | GitHub Actions CI (pytest + ruff + cargo build+test) |
| 0.2.2 | ✅ | ruff CI-clean config |
| 0.2.3 | ✅ | `docs/RELEASE_CHECKLIST.md` |
| 0.3.1 | ✅ | `pyproject.toml` + `pip install -e .` |
| 0.3.2 | ✅ | `CHANGELOG.md` + v0.1.0 tag |
| 0.5.1 | ✅ | Excel cell-level snapshots (5 cos, values+formulas+colors+hyperlinks) |
| 0.6.1 | ⏳ | **Deferred** — gets Claude CLI (edits scope files) |

### Phase R — Rust Port (7 crates, 51 unit tests)
Scaffold complete. Parity gate active with concrete gap measurement.

| Crate | Tests | Status |
|---|---|---|
| fm-types | — | Shared types |
| fm-tieout | 2 | ATCO-B_ST scores 48/48 |
| fm-extract | 8 | EDGAR stubs + prompt constants |
| fm-engine | 8 | Python-aligned assumption derivation + projection |
| fm-value | 24 | WACC/DCF/comps + 11 invariants |
| fm-excel | 9 | Writer + snapshot comparison |
| fm-cli | — | CLI + parity integration test |

**R.6 Parity Test Results (Rust vs Python, 90 keys compared per company):**
```
Company       | IS diffs | BS diffs | CFS diffs
--------------|----------|----------|----------
ASML_AS       |        5 |       22 |         0
ATCO-B.ST     |        0 |       16 |         0
NESN.SW       |       25 |       29 |         0
NOVO-B.CO     |        0 |       17 |         0
SAND.ST       |        0 |       20 |         0
```
- **IS: 3 of 5 companies at zero diffs** (ATCO, NOVO, SAND)
- **CFS: ALL 5 companies at zero diffs** (capex derivation fixed)
- **Previous baseline: 31-43 diffs, 50 keys → Now: 16-54 diffs, 90 keys**

### Phase 2E — Engagement Polish
| Task | Status | Files |
|---|---|---|
| 2.1.1 | ✅ | `scripts/qa_checklist.py`, `docs/QA_CHECKLIST.md` |
| 2.1.2 | ✅ | `scripts/run_engagement.py`, `docs/ONE_CLICK_ENGAGEMENT.md` |
| 2.1.3 | ✅ | `config/disclaimers.yaml`, `docs/TOS.md` |

### Not Yet Started
**Phase 1 — Accuracy Waves** (16 tasks, all dropped — blocked by Claude CLI)
**Phase 3 — Desktop v1** (6 tasks — NOT STARTED, plan-gated behind Phase 1 per `docs/MASTER_PLAN.md`)

## Key Fixes Applied
- `run_tieout.py`: _summary.json protected from zero-measure clobber
- `.gitignore`: fine-grained oracle file tracking
- `CLAUDE.md` at repo root for Claude Code CLI compatibility
- `.ruff.toml`: migrated to non-deprecated `[lint]` section
- `fm-engine`: SGA, capex, working capital days, DA derivation all aligned with Python

## Known Gaps for Full Parity (R.6 gate)
1. **Balance sheet**: Rust models only AP + LTD; Python includes deferred revenue, accrued expenses, other liabilities
2. **NESN.SW**: Rust doesn't handle negative EBIT / loss scenarios (tax carryforwards)
3. **Income tax**: Slight calculation differences from Python's effective rate method
4. **Edgar/XBRL live data**: fm-extract has stubs only (needs full implementation)
5. **Excel xlsx comparison**: xlsx reader needed for full cell-by-cell parity

## Blocker
**Claude CLI unavailable.** Model `claude-opencode-dsv4-flash` in `~/.claude/settings.json` doesn't resolve. Fix options:
1. Change model in settings.json to one your subscription supports
2. Set `ANTHROPIC_API_KEY` in `.env` and update `tieout/llm.py` to use Python SDK

## Git State
- HEAD: `5869cdc` (20 commits ahead of plan baseline `2735e00`)
- All local — NOT pushed to GitHub
