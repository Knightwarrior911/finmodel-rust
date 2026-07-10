# Finmodel — Financial Model Engine

## Project Memory
Project memory lives at `C:\Users\vinit\.claude\projects\C--Users-vinit-Documents-financial_model\memory\finmodel-master-plan.md`. Read that FIRST before any session.

Master plan: `docs/MASTER_PLAN.md` (committed 2026-07-03). Resume/session-start note: `docs/NEXT-SESSION.md`.

## Plan Summary (build track)
P0 (safety net: CI, snapshots, failure honesty) → PR (Rust port, 6 crates, cell-for-cell parity vs baseline) → P1 (accuracy: banks/insurers/held-out on Rust engine) → P2E (engagement polish) → P3 (Tauri desktop v1, no Python). P2S + P4 + P5 PARKED.

## Current State
Baseline `_baseline_wave0.json` **re-frozen 2026-07-10** (Wave 1 task 1.1.0): **307/334 (91.92%), 7 cos** (ATCO/SAND/ASML/NESN/NOVO/BAS/MC), opus-pinned, immutable per-company GT committed. Tie-out transport fixed (`claude --model`, was the recorded blocker). SAP.DE replaced by BASF (integrated report defeated face-window detection); MC.PA pinned + added. Guard green. Extraction gaps (dividends/net_income/intangibles/ppe_net conventions + BASF IS-detection) logged as Rust-engine targets per the Rust amendment.

## Key Verified Facts (don't re-derive)
- Tie-out baseline EXISTS: `tieout/results/_baseline_wave0.json` (91.92%, 307/334, 7 cos; opus-pinned, immutable per-company GT)
- Guard test: `tests/test_tieout_no_regression.py` exists
- Dynamic IS Phases 1-4 implemented (commit 9174435); only SaaS template unbuilt
- `engine.py` lacks insurance/REIT projection modes (layouts exist)
- No CI, no pyproject.toml, no packaging, no payments code
- `writer.py` is 3615-line monolith; hardcoded `anthropic` imports in 5+ files

## Cross-Ref Patterns to Reuse
- Dodo Payments: [[dodo-payments-snitch-billing]]
- NSIS Installer: [[snitch-nsis-installer-shipped]]
- Decko COM PPTX: [[decko-tauri-migration]]
- Tauri patterns: [[pdf-panda-tauri-rebuild]]
