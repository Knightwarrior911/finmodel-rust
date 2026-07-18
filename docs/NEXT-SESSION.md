# finmodel-rust — Resume / Mission

## AGENTIC-ANALYST PLAN — 27 task-cores done (Phases 0–1 complete; 2–6, 8, 9.1 cores landed)

Executing `local://agentic-financial-analyst-plan.md` (the reducer-driven native
analyst runtime). All backend logic cores are unit-tested; remaining work is
live-driver wiring, the Phase-2 UI DOM/CSS cutover, and desktop verification
(consolidated at Task 9.3). Latest gates (0 fail): `cargo test -p finmodel-app
--lib` **264**, `fm-agent` **47**, `fm-value` **29**, `npm --prefix ui test`
**124**. See "Session status" below for the per-task breakdown. All touched files rustfmt'd.

## LATEST SESSION — 2.1 single-event-path cutover + 2.2 Pause/Resume controls + move-to-project fix

Gates green (`scripts/gates.ps1`): **app-lib 294**, workspace green, **ui 130**,
research-eval 13. Debug port reverted (0 refs). s1–s7 verified live before the tail edits
(final build not re-smoked: PDF Panda holds debug port 9222 — config-only revert since).

- **2.1 result cards on the durable path (Rebuild 1 of the cutover).** All tool
  result cards now render from a durable **`ResultPartAdded`** event, not the
  transitional `chat_tool` channel. `ToolBatchOutcome` gained a `parts: Vec<ResultPart>`
  (tool_call_id/name/card); `driver.schedule_tools` populates it; the actor emits one
  durable `ResultPartAdded` per card (persist-before-broadcast). The **verify card** is
  also durable now: `LiveDriver` stashes it in `verify_card`, the actor takes it via a
  new `Driver::take_verify_card()` (default `None`) and emits `ResultPartAdded`. `chat.mjs`
  renders `result_part_added` → `appendCard` (+ mission verify badge); the `chat_tool`
  card-append path was removed (thinking-step lifecycle still rides `chat_tool`). Tests:
  `tool_result_emits_durable_result_part` (actor). Live-verified via s4/s6/s7 (quote,
  financials, verification cards all render on the durable path).
  **Also fixed:** `hideMission` now resets the `.mission-verify` className (was leaving a
  stale status colour) — the root of a confusing debug round.
- **2.2 Pause/Resume command-bar controls (this session).** Added a Pause button next to
  Stop in the composer (`index.html`); Pause calls `agent_pause`
  (→ `RunInterrupted`, resumable), and an interrupted terminal surfaces a **Resume**
  affordance that calls `agent_resume` (relaunch from the last complete boundary via
  `resumeRun`). Backend was already built+tested (`registry.pause`, `resume_run`→
  `launch_run`; actor `control_interrupt_yields_interrupted_resumable`). New UI test:
  `chat.test.mjs` "pause surfaces a Resume affordance that relaunches via agent_resume".
  This completes 2.2's command-bar control slice; the mission-shell Evidence dock remains
  (see "Genuinely remaining").
- **2.1 Rebuild 2 — DONE this session (single live event path).** Text deltas now
  ride an ephemeral `agent_event` `assistant_text_delta` (from `chat.rs`
  `emit_agent_ephemeral`); the thinking-step lifecycle rides durable
  `ToolStarted`/`ToolSucceeded`/`ToolFailed` keyed by `tool_call_id` (`ToolStarted`
  enriched with the tool `name` from call-meta). The legacy `chat_delta`/`chat_tool`
  listeners are removed from `chat.mjs`/`initChat`. So ALL live rendering (deltas, tool
  status/thinking, cards, plan, phase, approvals, memory, terminal) now flows on the
  single `agent_event` channel. Verified live s1–s7 (s2 streaming text on the new delta
  channel; s4 durable thinking + cards). `chat.test.mjs` stray-delta test updated to the
  `agent_event` channel.
- **Move-to-project fixed (user-reported).** The picker (`sidebar.mjs` move handler) now
  preselects the chat's CURRENT project — it always read "— No project —" before, so a
  chat already in a folder looked mis-placed and re-picking the shown value fired no
  `change` (looked dead). Zero projects now shows a "No projects yet" hint instead of a
  dead-end menu; clicking away restores the row. The move itself already worked
  (CDP-confirmed: NVDA chat relocated into its folder). Tests: two new `sidebar.test.mjs`
  cases (preselect + zero-projects), 127→129 UI.
- **2.1 status — honest scope (NOT fully the plan's 2.1).** The LIVE single-event-path is
  complete + verified. The plan's "subscribe→snapshot→gap-close" also names a RELOAD path
  (re-subscribe `agent_event` before `get_run_snapshot`, then `get_run_events_after` to
  close the gap) for byte-equal resume of an IN-FLIGHT run across a mid-run reload. That is
  NOT built: reload still uses `load_conversation` (rebuild from persisted message parts) —
  correct for COMPLETED chats, but does not resume a run interrupted by reload. Gate
  "live == reload byte-equivalent" holds for completed conversations, open for in-flight.
- **Sidebar redesign (user-requested).** Removed the confusing single-option "Personal"
  workspace dropdown (`#workspaceSelect` + label) — dead UI (no workspace-creation flow;
  Projects already group chats). `render()` guards the now-absent select. Kept the useful
  **Temporary chat** toggle (relabeled, full-width subtle button). Rebuilt the conversation
  rows: single-line title with ellipsis + compact right-aligned timestamp (`relTime` now
  emits `now`/`5m`/`1h`/`3d`, sidebar-only); the row actions are `position:absolute` (were
  consuming ~85px of width via `opacity:0`, squeezing titles to ~3 chars) with a fade-in
  gradient, so titles fill the row and actions overlay per-row on hover. Verified live in
  light + dark. `workspaces.test.mjs` unaffected (tests the reducer, not the markup).

## PRIOR SESSION — live-wiring pass (Phase 3/4/6/7 cores → live) + live Plan panel

Gates green (`scripts/gates.ps1`): **app-lib 293**, fm-agent/fm-value workspace green,
**ui 127**, research-eval 13. Debug port reverted (0 refs in `tauri.conf.json`); clean
release-safe build. Six backend cores wired into the live driver/actor pump + one
additive Phase-2 UI slice, each unit-tested and (backend) CDP-verified via
`tools/ui_smoke/s1..s7`.

- **3.2 live plan-step mapping.** The `run_turn` pump now holds the typed `Plan`,
  advances `advance_active()` when a tool batch begins and `complete_steps()` when it
  finishes (and completes remaining steps at Synthesize), re-emitting the whole revised
  `PlanUpdated` — transition-driven, never time-based (`actor.rs::emit_plan`). Test:
  `plan_steps_advance_through_tool_and_synthesis_transitions`.
- **3.3 canonical context assembly.** `LiveDriver::prepare` now assembles via
  `context::build_context` (system policy → workspace grounding → recalled memories →
  compacted branch → current turn). Refactored `build_context` to take the system prompt
  as a param so the live **analyst prompt** (with tool-routing guidance) stays the
  authority — `system_policy` is the weaker default. `apply_grounding` → `grounding_layers`
  (returns the pieces). Recall + record_use preserved.
- **3.4 overflow guard.** `request_model` retries once on `ContextOverflow` after
  `prune_history` drops the oldest turns (keeping system layers + latest N + current
  turn, and dropping any orphaned leading `tool` reply so providers never 400). Test:
  `prune_history_keeps_system_and_tail_and_drops_orphan_tool`.
- **4.2/4.4 independent verify recompute.** `verify()` now checks the accounting identity
  `gross_profit == revenue − cost_of_revenue` from sibling claims (`recompute_authoritative`);
  an inconsistent figure → `Unverified` → the reducer's repair pass → partial (never a
  verified badge). Test: `gross_profit_identity_catches_an_inconsistent_figure`. Live: the
  earnings-review mission shows **Verified 6/6** with the identity holding.
- **6.1 transient retry.** `request_model` retries the same provider once (750ms backoff)
  on a retryable category (`is_retryable`: rate-limit/capacity/transport/timeout); a
  second failure stops visibly. Extracted `accept_stream` (DRY across all retry arms).
  Full failover-across-roster remains the tested `request_with_retry` core (needs a
  multi-profile config → 9.3).
