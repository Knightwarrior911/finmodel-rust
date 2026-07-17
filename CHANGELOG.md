# Changelog

## Unreleased — Agentic analyst cutover (Phases A–B: contracts, SQLite, unified actor loop)

First phase of the persistent workspace-scoped analyst rebuild. Foundation only;
the app now runs on the unified agent path (see the Phase G cutover entry); the legacy JSON chat engine is unreachable at runtime.

### Fixes (legacy path, user-facing)
- **API key never persisted.** `keyring = "3"` was declared with no feature
  flags; keyring v3 gates every platform backend behind a feature, so the app
  silently used the in-memory **mock** store — the OpenRouter key saved within a
  session but was gone on restart, forcing repeated re-entry. Enabled
  `windows-native` (real Windows Credential Manager). Verified cross-process: a
  write now materializes as credential `openrouter_api_key.finmodel` visible to
  `cmdkey` from a separate process.
- **Sidebar layout at HiDPI.** Conversation titles now wrap (2-line clamp +
  word-break) instead of clipping; the sidebar no longer shows a horizontal
  scrollbar (`overflow-x: hidden`); rename/delete actions reserve space and fade
  in rather than overlapping the title. Lowered the shell `min-width` floor
  (800→600) and rationalized the responsive breakpoints around `--sidebar-w`
  (full 272px ≥1101, narrow 200px docked 601–1100, overlay drawer only ≤600) so
  2× HiDPI working widths dock the sidebar instead of overflowing/overlaying.
  Verified via CDP at 784 CSS px (docked grid, no page/list h-scroll, 2-line
  titles, no action overlap).

### Memory (Phase E) — shipped auto-capture-disabled
- Per product decision, automatic memory capture stays **off**
  (`extract_memory → 0`) pending a labelled ≥200-turn dataset to validate the
  ≥98% precision / ≥90% recall gate. The store + capture/precision-gate/dedup/
  supersession/recall backend and behavioral tests remain built and green; the
  quality gate is waived, not measured. Manual save/recall UI is not yet wired.

### Phase G — tools live: probe fixed, model set, first tool-calling agent turns
- **Capability probe bug (blocking all tools):** `probe_tools` sent
  `provider.require_parameters:true` with a forced `tool_choice` +
  `parallel_tool_calls`; OpenRouter routing matches no endpoint for that combo
  (404 "No endpoints found"), so every model — including gpt-4.1-mini and
  gemini-2.5-flash — probed `native_tools=false` and tools could never
  activate. Fixed (the probe's truth test is the forced `ping` entry in
  `message.tool_calls`); strict-json probe keeps the flag (validated combo).
- **Model:** `openai/gpt-4.1-mini` selected and probe-verified
  (`native_tools=true`, `strict_json=true`).
- **First live tool-calling agent turns:** quote turn ran
  `run_started → tool_started → tool_succeeded → assistant_checkpoint →
  run_completed` with a real `get_quote` figure persisted in the
  user→assistant branch; a build prompt correctly reached the two-step build's
  assumptions-review stage (same first stage as legacy).
- **Honest durable tool events:** `Driver::schedule_tools` now returns per-id
  outcomes and the actor emits `ToolFailed` for failed calls instead of an
  unconditional `ToolSucceeded` (contract test added; a replayed UI can no
  longer render failures as successes).
- **Remaining before cutover:** structured tool-result/assumption cards from
  agent events in the UI (parts consumer), approval parking (`agent_approve`),
  FallbackDispatcher affordance for no-key/tool-less modes, full parity
  battery — then legacy removal.

### Phase G — functional cutover to the unified agent path

The desktop app now runs entirely on the unified `agent_send` loop; the legacy
keyed/routed chat engine is unreachable at runtime.

- Conversations are store-backed: list/load/rename/delete read SQLite (load
  rebuilds the legacy render shape from the branch path); delete also drops any
  legacy JSON so startup import can't resurrect it. New turns persist only to
  SQLite. No JSON writes occur at runtime.
- Composer defaults to `agent_send`; Stop and the global-Escape handler use
  `agent_cancel`. Legacy `chat_send`/`chat_cancel` are unregistered from the
  IPC surface (verified: `chat_send` returns "command not found").
- Multi-turn history: the user turn links under the active leaf and the driver
  rebuilds provider context from the branch, so conversations keep full history
  and are not amnesiac (verified: a 2-turn conversation loads 4 messages with
  coherent context).
- Approvals: registry park/resolve hub + `agent_approve` command + Approve/
  Create-new-version/Deny UI on `approval_requested` (deny on cancel/10-min).
- No-key mode routes to the isolated FallbackDispatcher inside the loop.
- Tools exposed to the model now include `research`/`web_search`/`read_page`
  (were withheld from the legacy native schema); `research` arg is translated
  `query`->`question` at the executor boundary.

Live parity verified on gpt-4.1-mini across the main VP task families: direct
answer, quote, model build (assumptions stage), trading comps, research (cited),
and multi-turn context — each matching the legacy tool family/typed result.

Remaining before the release tag: mechanical deletion of the now-unreachable
legacy source (chat_send/route_intent/JSON structs/turn engine — dead code, no
behavior), and the signed installer + 7-day rollback rehearsal (needs the
minisign key). Both are cleanup/release steps; the runtime cutover is done.

### Phase G — agent loop live-verified; parity partial; cutover deferred
- **First live `agent_send` runs** (against a real OpenRouter model via the
  running app) exercised the whole `LiveDriver` pipeline end-to-end:
  `run_started → assistant_checkpoint → run_completed`, `stop: end_turn`.
- Fixed two bugs surfaced only under live runs: (1) tool-incompatible models
  took the machine's direct-answer shortcut and skipped `request_model`,
  producing an **empty turn** — `prepare` now always routes through Executing so
  the model is consulted; (2) `synthesize` inserted the assistant message as an
  orphan (`parent=None`) and swallowed store errors, so the answer **vanished on
  reload** — it now links under the active leaf (via `Db::active_leaf_id`) and
  propagates errors, yielding a correct `user → assistant` branch.
- **Parity result:** on direct-answer prompts the agent loop matches legacy
  `chat_send` (both return correct prose). Golden oracles cover earnings +
  trading_comps deterministically offline.
- **Not yet at full parity / cutover deferred:** the configured model
  (`deepseek/deepseek-v4-flash`) probed `native_tools=false`, so per plan
  decision 2 the keyed agent path is text-only for tool-seeking prompts and must
  offer a capable model or a typed Quick Action — the isolated
  `FallbackDispatcher` + Quick-Action affordance are the remaining Phase G work
  before legacy `route_intent`/JSON can be removed. Legacy stays the default.

