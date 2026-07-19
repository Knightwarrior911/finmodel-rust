# Finmodel — Financial Model Engine

## HANDOVER — v0.9.11 SHIPPED + LIVE (2026-07-19) — agentic doctrine: work until done
**Tagged `v0.9.11` on master; released to public finmodel-releases (Latest =
v0.9.11).** CI green; signed NSIS published; endpoint verified (latest.json
serves 0.9.11, sig 420 chars/no newline, installer 200, digest match).

The product doctrine changed: budgets are RUNAWAY GUARDS, not work quotas.
- fm-agent budget.rs: INTERACTIVE 200 rounds / 4M tokens / 1h / 8 children;
  WORKFLOW 1000 rounds / 20M tokens / 8h / 32 children. (A round is charged on
  BOTH ModelResponded and ToolsCompleted — old "10 rounds" was ~5 tool cycles;
  that starved a simple two-topic filing question on 2026-07-19.)
- Grace pass is real now: budget_stop → Action::Synthesize; the actor calls the
  new Driver::wrap_up() (default no-op) when machine.in_budget_grace(); the
  LiveDriver impl makes ONE no-tools streamed model call over gathered evidence
  and sets last_content before synthesize() persists. Previously synthesize()
  persisted stale prose — a run stopped right after tool results ended with
  "ask me to continue" and no answer.
- select_workflow: targeted lookups ("say anything", "mention", "discuss",
  "comment on"…) return None and stay interactive instead of escalating to a
  five-deliverable workflow.
- v0.9.10 postscript: its published latest.json carried a trailing newline in
  the signature → client "Invalid symbol 10, offset 420" at install. Fixed
  in-place (asset replaced; GitHub download CDN needed a byte-size change to
  bust). Checklist now documents the pitfall.
Gates: fm-agent 50 · app-lib 297 · UI 154. Release ritual: docs/RELEASE_CHECKLIST.md.

## HANDOVER — v0.9.10 SHIPPED + LIVE (2026-07-19) — warm-colleague UI polish + Grok-like cites
**Tagged `v0.9.10` on master (finmodel-rust); released to public finmodel-releases
(Latest = v0.9.10).** CI green. Signed NSIS published; updater endpoint verified
(latest.json serves 0.9.10, installer 200).
UI-only cycle on top of v0.9.9: tool activity story, mission/progress folded into the
thinking trail, shared approval vocabulary, indigo one-voice activity chrome, CSS-only
motion, numbered inline cites + Sources strip (letter avatars; no favicon network).
UI tests: 154 jsdom. Release ritual: docs/RELEASE_CHECKLIST.md.

## HANDOVER — v0.9.2–9.9 SHIPPED (2026-07-19) — skills system live, analyst spread, visual redesign
**Tags v0.9.2 through v0.9.9 on master (finmodel-rust); all released to public
finmodel-releases (Latest = v0.9.9).** CI green for every tag. Signed NSIS builds
published + endpoints verified (latest.json serves 0.9.9, installer 200).

This was an 8-release cycle spanning four themes: (1) skills library, (2) settings &
design context, (3) analyst-tool accuracy & coverage, (4) UI professionalism.

### Shipped this cycle

**13 built-in skills (v0.9.2).** 8 IB/analysis skills (dcf-valuation,
comparable-companies, precedent-transactions, earnings-analysis,
ma-accretion-dilution, lbo-screen, company-profile, credit-analysis)
+ 5 workflow skills (planner, orchestrator, task-executor, reviewer,
verification-loop). Bundled in the binary, seeded once to <config>/skills/,
never overwrites user files. **Lifecycle bug fix:** use_skill now records usage
(count + last_used) so actively-used skills don't age out to stale/archived.

**Settings sectioned + skill editor (v0.9.3).** Skills are first-class: inline
SKILL.md editor (view, edit, rename; rename moves the file), use counts + lifecycle
badges. Settings split into General / Connections / Memory / Skills tabs with
roving keyboard nav (dock vocabulary reused). Dialog widened from 520 to 780px.
PRODUCT.md + DESIGN.md + .impeccable/design.json committed (register=product,
north star "The Patient Analyst", codified rules and anti-references).

**Budget grace + human terminal messages + shares outstanding (v0.9.4).**
Rounds/tokens exhaustion now earns one final no-tools synthesis pass (grace)
instead of dying mid-task; deadline still hard-stops. Raw JSON payload no longer
leaks into chat — every terminal renders a human sentence. get_financials adds
shares outstanding from the 10-K cover page (dei taxonomy) and weighted-average
diluted shares as separate rows.

**Multi-year analyst spread (v0.9.5).** get_financials returns a full annual
spread (default 3 FYs, up to 6): income statement, balance sheet (cash, total
assets, LT debt, equity), cash flow (CFO, capex), diluted EPS, cover-page shares,
weighted-average diluted shares. Revenue growth YoY, margins, FCF, and net cash
computed deterministically in Rust — model never does the arithmetic. Multi-year
table card in chat. Discontinued XBRL tags no longer shadow current ones
(most-recent-data tag wins).Tested live against real TSLA EDGAR data.

**Quarterly/LTM bases + credit metrics + segment routing + 10-round budget
(v0.9.6).** get_financials gains basis: quarterly (last 8 fiscal quarters, Q4
derived as FY − Q1..Q3, marked with *), ltm (trailing 12 months via
fm_extract::ltm, the real comps basis). Interest expense, D&A, and short-term debt
join the annual spread; EBITDA, total debt, leverage, interest coverage, and net cash
are pre-computed. Segment questions route to 10-K item 8 segment note (not available
as structured XBRL). Interactive budget raised 8 to 10 rounds.

**Visual de-cartooning (v0.9.7).** Empty state: 38px centered hero to 21px
left-aligned workbench opener. Capsule pills to quiet 6px rectangles. New chat
button: saturated indigo to soft tint filling on hover. Composer: 16px radius to
10px, whisper shadow. User bubbles + cards: shadows removed (Overlay-Only Rule),
corners tightened. Memory pin: emoji to vector SVG. All remaining 999px capsules
flattened to radius-sm.

**Blocked-source fallback (v0.9.8).** Bot-protected websites (tesla.com, Akamai
403) no longer dead-end the analyst — the tool result carries the fallback
playbook (research synthesis, SEC proxy/10-K executive-officer sections, news).
System prompt doctrine: blocked source routes to next-best immediately instead of
asking permission.

**Thinking trail redesign (v0.9.9).** Boxed grey Thinking process panel becomes a
quiet timeline ledger: hairline rail with state nodes (indigo running, green done,
red failed), measured per-step durations in mono stamped on completion, breathing
accent dot instead of spinner, 220ms step entrance, reduced-motion honored. State
signaled once on the rail instead of three times per row.

### NOT done — remaining
- **Quarterly data coverage** is read-only (get_financials basis=quarterly);
  segment-structure XBRL instance parsing (dimensional facts) is a future feature.
- **Interest expense gap**: some issuers (TSLA post-FY2023) stop tagging it; the
  row-level label honestly reports what is reported.