- **7.3 skill lifecycle live.** `grounding_layers` excludes stale/archived skills from the
  catalog via `store::inactive_skill_names` (hand-dropped skills with no lifecycle row stay
  visible); a daily `age_skills` tick added in `lib.rs` (30d stale / 90d archive). Test:
  store lifecycle asserts `inactive_skill_names`.
- **Phase 2 slices — live Plan panel (2.3) + mission-status header (2.2).** `chat.mjs`
  now handles `agent_event` `plan_updated` → `renderPlan` (steps with
  pending/running/done + `.plan-panel` CSS) AND drives a `#missionHeader` pill
  (`updateMission`): workflow name · phase (clean terminal label: Delivered/Stopped/…) ·
  `N/M steps` · verification badge. Both additive to the working transitional path;
  reset on send/new-chat/load. Live-verified `tools/ui_smoke/s7_plan_panel.py`: an
  earnings-review mission renders its 5 steps (all reach done) and the header shows
  `Earnings review · 5/5 steps · ✓ Verified`.
- **9.4 CSP/capabilities confirmed** restrictive: `capabilities/default.json` exposes only
  `core:default` + window-drag + dialog/opener/updater (no fs/shell/http); CSP locks
  `script-src 'self'`, `connect-src 'self' ipc:`. Security tests pass in the gate.
- **2.4 / 9.3 responsive + theme verification (partial).** Drove the built UI in a real
  headless Chromium (file://) with a representative delivered-mission state (mission
  header + plan panel + financials table + verification card) across **wide 1280 /
  standard 1000 / tablet 768 / floor 620·600** × **light + dark** → **0 horizontal
  overflow** at every supported width in both themes; new components render + wrap
  correctly (long plan-step labels wrap, no clip). The 400px overflow is the app's
  PRE-EXISTING 600px shell floor (`#app`), not a regression. Reduced-motion is honored by
  the global `@media (prefers-reduced-motion: reduce)` rule (covers the new components).
- **2.4 undefined-alias → token mapping DONE (step 1).** Audited `style.css`: **7 tokens
  were used with no definition and no fallback** (`--secondary --tertiary --primary --panel
  --panel-2 --hover --accent-bg --mono --fg`), silently dropping `color`/`background`/
  `font-family` on activity rows, task badges, workspace/memory banners, approval cards,
  and temp-chat controls. Added them to `:root` as `var()` aliases of existing themed
  tokens (`--muted`/`--faint`/`--ink`/`--raised`/`--element`/`--accent-soft`/`--font-num`),
  so they resolve to the active theme at use time. Verified in a real browser: `.act-query`
  computes `#6e6c78` (light) / `#a09eab` (dark); `--panel`/`--accent-bg`/`--mono` all
  resolve per-theme. Re-audit: **0 undefined-no-fallback tokens**.
  Remaining 2.4/9.3: the 12 named acceptance-view captures, hard-coded Phase-D status-color
  tokenization (currently theme-correct via `[data-theme=dark] .act-*` overrides),
  dock-open responsive generalization (blocked on the unbuilt Evidence dock), and the
  comps/dcf/crash-resume/cancellation scenario legs.
- Smoke suite grew to **s1..s7** (`s7_plan_panel.py`) + `shot.py` (CDP screenshot). To
  re-verify live: re-add the `--remote-debugging-port=9222` window arg, `cargo build -p
  finmodel-app`, launch, run `s1..s7`. **Revert the port before any release build.**

### Genuinely remaining (with precise blockers, not deferrals of convenience)
- **Phase 2 mission shell (2.2) + visual finish (2.4).** DONE this session: 2.1 legacy
  `chat_delta`/`chat_tool` listener removal (single `agent_event` path) and **2.2's
  Pause/Stop/Resume command-bar controls** — Pause (`agent_pause`→`RunInterrupted`,
  resumable) + a Resume affordance (`agent_resume`, relaunch from last checkpoint), unit
  tested (`chat.test.mjs` pause→resume flow) atop the already-live backend
  (`registry.pause`, `resume_run`→`launch_run`; actor tests green). STILL REMAINING and
  unstarted — the bulk of 2.2 (plan steps 5–7): convert the reader aside into the tabbed
  **Evidence dock** (Model/Valuation/Sources/Artifacts/Reader), a new `ui/js/workbench.mjs`
  owning `body.dock-open` + focus return + the keyboard map (`Ctrl/⌘+1..5`, `Ctrl/⌘+J`,
  arrow plan/tab nav), migrating the EV/IFRS/tie-out controllers out of `#analystModal`
  into the dock's Model tab and deleting the modal; plus 2.4's dock-open responsive
  generalization + the 12 named acceptance-view captures. This is a large rewrite of a
  *working, live-verified* UI — sustained card parity + multi-viewport/theme/a11y live
  verification, its own session; do not rush at a tail.
- **6.2 progressive disclosure (live).** Blocked on a registry **core-tools designation**
  (so finance/control tools are never dropped) + the **MCP discovery bridges** (6.2
  steps 2/4/5). Inert below threshold with the fixed 12-tool registry; core (threshold +
  BM25 rank) tested.
- **5.2/5.3 real child fan-out (live).** Replacing the *already-working* parallel-tools
  fan-out with child `run_turn` instances is a deep schedule_tools rewrite; core
  (`child.rs::run_child_delegation`) tested with FakeDriver; verification needs the 9.3
  concurrent-child scenario.
- **8.2/8.3 commitments/schedules (live).** Blocked: nothing creates a schedule in
  production (`insert_schedule`/`insert_commitment` are test-only), so `scope_json` has no
  defined contract and the tick would be inert. Needs the commitment-**proposal → user
  approval → create-schedule** UI flow first (which defines `scope_json`). Cores
  (`extract_commitment`, `run_due_schedules` claim loop) tested.
- **9.3 full desktop scenario matrix.** s1..s7 cover the core flows live; the 7 golden
  missions × viewports × themes × a11y × crash/resume matrix remains.
- **External prerequisites (cannot be done here):** 7.1 automatic memory (needs a labelled
  ≥200-turn precision dataset per decision 4); 9.4 signed Windows bundle (needs the
  minisign signing key).

## PRIOR SESSION — number verification live (4.2/4.4) + financials card + FY-label fix

Gates green end-to-end (`scripts/gates.ps1` EXIT=0): **app-lib 290**, fm-agent/fm-value
workspace green, **ui 127**, research-eval 13. Debug port reverted (config == HEAD; no
`remote-debugging-port` in `tauri.conf.json`); clean release-safe build.

- **Number verification pipeline turned ON (was dormant).** `verify()` in
  `agent/driver.rs` no longer returns a bare `true` — it runs `verify_run` over the
  run's accumulated claims and emits a `verification` card.
  - `executors.rs::extract_claims` parses the exact-reported `financials` card
    (SEC EDGAR XBRL) into `Claim`s with workspace-scoped `source_id`; wired into
    `envelope_from_card`. Only numeric rows become claims; EPS gets a `/shares` unit.
  - `LiveDriver.run_claims` accumulates every batch's `env.claims` in `schedule_tools`.
  - `prepare()` sets `needs_verification` when a claim-producing tool
    (`get_financials`/`build_model`/`benchmark_peers`) is enabled, so financial turns
    verify; `verify()` is a no-op when a turn yields no claims.
  - Pure free fn `claim_authoritative` (testable without `AppHandle`) is the verify
    closure. **SLICE SCOPE (honest):** the authoritative value is currently the
    claim's own source-recorded figure, so a claim that parses is `Verified` — this
    proves numbers are sourced + badges the run, but does NOT yet detect a *restated*
    value. Genuine recompute (fm-value metrics) + model-prose crosscheck is the next
    step. Unparseable value → `Unverified` (unreachable from `get_financials`, which
    only emits numeric rows). Tests: `financials_card_extracts_material_claims`,
    `run_claims_verify_against_their_source_value`.
  - Live-verified over CDP (`tools/ui_smoke/s6_verify_numbers.py`): NVDA financials
    turn shows **Verified 6/6 — 6 of 6 material figures verified against SEC EDGAR XBRL**.
- **`financials` card renderer (was a blank "financials" fallback).** `cards.mjs`
  had no `case "financials"`, so the analyst's core output rendered as the unknown-card
  type string. Added `renderFinancials` (entity · FYxxxx · period end, figure table,
  SEC EDGAR link) + `renderVerification` (run badge + N/M source-checked). CSS in
  `style.css` (`.verify-badge`, `.card-verify.status-*`). Tests in `ui/tests/cards.test.mjs`.
