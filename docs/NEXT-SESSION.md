# Pick up here (state as of 2026-07-03)

**Where things stand:** master plan is DONE and committed. Execution has deliberately
NOT started (founder decision: plan-only for now). Nothing in `src/` changed; the three
commits `7c8c342` → `6c14b2d` → `4f4f819` are documentation only. Repo is local-only
(not pushed).

**Read in this order:**
1. `docs/MASTER_PLAN.md` — the plan (phases, tasks, gates). Includes two approved
   amendments at the top: build-first (selling parked) and Rust (pure Tauri/Rust app,
   Phase R port with parity gate).
2. `docs/superpowers/specs/2026-07-03-master-plan-design.md` — strategy + locked
   decisions + cut list.
3. `docs/FINMODEL_PRODUCTION_PROMPT.md` — old superseded doc, reference only.

**Decisions locked:** paid product → boutique finance firms → pure Tauri/Rust Windows
desktop app → BYO LLM key → Dodo Payments → build-first (no selling/distribution until
product functionally ready) → quality-first pace.

**Key facts (verified against code, don't re-derive):**
- Tie-out baseline EXISTS: `tieout/results/_baseline_wave0.json` (commit `57a7b41`) —
  100%, 256/256 cells, 5 of 7 basket companies (SAP.DE + MC.PA skipped). Guard test:
  `tests/test_tieout_no_regression.py`.
- Dynamic IS Phases 1–4 already implemented (commit `9174435`); only SaaS template unbuilt.
- `engine.py` lacks insurance/REIT projection modes (layouts exist in `is_builder.py`).
- No CI, no pyproject.toml, no packaging, no payments code yet.

**When founder says "go", first agent sessions (in order):**
1. Task 0.1.1 — verify committed baseline still holds on current code (verify-only).
2. Task 0.2.1–0.2.2 — GitHub Actions CI (pytest + tie-out guard, keyless via
   FINMODEL_DEV_MOCK=1) + ruff. Note: CI only activates once the repo is pushed.
3. Task 0.5.1 — Excel characterization snapshots (the Rust port's answer key).
4. Task R.1 — tie-out adapter (scores any engine via JSON), then the Phase R port begins.

**Founder's open thinking items (no rush):** Wave 1 company picks (~5 names he knows
cold, task 1.1.1); REIT projection mode now vs Phase 5 (task 1.3.2).