### Toolchain + dependency gate
- Pinned the exact CI stable toolchain via `rust-toolchain.toml` (`1.96.0`,
  with `rustfmt`/`clippy`); bumped app `rust-version` to `1.96`. Proved the
  existing core workspace + app build under the pin.
- Added `rusqlite = "=0.39.0"` (`bundled` + `backup`): SQLite 3.x with FTS5,
  statically linked. **No runtime `sqlite3.dll`.** Release exe size delta:
  21,181,440 → 22,304,768 bytes (**+1.07 MiB / +5.3%**).

### New `fm-agent` crate (pure reducer)
- `finmodel-core/fm-agent`: runtime-agnostic agent-loop reducer following the
  `fm-research` reducer/driver split. `AgentMachine::next(Input) -> Action`
  owns phase transitions (`Preparing → [Planning] → Executing ⇄
  AwaitingApproval → Synthesizing → Verifying → terminal`), budgets, one
  argument-repair, one verification-repair, approval parking, cancellation, and
  the single terminal reason. Typed vocabulary: IDs, phases, stop reasons,
  event kinds (durable/ephemeral), message-part kinds, tool risk, trust,
  `ToolResultEnvelope`, and the numeric `Claim` record. 30 unit tests.

### SQLite store (`src-tauri/src/store`)
- Store-actor architecture: `Db` (synchronous core owning the `Connection`) +
  `StoreHandle` (serializes short transactions on a dedicated blocking thread;
  never exposes the `Connection` through Tauri state).
- Full v1 schema (`PRAGMA user_version` authority): workspaces + public-entity
  allowlist, conversations, branch-linked messages/parts, agent runs + monotonic
  run events, tool invocations, pending interactions, sources/citations,
  artifacts, content-addressed blobs + refs + GC queue, scoped memories +
  memory-uses. FTS5 external-content indexes over message part text and memory
  content, kept aligned by triggers.
- Fixed PRAGMAs in correct order: `auto_vacuum=INCREMENTAL` + `secure_delete=ON`
  established on the zero-page DB before WAL; `foreign_keys=ON`,
  `synchronous=NORMAL`, `busy_timeout=5000`, `journal_mode=WAL` per open.
- Atomic blob publish (temp → fsync → rename → row); last-reference GC with
  retry and resurrection-safe re-reference; stale-temp reconciliation; online
  backup; interrupted-run repair on startup; integrity/FK/FTS checks.
- Idempotent, non-destructive JSON→SQLite migration: groups consecutive
  assistant messages into one logical message (ordered text/result parts),
  copies `llm_context` to `context_summary`, sets the active leaf, and
  quarantines malformed files without discarding good conversations. Wired into
  app startup (`store::init`); legacy JSON stays the source of truth until the
  Phase G cutover.
- 14 store tests: foreign keys, branch-path switching, workspace-scoped FTS +
  deletion, monotonic sequences, first-answer-wins approvals, blob
  reclamation/retry/resurrection, atomic publish/reconcile, interrupted-run
  repair, integrity/backup roundtrip, JSON migration grouping/idempotency/
  quarantine, and store-actor serialization. Full app-lib suite: 86 green.

### Phase B — unified actor loop, events, context, replay (`src-tauri/src/agent`)
- Single IPC event envelope (`agent/events.rs`): `AgentEventEnvelope` with
  durable (monotonic per-run `sequence`) vs ephemeral variants, replacing the
  old special event names. Persist-then-broadcast makes the store authoritative.
- Actor turn driver (`agent/actor.rs`): drives the pure `AgentMachine` to a
  terminal via a `Driver` trait, persisting every durable event before
  broadcasting, then finalizing the run row. `resume_run()` creates a NEW run
  linked by `resumed_from_run_id` from an interrupted one and refuses to reopen
  a terminal run. 5 fake-driver tests: persist-then-broadcast, live/replay
  equality, exactly one terminal event, approval request/resolve ordering,
  unverified→partial completion, and crash-repair→resume linkage.
- Context assembly + compaction (`agent/context.rs`): fixed stable block order
  (system/policy → workspace → summary → memories → branch → references → user →
  tools) and 90%→70% rolling compaction that always retains the latest four
  turns and any turn with an unresolved approval/artifact. 8 tests incl. the
  degenerate over-target case.
- Actor registry (`agent/registry.rs`): the active-run authority — one run per
  conversation, ≤3 active conversations, global 8 / per-run 4 execution slots,
  RAII deregistration, targeted cancellation. 7 tests.
- Real control/query Tauri commands (`commands/agent.rs`): `agent_cancel`,
  `agent_resume`, `list_active_runs`, `get_run_events_after`, `get_run_snapshot`
  (the race-free attach/reload contract). `agent_send` is deferred to Phase C,
  where the real provider/tool `Driver` lands. App-lib suite: 109 green.

### Phase C — typed tool registry, scheduler, provider adapter, security, fallback
- **Tool registry** (`agent/tools.rs`): 11 tool capabilities with strict arg
  validation, semantic validators (SSRF-guarded `read_page`), risk/trust
  metadata, stable catalog. 11 tests.
- **Scheduler** (`agent/scheduler.rs`): batch independent read-only calls,
  serialize writes and dependencies, cycle-safe. 7 tests.
- **Provider adapter** (`agent/provider.rs`): OpenRouter SSE
  `StreamAccumulator` (text + fragmented/parallel tool calls,
  finish_reason/usage, parse-error tolerance) mirroring the legacy wire shape;
  `decide_stream_tool_calls` capability probe (parallel only when a two-call
  probe observes it). 7 tests.
- **Egress/SSRF gate** (`agent/security.rs`): DNS-rebind-safe URL validation,
  reparse-safe output containment, confidential-query egress guard, secret
  redaction. 10 tests.
- **Fallback dispatcher** (`agent/fallback.rs`): isolated non-LLM intent router
  with typed Quick Actions, validated through the registry; filing-form-aware
  ticker extraction keeps single-letter tickers (F, T, C, V). 11 tests.
- Filing-form stripping (`10-K`, `8-K`, `S-1`, `20-F`) before ticker extraction
  in `fallback.rs` — single-letter tickers no longer discarded. Adversarial case:
  `"quote for F"` → `Some("F")`. App-lib: 153 green.