- **LTM/quarterly card renderer** uses the same single-column table card as annual
  (the period-axis columns work, but no tabbed toggle between bases).
- **Full tool docs** (parameters for every tool) are still model-visible schema
  only, not a user-facing reference.

### Build & verify
- All gates: pwsh -File scripts/gates.ps1
- Backend only: cd src-tauri && cargo test --lib (297 lib tests)
- Live EDGAR tests: cargo test --lib -- --ignored (3 live network tests)
- FM-agent: cargo test -p fm-agent (49 tests)
- FM-extract: cargo test -p fm-extract (66 tests)
- UI only: cd ui && node --test (143 jsdom tests)
- Release ritual: docs/RELEASE_CHECKLIST.md

## HANDOVER — v0.9.1 SHIPPED + LIVE (2026-07-19) — Phase 2 mission shell: the Evidence dock
**Tagged `v0.9.1` on `master` (`finmodel-rust`); released to public `finmodel-releases`
(flagged Latest).** CI run `29658614129` green; signed NSIS built + published; updater
endpoint VERIFIED serving `0.9.1` (installer HTTP 200). Real Tauri webview smoked over
WebView2 CDP: boot · dock · keyboard (Ctrl+1/5, Esc) · **real-backend `ev_bridge`** ·
focus return · 0 horizontal scroll. Gates green (`scripts/gates.ps1`): **core workspace ·
src-tauri lib · UI 137 · research-eval 13**. Only UI files changed; no Rust touched.
Debug port never added (release config clean, 0 refs).

This cycle finished **Task 2.2 (mission shell)** and the **Task 2.4 dock-open
responsive generalization**, and captured the dock/shell acceptance views live.

### Shipped this cycle (see CHANGELOG "Unreleased")
- **Evidence dock (Task 2.2, steps 5–8).** The right reader `<aside>` became a
  tabbed `#evidenceDock` (**Model / Valuation / Sources / Artifacts / Reader**).
  New `ui/js/workbench.mjs` is the sole authority for dock open/close/toggle,
  `body.dock-open`, focus return to the invoker, the roving tablist, and the
  keyboard map: **Ctrl/⌘+1–5** dock tabs, **Ctrl/⌘+J** toggle, **←/→/Home/End**
  tab nav, **↑/↓/Home/End** plan-step nav, **Esc** closes the dock only when focus
  is inside it and no run is active (an active run keeps Esc = Stop in `main.mjs`).
  Preserved Ctrl/⌘+N / Ctrl/⌘+K / Ctrl/⌘+Enter. Settings shortcut legend updated.
- **Model tools migrated + modal deleted.** EV/IFRS/tie-out forms moved verbatim
  from `#analystModal` into the dock's Model tab; `#analystModal` removed. `analyst.mjs`
  `openAnalyst()` now just `openDock("model")`; `reader.mjs` `openReader/closeReader`
  delegate to the dock. `cards.mjs` triggers unchanged. Live-verified over HTTP
  (headless Chromium): EV form filled → `ev_bridge` → rendered "Enterprise value ·
  1,100M" (parity with the old modal); Esc returned focus to `#newChatBtn`.
- **Live regions de-duplicated.** `#missionHeader` is no longer a live region
  (visual pill only); `#chatProgress` (polite) stays the single tool-status region,
  `#chatAlert` (assertive) stays approvals/errors.
- **2.4 dock responsive.** `body.dock-open` generalizes `reader-open`: third grid
  track ≥1025px, right overlay 861–1024px, bottom sheet 601–860px, full-screen
  drawer ≤600px. Verified live at 1440/1000/820/620 × light/dark: **0 horizontal
  scroll, composer always in view**; reduced-motion honored (dock transition ≈0s);
  dock-chrome tokens all resolve per-theme (no undefined aliases).

### NOT done — next session
- **2.3 populate the dock tabs.** Valuation / Sources / Artifacts are shell empty
  states; wiring them from `agent_event` (valuation strip, dedup source ledger with
  `[n]`→Reader locator, artifacts newest-first) is Task 2.3.
- **9.3 full desktop matrix (LIVE).** The dock/shell acceptance views were captured
  headlessly over HTTP (ES modules don't load over `file://` — a static server is
  needed: `node tools/ui_smoke/serve.mjs ui 8917`). The stream-state views
  (planning/execution/fan-out/approval/reload/recovery) and the 6 golden-mission
  legs still need the running Tauri app driven over CDP — build + relaunch with the
  temporary `--remote-debugging-port=9222` window arg, run `tools/ui_smoke/s1..s7`,
  then revert the port. Needs a configured provider key + port 9222 free (PDF Panda).

## HANDOVER — v0.9.0 (releasing, 2026-07-18) — the agentic runtime is now the LIVE path
**Working tree at `v0.9.0`** (`src-tauri/Cargo.toml` + `tauri.conf.json` in lockstep);
last shipped tag was `v0.8.6`. Gates green: **app-lib 294 · UI 130 · fm-agent/fm-value
workspace · research-eval 13** (`scripts/gates.ps1`). Debug port reverted (0 refs).

This cycle promoted the reducer-driven `fm-agent` runtime + `src-tauri/src/agent/`
driver/actor from built-but-dormant to the app's **live** path. Full per-task detail:
`docs/NEXT-SESSION.md` (top block) and the plan `local://agentic-financial-analyst-plan.md`.
(The v0.8.6 note below says "agentic-analyst goal COMPLETE" — that refers to the initial
user-feature goal; the deeper 27-task agentic PLAN is the current, still-in-progress work.)

### Shipped this cycle (see CHANGELOG v0.9.0 for user-facing copy)
- **Single `agent_event` channel (Task 2.1).** All live rendering — text deltas, tool
  status/thinking, result cards, plan, phase, approvals, memory, terminal — flows on one
  durable/ephemeral `agent_event` stream. Legacy `chat_delta`/`chat_tool` listeners removed.
  Cards ride a durable `ResultPartAdded`; deltas ride ephemeral `assistant_text_delta`;
  thinking steps ride `ToolStarted/Succeeded/Failed` keyed by `tool_call_id`. Verified live s1–s7.
  (Reload-via-snapshot+gap-close is NOT built: reload uses `load_conversation` rebuild —
  correct for completed chats, no in-flight-run resume across reload.)
