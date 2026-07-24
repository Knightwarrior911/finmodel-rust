# Finmodel — Financial Model Engine

Agentic financial-analyst desktop app (Tauri 2: Rust backend + vanilla-JS webview). Builds
3-statement + DCF Excel models from SEC EDGAR / ESEF / EDINET, benchmarks peers, reads filings
and PDFs, researches deals, reviews data rooms, and orchestrates user-defined subagents — every
material number sourced from a deterministic tool and cited.

## Golden rules (read before editing)
- **Never fabricate financial numbers.** Every material figure comes from a tool result and is
  cited; prose arithmetic is a bug the drift gate catches.
- **Local file access is user-gated** (approval-paused `Risk::LocalRead`); subagents are
  read-only and never open folders or nest.
- **Edit with the editor, not codegen scripts.** Anchored `edit`/`write` only — never patch Rust
  through JS template literals (it has corrupted files here repeatedly).
- **Version lockstep + pinned tool counts + register new commands in `commands/mod.rs`.**
- Verify by running; paste real output; evidence expires on edit. Say "untested" for live LLM
  paths that didn't run this session.

## Memory (durable — details in imported files)
@.claude/memory/architecture.md
@.claude/memory/workflows.md
@.claude/memory/conventions.md

## Product intent
See `PRODUCT.md` and `DESIGN.md` for voice/design contract (JARVIS-register senior colleague,
one indigo accent, evidence-forward, CSS-only motion). `CHANGELOG.md` is the user-facing history.

## References (not auto-loaded — open when needed)
- `docs/RELEASE_CHECKLIST.md` — authoritative release ritual.
- `docs/HANDOVER_LOG.md` — full dated session-handover archive (was this file's old body).
- Latest shipped: **v0.9.42** (see top of `CHANGELOG.md`).