### Phase C — registry executors + scripted Driver
- `agent/executors.rs`: validate→dispatch→`ToolResultEnvelope` seam with
  `SessionContext`, `ToolBackend`, `FakeBackend`, source/artifact promotion,
  cancel short-circuit, and SSRF rejection before backend invoke. 9 tests.
- `agent/driver.rs`: `ScriptedDriver` runs canned provider transcripts through
  `run_turn` + registry executors. Acceptance: two parallel reads → research →
  synthesize/verify → terminal, with recorded batches/results. 2 tests.
- `commands/chat.rs`: `ChatToolBackend` bridges existing tool cores into the
  executor seam; `analyze_pdf` registry contract fixed to `artifact_id` (never
  raw path). App-lib suite: 214 green.

### Phase C/F — corpus traversal + comps/DCF acceptance tests
- FallbackDispatcher skips path-like tokens so `C:/tmp/x.pdf` no longer yields
  ticker `C`; no-key corpus walks dispatch → registry validate → FakeBackend
  execute. `cancel_all` only cancels queued/running children.
- Phase F: comps peer-pool (10 children, one fail, cascade cancel), DCF export
  approval ordering via ScriptedDriver, earnings/comps plan assertions.
  App-lib suite: 223 green.

### Phase E — MemoryUpdated emitted before terminal (`agent/actor.rs`)
- `Driver::extract_memory` now returns the count of saved rows; `run_turn`
  emits exactly one durable `MemoryUpdated { count }` event **before** the
  terminal run event when capture saved rows, and none when it saved nothing
  (timeout/empty) — closing a gap against the event contract + Phase E
  event-order acceptance. The count rides the payload because the UI
  (`memory.mjs`/`reducer.mjs`) drops count-less notices.
- 2 actor tests: `memory_updated_precedes_single_terminal_when_saved` (one
  notice, precedes the single `RunCompleted`, `count` in payload, live==replay)
  and `no_memory_notice_when_capture_saves_nothing`. App-lib: 230 green.

### Phase C — provider stream→ModelOut mapper + earnings golden e2e
- `agent/driver.rs::model_out_from_stream`: the real `request_model` core —
  maps a `StreamAccumulator` into a reducer `ModelOut`, classifying each tool
  call's risk / `needs_approval` / `args_valid` through the `ToolRegistry`.
  Unknown tools fail closed (never auto-run). 4 tests over canned OpenRouter
  SSE JSON: content-only→final answer, parallel reads→read-only auto-run,
  `build_model`→LocalCreate auto-run, invalid-args/unknown→`args_valid=false`.
- `earnings_golden_fixture_end_to_end`: drives the golden `earnings_review`
  workflow (T2) via `plan_workflow` + `ScriptedDriver` + `FakeBackend` — plan
  requires `list_filings`/`read_filing`/`get_news`/`get_quote`, all four execute,
  filing promotes a `sec.gov` source, `AssistantCheckpoint` precedes a single
  terminal `RunCompleted`, verification passes (non-partial). App-lib: 228 green.

### Phase D — ordered structured message-part renderer (`ui/js/parts.mjs`)
- `ui/js/parts.mjs`: renders a backend-ordered list of typed parts (text ·
  attachment · activity · result · sources · artifact · approval · warning ·
  error · memory_notice) so live and reload produce the same snapshot. `result`,
  `activity`, and `memory_notice` delegate to injected hooks (cards.mjs
  `renderCard`, `activity.render`, `memory.render`) so the module stays free of
  the Tauri bridge; everything else is pure DOM. Source links are http(s)-only
  (`safeHttpUrl`); model text stays inert via `textContent`; unknown kinds are
  skipped with surrounding order preserved. Approval offers Approve once / Deny,
  plus Create new version for overwrite/export.
- `ui/tests/parts.test.mjs`: 13 tests (order, XSS-inert text, numbered sources +
  domain, non-http title-only, scheme rejection, artifact open hook, approval
  button sets + response wiring, error retry, hook delegation, unknown-kind skip,
  idempotent re-render). `ui/style.css`: `part-*` block + ≤860px responsive.
  Full UI suite: 115 green.

### Phase D — task tray + workspace chrome
- `ui/js/tasks.mjs`: non-blocking task tray reducer (≤3 visible, background
  vs focused, cancel hooks). 8 tests.
- `ui/js/workspaces.mjs`: workspace select + Temporary Chat + confidentiality
  banner state. 7 tests.
- `index.html` / `style.css` / `main.mjs`: chrome wired; responsive collapse for
  ~800×560 and ~1100×760; reduced-motion kills activity spinner. UI suite: 95 green.

### Phase E — memory notice + Undo window (`ui/js/memory.mjs`)
- Pure reducer for `MemoryUpdated` notices with 10s Undo, Temporary Chat
  suppression, dismiss, and bounded history. Wired into main chrome.
  7 tests. UI suite: 102 green.

### Phase F — embedded finance workflow specs (`fm-agent`)
- `fm-agent/src/workflows.rs`: six typed `WorkflowSpec` contracts — company
  brief, earnings review, trading comps, DCF/3-statement, M&A screen, pitch
  prep — each defining required/allowed tools, confidentiality, approval policy,
  budgets, verification requirement, and golden-fixture status.
- `builtin_workflows()` returns the full catalog; `workflow(id)` single-lookup.
- 8 tests: six present, allowed-tool consistency, golden-fixture identity, input
  validation, verification requirement, budget policy, membership checks.
  fm-agent suite: 38 green.

### Phase D — activity reducer + central state reducer (`ui/js/activity.mjs`, `ui/js/reducer.mjs`)
- `ui/js/activity.mjs`: pure state reducer + DOM renderer for tool execution
  activities. Reduces every `AgentEventEnvelope` into a keyed `ToolActivity`
  map by `tool_call_id`. Handles all states: queued, running, awaiting_approval,
  success, warning, error, cancelled, interrupted. Supports batch grouping,
  bounded output tail (6 lines), expandable detail, approval buttons, elapsed
  duration, error display, and dark-theme styling. 20 tests.
- `ui/js/reducer.mjs`: pure conversation state reducer for the agent event
  system. Processes `AgentEventEnvelope` events — run lifecycle, text streaming,
  tool status, approval, errors, memory notices. Produces immutable state
  snapshots with messages, draft text, phase label, run status, approval state.
  No DOM dependencies. 26 tests.
- Full UI suite: 80 green.

