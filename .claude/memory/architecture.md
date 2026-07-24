# Architecture

Finmodel is a **Tauri 2 desktop app** (Rust backend + vanilla-JS webview UI) that builds
3-statement + DCF Excel models from SEC EDGAR (and ESEF/EDINET for non-US filers), benchmarks
peers, reads filings/PDFs, researches deals, and runs an agentic analyst loop.

## Two separate Cargo workspaces (no root workspace)
- `finmodel-core/` — pure engine crates, workspace members:
  `fm-types, fm-engine, fm-value, fm-ifrs, fm-tieout, fm-extract, fm-excel, fm-fetch,
  fm-build, fm-research, fm-pptx, fm-mcp, fm-cli, fm-agent`.
  - `fm-extract` — SEC XBRL / companyfacts parsing, `pdf_pages()` (panic-safe per-page PDF text), LTM/period logic.
  - `fm-fetch` — EDGAR, ESEF (filings.xbrl.org), EDINET, market data, news.
  - `fm-research` — the research machine (own LLM loop, cited synthesis, quote grounding);
    `synth::validate_synthesis` rejects unknown/non-read/blank/mismatched citation quotes.
    `quality_eval.rs` = offline answer-quality grader + model×prompt sweep + committed gate.
  - `fm-agent` — budget `Policy` (INTERACTIVE/WORKFLOW), `Risk` enum, workflows, ids, `types::Claim`.
  - `fm-build` / `fm-excel` / `fm-pptx` — model/workbook/deck generation.
  - `fm-cli` — offline pipeline CLI (`build`/`verify`/`benchmark`/`research`) over committed
    non-US fixtures (no key needed); `examples/regen_snapshot.rs` re-pins a model tie-out
    snapshot from the current engine (see workflows.md).
    `fm-agent/examples/nonusa_agentic_drive.rs` drives the reducer end-to-end for a non-US
    earnings review with the operator standing in for the LLM (own-key-free).
- `src-tauri/` — the app crate `finmodel-app` (lib + bin). Owns the agent runtime, tool
  registry, Tauri commands. Depends on the core crates by path.

## src-tauri agent runtime — `src-tauri/src/agent/`
- `driver.rs` — `LiveDriver`: the turn loop (request_model → tools → synthesize → verify),
  `CostGuard`, prompt composition (`agent_system_prompt`), drift gate (`uncited_figures`),
  advisor (`advisor_note`/`parse_advisor_notes`), `mark_cache_prefix` lives in chat.rs.
- `actor.rs` — durable event/reducer layer; emits `ResultPartAdded` etc.; `take_side_cards`/`take_verify_card`.
- `tools.rs` — `ToolRegistry`, `ToolSpec`, `agent_schemas()` / `agent_schemas_read_only()`, catalog.
- `scheduler.rs` — `plan_batches`: read-only tools fan out in parallel; write-class serialize.
- `executors.rs` — `execute_batch` (thread::scope parallel), `tool_error_content`.
- `delegate.rs` — child-agent loop (`run_child_loop`) backing `delegate_analysis`, `run_agent`,
  AND `dispatch_swarm`; `child_tool_belt`/`agent_tool_belt` (swarm/delegate excluded — one level
  deep, no nesting), usage helpers.
- `agents.rs` — user-defined agents (AGENT.md store, mirrors `skills.rs`). Ships a 5-agent
  starter bench in `src-tauri/agents/` (`BUILTIN_AGENTS`); `seed_builtin_agents` seeds it once
  at startup (lib.rs) — never clobbers user edits, deletions sticky (`.seeded_v1` marker).
- `skills.rs` — user skills (SKILL.md store, `use_skill` tool, catalog_block). 13 built-ins in
  `src-tauri/skills/`, `seed_builtin_skills` at startup (same one-shot semantics).
- `modes.rs` — `AgentMode` (Analyst/Plan/Goal/Loop/Skeptic): policy, read_only, doctrine layer.
- `model_router.rs`, `provider.rs`, `context.rs` (build_context), `grounding.rs`, `verification.rs`,
  `memory.rs`, `subagents.rs`, `fallback.rs`.
- `registry.rs` — `ActorRegistry`: active-run authority + shared execution slots
  (`GLOBAL_SLOTS` 8 / `PER_RUN_SLOTS` 4). `acquire_active_slot(conversation_id)` lets a nested
  executor (the `dispatch_swarm` batch) borrow the SAME per-run/global permits, so a wide swarm
  (or several in one turn) can never oversubscribe the run's concurrency ceiling.

## src-tauri commands — `src-tauri/src/commands/`
- `chat.rs` — the big one: `build_chat_request`, `stream_completion_for_agent`, `run_tool`
  (all tool dispatch, incl. `tool_swarm` = `dispatch_swarm` parallel subagent fan-out),
  `seed_agent_messages_for_model`, `financials_from_facts`.
