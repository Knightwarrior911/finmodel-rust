# Conventions & Gotchas

## Product register (see PRODUCT.md / DESIGN.md)
- Voice: a calm, dry, quietly-brilliant senior colleague — the **JARVIS register** (persona
  lives in `SYSTEM_PROMPT` in chat.rs). Warm, precise to the decimal, one understated touch of
  wit, professional failure reports. NOT consumer-fintech gloss, NOT SaaS-dashboard clichés.
- One indigo accent voice; warm-neutral canvas; evidence-forward. Motion is CSS-only,
  compositor-friendly (opacity/transform), ~120–220ms, honors `prefers-reduced-motion`.
- Never surface snake_case tool ids / API enums / schema-speak in the UI — `labels.mjs` has the
  human copy (`toolRunningLabel`, `toolDoneLabel`, approval labels).

## Non-negotiable product rules
- **Never fabricate financial numbers.** Every material figure comes from a deterministic tool
  result and is cited. Prose arithmetic is a bug — the drift gate (`uncited_figures`) enforces it.
- **Citations are auditable.** Cite pills/source cards deep-link via Chrome text fragments
  (`deepSourceUrl`); financials columns link to the exact SEC filing (per-year accession).
- **Local file access is user-gated.** Only the artifact registry auto-runs on local files.
  Reading a user-named folder is `Risk::LocalRead` → PAUSES for approval. Subagents (delegate/
  run_agent) get read-only research tools only — they CANNOT open folders and never nest.

## Editing discipline (this codebase specifically)
- **Use the `edit`/`write` tools for Rust/JS**, not JS patch scripts. Hard-won lesson: routing
  Rust source through JS template literals / `String.replace` corrupted files 3× (backticks →
  shell, `\u{…}` → JS escape, `$'`/`$&` → replacement patterns). Anchored edits are reliable.
- **Line endings:** repo is LF; Windows git warns "LF will be replaced by CRLF" on nearly every
  file — that warning is NOISE, not an error.
- **Version lockstep:** `src-tauri/Cargo.toml` and `src-tauri/tauri.conf.json` must always match.
- **Tool count is pinned** in `tools.rs` tests (`names().len()`, `catalog.lines().count()`,
  `rank_for_query`). Adding/removing a `ToolSpec` → update all three pins.
- New Tauri command → also register it in `commands/mod.rs` `invoke_handler` list.
- New `renderCard` case → add a `labels.mjs` `TOOL_STORY` entry so the step reads warm.
- `chat_tool` events have **no UI consumer** — surface cards via the durable path
  (`take_side_cards` drained by the actor), not `emit_tool`.
- Model replies parsed as JSON: guard `find('{')..=rfind('}')` with `start <= end` (a `}` before
  `{` panics the slice). Applies to any new model-JSON parser.

## Prompt caching (Anthropic/Gemini via OpenRouter)
- `mark_cache_prefix` (chat.rs) anchors the **first** leading system layer (the large stable
  prefix: system + tools + scaffold + mode + catalog), NOT the last — `build_context` appends
  volatile summary/recalled-memories as trailing system layers, so anchoring the tail misses cache.

## Verification honesty (from the `executing-project-tasks` skill, followed here)
- Only claim what you ran this session. Paste real output. Evidence expires on edit.
- Smallest fix wins; no drive-by refactors or unrequested hardening.
- Live LLM paths (research, data room, run_agent) are integration-tested only when online;
  their pure logic is unit-tested. Say "untested" when the live leg didn't run.