### Phase F — workflow orchestrator + subagent pool (`agent/workflows.rs`, `agent/subagents.rs`)
- `agent/workflows.rs`: runtime workflow planner — validates `WorkflowSpec`
  against `ToolRegistry`, resolves allowed-tool set, sets budgets, produces
  `WorkflowPlan` with sequential steps. Pure planning, no I/O. 10 tests.
- `check_workflow_tools()`: startup drift detection — verifies every required
  tool is registered; returns missing tools.
- `agent/subagents.rs`: `SubagentPool` — manages child subagents for one
  parent workflow. Enforces `max_children` cap, tracks lifecycle
  (queued/running/succeeded/failed/cancelled), supports cascading
  cancellation via `cancel_all()`. 10 tests.
- App-lib suite: 173 green.

### Phase E — memory store + capture + recall
- `store/memory.rs`: `MemoryRepository` trait with two backends — SQLite
  (`SqliteMemoryRepository` wrapping `Db`) and in-memory
  (`InMemoryMemoryRepository` for pure reducer tests). Covers insert, get,
  get_by_public_id, FTS5-scoped search, supersede (close `valid_to` + link
  `superseded_by`), delete, and `record_use` for recall explainability.
  `MemoryScope` filter: workspace/conversation scoping and `global_only`.
  15 tests.
- `agent/memory.rs`: `MemoryCapture` — extracts memories from completed
  turns (verified claims + user statements), subject to `PrecisionGate`
  (rejects secrets, paths, URLs, short text, non-numeric claims). Dedup
  by `normalized_key` + scope; supersession closes `valid_to` on old
  versions and links `superseded_by`.
- `MemoryRecall` — queries relevant memories for context injection
  using the `MemoryRepository`, returns formatted lines with confidence
  and provenance.
- 14 tests: precision gate, claim extraction, user statement extraction,
  dedup, supersession, non-numeric rejection, scope isolation, recall
  formatting, empty recall. App-lib suite: 202 green.

## v0.5.1 — 2026-07-17

### Fixed — news recency & chat response completeness
- **Time-bound news actually respects the window.** A natural-language recency
  phrase ("in the last 24 hours", "today", "past week") now maps to Google
  News' `when:` operator so the feed is restricted server-side, and is enforced
  again client-side against each item's `pubDate` — so a "last 24 hours" query
  never returns years-old articles (previously it could surface, e.g., a 2006
  headline). Leading filler ("search the web for …") is stripped so the search
  text is a clean topic rather than a full sentence (`fm-fetch::news`).
- **No more dangling "Here's what I found:".** Deterministically-routed tool

  cards (news, web search, quote, filings, PDF) now end with a complete,
  self-contained sentence that reports the result count (e.g. "I found 8 recent
  headlines on this topic.") instead of a colon-terminated lead-in with nothing
  after it, and reads honestly when there are zero results (`src-tauri` chat).
- **Date-aware assistant.** The chat system prompt now states today's date (UTC)
  and instructs the model to rely on tool results for anything current or
  time-bound rather than its training data.

## v0.5.0 — 2026-07-17 (research-first copilot)

This release turns finmodel from a model-builder into a **research-first
copilot**: a factual/current question returns a source-grounded, cited answer
that stays reliable even when the selected model has weak or no native
tool-calling. The same line closes verified data-integrity, latency,
accessibility, CI, and release-safety gaps. Workspace, desktop-app, and mock-DOM
UI test suites are green; the current desktop debug build was smoke-tested over
CDP (WebView2) — direct IPC and the analyst UI path, not yet the signed installer.

### Research copilot (tool execution + research engine + latency)
- **Typed intent router with precedence + weak-model fallback.** Each turn is
  resolved to a typed intent (research / filing / news / build / benchmark /
  quote / direct answer); a model that can't call tools is routed deterministically
  to the same real action instead of emitting a fabricated answer. One tool
  registry owns schemas, typed args, validation, and execution; OpenRouter tool
  exposure is capability-gated on `supported_parameters`.
- **Pure `ResearchMachine` reducer + async driver.** A bounded search→read→
  synthesize cascade over the existing fetch/MCP infrastructure, with
  untrusted-page SSRF/injection neutralization, bounded weak-model synthesis with
  validation, deterministic `S#`-ordered events/cards, run ownership with
  streaming + cancellation, pooled clients, bounded caches, bounded parallelism,
  and retry-as-new-run.

### Analyst UX + workflows
- Cited answers render as normal assistant messages with a consulted-source tray
  and an in-app reader (loading / blocked / no-match / recovery states). Dialogs,
  sidebar, and conversation controls carry full modal a11y (role/aria-modal,
  focus trap + return, Escape, `aria-expanded`/`inert`, live-region announcements).
  Responsive desktop shell; honest onboarding that states what the current model
  and key can and cannot do. Filing Q&A, company brief, earnings review, and
  comparison/deal modes plus an `fm research` CLI; a suggested-assumption review
  bridge carries research provenance chat→workbook.

### Data integrity (Phase 6)
- **Two-outcome extraction gate.** Unsafe extractions (non-finite values,
  inconsistent vectors, empty / duplicate / out-of-order / unparseable periods,
  invalid currency) BLOCK workbook creation; a merely-imbalanced-but-finite
  extraction still builds but is flagged.
- **Real Verification.** The workbook's Verification report is now computed —
  balance-sheet identity `A = L + E` over each historical period, extraction
  discrepancies, and DCF/WACC structural checks — `passed` is true only when there
  are no critical failures, never a default placeholder.
- **Unified source-audit.** The Sources tab renders a typed audit row per
  research-sourced driver (line item, period, value, origin, `S#` evidence,
  per-row verification status); empty by default so committed snapshots stay
  byte-identical.
- **Sector honesty.** Bank / insurer / REIT / utility builds declare "layout
  supported; projection methodology not yet sector-specific" in both the workbook
  and the returned warnings — no half-built sector projection ships.
- **EV / IFRS / tie-out are desktop-reachable.** The enterprise-value bridge, the
  IFRS↔US-GAAP lease bridge, and the ground-truth tie-out score (previously
  CLI-only) are exposed as an Analyst-tools panel backed by `fm-value` /
  `fm-ifrs` / `fm-tieout`, kept out of the flat LLM tool list.

### CI, evals, and release safety (Phase 7)
- CI runs least-privilege (`permissions: contents: read`), with a research-eval
  **hard gate**, a Windows job exercising the desktop app's Tauri IPC layer, and
  the jsdom UI regression suite.
- Release checklist corrected end-to-end: Tauri version lockstep as the source of
  truth, tag-only-after-green-CI ordering, post-release endpoint verification, and
  an executable rollback procedure (stop-the-bleed via re-flagging Latest + roll
  forward with a signed hotfix).

## v0.4.0 — 2026-07-15

### Sellable-feature expansion (seven independent workstreams)
- **Live WACC inputs.** Live builds now fetch a real risk-free rate (10Y
  Treasury via `^TNX`) and a 2-year weekly regression beta vs the S&P 500
  (`^GSPC`), replacing the hardcoded 4.5% / 1.0 defaults. An explicit analyst
  value always wins; each override records a provenance note, and a failed fetch
  falls back to the default with a warning — a build never fails over market data.
- **Trading-comps tabs.** `build_model` accepts a peer set (`--peers "MSFT,GOOGL"`
  on the CLI, a `peers` array in chat / "build X with peers A, B"). Each peer's
  EDGAR filing + live quote becomes a `PublicCompPeer`; the previously-gated
  **Comps Peers** and **Comps Summary** sheets now ship with EV/Revenue,
  EV/EBITDA and P/E stat blocks plus EV/EBITDA-implied prices. Unreachable peers
  land in an excluded list; the build still succeeds.
- **One-click PPTX deck.** `--deck` (CLI) / always-on in chat writes a
  `<stem>_deck.pptx` beside the workbook: cover, valuation scorecard, revenue +
  EBITDA trajectory charts, and a trading-comps table (model); cover + peer
  table + EBITDA-margin chart (benchmark). New `add_table` deck archetype.
- **Read the actual filing.** New `read_filing` chat tool fetches the latest
  10-K/10-Q body from EDGAR, splits it into items, and returns a section
  (risk factors → Item 1A, MD&A → Item 7) — qualitative filing content without
  fabrication. `filing_doc` card with item chips and an open-in-browser link.
- **Scenario case from chat.** `build_model` accepts `case: base|upside|downside`
  (`--case` on the CLI, "build the downside case for X" in chat), driving the
  existing Base/Upside/Downside scenario engine; the model card tags a non-base case.
- **Drop a PDF, get a model.** New `analyze_pdf` tool + command runs the annual-
  report PDF + LLM extraction path on a local file; drag a `.pdf` onto the window
  to prime the composer. Requires an OpenRouter key.
- **UI polish.** Hover-to-copy on assistant messages; benchmark card is now
  horizontally scrollable (no 6-column cap) with a Copy-table (TSV) action;
  sidebar conversation filter (shown past 6 conversations) and a two-step delete
  confirm; `Ctrl/⌘+N` new chat, `Ctrl/⌘+K` filter, `Esc` stops a streaming
  reply, with a shortcut legend in Settings; refreshed example chips.

## v0.3.1 — 2026-07-15

### Fixed — chat robustness with weak / non-tool-calling models
- **No more fabricated answers.** When the selected model can't (or won't) call
  tools — e.g. it returns a hand-written list of fake "search results" instead
  of invoking `web_search` — the turn is now routed deterministically to the
  real tool so every figure and link comes from a tool result, never the model.
  Applies both when the model rejects the `tools` parameter and when it answers
  an explicit data request ("search the web for…", "build AAPL", "benchmark …")
  without calling a tool; the fabricated draft is dropped before the real card
  is shown. Bare definitional questions still get a direct model answer.