- **Pause / Resume controls (Task 2.2 — controls slice).** Composer Pause button →
  `agent_pause` (→ `RunInterrupted`, resumable; distinct from Stop's terminal `RunCancelled`);
  an interrupted terminal offers **Resume** → `agent_resume` (relaunch from last complete
  boundary via `resumeRun`). Backend already built+tested (`registry.pause`, `actor::resume_run`
  → `launch_run`; `control_interrupt_yields_interrupted_resumable`). UI test in `ui/tests/chat.test.mjs`.
- **Move-to-project fix.** Picker preselects the chat's current project; shows a "No projects
  yet" hint instead of a dead-end menu; blur restores the row. `ui/tests/sidebar.test.mjs`.

### NOT done — next session (precisely scoped in `docs/NEXT-SESSION.md` → "Genuinely remaining")
- **2.2 mission shell (bulk):** tabbed **Evidence dock** (Model/Valuation/Sources/Artifacts/
  Reader), new `ui/js/workbench.mjs` (owns `body.dock-open`, focus return, keyboard map
  ⌘1–5 / ⌘J / arrow nav), migrate EV/IFRS/tie-out controllers out of `#analystModal` into the
  dock's Model tab, delete the modal. Large rewrite of a working UI — its own session.
- **2.4 visual finish:** dock-open responsive generalization + the 12 named acceptance-view captures.
- **9.3 full desktop matrix:** 7 golden missions × viewport × theme × a11y × crash/resume via CDP
  (`tools/ui_smoke/s1..s7` cover the core flows today).
- **Backend cores tested but not wired live:** 5.2/5.3 child fan-out, 6.2 progressive disclosure + MCP,
  8.2/8.3 scheduler tick + commitment extraction. **External:** 7.1 auto-memory (labelled dataset),
  9.4 signed installer (minisign key).

### Build & verify (Claude Code CLI)
- All gates: `pwsh -File scripts/gates.ps1` (app-lib + fm-agent/fm-value workspace + ui + research-eval).
- Backend only: `cd src-tauri && cargo test --lib`. UI only: `cd ui && node --test` (jsdom, no browser).
- Run app: `cd src-tauri && cargo tauri dev`. Live CDP smokes: `tools/ui_smoke/s1..s7` (need the
  `--remote-debugging-port=9222` window arg in `tauri.conf.json` temporarily; revert before release).
- Release ritual: `docs/RELEASE_CHECKLIST.md` (version lockstep, signed NSIS build, publish to the
  public `finmodel-releases` repo).

---

## HANDOVER — v0.8.6 skills system, LIVE (current, 2026-07-18)
**Branch `master`, tagged `v0.8.6` (pushed).** Live — endpoint VERIFIED serving
`0.8.6`, installer 200. Agentic-analyst goal COMPLETE; below are post-goal user features (all shipped).
### v0.8.6 — SKILL.md skills system + self-evolution
- `agent/skills.rs`: parse SKILL.md (frontmatter `name`+`description`+optional `parameters`, body);
  `list/save/get/delete` in `<config>/skills/<name>.md`; `catalog_block` (names+desc only);
  `is_valid_name` (traversal-safe). 6 unit tests.
- Discovery = progressive disclosure: `apply_grounding` appends the catalog (names+desc) to the system
  prompt; a `use_skill(name)` tool (registry spec + `agent_tool_schemas` + `run_tool` dispatch →
  `tool_use_skill` returns the body as the tool summary, null display) loads full steps on demand.
  Registry count 12→13; count tests updated.
- Commands `skills_{list,get,save,delete}` + Skills manager in the Settings modal (`settings.mjs`
  `loadSkillsList` + `skillSaveBtn`; `index.html`).
- Self-evolution: `skill_suggest(transcript)` → model abstracts a solved turn into a generalized
  SKILL.md draft via `settings::complete_once` (honors `base_url` — NOT the OpenRouter-hardcoded
  `fm_extract::llm_complete_with`). "Save as skill" button (`chat.mjs`, after ≥2-tool turns; tracks
  `activeTurn.toolSeq`) → `openSettingsWithSkillDraft` prefills the editor.
- `core.mjs` `call()` now tolerates raw-string returns (parse, else raw) — also fixes project-grounding
  pre-fill in the modal.
- VERIFIED live: `define-terms` → "Define EBITDA" made the agent call `use_skill` + follow it
  (`[SKILL-USED]`); `skill_suggest` produced a generalized `compare-company-revenue-net-income` draft;
  "Save as skill" appeared after a 3-tool turn. 223 lib + 116 UI green.


### v0.8.5 — design polish (product-register craft pass)
- CSS + icons only (no logic change). Commanding hero type (`clamp` display, tight tracking);
  replaced ad-hoc emoji (folders/gear/move in `sidebar.mjs`; per-tool thinking icons + fan-out in
  `chat.mjs` `thinkIcon`; dashboard title in `projects.mjs`) with mono line-SVGs matching the set;
  softer `--canvas` (#fdfdfc); elevated composer + refined chips; shared `--ease` tokens. Verified
  live light+dark. Guided by frontend-design + impeccable skills. NOTE: user first asked for Three.js
  — declined as the wrong tool for polish (WebGL doesn't fix type/spacing/hierarchy; against IB aesthetic).

### v0.8.4 — project folders
- Schema **v2** (`store/migrations.rs` `apply_v2`): `projects` table + nullable
  `conversations.project_id` (no FK). Store CRUD + commands `projects_{list,create,rename,delete}` +
  `conversation_set_project` (`commands/agent.rs`); `list_conversations` returns `project_id`.
  `agent_send` gained `project_id`, assigned only when it actually CREATES the row
  (`create_conversation(...).is_ok()`) — the client pre-allocates the id, so the param-absent check
  is unreliable.
- Project grounding repathed to `<config_dir>/projects/<project_id>/finmodel.md`; `grounding.rs`
  (`read_project`/`project_file`/`is_valid_id`) + `apply_grounding` key on the conversation's
  `project_id` (looked up from the store). VERIFIED live: `[TSLA-PROJ]` grounding applied in-project,
  absent in loose chats.
- UI: sidebar collapsible folders + New Project + move-select (`sidebar.mjs`); project settings modal
  (name + grounding + delete, `projects.mjs` + `index.html`); center dashboard ("+ New chat in
  project" → `setPendingProjectId` → grounded first turn).
- **Caveats:** (1) project file-attachments deferred ("coming soon" in the modal); (2)
  `project_delete` orphans `projects/<id>/finmodel.md` on disk (DB row + `project_id` cleared, file
  left); (3) **SKILL.md skills system + self-evolution is the last unstarted user ask** — RE the
  format from the openclaw/hermes GitHub URLs (see `docs/AGENTIC_ANALYST_GOAL.md`).

### v0.8.3 — grounding layers + real-time thinking trace (post-goal user requests)
- **Two-layer grounding** (`agent/grounding.rs`): `read_global` (`<config_dir>/config.json`,
  `instructions` string|array) + `read_project` (`<config_dir>/workspaces/<ws>/finmodel.md`,
  fallback `claude.md`) → `chain(base → global → project)`. Wired via `LiveDriver::apply_grounding`
  (called after `prepare` sets `self.messages`; rewrites `messages[0]` system content). Commands
  `grounding_{get,set}_{global,project}` in `commands/agent.rs`. Workspace ids validated
  (`is_valid_workspace_id`, `[A-Za-z0-9_-]`) against path traversal. VERIFIED live: a global rule
  made the model prefix its reply exactly. 8 grounding unit tests.
- **Thinking Process panel** (`chat.mjs`): per-turn collapsible trace — each tool `start` adds a
  step (icon + `phaseLabel` + "In progress"), `done`/`error` flips to ✓ Success / ✗ Failed; result
  cards render below; fan-out is a note step; auto-collapses on finalize (CSS in `style.css`).
  Replaced the old inline `toolStatusNode` rows. VERIFIED live.
- **Memory eval** (`agent/memory.rs`): `is_durable_preference` classifier + 181-turn labelled
  dev/held-out set (`auto_capture_eval`). Keyword-rules measured **P=0.87 / R=0.84 on held-out —
  BELOW the 98/90 gate**, so auto-capture stays OFF (needs a model classifier on real data). The
  classifier feeds `MemoryCapture` only; `LiveDriver` production path unchanged.
- **All requested user features are shipped** through v0.8.6: thinking trace, grounding layers, project
  folders, design polish, and the SKILL.md skills system + self-evolution. No outstanding asks.

### v0.7.2 — any OpenAI-compatible provider + full income statement
- `Settings.base_url` (default OpenRouter). `chat_completions_url`/`provider_base`/
  `is_openrouter` helpers in `settings.rs`. Chat stream (`openrouter_stream_async`
  reads `chat_completions_url(&read_settings(app))`), probes, and `list_models` all
  follow the provider; `test_model` branches OpenRouter-catalog vs direct probe;
  strict-json probe drops OpenRouter-only `provider.require_parameters` for others.
  UI: Provider dropdown + base-URL field in Settings (`settings.mjs` PROVIDERS).
  Own-key only — NO subscription OAuth (ToS/ban risk for a sold product; user chose this).
- `get_financials` widened to the full income statement (explicit us-gaap tag list):
  revenue, cost of revenue, gross profit, operating income, net income, diluted EPS.

### v0.8.0 — agentic experience (goal milestones M1–M5)
- **M1 progress** ✓ live: `phase_changed` → progress labels (`agentPhaseLabel`);
  friendly per-tool labels (`phaseLabel`: "Fetching financials…"). Literal "Plan:"
  line is prompt-nudged but MODEL-DEPENDENT — gpt-4.1-mini stays concise and skips
  it; the progress stream is the visible plan.
- **M2 follow-through** ✓ live: system-prompt mandates end-to-end multi-step; verified
  compound queries (Apple vs Microsoft, Tesla vs Ford) run all tools + answer, no punt.
- **M3 tool cards** ✓ friendly labels + result cards.
- **M4 fan-out** ✓ DONE + live: parallel-wave calls become real `SubagentPool` children
  (Phase F); `schedule_tools` emits `agent_subagent` lifecycle events plus a `fanout`/`fanout_done`
  banner. UI: inline "⚡ N ran in parallel" banner (`chat.mjs`) AND live task-tray rows
  (`main.mjs` feeds `agent_subagent` → `tasks.mjs` `SubagentUpdate` reducer → `#taskTray`).
  VERIFIED live (v0.8.2): a 3-company revenue+net-income compare spawned `get_financials · AAPL/
  MSFT/GOOGL` subagent rows in the tray, resolving as each finished. +2 UI tests (116 total).
- **M5 memory** PARTIAL: memory drawer ✓ live (`memory_list`/`memory_delete` in
  `commands/agent.rs`; Settings "Saved memories" list + delete; verified UI+DB). Automatic
  (unattended) capture STILL OFF — gated on the ≥200-turn labelled precision dataset
  (plan decision 4); a data task, not a code task.

### Goal status (`docs/AGENTIC_ANALYST_GOAL.md`) — COMPLETE
Goal COMPLETED: all 8 DoD points + M1–M4 verified live; M5 memory drawer done + verified. M5
automatic capture was later MEASURED in v0.8.3 (`auto_capture_eval`): keyword-rules P=0.87/R=0.84,
below the 98/90 gate → stays off (needs a model classifier on representative data). Gates: 217 lib
 + 116 UI + 47 fm-fetch green. Signing/publish recipe unchanged (see v0.6.0 below).

## HANDOVER — v0.7.1 get_financials, LIVE RELEASE (superseded by v0.8.0)
**Branch `master`, tagged `v0.7.1` (pushed).** Live release — endpoint VERIFIED
serving `0.7.1`, installer 200. Fixes the recurring "what were Tesla's 2025
sales" failure where research read risk factors and said the figure was
"undisclosed," then punted to build_model.

New tool **`get_financials`** (`tool_get_financials` in `commands/chat.rs`;
`ToolName::GetFinancials`; registry spec + schema + `agent_tool_schemas`): pulls
exact annual figures from EDGAR XBRL. Uses `fm_fetch::edgar::fetch_companyfacts_raw`
(the typed `CompanyFacts::FactValue.fy` is `Option<String>` but SEC sends `fy` as
a NUMBER → typed decode fails; the raw `Value` path is the working one build_model
uses too). Picks the first `xbrl_tag_map` candidate tag with an annual value
(`fp=="FY"`, `form` contains "10-K"), by requested fiscal year else latest,
latest `filed` wins. Reports revenue/gross profit/operating income/net income/
diluted EPS (Tesla only surfaced revenue+net income — GrossProfit/OperatingIncomeLoss/
EPS tags not in its facts under those keys; widen tags later if needed).
`SYSTEM_PROMPT` now routes reported-figure queries here and says answer the number
directly, don't punt. VERIFIED LIVE in-app: "What were Tesla's sales for 2025?"
→ "$94.83 billion, per its annual report filed with the SEC." Ignored live test
`get_financials_tsla_fy2025_revenue_live`. 208 lib green.

## HANDOVER — v0.7.0 memory + tool/UX upgrades, LIVE RELEASE (superseded by v0.7.1)
**Branch `master`, tagged `v0.7.0` (pushed).** v0.7.0 is the live release —
updater endpoint VERIFIED serving `0.7.0`, installer URL 200. All five changes
were LIVE-verified in the running app over CDP (memory round-trip, Tesla routing,
scroll, UI). Same signing/publish recipe as v0.6.0.

### What shipped (all live-verified)
- **Memory is a real feature (manual save + recall).** `LiveDriver::extract_memory`
  captures explicit `remember:`/`note:`/`save to memory:` directives
  (`parse_memory_directive` in `agent/driver.rs`; PrecisionGate-guarded;
  questions rejected), workspace-scoped, → `MemoryUpdated{count}` → inline
  "Memory saved · N" pill (`renderMemorySaved` in `chat.mjs`). `LiveDriver::prepare`
  recalls via `fts_query` (stopword-filtered, FTS5-safe) → `SqliteMemoryRepository::search`
  (workspace scope only — the store AND-joins scope, so ws+conv together excludes
  ws-only rows) → injects a "Recalled context" system message + `record_use`.
  VERIFIED: saved "prefer revenue in USD millions", a later revenue question
  answered in USD millions unprompted; `memory_uses` row written. **Automatic
  (unattended) capture still OFF** — this is explicit manual save + recall.
  Management drawer/commands (list/delete/toggle) NOT built yet (deferred).
- **Parallel tool execution.** `executors::execute_batch` now runs a wave's
  independent calls concurrently via `std::thread::scope`, capped at
  `PER_RUN_SLOTS` (4); `B: ToolBackend + Sync`. Result order preserved (caller
  walks calls in order). Token efficiency was already good (loop pushes
  `env.summary`, not raw data).
- **Sharper routing.** `read_filing` schema + `SYSTEM_PROMPT` steer specific
  reported-figure queries (revenue/sales/EPS/margins) to research/build_model,
  not narrative items. VERIFIED: "tesla sales of 2025" now runs research + builds
  a real TSLA model (was the item-7/8 dead-end).
- **Live auto-scroll.** `chat.mjs` stick-to-bottom flag driven by user scroll
  (was: `scrollToBottom` re-checked `nearBottom()` post-growth and disengaged
  after the first big chunk).
- **UI polish** (`ui/style.css`): composer focus ring (`accent-soft`), bordered
  user bubble + shadow, line-height 1.65, memory pill. Restrained — kept the
  editorial-finance aesthetic.

### Reference study (concepts only; no upstream code)
Oh My Pi (`can1357/oh-my-pi`, MIT) + Grok Build (`xai-org/grok-build`, Apache-2.0)
+ opencode (`anomalyco/opencode`) informed parallel execution, compact summaries,
durable events, tool-registry patterns. open-design (`nexu-io/open-design`) was
design inspiration; deliberately NOT copied its colorful design-tool look (wrong
for an IB tool). All patterns reimplemented in Rust/vanilla-JS.

### Gates: 208 lib + 114 UI + 47 fm-fetch green.

## HANDOVER — v0.6.1 fix: 10-K filing reads, LIVE RELEASE (superseded by v0.7.0)
**Branch `master`, tagged `v0.6.1` (pushed).** v0.6.1 is the live release —
updater endpoint VERIFIED serving `0.6.1`, installer URL returns 200. **Fix:**
`read_filing` returned "Item 7/8 not available / not yet filed" for real 10-Ks
because `fetch_filing_doc` reused the web-article extractor
(`websearch::strip_html` — `<h*>/<p>/<li>` only, 20 KB cap), so div/span/table
item bodies were dropped and Item 7/8 (megabytes into the doc) never appeared.
Added `fm-fetch::edgar::strip_filing_html`: full-DOM walk (all elements incl.
tables), newline at every block boundary so headings are line-anchored for
`split_filing_items`, no size cap. `strip_html` still serves web_search/read_page
(unchanged). Live-verified: TSLA 10-K (2026-01-29) now yields items 1–16 incl 7
& 8 (428K chars). Gates: fm-fetch 47 + app lib 205 green. Same signing/publish
recipe as v0.6.0 below.

## HANDOVER — v0.6.0 agentic analyst engine, LIVE RELEASE (superseded by v0.6.1, 2026-07-17)
**Branch `master`, tagged `v0.6.0` (pushed to origin).** Source `finmodel-rust`
PRIVATE; releases → PUBLIC `finmodel-releases`. **v0.6.0 was the live release** (now superseded by v0.6.1) —
signed NSIS installer built + published (`gh release create v0.6.0`, assets
`finmodel_0.6.0_x64-setup.exe` + `latest.json`); the updater endpoint
`…/finmodel-releases/releases/latest/download/latest.json` was VERIFIED serving
`0.6.0` and the installer URL returns 200. v0.5.x clients auto-update on next
launch (new version strictly greater than installed). Signing key
`C:\Users\vinit\.tauri\finmodel.key` (OUTSIDE repo, empty password, pubkey id
`F055E4EA3C7A218C` — matches `tauri.conf.json`). Disk was 41G free at build time.

### What v0.6.0 is
The desktop app now runs ENTIRELY on the unified, workspace-scoped `agent_send`
loop (Phases A–G of the agentic-analyst plan, all complete). The legacy
keyed/routed JSON chat engine is DELETED, not just disabled — `chat_send`/
`chat_cancel`/`chat_send_blocking`, the old LLM turn loop, `route_intent`/`Intent`,
JSON persistence (`Conversation`/`ChatMsg` + read/write), the research/fallback
turn helpers, and the test-only `validate_tool_args` island were removed
(`commands/chat.rs` 3900 → ~1620 lines). Live behavior: streaming turns; tool
calling (build 3-statement + DCF models, trading comps, research WITH citations,
quotes, filings, PDF analyze); multi-turn memory (branch-linked history);
structured result cards; Approve/Deny parking for file-overwrite/export actions;
SQLite-backed conversations (list/load/rename/delete; load rebuilds render shape
from the branch path); model tool-capability auto-detected on Settings save;
no-key demo fallback via the isolated FallbackDispatcher. Automatic memory
capture stays OFF (`extract_memory → 0`) pending a labelled quality-gate dataset;
the capture/recall backend + tests are built and green.

### Gates (green this session)
- `cargo test -p finmodel-app --lib` — **205 passed, 0 failed**, clean build (0 warnings).
- `npm --prefix ui test` — **114 passed, 0 failed**.
- App launches + runs post-deletion (no startup regression).

### Release recipe that WORKED (supersedes the v0.4.0 "sign separately" note)
The build-time signing path works IF the key is passed as CONTENTS via an env
OBJECT, never `$(cat)` in an embedded shell (that mangles the blob — the old
gotcha). This session spawned the build from a JS runtime, reading the key
in-process so no shell touched it:
  env `{ CI:"true", TAURI_SIGNING_PRIVATE_KEY:<file contents>,
  TAURI_SIGNING_PRIVATE_KEY_PASSWORD:"" }` + `cargo tauri build --bundles nsis`
(from `src-tauri/`). Output: `target/release/bundle/nsis/finmodel_<v>_x64-setup.exe`
`+ .exe.sig`. Then bump `version` in `tauri.conf.json` + `src-tauri/Cargo.toml`
(lockstep), write `latest.json` = `{version, notes, pub_date, platforms:
{"windows-x86_64":{signature:<.exe.sig contents>, url:<release download url>}}}`,
and `gh release create vX.Y.Z --repo Knightwarrior911/finmodel-releases <setup.exe>
<latest.json>`. MUST set `CI=true` explicitly (sandbox default `CI=1` makes
tauri-cli reject `--ci`). `TAURI_SIGNING_PRIVATE_KEY_PATH` is NOT honored.

### Only deferred item
Signed installer is DONE. The 7-day rollback rehearsal (install v0.6.0, force an
update round-trip, confirm downgrade path) was not exercised this session — the
release itself is verified serving; a live end-user update-then-rollback drill is
the one remaining release-hygiene step.


## HANDOVER — v0.5.0 research-first copilot (superseded by v0.6.0)
**Branch `master`, tagged `v0.5.0`.** The research-first roadmap (Phases 0–7) is
committed and published. `v0.5.0` was the release prior to v0.6.0 — NSIS updater
payload published to `finmodel-releases` with Tauri updater (minisign) signature.
It has since been superseded (see the v0.6.0 handover above). v0.4.x clients will be offered
the update on next launch (auto-update behavior not verified in this session).
All test suites are green (core workspace, app lib 72, UI 34, research-eval 13).
Debug build smoke-tested over CDP/WebView2 (direct IPC + full analyst UI path).
Key surfaces added this line:
- **Research copilot** — typed intent router + single tool registry + capability-gated
  OpenRouter in `src-tauri/src/commands/chat.rs`; `ResearchMachine` reducer + async
  driver + bounded collector in `finmodel-core/fm-research`.
- **Data integrity (Phase 6)** — `fm-build/src/lib.rs`: `validate_extraction`
  (two-outcome BLOCK gate), `verify_balance_identity` + folded DCF/WACC into a real
  `Verification`, `period_key` parser, `SourceAuditRow` population, sector-honesty
  note. `fm-excel/src/sheets/sources.rs` renders the audit rows (empty ⇒ snapshot
  parity). `SourceAuditRow` type in `fm-excel/src/input.rs`.
- **Analyst actions (Phase 6.5)** — `src-tauri/src/commands/analysis.rs` (`ev_bridge`,
  `ifrs_bridge`, `tie_out` Tauri commands over `fm-value`/`fm-ifrs`/`fm-tieout`;
  new deps in `src-tauri/Cargo.toml`); UI in `ui/js/analyst.mjs` + the analyst modal
  in `ui/index.html`, launched from the model card. Deliberately NOT in the flat LLM
  tool list. Tests: `analysis.rs` unit tests, `fm-excel/tests/source_audit.rs`,
  `ui/tests/analyst.test.mjs`.
- **CI/release (Phase 7)** — `.github/workflows/ci.yml`: least-privilege permissions,
  research-eval hard gate, Windows app-lib job, UI job. `docs/RELEASE_CHECKLIST.md`
  corrected (version lockstep, tag-after-green-CI, post-release verify, executable
  rollback).

**Next:** review the diff, commit, bump the Tauri version (`tauri.conf.json` +
`src-tauri/Cargo.toml` in lockstep) for the research-first release, then follow
`docs/RELEASE_CHECKLIST.md` for the signed build + publish. Toolchain note: local
`cargo fmt --check`/`clippy` on the whole workspace flags PRE-EXISTING drift in
untouched files (e.g. `fm-value` clippy lints, `adhoc.rs` import ordering) — this
session formatted only its touched files with `rustfmt --edition 2021`.

## HANDOVER — v0.4.0 sellable-feature expansion, LIVE RELEASE (2026-07-15)
**Branch `master`.** Source `finmodel-rust` PRIVATE; releases → PUBLIC
`finmodel-releases`. **v0.4.0 is the live release** — committed (`36203e2`) + tagged
`v0.4.0` + pushed to `origin/master`; signed NSIS installer built and published to
`finmodel-releases` (tag `v0.4.0`, assets `finmodel_0.4.0_x64-setup.exe` +
`latest.json`); updater endpoint
`…/finmodel-releases/releases/latest/download/latest.json` verified serving 0.4.0 and
the installer URL returns 200. v0.3.x clients auto-update on next launch. Disk volatile:
`df -h /c` before any `cargo` (>6G for a signed build; reclaim via
`rm -rf src-tauri/target/debug finmodel-core/target/debug`). Signing key stays at
`C:\Users\vinit\.tauri\finmodel.key` (NEVER commit). Sign gotcha: build-time
`TAURI_SIGNING_PRIVATE_KEY="$(cat …)"` mangled the key blob in the embedded shell —
sign the built installer directly with `cargo tauri signer sign -f C:/Users/vinit/.tauri/finmodel.key -p "" <setup.exe>`.
Seven independent workstreams shipped (all flag/opt-gated, defaults unchanged so every
parity oracle stays byte-identical):
- **A — live WACC inputs.** `fm-fetch/src/market.rs`: `fetch_risk_free_rate` (`^TNX`),
  `fetch_price_history`, `compute_beta` (pure, tested), `fetch_beta` (2y weekly vs
  `^GSPC`). Wired into `model.rs::render_build` + `fm-cli` build, only when the caller
  left the 4.5%/1.0 defaults; provenance/fallback warnings; never fatal.
- **B — trading comps.** `fm-research/src/comps.rs` (`peer_from_metrics`,
  `build_public_comps`, tested). `BuildOptions` gains `peers`/`public_comps`; peer
  assembly (EDGAR + quote, excluded list) in `render_build` + CLI `--peers`. Fills
  the gated Comps Peers / Comps Summary sheets. Chat `build_model` `peers` array.
- **C — one-click PPTX deck.** `fm-pptx/src/writer/deck.rs`: `add_table` archetype +
  `write_model_deck`/`write_benchmark_deck` (+`ModelDeckInput`), inspect-tested.
  `BuildOptions.deck` / `BenchOpts.deck`, CLI `--deck`, chat always-on; `pptx_path`
  in the model/benchmark cards ("Open deck").
- **D — read the filing.** `fm-fetch/src/edgar.rs`: `fetch_filing_doc` + pure
  `split_filing_items` (tested). Chat tool `read_filing` (item 1A/7 clip), `filing_doc`
  card. Router: 10-K/risk-factors/MD&A + ticker → read_filing.
- **E — scenario case.** `BuildOptions.active_case` drives the existing scenario
  engine; chat `case` enum, CLI `--case`, router bear/bull, model card case tag.
- **F — analyze a PDF.** `model.rs::analyze_pdf_blocking` + `analyze_pdf` command/tool
  (reuses the non-US PDF+LLM path, `source="pdf"`, needs a key); webview drag-drop of
  a `.pdf` primes the composer; router on a quoted `.pdf` path.
- **G — UI polish.** copy-message, benchmark scroll + Copy-table (TSV), sidebar filter
  + two-step delete confirm, `Ctrl/⌘+N`/`Ctrl/⌘+K`/`Esc`-cancels-stream + Settings
  legend, refreshed chips. Chat now exposes **10 tools**.

## HANDOVER — Chat-first desktop redesign, v0.3.1 (superseded by v0.4.0, 2026-07-15)
**Branch `master`.** Source `finmodel-rust` PRIVATE; releases → PUBLIC
`finmodel-releases`. v0.3.1 was the prior live release (chat-first redesign +
weak-model safety net); superseded by v0.4.0 above. Disk volatile: `df -h /c` before
any `cargo`; a signed release build needs >6G free — reclaim with
`rm -rf src-tauri/target/debug finmodel-core/target/debug`.

The desktop app (`src-tauri/` + `ui/`) is now a **chat-first, claude.ai-style**
interface (replaced the old two-tool-card workspace). See `src-tauri/CLAUDE.md`
and `ui/CLAUDE.md` for the per-area maps.

### What shipped (v0.2.1 → v0.3.0 redesign → v0.3.1 fixes)
- **Chat engine** `src-tauri/src/commands/chat.rs` — conversation store
  (`app_config_dir()/conversations/<id>.json`; `list/load/delete/rename_conversation`)
  + an **OpenRouter tool-calling loop with live SSE streaming**. Events (copy
  `emit_progress` pattern): `chat_delta` (token chunk), `chat_tool`
  (`start|done|error`, carries `card`), `chat_done`, `chat_reset` (drop a
  fabricated draft). Single-flight + cancel via managed `ChatGate`
  (`chat_send`/`chat_cancel`). 8 tools (`build_model`, `benchmark_peers`,
  `web_search`, `read_page`, `get_news`, `research_deal`, `get_quote`,
  `list_filings`) call **shared blocking cores directly** — NOT the IPC command
  wrappers: `model::{build_model_blocking, prepare_model_core, finalize_model_core}`,
  `benchmark::benchmark_blocking`, `search::mcp_from_settings`, `fm_research`,
  `fm_fetch`. All are `pub(crate)`.
- **Deterministic router = weak-model safety net.** `route_fallback(msg)` (keyword
  rules, ticker regex, benchmark-before-build precedence) runs when there's no API
  key AND as a safety net when a model rejects `tools` (400/404) or answers an
  EXPLICIT data request without calling one. This guarantees finance numbers/links
  come from a tool result, never a fabricated free-form answer. `ai21/jamba-large`
  is weak at tool-calling but SAFE via this net; a real tool-calling model
  (Anthropic/OpenAI) is better for full NL. Bare definitional Qs still answer directly.
- **Read-path hardening** `finmodel-core/fm-fetch/src/websearch.rs` — `client()`
  now sends full browser headers + cookie store + gzip/brotli + 20s timeout;
  `fetch_page_text` → **`fetch_page` → `FetchedPage{title,text,status}`**,
  `PageStatus{Ok,Blocked,Thin}`, pure `classify_status(u16)` (403/429/503→Blocked,
  <200 chars→Thin). `fm-research/web.rs` adds `read_page_full` (+`read_page` shim
  keeps `agent.rs` untouched); `search::read_page` command returns
  `{title,text,status}`. Reader shows an honest blocked/thin prompt, never a dead end.
- **Frontend** `ui/app.js` DELETED → ES modules `ui/js/{core,sidebar,chat,cards,
  reader,settings,update,main}.mjs` via `<script type="module">`. 3-region grid
  (`index.html`), light+dark tokens (`[data-theme="dark"]`), bundled **IBM Plex
  Sans/Mono** woff2 in `ui/fonts/`. Model control tokens (`<|eom|>`) stripped
  (`strip_control_tokens` / `stripControlTokens`); streaming caret keyed off a
  `.streaming` class removed on finalize.
- **Kept:** every old Tauri command stays registered (CLI/tests/back-compat).
  `prepare_model`/`finalize_model` reused for the in-chat assumptions grid card.

### Build + release (updated; supersedes the CI=true note below)
- Set `CI` EXPLICITLY to `true` or `false` for the build — the sandbox's default
  `CI=1` makes tauri-cli's `--ci` flag reject with "invalid value '1'". Signing
  key `C:\Users\vinit\.tauri\finmodel.key` (OUTSIDE repo, contents not path).
  Build via a subprocess with env `{TAURI_SIGNING_PRIVATE_KEY:<contents>,
  TAURI_SIGNING_PRIVATE_KEY_PASSWORD:"", CI:"false"}` → `cargo tauri build
  --bundles nsis`. Bump `version` in `tauri.conf.json` + `src-tauri/Cargo.toml`,
  add a CHANGELOG entry, then `gh release create vX.Y.Z --repo
  Knightwarrior911/finmodel-releases <setup.exe> <latest.json>`. Auto-update needs
  the new version STRICTLY greater than installed.

### Gates (all green this session)
- `cd src-tauri && cargo test` — 13 chat unit tests (`build_chat_request`,
  `sse_accumulate` incl. split tool-call fragments, `route_fallback` + precedence,
  `strip_control_tokens`, conversation round-trip, `iso_utc`).
- `cd finmodel-core && cargo test -p fm-fetch -p fm-research` (+ `--workspace`,
  45 ok/0 failed) — incl. `classify_status`/`FetchedPage` tests.
- `node --check ui/js/*.mjs`; browser-driven flow tests (ES modules need HTTP,
  NOT `file://` — serve `ui/` and mock `window.__TAURI__` incl. `event.listen`).

### Not run (needs live env/keys)
Live `cargo tauri dev` E2E with a real OpenRouter key; Python tie-out / pytest
release gates (engine correctness, unchanged by this UI/chat work).

## HANDOVER — Desktop app shipped + auto-update LIVE (previous, 2026-07-14)
**Two repos:** source **`finmodel-rust` is PRIVATE**; releases go to the PUBLIC
**`finmodel-releases`** (github.com/Knightwarrior911/finmodel-releases). The Tauri
updater fetches `latest.json` UNAUTHENTICATED, so a private repo 404s — releases
MUST be published to the public repo, and its endpoint is baked into the exe at
build time. Disk volatile: `df -h /c` before any `cargo`. All work pushed to
`origin/master` (through `93386f5`). App installed here: `%LOCALAPPDATA%\finmodel\finmodel-app.exe`.

Shipped this session (v0.1.0 → **v0.1.1**), desktop app = `src-tauri/` + `ui/`:
- **UX redesign (`ui/`)** — self-explanatory two-tool workspace: (1) *Build a full
  model* (one ticker → 3-statement + DCF Excel), (2) *Benchmark a peer set*
  (comma-sep US tickers → comps). Format help + live-parsed ticker echo, Live/Demo
  mode banner, "You get" tags, save-note. Verified in headless browser (invoke mocked).
- **Auto-update (LIVE)** — `tauri-plugin-updater` inited in `lib.rs` (desktop-only);
  `plugins.updater` pubkey+endpoint + `createUpdaterArtifacts:true`; `updater:default`
  capability; backend cmds `check_for_update`/`install_update` (download+relaunch).
  **Always-visible FOOTER control** (app version + status button: Check → Checking →
  Up to date·vX / Update available→install), Snitch-style; also silent launch check +
  Settings "Check now". Remote strings HTML-escaped (`escapeHtml`).
- **SEC filing-doc fetch** — `fm-fetch::edgar` `recent_filings`/`search_filings` +
  `Filing` + `DEFAULT_FORM_TYPES`; reachable via `fm filings <ticker> [--form][--limit]`.

**Signing + release (see `docs/RELEASE_CHECKLIST.md` §6):**
- Minisign private key: **`C:\Users\vinit\.tauri\finmodel.key` — OUTSIDE repo, NEVER
  commit.** Public key is in `tauri.conf.json`. CI secret `TAURI_SIGNING_PRIVATE_KEY`
  = the file's CONTENTS (not path); password empty.
- Build+sign: `cd src-tauri && CARGO...` → run with env `CI=true`
  `TAURI_SIGNING_PRIVATE_KEY="<contents>"` `TAURI_SIGNING_PRIVATE_KEY_PASSWORD=""`
  `cargo tauri build --bundles nsis` → `target/release/bundle/nsis/finmodel_<v>_x64-setup.exe`
  + `.exe.sig`. (MUST set `CI=true` or tauri-cli mis-parses the shell's `CI=1`.
  `TAURI_SIGNING_PRIVATE_KEY_PATH` is NOT honored — pass the key string.)
- Publish: bump `version` in `tauri.conf.json` + `src-tauri/Cargo.toml`; then
  `gh release create v<X.Y.Z> --repo Knightwarrior911/finmodel-releases <setup.exe> <latest.json>`.
  `latest.json` = `{version, notes, pub_date, platforms:{"windows-x86_64":{signature:<.sig contents>, url}}}`.
  Endpoint `…/finmodel-releases/releases/latest/download/latest.json` verified serving 0.1.1.

**Remaining:** rebrand pdf-panda placeholder icons (`src-tauri/icons/`); wire live
market inputs (`share_price=0.0` → `fm-fetch::market::fetch_quote`) so DCF upside is
real; research port items 3–6 (news, PPTX decks, browser pipeline, agent/orchestrator).

## HANDOVER — Benchmarking subsystem (previous, 2026-07-12)
Rust workspace: `finmodel-core/` (11 crates). Build/verify from there:
`CARGO_INCREMENTAL=0 cargo test --workspace` (33 suites, 0 failed) and
`RUSTFLAGS="-D warnings" cargo build --workspace` (clean). Disk is volatile on
this box — `df -h /c` before any `cargo`; clear only `target/debug/incremental`.
Run the CLI via `cargo run -q -p fm-cli -- <cmd>` (the shell can't exec the .exe
directly). Pass Windows-style paths (`--out C:/tmp/x.xlsx`); git-bash `/c/tmp` mangles.

### Flagship: `fm benchmark` — benchmark filings → IB-grade Excel comps
`cargo run -q -p fm-cli -- benchmark --tickers "AAPL,MSFT,TSM,NVO" [--out X.xlsx] [--csv X.csv] [--ltm] [--multiples] [--usd] [--title ...]`
- Fetches SEC EDGAR XBRL per ticker; **US-GAAP AND IFRS** filers (foreign 20-F:
  TSM/SAP/NVO/SHEL/ASML) — `fm-extract::xbrl::{select_taxonomy,ifrs_tag_map}`, no LLM.
- 18 metrics / 7 dimensions (Scale, Growth incl. rev CAGR, Profitability incl. FCF
  margin, Returns, Capital Return, Liquidity, Leverage) + EDGAR **Sector** column +
  exact **taxonomy-qualified us-gaap:/ifrs-full: tag provenance** (cell notes) +
  MEDIAN/MEAN/MIN/MAX block (cached).
- `--ltm`: last-twelve-months (FY + latest YTD − prior-YTD; latest instant BS;
  freshest-tag + staleness guard). `--multiples`: EV/EBITDA, EV/Rev, P/E, mkt cap
  from filing-derived EV × live Yahoo price (no key; US filers — foreign blank due
  to ADR ratio). `--usd`: spot-FX normalize monetary metrics for mixed-currency
  global comps (Yahoo `{CCY}USD=X`); ratios/multiples FX-neutral. Extraction
  anchors target years to the filer's own latest FY (behind-calendar filers work).

### Other CLI: `build <ticker>` (full 3-statement model+DCF Excel), `verify`
(5 model snapshots, 0 diffs), `ifrs --xlsx` (IFRS-16 bridge), `ev-bridge --xlsx`
(EV bridge), `score`/`compare` (tie-out). All 7 exercised green.

### Key crates & gates
- `fm-research` — benchmark pipeline (`metrics_from_extraction`, `build_benchmark_table`,
  `benchmark_tickers_opts(BenchmarkOpts{ltm,multiples,to_usd})`, `apply_ltm/_multiples/_fx`).
- `fm-excel::adhoc` — AdHoc/benchmark table writer; `fm-excel::bridge` — EV+IFRS bridges.
  Both **oracle-gated** vs Python (`tieout/build_*_oracle.py` → `excel_snapshots/*.json`,
  `fm-excel/tests/{adhoc,ev_bridge,ifrs_bridge}_parity.rs`).
- `fm-fetch::{edgar,market}` — EDGAR XBRL/SIC + Yahoo quotes/FX. `fm-extract::{xbrl,ltm,edgar}`.
- Desktop app `src-tauri/` — `benchmark_peers` command + UI card (`ui/`). `src-tauri/target`
  was reclaimed for disk; next app build is COLD (~4-5GB). GUI click-through untested
  (needs a desktop session + WebView2 CDP per `automated-testing` skill).

### Follow-ups (resource-gated, in `docs/NEXT-SESSION.md`)
Non-EDGAR foreign filers (PDF+LLM, needs `OPENROUTER_API_KEY`); app GUI smoke
(desktop session); forward/NTM multiples & news (external feeds). Parity rule:
port calc → oracle-gate vs Python → reachable consumer; gates use committed
snapshots (not live parse), so extraction changes don't break them.

## Project Memory
Read the HANDOVER section above + `docs/NEXT-SESSION.md` (current resume note)
FIRST. The sections below are historical (Python-era tie-out track), kept for
context. Master plan: `docs/MASTER_PLAN.md`. Changelog: `CHANGELOG.md`.

## Plan Summary (build track)
P0 (safety net: CI, snapshots, failure honesty) → PR (Rust port, 6 crates, cell-for-cell parity vs baseline) → P1 (accuracy: banks/insurers/held-out on Rust engine) → P2E (engagement polish) → P3 (Tauri desktop v1, no Python). P2S + P4 + P5 PARKED.

## Current State
Baseline `_baseline_wave0.json` **re-frozen 2026-07-10** (Wave 1 task 1.1.0 + harden-basket sprint): **339/350 (96.86%), 7 cos** (ATCO/SAND/ASML/NESN/NOVO/BAS/MC), opus-pinned, immutable per-company GT committed. Tie-out transport fixed (`claude --model`, was the recorded blocker). SAP.DE→BASF; MC.PA pinned + added. Hardened: BASF IS-detection fixed (`_extract_financial_section` now matches "statement of income"/"sales"), MC GT corrected (was LVMH's condensed financial-review BS → now the primary statements, correct brands-vs-goodwill split). Guard green; fm-tieout CI fixture landmine fixed. Remaining 11 mismatches (net_income group-vs-total, SG&A split, dividends_paid, ppe_net RoU) are Rust-engine targets per the Rust amendment.

## Key Verified Facts (don't re-derive)
- Tie-out baseline EXISTS: `tieout/results/_baseline_wave0.json` (96.86%, 339/350, 7 cos; opus-pinned, immutable per-company GT)
- Guard test: `tests/test_tieout_no_regression.py` exists
- Dynamic IS Phases 1-4 implemented (commit 9174435); only SaaS template unbuilt
- `engine.py` lacks insurance/REIT projection modes (layouts exist)
- CI GREEN on GitHub (Actions: ruff + pytest-mock + cargo build/test --workspace); `pyproject.toml` + `requirements.txt` present. No desktop packaging/installer yet, no payments code.
- `writer.py` is 3615-line monolith; hardcoded `anthropic` imports in 5+ files

## Cross-Ref Patterns to Reuse
- Dodo Payments: [[dodo-payments-snitch-billing]]
- NSIS Installer: [[snitch-nsis-installer-shipped]]
- Decko COM PPTX: [[decko-tauri-migration]]
- Tauri patterns: [[pdf-panda-tauri-rebuild]]