- **FY-label trust bug fixed (`commands/chat.rs`).** `get_financials` labelled the
  card from the raw XBRL `fy` field, which tags a datapoint with the *reporting*
  filing's fiscal year — a comparative FY2024 figure inside a later 10-K carried
  `fy: 2026`, so the card read "FY2026 · period ended 2024-01-28" next to a green
  Verified badge (and poisoned `claim_key` to `.fy2026`). New `fiscal_year_label(fy,
  period_end)` trusts issuer `fy` only within ±1 of the period-end year, else uses the
  period-end year. Now reads **FY2024**; `claim_key` == `nvda.revenue.fy2024`. Test:
  `fiscal_year_label_prefers_period_over_stale_reporting_fy`.
- Smoke scripts added: `tools/ui_smoke/s6_verify_numbers.py`, `shot.py` (CDP
  `Page.captureScreenshot` to file). To live-verify again: re-add the debug port line
  to `tauri.conf.json` window config, `cargo build -p finmodel-app`, launch, then run
  the smoke scripts on `:9222`. **Revert the port before any release build.**

**Remaining for real restatement detection (4.4):** `claim_authoritative` must return
an *independent* recompute (e.g. gross_profit = revenue − cost_of_revenue via fm-value
metrics) rather than the claim's own value, so a mismatch yields `Unverified` and the
reducer's repair path fires. Also: model-prose claim extraction (compare the model's
stated numbers vs tool sources) is still unbuilt.

### Phase 0 — truthful harness (DONE)
- Reducer gained `Input::Interrupt` → `RunInterrupted` (resumable, distinct from
  terminal `Cancel`). `machine.rs` + tests.
- Actor/driver contract (`actor.rs`): `PreparedInfo{workflow,escalation}`,
  `Driver::make_plan()->Plan`, `elapsed_ms()`/`control_signal()`; the `run_turn`
  pump now feeds `Tick`/`WorkflowAccepted`/`Cancel`/`Interrupt` at I/O boundaries
  and emits a populated `PlanUpdated`.
- Pause vs stop: registry interrupt token + `pause()`, `agent_pause` command,
  `LiveDriver::control_signal`.
- Resume EXECUTES: shared `launch_run` in `commands/agent.rs`; `agent_resume`
  registers+spawns+drives (was a row-only orphan); rolls back on start failure.
- Invocation + typed checkpoint persistence: `tool_invocations` rows
  running→succeeded/failed; typed `AssistantCheckpoint` payload.

### Phase 1 — one contract (DONE)
- 1.1 Dual tool registries collapsed → `agent/tools.rs::ToolRegistry` is the sole
  authority (`params_schema`, `model_visible`, `agent_schemas()`, `shared()`,
  `validate_workflows()`); removed `chat.rs` `ToolName`/`tool_schemas`/
  `agent_tool_schemas`; string dispatch. Schema parity proven pre-deletion.
- 1.2 `ToolResultEnvelope` carries claims/progress/terminate + bounded summary;
  sources/artifacts promoted to store before the next model turn; typed
  `ArtifactCreated` emitted; deterministic-arithmetic system-prompt policy.
- 1.3 Normalized `ProviderError` categories + `classify_provider_error`; fixture
  parity test (OpenRouter vs DeepSeek → identical transcript); LiveDriver reasons
  in categories, not wire strings.
- 1.4 Typed event payload module (`events::payloads`) + real-emit conformance
  test; `SCHEMA_VERSION`→2; UI reducer rejects a strictly-newer schema.
- 1.5 `agent/model_router.rs`: `ModelRole`/`ModelProfile`/`ModelRoster`
  (deterministic routing, capability validation, fallback dedup, per-account
  credentials, no cross-profile leakage). `Settings::model_profiles` added.

### Deferrals recorded (in the todo list)
- 1.2 `ResultPartAdded` live emission → Phase 2 UI cutover.
- 1.3 connect/no-progress timeout split + drop `LiveDriver::remaining()` → Phase 6.
- 1.4 child/verification/commitment/schedule payload types → typed as those kinds land.
- 1.5 role-profile settings UI + wiring `ModelRoster::route(role)` into
  LiveDriver/child supervisor → Phase 2/5.

### Phase 2 (The Ledger Desk) — 2.1 reducer core DONE; rest needs a desktop run
**2.1 headless core (DONE, `npm test` 124 pass):** `reducer.mjs` restructured into
an idempotency+schema wrapper over `reduceInner`; both `reducer.mjs` and
`activity.mjs` now consume the real Rust envelope (`event.kind` + `event.payload`)
alongside legacy flat shapes; durable events dedup by per-run `(seqRunId,
seqApplied)` so snapshot + gap-close reload reproduces byte-equivalent state
(tested). `PlanUpdated` stores the whole plan payload.
**2.1 remainder (needs `cargo tauri dev`):** wire `main.mjs` → subscribe
`agent_event` BEFORE `get_run_snapshot` → gap-close via `get_run_events_after`
→ feed the tested reducers; remove legacy `chat_*` listeners ONLY after live
fixture parity. Then 2.2 mission shell, 2.3 plan/tool-tree/evidence render, 2.4
visual system — all DOM/CSS gates verified live in the running app.
**Then Phases 3–8 are backend-heavy and headlessly unit-testable** (workflow
selection/plan/context/compaction, verification/approvals, real child runs,
resilience, memory/skills, commitments/scheduler); live end-to-end verification
is consolidated in Task 9.3.

### Phase 3 (planning + context) — 3.1 & 3.2 DONE; 3.3/3.4 logic tested, wiring pending
- **3.1 DONE** (`fm-agent workflows` 13 pass): `select_workflow(user_msg)` —
  deterministic high-confidence intent → workflow id; golden fixtures select
  earnings_review / trading_comps; "Check on NVDA" → None (interactive). Wired
  into `LiveDriver::prepare` (escalates policy, turns on planning + verification,
  tags the plan; required-tools guard falls back to interactive).
- **3.2 DONE**: `WorkflowSpec::initial_plan(objective)` splits the `plan_template`
  arrow-chain into stable steps `s1..sN`; `make_plan` uses it (interactive turns
  get a 2-step plan). Remainder (needs live steering path): map plan-step status
  from tool/verify transitions + steering revision.
- **3.3/3.4 LOGIC built + tested** in `agent/context.rs` (`build_context` stable
  layer order + `compact_turns` 90%→70%, KEEP_LATEST=4, unresolved/no-summary
  never compacted) — 7 tests cover the gate. **Activation pending (live path):**
  replace `LiveDriver::prepare`'s ad-hoc rebuild with `build_context`; spill
  oversized tool results to the blob store; overflow-retry-once. These change the
  live prompt/runtime, so verify with the desktop drive (9.3).

### Session status — 23 plan-tasks core-complete, all green
Suites: Rust app-lib **256**, fm-agent **45**, fm-value **29**, UI **124** — 0 fail.

**Backend cores DONE + unit-tested this session (beyond Phases 0–1):**
- 3.1 workflow selection; 3.2 workflow-grounded plan; 3.3/3.4 compaction wired into
  live `prepare()` (`context::compact_turns`, analyst prompt preserved).
- 4.1 source dedup (workspace-scoped ids, no cross-boundary leak) + `insert_citation`.
- 4.2 `agent/verification.rs` — claim verify with metric-specific tolerance; the
  15%-vs-12% mismatch gate is caught; `fm-value::metrics` is the single tolerance authority.
- 4.4 `fm-value::metrics` — growth/margin/ratio/cagr/bridge/scale + tolerance.
- 5.1 `delegations` table (v3 migration) + persist + `outcome_unknown` recovery.
- 6.1 `model_router::decide_retry` retry/failover/cost-stop + `over_spend_cap`.
- 8.1 `commitments`/`schedules` tables (v3) + exclusive transactional `claim_due_schedule`.

**Remaining = live-integration wiring + UI, all listed as todo items:**
- Desktop-only: 2.2–2.4 DOM/CSS + 2.1 live cutover (`main.mjs` subscribe→snapshot→
  gap-close, remove legacy `chat_*`). Verify with `cargo tauri dev` (Task 9.3).