- **Control tokens stripped.** Model pseudo-tokens such as `<|eom|>` no longer
  leak into the displayed / stored assistant text.
- **Streaming caret stops.** The blinking accent caret now clears when a
  response finishes instead of pulsing indefinitely under the last message.

## v0.3.0 — 2026-07-15

### Chat-first UI redesign (claude.ai-style)
- **New shell** — the tool-card app is replaced by a chat-first interface: a
  left sidebar with conversation history (rename/delete, collapse), a centered
  chat pane where requests are typed in natural language, and a slide-in reader
  panel on the right. Vanilla ES modules under `ui/js/` (`core`, `sidebar`,
  `chat`, `cards`, `reader`, `settings`, `update`, `main`) replace the single
  `ui/app.js`.
- **Light + dark mode** — the existing indigo / warm-neutral token system is
  extended with a `[data-theme="dark"]` palette; a sidebar toggle and a Settings
  "Theme" select (System / Light / Dark) persist the choice and follow the OS
  when set to System.
- **Typography** — IBM Plex Sans (400/500/600) + IBM Plex Mono (400/500) are
  bundled as woff2 in `ui/fonts/`; all financial figures, tickers and table
  numerics use the mono face with tabular numerals.
- **Chat engine** — a new `chat` command module runs an OpenRouter tool-calling
  loop with live SSE token streaming (`chat_delta`/`chat_tool`/`chat_done`
  events) over the existing key + model settings, plus a deterministic no-key
  fallback router. Every engine capability is exposed as a chat tool with a rich
  inline result card: `build_model`, `benchmark_peers`, `web_search`,
  `read_page`, `get_news`, `research_deal`, `get_quote`, `list_filings`. Tools
  call the shared blocking cores directly (no shelling through command wrappers).
- **Assumptions grid, in chat** — `build_model` presents the editable per-year
  assumptions grid as an interactive card; "Build with these assumptions"
  finalizes via the existing `prepare_model`/`finalize_model` session cache.
- **Conversations** — persisted to `app_config_dir()/conversations/<id>.json`
  with `list`/`load`/`delete`/`rename` commands; tool results are stored as
  assistant messages carrying their card.

### Fixed / Improved — web-page read path
- **Bot-block resilience** — the basic (non-Roam) page fetcher now sends a full
  browser header set (UA, Accept, Accept-Language, Upgrade-Insecure-Requests),
  a cookie store, gzip/brotli, and a 20s timeout. Responses are classified as
  `ok` / `blocked` (403/429/503) / `thin` (<200 chars) instead of a silent
  dead-end: `fetch_page_text` → `fetch_page`/`FetchedPage`; the reader shows an
  honest "site blocks automated reading — open externally or configure Roam"
  prompt (keeping any partial text) rather than a blank pane.

## v0.2.1 — 2026-07-15

### Fixed / Improved — web search (post-0.2.0)
- **Results are now interactive.** Previously only the result's title *text* was
  clickable, so clicking the snippet/URL/card body (the natural target) did
  nothing. The **entire result card** now opens the in-app reader (hover +
  focus affordances, `cursor: pointer`), with explicit per-result **Read here**
  and **Open in browser ↗** buttons and keyboard support (Tab to focus,
  Enter/Space to open, ↑/↓ to move between results).