- `agent.rs` — `agent_send`/`agent_resume`, skills_*/agents_* CRUD commands.
- `dataroom.rs` — data-room review (walk/extract/chunk/BM25/`resolve_findings`).
- `secrets.rs` — OpenRouter key in OS credential store (keyring), service `finmodel`,
  account `openrouter_api_key`. NEVER in settings.json.
- `settings.rs`, `model.rs`, `artifacts.rs`, `research.rs`, `benchmark.rs`, `update.rs`, etc.
- Commands are registered in `src-tauri/src/commands/mod.rs` (the `invoke_handler` list) — add new ones there.

## UI — `ui/` (vanilla ES modules, no framework, no bundler)
- `index.html` — all markup (settings modal has tabs: general/connections/memory/skills/agents/scheduled).
- `js/chat.mjs` — turn orchestration, `sendViaAgent`, `waitForAgentTerminal`, thinking trail.
- `js/cards.mjs` — every result card (`renderCard` dispatch); the ONLY card treatment.
- `js/composer.mjs` — input box, model pill, mode chip.
- `js/core.mjs` — `call`, `escapeHtml`, `openExternal`/`openPath`, `deepSourceUrl` (text fragments).
- `js/labels.mjs` — warm human copy (`TOOL_STORY`, approval labels).
- `js/settings.mjs`, `js/evidence.mjs` (Sources dock), `js/reader.mjs`, `js/parts.mjs`.
- Tests: `ui/tests/*.test.mjs` (jsdom, `node --test`); harness reads real `index.html`.

## Runtime data (Windows)
- Config dir: `C:/Users/<user>/AppData/Roaming/com.finmodel.desktop/`
  — `settings.json`, `skills/*.md`, `agents/*.md`, `finmodel.db` (SQLite conversations/runs).
- App identifier: `com.finmodel.desktop`.

## SSE streaming and typed events (v0.9.42)
- `LiveFrame` (chat.rs): typed result of parsing one SSE data payload — carries `text`,
  `reasoning`, `finish_reason`, `native_finish_reason`. `apply_delta` returns
  `Result<LiveFrame, ProviderError>` — a typed parser with NO meta side effects for
  reasoning/finish; callers accumulate frame fields into `TurnMeta`.
- `SseParser` (chat.rs): reusable byte-stream parser. `feed_bytes` drains `sse_take_lines`,
  calls `apply_delta`, accumulates meta. Returns `Vec<SseEvent>`. Production loop checks
  `parser.seen_done()` after each batch to break immediately on `[DONE]` (prevents keep-alive
  timeout). Provider errors emit `SseEvent::Error(e.clone())` immediately.
- `SseEvent` enum: `Frame(LiveFrame)`, `Done { content, calls, meta }`, `Eof { ... }`,
  `Error(ProviderError)`. `finish()` consumes the parser for terminal state.
- `sse_take_lines` (chat.rs): drains complete SSE lines from a raw byte buffer; incomplete
  trailing bytes (including mid-codepoint UTF-8) stay in `buf` until the next chunk.
- `stream_terminal_ok` (chat.rs): `[DONE]` always succeeds; without it, a non-empty
  `finish_reason` is required — all other EOF is `incomplete_stream`.

## Provider subscription quality (v0.9.42)
- `omp_gateway.rs`: `OwnedOmpProcesses` holds `Child` handles for app-spawned broker/gateway.
  `shutdown_owned_processes()` kills owned process trees via `taskkill /T /F /PID` on Windows.
  `ensure_cursor_gateway()` validates existing listeners (healthz + version + auth + models)
  before reusing; never reuses unauthenticated or incompatible services.
- `gateway_bearer()` reads `~/.omp/auth-gateway.token` backend-only; never serialized to
  Settings/UI/events. `GATEWAY_DUMMY_BEARER` removed.
- `RetryDecision` (driver.rs): pure enum (`Accept`/`FailNoEvidence`/`Retry`) + helper
  `retry_decision(is_non_answer, is_cancelled, has_tool_evidence)`. `ensure_answer_stream`
  uses it — retries only with tool evidence, one compose-nudge retry, no tools on retry.
- `omp_subscription_capability` (settings.rs): derives `ModelCapability` from model prefix
  (`opencode-go/` → native_tools=true, `cursor/` → native_tools=false). `apply_omp_capability`
  seeds capability on OMP gateway without live probe. `update_selected_model` invalidates on
  same-base model switch.
- `cursor_omp_status` (subscription.rs): `reusable()` method accepts refreshable Cursor OAuth
  (checks `refresh` field presence). Stored expiry is diagnostic, not authoritative.
- Non-answer classifier: `is_non_answer_text` rejects empty/status variants (`done`, `complete`,
  etc.). `ensure_answer_stream` retries once with compose nudge; fails visibly without evidence.