- Driver wiring (cargo/ScriptedDriver-testable): 4.2/4.4 `driver.verify()` uses the
  verification engine + metrics recompute + one repair; 6.1 `request_model` retry
  loop + fallback rotation; 5.2/5.3 child supervisor executes `delegate` + at-least-once
  delivery; 8.2/8.3 commitment extraction + `scheduler.rs` claims due rows through
  `ActorRegistry`; 4.3 durable approvals (and close the pause-during-approval gap:
  persist pending_interactions, don't fabricate Deny on interrupt).
- 7.x memory/skills governance; 6.2 progressive disclosure/MCP; 9.x gates + packaging.
Live end-to-end verification of everything is consolidated in Task 9.3.

**Also landed (Phase 9 + on-disk artifacts):**
- 8.2 commitment extraction (`agent/commitments.rs`); 4.3 store-half durable
  approvals (`unresolved_pending` + `expire_pending`, fail-closed);
  6.2 progressive-disclosure threshold + ranking; 9.1 workflow completion gate
  (`WorkflowSpec::missing_parts`/`is_complete`).
- **9.2 DONE** — durable gate runner `scripts/gates.ps1` runs all four gates and
  fails on any non-zero; verified green end-to-end (core workspace, app-lib 264,
  UI 124, research-eval 13).
- **9.4 notices** — `docs/THIRD_PARTY_NOTICES.md` (MIT attributions for the
  clean-room-reimplemented Oh My Pi / OpenClaw / Hermes behavior). Remaining 9.4:
  CSP/capabilities confirm + signed Windows bundle (desktop build).
- **5.3 delivery core** — `store`: at-least-once child-result delivery via a
  `delivery_state` CAS keyed by owner `claim_id`
  (`claim`/`ack`/`release`/`undelivered_completed`/`reclaim_stale_deliveries`,
  time-based). Proven: two consumers race → one wins; non-owner can't ack/release;
  crash-before-ack is reclaimed; delivered is never re-claimed (app-lib 265).
  Remaining 5.3: the actor calls claim→append-to-parent→ack (live).
- **3.2 step-mapping core** — `fm-agent` `Plan::set_step_status`/`complete_steps`/
  `advance_active`: tool/verify transitions → plan-step status is a pure plan
  transformation, never time-inferred (fm-agent 48). Remaining 3.2: the pump
  applies it as tools complete + emits the revised `PlanUpdated` (live).
- **Phase 5 COMPLETE (5.1–5.4)** — `agent/child.rs` `run_child_delegation`: a
  one-level child supervisor that persists the delegation + child run BEFORE
  execution (5.1), drives a real child through `run_turn` (any `Driver`), maps the
  terminal turn → delegation status, and delivers the single result at least once
  via the 5.3 CAS with an async `deliver` callback (ack on append success, RELEASE
  on failure — no fake ack). Proven in the actor harness (FakeDriver + in-memory
  store + CollectSink, no AppHandle): child runs + finalizes; depth limit blocks
  grandchildren; cancelled child never delivered; failed append is redeliverable;
  and 3 peers fan out CONCURRENTLY (`tokio::join!`) each delivering once (app-lib
  270). Remaining Phase 5: the live parent pump invokes the supervisor with a
  LiveDriver child + performs the real parent-context append inside `deliver`.
- **6.1 retry orchestrator** — `model_router::request_with_retry`: composes
  `decide_retry` + `over_spend_cap` + failover rotation + per-attempt cost
  accounting over injected `attempt`/`backoff` (ScriptedDriver-testable). Proven:
  first-try success; retryable→retry-same-profile→success; failover→next profile;
  spend cap stops before overspending; non-retryable/exhausted-fallbacks stop with
  reason (app-lib 276, model_router 13). Remaining: LiveDriver.request_model calls
  it over roster profiles (live-only behavior; tracked in todo).
- **Phase 8 COMPLETE (8.1–8.3)** — 8.3 `scheduler::run_due_schedules`: claims each
  due schedule once (8.1 CAS), runs the follow-up via an injected closure, and
  finalizes `done` / retries (back to `pending` with backed-off `next_due`) /
  fails terminally after `max_attempts`, with attempt accounting
  (`store::finish_schedule`/`fail_schedule_attempt`/`schedule_state`). Proven:
  two due schedules both finalize; a failing one retries then fails at max
  (app-lib 278). Remaining: the live periodic tick invokes the sweep + spawns the
  real follow-up run.
- **3.4 blob-spill core** — `store::spill_result`: oversized tool results spill to
  the content-addressed blob store, returning a char-boundary-safe bounded preview
  + opaque id (full result recoverable); small results stay inline (app-lib 279).
  Remaining 3.3/3.4: full build_context layer assembly + rolling-summary
  persistence + overflow-retry (live context path).
- **4.3 durable-approval COMPLETE (core)** — `agent/approvals.rs`
  `expire_and_deny_stale_approvals` + `store::expire_pending_runs`: the expiry
  sweep expires stale `pending_interactions` AND signals `Deny` to each affected
  run's parked oneshot (`RegistryHub::resolve_approval`), so `await_approval`
  returns promptly and NEVER hangs (the MUST). Proven headlessly: a parked waiter
  resolves to `Deny`, the row is expired fail-closed (app-lib 280). Remaining 4.3:
  the driver inserts the pending row on park + a periodic tick runs the sweep.
- **7.3 skill lifecycle COMPLETE (core)** — schema v4 `skill_lifecycle` +
  `store` methods: deterministic aging (active→stale→archived by disuse),
  `record_skill_use` (revives stale), `supersede_skill` (lineage), `restore_skill`,
  `active_skill_names` (stale/archived excluded from default context, still
  inspectable/restorable). Proven (app-lib 281). Remaining 7.3: settings UI +
  wiring catalog injection to `active_skill_names`.
- **4.2/4.4 verify-loop core** — `verification::verify_run`: per-claim
  `verify_direct` + `rollup` + `needs_repair`, over an injected authoritative
  recompute (source or `fm-value::metrics`); missing evidence never certifies; the
  run rolls up to the weakest status (app-lib 282). Remaining: LiveDriver.verify
  extracts run claims + calls it + feeds the repair pass (live).
- **1.3 timeout hardening COMPLETE** — the adapter (`commands/chat.rs`) owns
  `CONNECT_TIMEOUT` (5s) + `NO_PROGRESS_TIMEOUT` (20s) + the per-request ceiling
  (= reducer `remaining`), and the reducer boundary `Tick` owns the overall
  deadline; the temporary in-driver `LiveDriver.request_model` `remaining().is_zero()`
  guard is now removed (plan line 315: "only then remove"). app-lib 282 green.
- **4.3 approval PARKING wired (PARTIAL — see "4.3 WRITE-RISK GATE" entry below,
  which supersedes the "Phase 4 DONE" claim: the gate was dormant until the
  proposal-time fix).** `LiveDriver.await_approval` now
  inserts a durable `pending_interactions` row before parking and resolves it
  after (additive, tested `insert_pending`/`resolve_pending`), and `lib.rs` setup
  spawns a 60s approval-expiry sweep tick (`expire_and_deny_stale_approvals`,
  600s cutoff = the in-driver safety window). The tested expiry→Deny property now
  operates on real rows; `await_approval` never hangs (timeout+cancel+sweep).
  app-lib 282. (Sweep loop's periodic firing is runtime-verified at 9.3.)
- **1.5 UI role-profile picker COMPLETE** — backend round-trip
  (`settings::settings_view_json` exposes `model_profiles`; `save_settings` accepts
  it; 2 Rust tests) + additive collapsed "Model roles (advanced)" section in
  `index.html`/`settings.mjs` (worker+verifier provider/model/credential-account,
  untouched existing fields) + `dialog.test.mjs` (load-populates + save-sends;
  UI 126). Secrets stay as OS-keychain `credential_ref` names, never in the UI.
  v1 scope: UI edits `provider_base`/`model`/`credential_ref` for worker+verifier;
  `fallbacks` is sent `[]` (not yet UI-editable) and `context_window`/`native_tools`/
  `structured_output`/`cost_*` fall back to serde defaults for UI-entered profiles —
  the 6.1 live call-site must not assume full-fidelity profiles from the picker.
- **7.2 skill-surface slice COMPLETE** — `skills_list` now overlays lifecycle
  state/use/version (best-effort), `skills_save` upserts the `active` row,
  `skill_restore` command + a Restore button in `loadSkillsList` for
  stale/archived skills (reversible); `dialog.test.mjs` proves the state badge +
  restore invoke. Reuses 7.3's tested `skill_lifecycle` store.
- **7.2 memory-pin slice COMPLETE** — schema **v5** adds `memories.pinned`;
  `store::set_memory_pinned`/`is_memory_pinned` (Rust round-trip test);
  `memory_pin` command; `memory_list` overlays `pinned` (pinned sort first);
  `loadMemoryList` shows a 📌 badge + Pin/Unpin toggle; `memory.test.mjs` proves
  the toggle invoke + badge. Delete + pin together satisfy memory reversibility.
  Suites after this slice: app-lib **285**, UI **122** (authoritative per-file
  sum; earlier 126/127 readings were transient streamed-output misreads).
  **7.2 remaining (next session):** memory edit + supersede + filter, and
  recalled-memory attribution in the mission (the last belongs with the Phase 2
  mission shell). Same picker-sized shape — a `superseded_by` column already
  exists; edit needs an `update_memory_value` repo method + command + UI.
- **7.2 memory-edit slice COMPLETE + LIVE-VERIFIED** — `store::update_memory_value`
  / `memory_content` (Rust test) + `memory_edit` command + inline Edit affordance
  in `loadMemoryList` (swaps row → input + Save) + `memory.test.mjs`. Verified in
  the running app (`tools/ui_smoke/s3_memory_edit.py`): created a memory via a real
  "remember:" turn, edited it through the bridge, confirmed the change persisted
  (app-lib 286, UI 123). Remaining 7.2 memory: supersede (lineage; `superseded_by`
  column exists) + filter (client-side) + recall attribution (Phase 2 mission).
- **7.2 memory-filter slice COMPLETE** — client-side substring filter
  (`#memoryFilter` input toggles `.memory-row` visibility by `data-content`);
  `memory.test.mjs` covers it (UI 124). Pure client logic — JSDOM authoritative.
  Memory management surface now: **pin, edit, filter, delete** (all done);
  remaining 7.2: supersede (lineage) + recalled-memory attribution (Phase 2).
  NOTE: debug Tauri EMBEDS `../ui` at compile time — any UI change needs
  `cargo build -p finmodel-app` (after stopping the exe, Windows file lock) to
  appear live; JS-only changes are NOT hot-served. Running exe is fully current.
- **Live smoke suite in `tools/ui_smoke/` (4 scripts, all green over CDP):**
  `s1_boot_ipc.py` (boot + IPC + v5 migration + 1.5/7.2 render), `s2_live_turn.py`
  (plain Q&A turn), `s3_memory_edit.py` (create via "remember:" → edit → persist),
  `s4_tool_turn.py` (**tool-using analyst turn: get_quote runs, quote card renders,
  answer given** — the product's core purpose, verified live). Re-runnable with the
  app on :9222. Core desktop flows are now genuinely layer-5 verified.
- **4.3 WRITE-RISK GATE — real gap found live + FIXED + verified.** Live testing
  revealed `classify_write_risk` was defined+tested but NEVER called live, so
  write/overwrite/export tools auto-ran with no approval (build_model overwrote a
  file silently). Fix: refine risk at PROPOSAL time in `model_out_from_stream`
  (new `refine_write_risk` + `out_dir` param) so an overwrite/export sets
  `needs_approval` → reducer → ApprovalRequested → UI card → agent_approve →
  park/resolve. Extracted ONE shared resolver (`commands::model::model_filename`
  + `default_output_root`) used by both the build command and the refinement (no
  drift). Unit test `stream_build_model_overwrite_refines_to_approval` + live
  `s5_write_approval.py`: **re-building an existing MSFT model now shows the
  approval card; Deny ends the run and leaves the file byte-identical** (33591→
  33591). app-lib 287.
- **9.3 Desktop verification — LIVE CDP LOOP WORKING (layer 5 no longer skipped).**
  Baked `additionalBrowserArgs: "--remote-debugging-port=9222 --remote-allow-origins=*"`
  into `tauri.conf.json` window[0], `cargo build -p finmodel-app` (~90s incremental),
  launched `target/debug/finmodel-app.exe`, drove it via `tools/ui_smoke/`
  (`cdp_client.py` from the automated-testing skill + `s1_boot_ipc.py`,
  `s2_live_turn.py`). Verified LIVE against the running app: boot + shell; real IPC
  bridge (`load_settings`/`memory_list`/`skills_list`); **schema-v5 migration applied
  to the real DB** (memory_list `SELECT … pinned` succeeds); 1.5 role-picker + 7.2
  lists render; and a **full agent turn end-to-end** (agent_send → LiveDriver →
  gpt-4.1-mini → answer persisted+rendered), confirming this session's live-path
  edits (remaining()-guard removal, await_approval park-insert, sweep tick) do NOT
  regress the product. Remaining 9.3: full scenario matrix (7 golden missions,
  viewport widths, themes, a11y, crash/resume) + driving the not-yet-wired live
  items after they land.
  **SECURITY — RESOLVED: the `:9222` debug port was REVERTED from
  `tauri.conf.json` + rebuilt clean this session (config matches HEAD; no port in
  the shipping binary). To resume CDP verification next session: re-add
  `"additionalBrowserArgs": "--remote-debugging-port=9222 --remote-allow-origins=*"`
  to `app.windows[0]`, `cargo build -p finmodel-app` (stop the exe first — Windows
  file lock), launch the exe, then run `tools/ui_smoke/s1..s5`. REVERT again before
  any release build (9.4).**


**Repo split (2026-07-10):** The original Python lives in the separate `finmodel`
repo (github.com/Knightwarrior911/finmodel) and is PARKED — we do NOT touch it.
ALL work now happens here, in `finmodel-rust`
(github.com/Knightwarrior911/finmodel-rust), cloned locally at
`C:/Users/vinit/Documents/finmodel-rust`.


## LATEST SESSION (2026-07-15) — v0.4.0 sellable-feature expansion — SHIPPED / LIVE

Full detail in `CLAUDE.md` (top HANDOVER). Seven independent workstreams, all
opt/flag-gated so defaults are unchanged and every parity oracle stays green
(`finmodel-core` workspace tests + `src-tauri` tests pass):
- **A** live WACC inputs (10Y `^TNX` risk-free + 2y weekly regression beta vs `^GSPC`).
- **B** trading-comps tabs (`--peers` / chat `peers`) filling the gated Comps sheets.
- **C** one-click PPTX deck (`--deck` / chat always-on; model + benchmark).
- **D** `read_filing` — real 10-K/10-Q item text in chat.
- **E** scenario case from chat/CLI (`--case base|upside|downside`).
- **F** `analyze_pdf` — drop a local annual-report PDF, get a model (needs a key).
- **G** UI polish: copy-message, benchmark scroll + copy-table, sidebar filter +
  delete-confirm, keyboard shortcuts + legend, refreshed chips. Chat = 10 tools.
- **Shipped:** committed (`36203e2`) + tag `v0.4.0` pushed to `origin/master`; signed
  NSIS installer built + published to public `finmodel-releases` (tag `v0.4.0`,
  `finmodel_0.4.0_x64-setup.exe` + `latest.json`); updater endpoint verified serving
  0.4.0 and the installer URL returns 200. v0.3.x clients auto-update on next launch.
- **Sign gotcha:** build-time `TAURI_SIGNING_PRIVATE_KEY="$(cat …)"` mangled the key
  in the embedded shell — instead sign the built installer with
  `cargo tauri signer sign -f C:/Users/vinit/.tauri/finmodel.key -p "" <setup.exe>`.
- **Next:** rebrand the pdf-panda placeholder icons (`src-tauri/icons/`) before a wider
  public push; optionally thread peers/deck/case through the chat review→finalize path
  (currently applied on the skip_review build path).

## LATEST SESSION (2026-07-14) — Desktop app shipped + auto-update LIVE

Full detail in `CLAUDE.md` (top HANDOVER). Summary:
- **SEC filing-doc fetch** (research port item 2) — `fm-fetch::edgar`
  `recent_filings`/`search_filings` + `fm filings <ticker>` CLI. Done.
- **App UX redesign** (`ui/`) — self-explanatory two-tool workspace (Build a full
  model / Benchmark a peer set): ticker-format help, live-parsed echo, Live/Demo
  mode banner, "You get" tags, save-note. Fixes "I didn't know what to do / what
  format / what it can do". Verified in headless browser.
- **Signed auto-update — LIVE.** `tauri-plugin-updater` wired; always-visible
  FOOTER control (version + Check/Up-to-date/Update-available→install) like Snitch
  Voice; silent launch check + Settings "Check now". Shipped **v0.1.0 → v0.1.1**.
- **Release channel:** source repo is PRIVATE, so releases go to the PUBLIC
  `finmodel-releases` repo (updater fetches unauth). Minisign private key at
  `C:\Users\vinit\.tauri\finmodel.key` (never commit). Process: `RELEASE_CHECKLIST.md` §6.
- Installed on this box: `%LOCALAPPDATA%\finmodel\finmodel-app.exe`. Pushed to
  `origin/master` through `93386f5`.
- **Next:** rebrand pdf-panda icons; wire live `share_price` (fetch_quote) for real
  DCF upside; research port items 3–6 (news, PPTX, browser, agent).

## LATEST SESSION (2026-07-12) — Benchmark subsystem (filings → Excel)

Build/verify: `cd finmodel-core && CARGO_INCREMENTAL=0 cargo test --workspace`
(all green). Warnings gate: `RUSTFLAGS="-D warnings" cargo build -p fm-research
-p fm-excel -p fm-cli -p fm-extract`. Disk C: chronically tight (~4.5 GB) — clear
only `target/debug/incremental` between builds; keep `deps`. Run built exes via
`cargo run -q -p fm-cli -- …` and pass Windows-style `--out C:/tmp/x.xlsx`
(git-bash `/c/tmp/…` mangles to `C:\c\tmp`).

### Done this session — research port item 1 DONE (benchmark + EV bridge + IFRS bridge)
- **Research → Excel benchmarking** — ported `src/research/output_writer.py`
  (`pick_adhoc_layout` + `AdHocExcelWriter.write_research`) → `fm-excel::adhoc`
  on the shared cell-model/render engine. Cell-for-cell oracle-gated
  (value/formula/fill): `tieout/build_adhoc_oracle.py` →
  `tieout/excel_snapshots/ADHOC_bench_snapshot.json`,
  `fm-excel/tests/adhoc_parity.rs` = **0 diffs** + 8 decision-tree unit tests.
- **`fm-research` crate (new)** — `metrics_from_extraction` (pure, latest-FY
  scale/growth/profitability/returns/leverage), `build_benchmark_table`,
  `render_benchmark`, `benchmark_tickers` (live EDGAR). 6 unit tests.
- **`fm benchmark --tickers … [--out] [--title]`** — live-verified on
  AAPL/MSFT/GOOGL/AMZN/META (real FY2025 XBRL). Grouped headers, MEDIAN/MEAN/
  MIN/MAX block (formulas + cached results), currency column, per-cell EDGAR
  provenance notes (`Cell.comment` → xlsx notes in `render.rs`).
- **XBRL/metrics correctness**: added `short_term_debt` tag key (total debt =
  LT + current portion, so leverage isn't understated); gross profit falls back
  to revenue − COGS when GrossProfit is untagged.
- **EV-bridge worksheet** — ported `ResearchExcelWriter.write_ev_bridge` →
  `fm-excel::bridge::build_ev_bridge_sheet`; `fm ev-bridge --xlsx PATH
  [--ltm-revenue --ltm-ebitda]` renders it. Oracle-gated full + sparse
  (`ev_bridge_parity.rs`, 0 diffs) — sparse covers the dynamic row-skip / EV
  formula / multiples row-refs. Faithful bug-for-bug on the no-revenue EBITDA
  row-ref quirk (documented in `bridge.rs`).
- **Benchmark enriched to 16 metrics / 6 groups** — added Growth (revenue CAGR),
  Profitability (FCF margin), Liquidity (current ratio), Leverage (interest
  coverage) alongside the originals; all filings-derived + unit-tested. Live
  re-verified on AAPL/MSFT/JPM/WMT/XOM (XOM honestly failed: no us-gaap facts).
- **`fm verify` regression fixed** — it globbed the new `{sheets}`-only gate
  oracles and crashed ("missing periods"); now filters structurally
  (`model_output` present && not `*_full_*`). All CLI commands exercised:
  verify (5 snaps, 0 diffs), ifrs, build (offline SAND.ST), ev-bridge, benchmark.
- **IFRS-16 bridge worksheet** — ported `write_ifrs_bridge` →
  `fm-excel::bridge::build_ifrs_bridge_sheet` (plain `IfrsBridgeInput`, keeps
  fm-excel fm-ifrs-free); `fm ifrs --xlsx …` renders it. Oracle-gated full +
  simple (`ifrs_bridge_parity.rs`, 0 diffs) covering adjusted/computed EBITDA,
  EBITA present/absent, margins present/absent, both directions. Faithful
  bug-for-bug on the Pre-IFRS EBITA-margin row-ref quirk. `pdf_url` source-link
  path intentionally not ported (no PDF context in the CLI).
- **LTM basis** (`1fe063e`) — `fm benchmark --ltm`: trailing-twelve-months scale/
  margins/returns/leverage (growth stays annual), `fm-extract::ltm` (FY + YTD −
  prior-YTD; latest instant BS; freshest-tag + staleness guard). The standard IB
  comps basis. Live-verified AAPL LTM rev $451B. Also this session: sector column,
  tag-level provenance, capital-return metrics, CSV export, e2e benchmark gate.
- Commits: `6f2a097` benchmark · `5c967e8` EV bridge · `55e3c06` enriched+verify ·
  `bbf064f` IFRS bridge · `8538d73` CSV · `5aa65d2` sector · `12195bb` provenance ·
  `c7a10ef` app panel · `ed8f0bc` capital-return · `1fe063e` LTM · `3129b20`
  trading multiples · `343f1f7` global IFRS filers + data-anchored years ·
  `cf74a40` --usd FX normalization (global mixed-currency comps). Update `up to`.
## LATEST SESSION (2026-07-11) — Excel polish + IFRS + research start

All work committed (branch `master`, up to `34a3024`). Build with
`cd finmodel-core && RUSTFLAGS="-D warnings" cargo test -p fm-excel -p fm-build -p fm-value -p fm-ifrs`.
⚠️ Disk C: chronically tight (~2 GB now). Free `*/target/debug/{incremental,deps}`
between heavy builds; a full `cargo build -p fm-cli`/Tauri build is ~1–8 min and
has ENOSPC'd before. Prefer running built exes by ABSOLUTE path (the shell rejects
`./foo.exe`).

### Done this session
- **Excel formatting → 100% parity** with the Python writer. `render.rs` applies
  the `writer.py::_Fmt` system at render time (Arial 10; blue hardcoded inputs /
  green cross-tab `=X!` links / black same-tab formulas / navy-bold totals+titles /
  sand section headers / gray-italic drivers+memos; column widths; frozen panes;
  hidden gridlines; borders). `Cell` gained render-only fields (bold/italic/
  top_border/bottom_border/center/font_hex); IS/BS/CF/Cover/Assumptions/Sources
  builders tag subtotals, drivers, memos, checks, period headers.
  **Format oracle:** `tieout/diff_formats.py` (openpyxl) vs `tests/render_dump.rs`
  output → **1192/1192 cells** match bold/italic/color across all 6 sheets.
  Content gates (value/formula/fill) unaffected. Commits `5c88660`, `ccaec21`.
- **Formula caches**: `Cell.cached` + `Formula::set_result`; IS/BS/CF projected
  cells cache engine values so LibreOffice shows numbers offline (`bb4db02`,
  `tests/formula_cache.rs`).
- **App UI reskin**: warm light chrome + indigo accent (Snitch/PDF-Panda language),
  `ui/` (`a60eaf3`). App builds + launches (`src-tauri/target/debug/finmodel-app.exe`).
- **IFRS (DONE):** new `fm-ifrs` crate ports `kb/ifrs.py` (IFRS16↔US-GAAP EBIT/
  EBITDA/EBITA conversion, margins/deltas, `auto_convert`) + `us_gaap_leases.
  compute_ifrs_adjustments` (ASC 842 → ROU dep + lease interest, exact fallback
  order). Oracle-gated (6 tests). Reachable: `fm-cli ifrs …`. Commit `8451ce7`.
- **Research phase 1 (DONE):** `fm-value::ev_bridge` ports `kb/ev_bridge.py`
  (equity→EV checklist; goodwill never subtracted R-014; `compute_unfunded_pension`
  R-015). Oracle-gated (3 tests). Reachable: `fm-cli ev-bridge …`. Commit `34a3024`.

### NEXT — finish the research subsystem (`src/research/`, ~600 KB Python)
Port order (each: port calc → oracle-gate vs Python → reachable consumer):
1. ✅ **Research → Excel (DONE 2026-07-12)** — all three worksheets ported +
   oracle-gated + CLI-reachable: *Benchmarking* (`AdHocExcelWriter.write_research`
   → `fm-excel::adhoc` + `fm-research` + `fm benchmark`, `adhoc_parity.rs`),
   *EV bridge* (`write_ev_bridge` → `fm-excel::bridge` + `fm ev-bridge --xlsx`,
   `ev_bridge_parity.rs` full + sparse), *IFRS-16 bridge* (`write_ifrs_bridge`
   → `fm-excel::bridge` + `fm ifrs --xlsx`, `ifrs_bridge_parity.rs` full + simple).
   Remaining (separate follow-ups, NOT item 1): 🟢 non-US **IFRS filers on EDGAR
   now work** (TSM/SAP/NVO/SHEL/ASML via ifrs-full taxonomy, `343f1f7`). Only
   PURE foreign listings not on EDGAR at all need the PDF+LLM path
   (OPENROUTER_API_KEY). ✅ Tauri app peer-benchmark panel DONE
   (`benchmark_peers` command + UI card; binary compiled/linked/embedded &
   committed this session; GUI click-through untested — needs WebView2 CDP + a
   desktop session). ⚠️ `src-tauri/target` was DELETED to reclaim disk — the next
   app build is a COLD multi-GB build again (source is committed, was verified to
   compile). ⚠️ DISK VOLATILE: an external process swung C: free space from ~5 GB
   → ~170 MB → ~16 GB within minutes this session. Always `df -h /c` before a
   `cargo` command; a cold app rebuild needs ~4–5 GB. The `pdf_url`
   filing-source-link path of the bridges is a Python-only feature (no PDF ctx).
2. ✅ **SEC EDGAR client (DONE 2026-07-14)** — ported `get_recent_filings` /
   `search_filings` from `src/research/sec_edgar.py` → `fm-fetch::edgar`
   (`recent_filings` / `search_filings` / `Filing` / `DEFAULT_FORM_TYPES`):
   submissions history → filing records + direct primary-doc Archive URLs.
   Pure parse gated by unit tests (`parse_recent_filings_*`), live paths
   `#[ignore]`. Reachable via `fm filings <ticker> [--form] [--limit]`;
   live-verified on AAPL + TSM. (CIK/companyfacts/SIC already existed.)
   Remaining EDGAR follow-up: fetch/parse the actual filing document body
   (full-text 10-K/20-F sections) — only needed if the extraction pipeline
   should read filing prose beyond structured XBRL.
3. **Market data**: 🟢 quotes DONE — `fm-fetch::market::fetch_quote` (Yahoo, no
   key) powers `fm benchmark --multiples` (EV/EBITDA, EV/Rev, P/E). Still TODO:
   `news.py` headlines; FX rates for cross-currency comps (needs an FX feed).
4. **PPTX decks** (`pptx_writer.py` 144 KB + editor/render/inspector) — big; IB slides.
5. **Browser pipeline** (`browser_pipeline.py` 81 KB) — non-US annual-report extract.
6. **Agent/orchestrator** (`agent.py` 39 KB, `orchestrator.py`) — NL query → tools → Excel/deck.

### Also still open (pre-existing, non-blocking)
- ✅ **Auto-update WIRED (2026-07-14)** — `tauri-plugin-updater` initialized in
  `lib.rs` (desktop-only); `plugins.updater` pubkey + `releases/latest/download/
  latest.json` endpoint + `createUpdaterArtifacts:true` in `tauri.conf.json`;
  `updater:default` capability; backend `check_for_update`/`install_update`
  commands; frontend silent-startup "Restart & update" banner + Settings "Check
  now". Minisign keypair generated (private key at `C:\Users\vinit\.tauri\
  finmodel.key`, OUTSIDE the repo — never commit; add as CI secret
  `TAURI_SIGNING_PRIVATE_KEY`). Signed `cargo tauri build --bundles nsis`
  verified: emits `-setup.exe` + `.exe.sig`. Full release/`latest.json` process
  in `docs/RELEASE_CHECKLIST.md` §6. **Auto-update is LIVE:** v0.1.0 published to
  the PUBLIC `github.com/Knightwarrior911/finmodel-releases` (source repo is
  private → updater fetches unauthenticated, so releases go to a public channel,
  mirroring `pdf-panda-releases`); endpoint verified 200. Future releases just
  bump the version and re-run `RELEASE_CHECKLIST.md` §6. **Remaining:** rebrand
  the pdf-panda placeholder icons in `src-tauri/icons/`.
- **App market inputs** default (`risk_free=0.045`, `share_price=0.0`) — needs live feed.
- Valuation-tab per-role emphasis (DCF/WACC/Sens/Comps) not format-oracle-measured
  (they get the base render system; IS/BS/CF/Cover/Assumptions/Sources are 100%).

### Gates & regen workflow (read before Excel/valuation work)
- **Content gates (value/formula/fill):** `cargo test -p fm-excel` runs
  `snapshot_parity` (empty-IS, 5 cos), `full_is_parity` (IS/BS/CF std+sectors+XBRL),
  `valuation_parity` (Cover/DCF/WACC/Sensitivities/Comps Peers/Comps Summary — 0 diffs
  vs `tieout/excel_snapshots/SAND_ST_val_full_snapshot.json`), `adhoc_parity`
  (benchmark table vs `ADHOC_bench_snapshot.json`), `ev_bridge_parity` (full +
  sparse vs `EV_BRIDGE{,_SPARSE}_snapshot.json`), `ifrs_bridge_parity` (full +
  simple vs `IFRS_BRIDGE{,_SIMPLE}_snapshot.json`), `formats`, `roundtrip`,
  `formula_cache`. **Valuation + Comps + Benchmark + EV/IFRS-bridge tabs all gated.**
- **Oracles (Python-side, regen when the writer/inputs change):**
  `py tieout/build_full_is_oracle.py` → `*_full_snapshot.json` (+ sector/xbrl);
  `py tieout/build_val_oracle.py` → `SAND_ST_val_full_snapshot.json` (embeds
  WACCOutput/DCFOutput/PublicCompsOutput + writes `tests/snapshots/SAND_ST_val_full.xlsx`);
  `py tieout/build_adhoc_oracle.py` → `ADHOC_bench_snapshot.json`;
  `py tieout/build_ev_bridge_oracle.py` → `EV_BRIDGE{,_SPARSE}_snapshot.json`;
  `py tieout/build_ifrs_bridge_oracle.py` → `IFRS_BRIDGE{,_SIMPLE}_snapshot.json`.
- **Format parity (bold/italic/color) — 2-step, order matters:**
  1. `cargo test -p fm-excel --test render_dump` → writes `tests/snapshots/SAND_ST_rust.xlsx`
     (must re-run after ANY render.rs / sheet-builder change).
  2. `py tieout/build_full_is_oracle.py` (writes `SAND_ST_full.xlsx`), then
     `py tieout/diff_formats.py` → prints per-sheet % and exits non-zero if <100%.
  `tests/snapshots/*.xlsx` are git-ignored scratch — safe to delete/regenerate.
- Snapshot/content gates are blind to fonts/borders/widths/freeze — those live only
  in `render.rs` + the `Cell` emphasis fields, measured only by `diff_formats.py`.

## THE MISSION

Make the Rust Excel output match the Python output **100%**. Right now the Rust
app produces a bare data dump; the Python produces a rich, formula-driven,
investment-banker-grade workbook. Porting that is the top priority — it is the
product.

**Answer key:** `src/writer.py` — **196 KB** (thousands of lines) of openpyxl
logic: 6+ sheets (Cover, Assumptions, Income Statement, Balance Sheet, Cash Flow,
Sources…), live cross-sheet formulas (`=CHOOSE($D$9,…)`, `=IF(IS!F11<>0,…)`),
tier-colored cells (FILING/MARKET/DERIVED/ASSUMPTION/UNVERIFIED), and
`file:///…#page=N` hyperlinks back to the source filing. This is the target.

**Reference output to eyeball:** `models/*.xlsx` (old Python-generated rich models,
e.g. `models/KO_model.xlsx` 93 KB, `ATCO_full_model.xlsx` 89 KB). Open one to see
what "matches 100%" means.

## THE PARITY GATE (already have the ground truth)

`tieout/excel_snapshots/*.json` (5 companies: SAND_ST, ASML_AS, NOVO-B_CO,
NESN_SW, ATCO-B_ST) — Phase 0.5 **cell-level snapshots** of the Python workbook:
per sheet, an array of `{ row, cells: [{ ref, value, formula, fill }] }`. These
are the frozen "correct" cells to match.

⚠️ **Known blocker to fix first:** `finmodel-core/fm-excel/src/compare.rs`
`compare_sheets()` expects a `{ headers, rows:[{label,values}] }` shape — it
CANNOT read the snapshot's `{row, cells:[{ref,…}]}` format. Write a new comparator
that loads the real snapshot format and diffs it against the Rust-generated
workbook cell-by-cell (value + formula + fill). That comparator IS the R.5 gate.

## SUGGESTED APPROACH (port writer.py → Rust, gated)

1. Build a snapshot comparator matching the real `excel_snapshots` format.
2. Port `src/writer.py` sheet-by-sheet into `finmodel-core/fm-excel` using
   `rust_xlsxwriter` (already a dep): Cover → Assumptions → IS → BS → CF →
   valuation/DCF → Sources. After each sheet, diff against the snapshot; drive
   diffs to zero before moving on.
3. Reproduce EXACTLY: cell positions, formulas (as formula strings), number
   formats, fills/tier colors, hyperlinks. rust_xlsxwriter supports formulas,
   formats, colors, and hyperlinks.
4. Once sheets match, rewire the app (`src-tauri/src/commands/model.rs` +
   `finmodel-core/fm-build`) to use the rich writer instead of the current stub.

## CURRENT STATE OF THE RUST PORT (what's done vs stub)

- ✅ `fm-engine` — projection engine, cell-for-cell parity vs `src/engine.py` (CI-enforced)
- ✅ `fm-value` — WACC/DCF/comps + invariants
- ✅ `fm-extract` — XBRL parse, LLM prompts (verbatim), financial-section finder,
  native Rust PDF text extraction (pdf-extract, no Python), OpenRouter provider +
  live model list. `fetch_xbrl` returns Err for non-US (never fabricates).
- ✅ `fm-fetch` — EDGAR CIK/XBRL, PDF download, DDG annual-report discovery (live-validated on Sandvik)
- ✅ `fm-build` — shared reconcile+project+sheet-assembly (CLI and app both use it)
- ✅ `src-tauri` + `ui` — Tauri desktop app; compiles; ticker→build→Excel→Open, Settings (OpenRouter key + live model picker). Built exe ran (window opened).
- ✅ `fm-excel` writer — **DONE**. Full port of `writer.py` (Cover/Assumptions/IS/BS/CF/DCF/WACC/Sensitivities/Sources). Snapshot comparator cell-level gate: **0 diffs** empty-IS (`tests/snapshot_parity.rs`, 5 cos), full-IS (`tests/full_is_parity.rs`), valuation (`tests/valuation_parity.rs` vs `SAND_ST_val_full_snapshot.json`). App + CLI rewired via `fm_build` + `fm_excel::render`.
- ✅ Non-US live extraction wired into app `build_model`: EDGAR miss → `fm_extract::fetch_non_us_filing` (PDF discovery + LLM). Demo tickers map to real company names.

### Excel writer — known gaps (parity-complete; product follow-ups)
- ✅ **Number formats** added (`model.rs` FMT_* + `render.rs`; `tests/formats.rs`): drivers/rates render `0.0%`, monetary cells `#,##0`. Verified in `xl/styles.xml`. (Not in the snapshot gate — openpyxl doesn't capture number formats.)
- ✅ **IS body ported (standard sector).** `is_structure.rs` (`ISRow`/`build_standard_is`/`compute_is_row_map`) + full IS body in `sheets/is_stmt.rs` (revenue-growth-driven revenue, margin-driven COGS/GP, EBIT/EBITA/EBITDA buildup, interest→BS debt schedule, tax, EPS). Writer is **parameterized**: `WorkbookInput.is_structure` empty → header-only IS (committed-snapshot gate stays green); populated → full IS + BS/CF reference a **dynamic IS row-map** via `WorkbookInput::is_row()` (falls back to the empty-IS `IS_R` positions). App path (`fm_build`) now emits a full IS.
  - **Oracle + gate:** `tieout/build_full_is_oracle.py` runs the reference `src/` with a built `is_structure`, sourcing historicals from the committed snapshots' `model_output`, and commits `*_full_snapshot.json`. `tests/full_is_parity.rs` diffs the Rust IS/BS/CF against the oracle — **0 diffs across 4 companies** (SAND/ASML/ATCO/NOVO). NESN excluded: null `gross_profit` in its reconstructed historicals crashes the Python reference `_derive_assumptions` (oracle-gen only; Rust is unaffected).
  - **Sector coverage (done):** `build_is_structure(sector,…)` dispatches to `build_{standard,utility,bank,insurance,reit}_is`; `is_stmt.rs` handles the `utility_*` slot formulas; `assumptions.rs` relabels drivers for non-standard sectors. Gated by sector oracles (force each sector on SAND).
  - **XBRL detail (done):** `build_standard_is_detailed` handles revenue segments (`rev_seg_*`), detailed COGS (`cogs_seg_*`), and opex line items (`opex_*`, incl. extra items held-flat + subtracted into EBIT); `is_stmt.rs` emits the segment/sum formulas and the "REVENUE BREAKDOWN BY SEGMENT" memo block (`seg_*`); `apply_filing_labels` overrides labels from `notes.filing_labels`. `fm_build` parses `extraction.notes` (segments/opex/cogs_detail/filing_labels), replicates cli.py's cogs/rd/sga remap, and selects the detailed structure. Gated by a synthetic oracle `SAND_ST_xbrl_full_snapshot.json` (2 segments + cogs_detail + rd/sga + extra opex) — `tests/full_is_parity.rs::xbrl_detail_reproduces_oracle` = **0 diffs on IS/BS/CF**; `fm_build` wiring covered by `test_build_detailed_is_from_notes`. The **entire `is_builder.py` IS logic is now ported**; NESN's standard oracle remains excluded (null `gross_profit` crashes the Python reference during oracle-gen; Rust engine unaffected — its empty-IS committed snapshot still gates green).
- ✅ **Valuation tabs** (DCF/WACC/Sensitivities + Cover valuation summary). `fm-value` full `compute_wacc`/`compute_dcf`; `fm-build` always emits valuation tabs (offline fallback beta=1.0). Oracle: `py tieout/build_val_oracle.py` → `SAND_ST_val_full_snapshot.json`.
- ✅ **Comps Peers / Comps Summary** — ported; gated via synthetic `PublicCompsOutput` in `SAND_ST_val_full_snapshot.json` (valuation_parity 0 diffs). Emitted when `WorkbookInput.public_comps` is Some; app path still `None` until a peer feed is wired.
- ✅ **Formula cached results** — `Cell.cached` + `Formula::set_result` in render; DCF/WACC/Sens/Comps cross-links carry engine values. Gated by `tests/formula_cache.rs`.
- 🟡 **App market inputs are placeholders** — `risk_free_rate=0.045`, `current_share_price=0.0`, `company=ticker`, `fye="Dec"` (no live market feed). Valuation still computes; price/upside stay zero until a feed is wired.

## HOW TO VERIFY / BUILD

- Engine tests: `cd finmodel-core && RUSTFLAGS="-D warnings" cargo test --workspace`
  (must stay green; ~19 suites).
- App backend compiles: `cd src-tauri && cargo check`.
- Run the app: `cd src-tauri && cargo tauri dev` (cargo-tauri 2.11 installed).
- ⚠️ **Disk:** C: is chronically tight. A full Tauri build needs several GB.
  Free `finmodel-core/target/debug` and `src-tauri/target` between heavy builds.
  Release exe (~11 MB) currently at the old path; rebuild with `cargo build --release`.
- Icons in `src-tauri/icons/` are PLACEHOLDERS from pdf-panda — rebrand before shipping.
- Reference app to mirror for patterns: `C:/Users/vinit/pdf-panda-tauri` (shipped Tauri app).

## AFTER THE EXCEL PORT (roadmap to sellable)
Wire non-US live extraction into the app → licensing/activation (reuse Snitch) →
installer (`cargo tauri build`) → rebrand icons → stranger test.