- **Reader upgraded** — loading spinner; full markdown rendering with
  find-on-page; **Copy link** + **Open in browser** actions; external links and
  CTAs open in the OS browser. JS-heavy / protected pages that return no
  readable text now show a clear "open externally / set up Roam" prompt instead
  of a blank pane.
- **Better fallback content** — the basic (non-Roam) page reader now extracts
  the main content as lightweight markdown (headings / paragraphs / lists, with
  nav / header / footer / scripts stripped and nested-block de-duplication)
  instead of dumping a whitespace-collapsed nav-junk blob; falls back to flat
  body text when structural extraction is too thin.
- **Search UX** — loading skeleton while querying, clearer result count / empty
  states, and a "use Roam for richer results" hint (opens Settings) when on the
  basic backend.

## v0.2.0 — 2026-07-14

### Fixed — correctness bugs (Phase 1)
- **Cross-currency comps** — `apply_multiples` now reconciles the live quote
  price into the metric currency before computing market cap / EV, so a USD
  `--usd` run no longer blends a native-currency market cap with USD-converted
  net debt (`fm-research`). Native `share_price`/`price_currency` are preserved
  for disclosure.
- **Hard-coded calendar year** — the `2024/2025/2026` fallbacks in
  `fm-extract` (`detect_years`, `build_result`) and `fm-cli`/`src-tauri`
  period labels are gone; a single civil-date helper (`fm_extract::date`,
  `current_year`/`today_iso`) drives all year math. `compute_target_years`
  wall-clock fallback is self-referential (no 2032 breakage).
- **UI hardening** — all remote/untrusted strings escaped before `innerHTML`;
  settings errors surface inside the open Settings card; a mistyped US ticker no
  longer detours to the non-US PDF path; the updater's stuck "installing" state,
  a non-clearing API key, a silent Gordon `TV=0`, and a silent WACC clamp are
  all fixed. Stale doc-strings corrected.

### Added — data quality (Phase 2)
- EDGAR client + Yahoo quote/FX resilience (retries, explicit error surfaces);
  DCF/statement **invariant checks** wired to user-visible warnings; live market
  inputs (price/FX) flow into the model with provenance.

### Added — analyst flexibility (Phase 3)
- `BuildOptions` threaded end-to-end: an **Advanced options** panel and a
  **per-year editable assumptions grid** (two-step prepare → finalize), CLI
  parity (`--period`, projection/driver overrides), and a selectable
  **reporting-period basis** (annual / quarterly / semi / LTM,
  `fm_extract::PeriodBasis`) across build + benchmark.

### Added — UX + ship (Phase 4)
- Real-time **build progress events**, a **Recent outputs** list, a compact
  **valuation preview** strip (implied price / upside / WACC / EV), refreshed
  copy, and regenerated app icons (finmodel chart glyph).

### Added — research subsystem port (Phases 5–9)
- **News** (Phase 5) — Google News RSS headlines via `fm-fetch` (quick-xml
  parser), `fm deal`-adjacent `fm news` CLI + app strip; research scoring
  helpers (`rank_urls`, `has_deal_content`, `is_sufficient`) ported to
  `fm-research::scoring`.
- **PowerPoint** (Phase 6) — new `fm-pptx` crate: OOXML/DrawingML deck
  inspect / edit / pure writer fns / EV+IFRS deck rendering (zip + quick-xml,
  no python-pptx), tied out against `tieout/build_pptx_oracle.py` (23 tests).
- **Non-US extraction** (Phase 7) — regex financial extractor + jurisdiction
  tables + discovery upgrade in `fm-extract`/`fm-fetch`, tied out vs pinned
  Python goldens.
- **In-app web search** (Phase 8) — a new blocking-stdio MCP client crate
  (`fm-mcp`, mock-server handshake gate), a `fm-research::web` facade (Roam MCP
  when configured, DDG + tag-strip HTTP fallback) with a web-appropriate ranker
  (drops SERP chrome, keeps content domains), a **Search** tool card + in-app
  reader pane (sanitized markdown, find-on-page, open-in-browser), and
  `web_search`/`read_page`/`test_mcp` Tauri commands.
- **M&A research agent** (Phase 9) — `fm-research::agent`: NL query routing,
  target/acquirer parsing, regex **deal synthesis**, and a search→read→
  synthesize cascade with a sufficiency stop-condition, exposed as `fm deal`.

All ported logic is unit-tested; live network/MCP paths are `#[ignore]`d.
Full workspace suite green; `src-tauri` + `fm-cli` compile clean.

## v0.1.1 — 2026-07-14 (previously shipped)

### Added — desktop auto-update
- **Signed self-update** — the desktop app now checks GitHub Releases on launch
  and installs newer builds, verified against a minisign `pubkey`. Wiring:
  `plugins.updater` (pubkey + `releases/latest/download/latest.json` endpoint) +
  `createUpdaterArtifacts: true` in `tauri.conf.json`; `tauri_plugin_updater`
  initialized in `lib.rs` (desktop-only) with `updater:default` capability; two
  backend commands (`check_for_update`, `install_update` → download + relaunch);
  a silent startup check that raises a **"Restart & update"** banner only when a
  newer version exists, plus a **Settings → "Check now"** control. Signing keys
  generated (private key kept outside the repo); a signed `cargo tauri build
  --bundles nsis` verified end-to-end — produces `finmodel_0.1.0_x64-setup.exe`
  **+ `.exe.sig`**. Release/signing/`latest.json` process documented in
  `docs/RELEASE_CHECKLIST.md` §6. Hardening: all remote/untrusted strings
  (update version/notes, OpenRouter model IDs) are HTML-escaped before any
  `innerHTML` interpolation. **Live:** v0.1.0 published to the public
  `finmodel-releases` repo (private source → unauthenticated updater needs a
  public channel); the `latest/download/latest.json` endpoint is verified 200.
- **Always-visible update control (v0.1.1)** — a persistent footer shows the app
  version and a one-click update status/button (Check for updates → Checking →
  Up to date · vX / Update available → install), mirroring the Snitch Voice
  pattern instead of hiding the check in Settings. `load_settings` now returns
  the running version. Fixed a CSS bug where `.banner { display:flex }` overrode
  the `hidden` attribute, so the update banner showed spuriously. Published
  v0.1.1 to `finmodel-releases`; the endpoint serves 0.1.1 and installed 0.1.0
  clients are offered the update (end-to-end auto-update verified).

