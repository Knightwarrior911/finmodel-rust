# Finmodel — Financial Model Engine

## Project Memory
Project memory lives at `C:\Users\vinit\.claude\projects\C--Users-vinit-Documents-financial_model\memory\finmodel-master-plan.md`. Read that FIRST before any session.

Master plan: `docs/MASTER_PLAN.md` (committed 2026-07-03). Resume/session-start note: `docs/NEXT-SESSION.md`.

## Plan Summary (build track)
P0 (safety net: CI, snapshots, failure honesty) → PR (Rust port, 6 crates, 256/256 parity) → P1 (accuracy: banks/insurers/held-out on Rust engine) → P2E (engagement polish) → P3 (Tauri desktop v1, no Python). P2S + P4 + P5 PARKED.

## Current State
Tree clean at `2735e00`. Baseline `_baseline_wave0.json` at `57a7b41`: 256/256, 5/7 cos. Dynamic IS P1-4 shipped. **Autonomous execution started 2026-07-09** — user said to run overnight until everything is executed. On-going session updates the memory file as tasks complete.

## Key Verified Facts (don't re-derive)
- Tie-out baseline EXISTS: `tieout/results/_baseline_wave0.json` (100%, 256/256, 5/7 cos)
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
