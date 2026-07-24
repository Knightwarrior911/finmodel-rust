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
  Reading a user-named folder is `Risk::LocalRead` → PAUSES for approval. Subagents
  (`delegate_analysis` / `run_agent` / `dispatch_swarm`) get read-only research tools only — they
  CANNOT open folders and never nest (a swarm worker cannot itself swarm/delegate).
- **`dispatch_swarm` shares the run's slots.** The batch swarm acquires each child's execution
  slot via `ActorRegistry::acquire_active_slot` (GLOBAL 8 / PER_RUN 4) — never its own pool — so it
  can't oversubscribe. It needs an active unified-agent run; the legacy `chat_send` path returns a
  clean error instead of running unbounded. Failed slices bill like a failed `delegate_analysis`
  (partial spend not recharged); only returned briefs aggregate into the card's `usage`.

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
- **Gross margin without a gross-profit line (v0.9.38 fix — do NOT revert):**
  `fm-engine::derive_assumptions` derives `gross_margin` from `revenue − cogs` when a filing
  reports COGS but no explicit `gross_profit` subtotal (IFRS "by function" filers — Nestlé /
  NESN.SW). Reading `gross_profit` ONLY (the old "match engine.py" behavior) yields a 0% margin
  that cascades to negative EBIT / equity / total assets across the whole projection. Every
  other layer already derives rev−cogs (fm-research metrics, fm-extract LTM/period, the fm-excel
  projection formula) — the engine was the lone hold-out. NESN is excluded from `full_is_parity`
  (the Python reference crashes on its null gross_profit); its model snapshot is pinned from the
  corrected Rust engine, not the defunct Python oracle.

## Prompt caching (Anthropic/Gemini via OpenRouter)
- `mark_cache_prefix` (chat.rs) anchors the **first** leading system layer (the large stable
  prefix: system + tools + scaffold + mode + catalog), NOT the last — `build_context` appends
  volatile summary/recalled-memories as trailing system layers, so anchoring the tail misses cache.

## Verification honesty (from the `executing-project-tasks` skill, followed here)
- Only claim what you ran this session. Paste real output. Evidence expires on edit.
- Smallest fix wins; no drive-by refactors or unrequested hardening.
- Live LLM paths (research, data room, run_agent) are integration-tested only when online;
  their pure logic is unit-tested. Say "untested" when the live leg didn't run.
- Answer-quality gate: `fm-research/tests/baselines/quality_v1.json` pins the gold-corpus
  hash + metric weights (and `WEIGHTS_VERSION`) EXACTLY — editing `gold_answers.json`,
  renaming a metric, or reweighting MUST deliberately refresh it. Per-case + mean scores are
  regression FLOORS (`>=`); never lower a floor to make a regression pass. Facts match answer
  PROSE only (not citation quotes); `quote_integrity` mirrors `validate_synthesis` (verbatim,
  case-sensitive), so the scorer never grants grounding production would reject.

## Typed event contract (v0.9.42)
- `apply_delta` returns `Result<LiveFrame, ProviderError>` — NOT `Result<Option<String>, _>`.
  `LiveFrame` carries typed fields; callers accumulate into `TurnMeta` themselves.
- Reasoning, finish_reason, native_finish_reason are NEVER mutated inside `apply_delta`.
  They live in `LiveFrame` and callers push them into `meta` after each frame.
- `SseParser::feed_bytes` returns `Vec<SseEvent>`. Production loop MUST check
  `parser.seen_done()` after each batch to break on `[DONE]` immediately — prevents
  keep-alive timeout on SSE connections.
- Provider errors emit `SseEvent::Error(e.clone())` immediately AND store `terminal_error`.
  The production loop breaks on error; `finish()` also returns it.

## OMP lifecycle conventions (v0.9.42)
- `OwnedOmpProcesses` holds `Child` handles only for processes the app spawned.
  `shutdown_owned_processes()` kills only owned process trees — pre-existing external
  OMP processes must survive.
- On Windows, use `taskkill /T /F /PID` (process tree kill) — `child.kill()` only kills
  the wrapper, not serving descendants.
- Never use `taskkill /F /IM omp.exe` in tests — that kills ALL omp processes globally.
  Capture specific PIDs via `netstat -ano` and kill only those.
- `PortStateGuard` RAII: capture initial port state before test, restore on drop even on panic.
- Gateway bearer token (`~/.omp/auth-gateway.token`) is backend-only — never serialized to
  Settings, UI payloads, events, logs, or durable chat state.

## Provider capability conventions (v0.9.42)
- `model_capability` is invalidated on: model change, API key change, base_url change,
  provider connect/import, cursor wire/reconcile, startup reconciliation.
- OpenCode Go models (`opencode-go/`) get `native_tools=true` from OMP catalog — no
  forced non-streaming probe (that times out at 120s).
- Cursor models (`cursor/`) get `native_tools=false` — direct chat only, no finmodel tools.
  Agent send with Cursor + tools_ok=false returns a clear error directing to OpenCode Go.
- `update_selected_model` centralizes model mutation and always invalidates capability.
- `is_non_answer_text` classifies: empty, `done`, `complete`, `completed`, `finished`,
  `task complete`, `task completed` (case-insensitive, punctuation-stripped).