### Changed — desktop app UX (self-explanatory workspace)
- **Guided, discoverable UI** (`ui/index.html`, `ui/app.js`, `ui/style.css`) —
  the app now teaches the user what it does and exactly how to use it, instead
  of a bare pair of unlabeled inputs. New: a purpose headline; a **two-tool
  layout** (1 · Build a full model — one ticker → 3-statement + DCF; 2 ·
  Benchmark a peer set — comma-separated US tickers → comps); **inline
  ticker-format help** with concrete examples (`SYMBOL` vs `SYMBOL.EXCHANGE`;
  "two or more US tickers, comma-separated") and a **live parsed echo** (ticker
  normalization / peer count as you type); **"You get" outcome tags** naming
  every sheet/metric produced; a **contextual mode banner** that states honestly
  what works right now (benchmarking needs no key; full models need a key beyond
  the 5 demo companies) with a Live/Demo pill; a **save-location note**
  (Documents\finmodel\); and a results panel hint distinguishing historical vs
  projected columns. Buttons stay disabled until input is valid. The Tauri
  invoke contract is unchanged (`build_model` / `benchmark_peers` /
  `open_path` / settings). Verified against all states (empty, live/demo,
  populated model + benchmark, settings) in a headless browser with a mocked
  bridge.

### Added — research/benchmarking subsystem (filings → Excel)
- **SEC filing-doc index** (`fm filings <ticker> [--form 10-K] [--limit N]`) —
  ports `get_recent_filings` / `search_filings` from `src/research/sec_edgar.py`
  into `fm-fetch::edgar`: resolves a company's recent filings from the SEC
  submissions history into `Filing` records (form type, filing date, report
  date, accession number) each carrying a direct URL to its primary document in
  the EDGAR Archives (`…/Archives/edgar/data/{cik}/{accession}/{doc}`, leading
  zeros stripped, dashes removed — faithful to the Python URL construction).
  `search_filings` filters by a form-type set (`DEFAULT_FORM_TYPES` =
  10-K/10-Q/8-K/20-F/6-K); `recent_filings` filters a single type. The parse +
  URL construction is a pure, network-free function gated by unit tests
  (`parse_recent_filings_*`); live EDGAR paths covered by `#[ignore]` tests.
  Live-verified on AAPL (US 10-K/10-Q/8-K) and TSM (foreign 20-F/6-K filer).
- **Desktop app: peer-benchmark panel** — new `benchmark_peers` Tauri command
  (`src-tauri/src/commands/benchmark.rs`) wrapping `fm_research::benchmark_tickers`
  + `render_benchmark`; writes xlsx+csv to Documents/finmodel/ and returns a JSON
  summary. New UI card (tickers input, preset peer sets, results table, Open
  Excel/CSV). App lib + full binary compile & link; frontend embeds. Underlying
  pipeline live-verified via the identical CLI path.
- **USD normalization** (`fm benchmark --usd`) — converts absolute monetary
  metrics to USD at spot FX (Yahoo `{CCY}USD=X`, no key) so mixed-currency global
  peer sets are directly comparable and their MEDIAN/MEAN are meaningful; ratios
  and multiples are FX-neutral and untouched. Per-currency rate cache; the Ccy
  column shows each row's value currency (USD when converted, native if FX
  unavailable — never silently mixed). Live-verified: TSM TWD→$90B, SAP EUR→$42B,
  NVO DKK→$47B alongside AAPL $416B.
- **Global IFRS filers** — foreign 20-F filers reporting under `ifrs-full` on
  EDGAR (TSM, SAP, NVO, SHEL, ASML, …) now benchmark from structured XBRL, **no
  LLM**. `fm-extract::xbrl::ifrs_tag_map` (canonical → IFRS concepts) +
  `select_taxonomy` (picks us-gaap vs ifrs-full by concept count) + broadened
  currency detection (TWD/EUR/DKK/… dominant-unit). Provenance is taxonomy-
  qualified (`us-gaap:` / `ifrs-full:`). Also: **data-anchored target years** —
  the extraction window anchors to the filer's own latest reported annual FY
  (not the wall clock), so late-window / behind-calendar filers extract too.
  Unit-tested (IFRS parse, owners-of-parent NI preference); live-verified
  TSM/SAP/NVO/SHEL/ASML. Gate-safe (committed-snapshot gates unaffected).
- **Trading multiples** (`fm benchmark --multiples`) — the heart of IB comps:
  EV/EBITDA, EV/Revenue, P/E and market cap, computed from filing-derived EV
  components (net debt, diluted shares, EBITDA, net income) × a live share price
  (Yahoo Finance, no key; `fm-fetch::market::fetch_quote`). Combinable with
  `--ltm`. Columns render only when priced; per-cell notes mark the price as a
  market input (not a filing figure). Blank on missing components / negative
  earnings — never fabricated. Unit-tested; live-verified (AAPL P/E 38.6x,
  EV/EBITDA 29.8x, mkt cap $4.7T).
- **LTM (last-twelve-months) basis** — `fm benchmark --ltm` reports scale /
  margins / returns / leverage / liquidity / capital-return on a trailing-twelve-
  months basis (`FY + latest YTD − prior-year YTD`; balance sheet = latest
  instant), the standard IB comps basis; growth & CAGR stay annual. Per-row label
  becomes `LTM <as-of>`. `fm-extract::ltm` (extract_ltm / fetch_ltm /
  fetch_xbrl_bundle — one companyfacts download → annual + provenance + LTM).
  Freshest-tag selection + staleness guard drop discontinued tags (e.g. AAPL's
  untagged interest expense) rather than surface a stale figure. Unit-tested
  (stitch, annual fallback, stale-drop); live-verified (AAPL LTM rev $451B).
- **Benchmark metric set (18 across 7 dimensions)**: Scale (revenue/EBITDA/net
  income), Growth (YoY + full-window revenue CAGR), Profitability (gross/EBITDA/
  net/FCF margin), Returns (ROE/ROA), Capital Return (dividend payout + total
  shareholder payout, from the CFS), Liquidity (current ratio), Leverage (net
  debt / net-debt-to-EBITDA / interest coverage) — all from filings, unit-tested.
- **Tag-level provenance** — each raw benchmark figure now cites the exact
  matched us-gaap XBRL tag (e.g. `us-gaap:RevenueFromContractWithCustomerExcludingAssessedTax`),
  not just the fiscal year. `fm-extract::parse_xbrl_to_raw_with_provenance` /
  `fetch_xbrl_with_provenance` (additive; `fetch_xbrl`/`parse_xbrl_to_raw` are
  now thin wrappers). Unit-tested (winning-tag capture).
- **`fm verify`** now filters snapshots structurally (`model_output` present &&
  not `*_full_*`), so the new gate oracles (adhoc / ev_bridge / ifrs_bridge)
  never break it.
- **Sector column** — best-effort EDGAR SIC industry (submissions endpoint) per
  peer, so financials (banks/insurers) whose leverage/coverage read differently
  are visible; never fails the run. `fm-fetch::fetch_company_sic` + `SicInfo`.
- **`fm benchmark --csv PATH`** exports the raw benchmark grid (header + one row
  per company, values verbatim) for drop-in use in a banker's own model.
- **`fm benchmark --tickers AAPL,MSFT,… [--out …] [--title …]`**: fetches each
  peer's SEC EDGAR XBRL companyfacts, computes latest-FY scale / growth /
  profitability / returns / leverage metrics, and renders an IB-grade comparison
  workbook with grouped headers, a MEDIAN/MEAN/MIN/MAX summary block (live Excel
  formulas + cached results for offline viewers), a reporting-currency column,
  and per-cell provenance notes back to the filing. Live-verified on
  AAPL/MSFT/GOOGL/AMZN/META (real FY2025 figures).
- **`fm-excel::adhoc`**: port of `src/research/output_writer.py`
  (`pick_adhoc_layout` + `AdHocExcelWriter.write_research`) onto the shared
  cell-model/render engine. Gated cell-for-cell (value/formula/fill) against a
  Python oracle — `tieout/build_adhoc_oracle.py` → `ADHOC_bench_snapshot.json`,
  `tests/adhoc_parity.rs` (0 diffs), plus decision-tree unit tests.
- **`fm-research` crate**: `metrics_from_extraction` (pure), `build_benchmark_table`,
  `render_benchmark`, `benchmark_tickers` (live). Unit-tested; failures reported,
  never fabricated.
- **XBRL**: added a `short_term_debt` tag key (current portion / CP / revolvers);
  benchmark total debt = long-term + short-term so leverage isn't understated.
  Gross profit falls back to revenue − COGS when a filer omits the GrossProfit tag.
- `Cell.comment` → xlsx notes in the render engine (provenance; ungated).
- **EV-bridge worksheet** — port of `ResearchExcelWriter.write_ev_bridge` →
  `fm-excel::bridge`; `fm ev-bridge --xlsx PATH [--ltm-revenue --ltm-ebitda]`
  renders equity value → EV checklist → valuation multiples → rules, with live
  MC/EV formulas and source notes. Oracle-gated full + sparse
  (`ev_bridge_parity.rs`), the sparse case covering dynamic row-skip / formula
  row-refs.
- **IFRS-16 bridge worksheet** — port of `ResearchExcelWriter.write_ifrs_bridge`
  → `fm-excel::bridge`; `fm ifrs --xlsx PATH [--company --period
  --standard-depreciation --standard-amortization --short-term-rent]` renders
  EBITDA derivation (adjusted/computed) → IFRS-16 adjustment → EBIT/EBITA bridges
  → excluded items → data sources, direction-aware (IFRS↔US GAAP). Oracle-gated
  full + simple (`ifrs_bridge_parity.rs`) covering the branchy paths. Completes
  research-port item 1 (benchmark + EV bridge + IFRS bridge all gated).

**Phase 1 Wave 1 (task 1.1.0) + harden-basket sprint: tie-out unblocked, basket fixed & hardened, baseline re-frozen to 339/350 (96.86%) on 7 industrials.**

### Fixed
- Tie-out LLM transport: pass explicit `--model` — headless `claude -p` inherited the broken global `claude-opus[1m]` alias (rc=1), which had blocked all of Phase 1. `tieout/llm.py` (opus examiner), `src/extractor.py` (opus default; override `FINMODEL_LLM_MODEL` / `FINMODEL_TIEOUT_MODEL`).
- `tieout/pin_filings._download`: single-iterator download — was calling `iter_content()` twice on one streamed response, truncating large PDFs (root cause of "MC.PA discovery failed").
- BASF income-statement extraction: `_extract_financial_section` now recognizes "statement of income"/"statement of operations" titles (BASF titles its IS "Statement of Income", not "income statement"), so the IS reaches the model (BAS.DE 34/52 → 50/52).
- MC.PA ground truth corrected: it was built from LVMH's *condensed* financial-review balance sheet (intangibles = brands + goodwill combined = 49,611). Added a per-company `gt_start_page` hint so the GT face-window uses the *primary* consolidated statements (brands 25,589 + goodwill 24,022 split); coverage 32 → 48 cells (MC.PA 28/32 → 44/48).
- `fm-tieout` Rust test no longer reads a gitignored modelcache — committed `tests/fixtures/atco_model.json` + `include_str!` (CI-safe on a fresh clone).

### Changed
- Basket: SAP.DE → BASF (BAS.DE). SAP's 344-page integrated report (parent-HGB statements before consolidated IFRS + 17 decoy pages) defeats face-window detection; BASF's standalone consolidated-statements PDF ties out cleanly (52-cell GT). MC.PA pinned + added (32-cell GT).
- Ground truth committed + immutable per company (`tieout/groundtruth/*.json`); previously only ATCO was committed and the rest rebuilt per-run (non-deterministic).
- Baseline re-frozen (`tieout/results/_baseline_wave0.json`): 339/350 (96.86%) across 7 industrial companies. The old 256/256 was built on a Claude model generation that can no longer be invoked (unreproducible).
- Phase R parity gate wording: 256/256 → 339/350 / cell-for-cell (MASTER_PLAN.md, CLAUDE.md, RELEASE_CHECKLIST.md, FINMODEL_PRODUCTION_PROMPT.md).

### Known gaps (Rust-engine extraction targets, per the Rust amendment)
- 11 remaining mismatches are extraction-convention targets: `net_income` group-vs-total incl. minorities (BASF, MC); `sga` selling-vs-G&A split (MC); `dividends_paid` (ATCO, NESN); `ppe_net` IFRS-16 right-of-use (ATCO).

## v0.1.0 (current)

**Initial baseline — 256/256 tie-out on 5 European industrials. Dynamic IS Phases 1–4 implemented.**

- Master plan committed (`7c8c342`)
- Amendments: build-first, Rust
- Project packaging: `pyproject.toml` with setuptools, `finmodel` CLI entry point
- Release checklist and changelog established
