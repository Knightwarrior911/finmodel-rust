# Finmodel — Financial Model Engine

## HANDOVER — v0.9.37 SHIPPED + LIVE (2026-07-21) — dispatch_swarm parallel subagent fan-out
**Tagged v0.9.37; Latest on finmodel-releases; CI green (run 29854045813, all 5
jobs); endpoint VERIFIED (latest.json 0.9.37, sig 420, installer HTTP 200,
6,823,657 bytes). Gates: app-lib 389, ui 208, python 234 (9 skip).**

- **`dispatch_swarm` (new tool, 18th in the registry):** one call spawns an army
  of read-only research subagents that run in PARALLEL, one per slice. Args
  `{ context, tasks[] }` — shared context prepended to every worker + up to
  `MAX_SWARM=8` slices `{ name?, task, agent? }`. Reuses `run_child_loop`
  (`tool_run_agent`/`tool_delegate`) so it is guaranteed fan-out, never new agent
  behavior. Consolidated `swarm` card: one panel per worker in input order,
  per-slice trail, `k/n returned a brief` tally; failed slices marked not dropped.
  Base doctrine nudges the analyst to swarm a divisible task automatically.
- **Bounded via shared slots:** new `ActorRegistry::acquire_active_slot` lets the
  batch borrow the run's per-run/global permits (4/8) instead of an inner pool —
  several swarms in one turn can't oversubscribe; global-then-per-run order =
  deadlock-free. Excluded from child/agent belts (one level deep). Returned
  briefs' spend aggregates into the card's `usage`, charged once (a failed slice
  bills like a failed `delegate_analysis` — partial spend not recharged).
- Files: `agent/tools.rs` (spec+schema+validate, count pins→18), `commands/chat.rs`
  (`tool_swarm`/`parse_swarm_tasks`/`build_swarm_output`, doctrine), `agent/registry.rs`
  (shared slot), `agent/delegate.rs` (belt exclusions), `ui/js/cards.mjs`+`style.css`
  (`renderSwarm`). Referenced OMP `@oh-my-pi/pi-coding-agent@17.0.4` `src/task/` batch contract.
- Commits: feature `dd8969d`, memory/doc reconcile `e5a6201`. Memory refreshed:
  `.claude/memory/{architecture,workflows,conventions}.md` (workflows release ritual
  reconciled to the authoritative checklist: sign-in-build, tag-after-CI).
- **Live leg untested here:** the swarm's live LLM legs (child loops hitting the
  provider) were NOT run this session (no OpenRouter key/network). All deterministic
  logic — parse, fan-out, shared-slot bounding, aggregation, card, belt exclusion —
  is unit-tested; the child-loop it drives is the same code the existing delegate
  path uses. Live tie-out (checklist step 1) not run — Python core untouched; the
  deterministic tie-out regression guard passed inside the 234 pytest gate.

## HANDOVER — v0.9.36 SHIPPED + LIVE (2026-07-21) — reject blank citation quotes + answer-quality eval harness
**Tagged v0.9.36; Latest on finmodel-releases; CI green (run 29828081488);
endpoint VERIFIED (latest.json 0.9.36, sig 420, installer HTTP 200, 6,791,382
bytes). Gates: fm-research 131 lib + 15 eval, app-lib builds clean.**

- **Grounding fix (production):** `synth::validate_synthesis` now rejects
  blank/whitespace-only citation quotes (`BlankQuote`) — an empty quote is a
  substring of every source, so it previously passed and let a citation ground
  nothing. The quality grader mirrors this rule.
- **Answer-quality eval harness** (`fm-research/src/quality_eval.rs`): offline
  grader (`grade`) + model×prompt sweep (`run_sweep` / `run_sweep_from_json`) +
  committed regression gate (`tests/baselines/quality_v1.json` — gold hash +
  weights pinned EXACTLY, per-case/mean scores as regression FLOORS) + CLI
  (`examples/quality_sweep.rs`, exit 1 below floor / 2 on bad input). Metrics:
  completeness (answer PROSE only), section + citation coverage, `quote_integrity`
  (verbatim case-sensitive substring — same rule as `validate_synthesis`), and
  cited-`Read`-source sufficiency. Gold facts use `any_of` phrasings with exact
  numeric anchors (paraphrase-tolerant, wrong-magnitude-safe).
- Memory refreshed: `.claude/memory/{architecture,workflows,conventions}.md`.
- Commits: feature `062ebcc`, release `4b90f34`.
- **Live leg untested here:** model×prompt generation is a separate app-layer
  producer (openrouter unreachable this session); the harness scores its JSON
  artifacts offline. The blank-quote fix's live synthesis path is unit-tested,
  not run against a live model this session.

## HANDOVER — v0.9.35 SHIPPED + LIVE (2026-07-21) — house dollar rule finished (section-first $ + per-share cents)
**Tagged v0.9.35; Latest on finmodel-releases; CI green (run 29815321599);
endpoint VERIFIED (latest.json 0.9.35, sig 420, installer HTTP 200, 6,787,273
bytes). Gates: full workspace green + new cell-format tests.**

- **Section-first `$`** on every statement section (IS/BS/CF) via a row-level
  selector in the statement builders; ordinary dollar rows stay plain.
- **Per-share/price cells show cents** (`$#,##0.00`) workbook-wide: IS EPS, CF
  Dividend/Share, Assumptions Dividend/Share + Current Share Price, DCF
  implied/current price + both sensitivity matrices, Sensitivities tables, Cover
  prices, Comps peer Price/52w/LTM-EPS + Implied Per-Share Price. Aggregate `$M`
  figures and share counts stay integer; `$` suppressed for non-USD.
- New helper `fmt_per_share(currency)` in `fm-excel/src/model.rs`. Tests:
  `fm-excel/tests/formats.rs` (Assumptions via SEK snapshot + every valuation
  sheet via fixtures) and a `fm-build` statement test (USD + EUR).
- Completes the number-format spec deferred in v0.9.34.

## HANDOVER — v0.9.34 SHIPPED + LIVE (2026-07-21) — memo artifacts findable/openable + format spec + region-aware agents
**Tagged v0.9.34 (release 0a00d17 + fallback fix dd041c1); Latest on
finmodel-releases; CI green (run 29811922760); endpoint VERIFIED
(latest.json 0.9.34, sig 420, installer HTTP 200, 6,795,445 bytes).
Gates: app-lib 385, UI 207, fm-excel clean.**

PRIORITY BUG (user: drafted earnings memo "saved somewhere" but could not
find/open it). Traced via the live DB + %TEMP%: the tool DID run and wrote
Tesla_earnings_note_*.md/.pptx to %TEMP%/finmodel-memos (out_dir empty),
but (a) %TEMP% is undiscoverable, (b) same-day filename overwrote prior
drafts, (c) open_path REJECTS unregistered raw paths and openPath()
swallowed the failure -> dead buttons. Fixes (chat.rs tool_draft_memo,
model.rs, artifacts.rs, lib.rs, core.mjs, cards.mjs):
- Durable default dir Documents/finmodel/memos (document_dir->config_dir->
  temp fallback); unique collision-safe {stem}_{kind}_{date}_{hms}[_n]
  basename shared by .md+.pptx; verify path.exists() after write.
- push_recent now registers the file AND its parent folder; new
  model::rehydrate_recent(app) called in lib.rs setup (list_recent was
  NEVER invoked - persisted Recent never rehydrated -> buttons broke after
  restart, incl. model cards). open_path gate now satisfied.
- core.openPath returns bool (was undefined, swallowed); cards.mjs
  openFileOrHint surfaces .open-fail-hint on failure.
- memo card inline preview (card.preview = truncate(md,4000)).
- earnings_release memo kind: section plan, DRAFT—NOT FOR DISTRIBUTION
  banner (render_markdown), fallback_text Outlook no-guidance + word-token
  guidance detector, memoKindLabel, schema enum.

FORMATTING (user image spec §3): fm-excel adhoc.rs + model.rs number-format
CODES normalized verbatim (zero/text _) alignment padding; price/per-share
get $; adhoc dollar made plain per the "other dollar rows plain" rule).
DEFERRED (stated to user): $-only-on-section-first-row placement needs a
row-level selector in the statement builders + a workbook-XML test +
visual verify; model fmt_dollar still $-prefixes all USD (pre-existing).

REGION-AWARE AGENTS: chat.rs GROUND_RULES now states get_financials non-US
coverage (20-F native ccy, ESEF by name/LEI, EDINET), list_filings/
read_filing are SEC-by-ticker incl. foreign 20-F (form:20-F) but not
home-market-only, fallback to research/web/IR; analyze_pdf only on an
attached artifact. diligence-reviewer.md risk-source line domicile-neutral.

STILL OPEN: live draft->open flow verified by COMPONENTS (unit tests) only
— full end-to-end needs a live LLM run (network blocked all session).

## HANDOVER — v0.9.33 SHIPPED + LIVE (2026-07-20) — starter agent bench
**Tagged v0.9.33 (release 06d573d + test commit); Latest on finmodel-releases;
CI green (run 29807147739); endpoint VERIFIED (latest.json 0.9.33, sig 420,
installer HTTP 200, 6,792,617 bytes). Gates: app-lib 382, UI 205.**

Agents feature shipped empty in v0.9.31; now seeds 5 read-only research
specialists on first run: diligence-reviewer, comps-analyst, earnings-reviewer,
credit-analyst, deal-screener. Bundled AGENT.md in src-tauri/agents/
(BUILTIN_AGENTS); agents::seed_builtin_agents mirrors seed_builtin_skills
(one-shot .seeded_v1 marker, never clobbers, sticky deletes), called in
lib.rs setup after the skills seed. Each agent wires REAL built-in skills
(reviewer/earnings-analysis/verification-loop, comparable-companies/precedent-
transactions, credit-analysis/lbo-screen, ma-accretion-dilution). Tests:
bundled parse + name==stem + every cited skill in BUILTIN_SKILLS + first-run
no-clobber + sticky delete + END-TO-END preload (seed both, compose each
prompt, assert ## Skill: present, none flagged missing). Existing installs
get the bench on next launch (seed runs at startup).

## HANDOVER — v0.9.32 SHIPPED + LIVE (2026-07-20) — bug-hunt pass (4 fixes)
**Tagged v0.9.32 (release c47d712 over fix 56f772f); Latest on
finmodel-releases; CI green (run 29800807523); endpoint VERIFIED
(latest.json 0.9.32, sig 420, installer HTTP 200, 6,799,758 bytes).
Gates: app-lib 379, UI 205.**

Feature-by-feature bug hunt: (1) agent<->data-room dead-end — run_agent
contract + agent GROUND_RULES (chat.rs tool_run_agent) now state agents
have read-only research tools and CANNOT open folders; orchestrator runs
analyze_data_room itself then hands findings to the agent. (2) prompt
cache — chat.rs mark_cache_prefix anchors the FIRST (stable) leading
system layer, not the last; build_context appends volatile summary/
memories as trailing system layers, so the old anchor missed cache every
turn. (3) panic — parse_advisor_notes (driver.rs) + resolve_findings
(dataroom.rs) guard start<=end before content[s..=e]; "}" before "{"
used to panic. (4) budget — run_data_room accumulates per-question usage
onto card.usage (reuses delegate::add_usage/usage_value); parent charge
hook (driver.rs:1929) does the rest.

Audited clean (no change): scheduler risk-gating (parallel grouping, not
approval), drift gate (well-guarded), uncited_figures/contains_number
(boundary-aware). Still OPEN from prior: data_room_live_smoke + run_agent
live path unexercised (openrouter.ai was network-blocked all session).

## HANDOVER — v0.9.29-31 SHIPPED + LIVE (2026-07-20) — auditability, data room, user agents
**Three releases, all tagged + published + endpoint-verified (Latest =
v0.9.31, sig 420, installer HTTP 200). CI green on every tag.**

v0.9.29 — source auditability: text-fragment deep links (core.mjs
deepSourceUrl/fragmentQuote; cite pills + src-cards anchor on quotes/
snippets), financials card per-period filing URLs (AnnualFact grew accn;
filing_index_url strips dashes/zeros in path, keeps dashes in filename).
v0.9.30 — data room review: commands/dataroom.rs (walk_room symlink-safe
depth<=8, extract via fm_extract::pdf_pages [now pub], chunk_room, bm25,
resolve_findings PURE: excerpt-number resolution + verbatim quote
verification), Risk::LocalRead (pauses for approval), room card with
per-finding chips (file/p.N/verified badge, click opens file).
v0.9.31 — user agents: agent/agents.rs (AGENT.md mirror of skills; parse/
CRUD/catalog_block/agent_system_prompt with skill preload), run_agent on
generalized delegate::run_child_loop, agent_tool_belt (read-only +
use_skill, no nesting), agents catalog in grounding_layers, Settings ->
Agents tab, agents_* commands.

OPEN: data_room_live_smoke (--ignored) still network-blocked —
openrouter.ai unreachable from this box all session (curl gets nothing);
run at first window. run_agent live path likewise unexercised (same
adapter as production turns). Human click-through wanted: Settings ->
Agents (create dd-reviewer), ask a question that dispatches it; data
room approval prompt on a real folder.

## HANDOVER — v0.9.28 SHIPPED + LIVE (2026-07-20) — JARVIS persona + cost honesty + delegation polish
**Tagged v0.9.28 (abf0149); released to public finmodel-releases (Latest).
CI green (run 29768979254); signed NSIS published; endpoint VERIFIED:
latest.json serves 0.9.28 (sig 420), installer HTTP 200. Gates: app-lib
369, UI 201.**

Pass 3: (a) JARVIS-register PERSONALITY appended to SYSTEM_PROMPT in
chat.rs (chief-of-staff bearing, one wit touch, professional failure
reports) — test persona_reaches_every_live_prompt. (b) delegate child
usage rides card.usage → driver charges CostGuard (generic: any card
with usage charges). (c) cancel: run_tool + tool_delegate +
run_delegate_loop take the parent CancellationToken (ChatToolBackend
passes ctx.cancel); child checks between rounds + streams abort. (d)
delegate cards carry trail[] (tool/subject/first line) → collapsed
details in renderDelegate. (e) advisor auto-on: advisor_model_for —
explicit setting wins, else Skeptic/Goal use cfg.model fresh-context.
(f) turn_cost side card (tokens always, usd >= $0.0005) via
turn_cost_card + CostGuard::token_counts. (g) FOUND+FIXED: chat_tool
events have NO UI consumer — self_check/advisor/turn_cost now emit via
Driver::take_side_cards drained by the actor at Action::Verify onto the
durable ResultPartAdded path (live + persisted via turn_results).

Still deferred: sliding cache breakpoint (needs live Anthropic verify);
research machine internal LLM spend still outside CostGuard (delegate is
now inside); emit_tool/chat_tool legacy channel is consumer-less —
candidate for removal in a cleanup pass.

## HANDOVER — v0.9.27 SHIPPED + LIVE (2026-07-20) — harness pass 2: drift rule, advisor, delegation, prompt profiles
**Tagged v0.9.27 (release commit 2cdae82 + chore); released to public
finmodel-releases (Latest). CI green (run 29766978150); signed NSIS
published; endpoint VERIFIED: latest.json serves 0.9.27 (sig 420),
installer HTTP 200 (6,738,342 bytes). Gates: app-lib 365, UI 199.**

OMP-gap roadmap items 3-6, all live: (3) drift rule — driver.rs
uncited_figures/contains_number (pure, tested) + drift_detected wrapper
(whitelists the WHOLE visible history); one corrective round inline in
request_model, self_check card in the trail. (4) advisor — settings
advisor_model (blank=off), driver::advisor_note at synthesize start,
streams under <run>:advisor (never paints), parse_advisor_notes strict
JSON, "Second look" card + renderer. (5) prompt profiles —
chat::model_scaffolding (segment match; gemini!=mini) woven via
driver::agent_system_prompt. (6) delegation — agent/delegate.rs:
delegate_analysis tool, child loop (4 rounds max, read-only belt minus
delegate/draft_memo/use_skill, last round tool-less), compact brief
yield, wave concurrency PROVEN by executors test. CRITICAL FIX: prepare()
rebuilds messages and had silently dropped BOTH the v0.9.25 mode doctrine
and any seed layers — all prompt layers now compose in
agent_system_prompt at the live seam (test pins Plan/Skeptic + scaffold
stacking). Escaping lesson (3 incidents): never route Rust through JS
template literals / String.replace — use write/edit tools or
split/join.

Still deferred: sliding cache breakpoint on role:tool content (needs live
Anthropic verify); advisor cost not in CostGuard (parity with research).

## HANDOVER — v0.9.26 SHIPPED + LIVE (2026-07-20) — harness pass 1: prompt caching + self-repair tool errors
**Tagged v0.9.26 on 6edf3af; released to public finmodel-releases (Latest).
CI green (run 29763540825); signed NSIS published; endpoint VERIFIED:
latest.json serves 0.9.26 (sig 420), installer HTTP 200 (6,689,614 bytes).
Gates: app-lib 356, UI 195.**

First two items of the OMP-gap roadmap (harness quality: make every model
perform better in finmodel): (1) prompt caching — mark_cache_prefix in
src-tauri/src/commands/chat.rs puts an OpenRouter cache_control ephemeral
anchor on the LAST leading system layer for anthropic/ + google/ models
(tools+system+mode doctrine cache across tool-loop rounds; OpenAI-class is
automatic, untouched). Deliberate follow-up: a second SLIDING breakpoint on
the last history message needs live verification that OpenRouter forwards
cache_control on role:tool multipart content before enabling. (2) tool
errors that teach — executors::tool_error_content echoes the tool catalog
(UnknownTool) or exact params schema + required list (MissingArg/Invalid,
900-char cap) into the role:tool message; runtime errors stay terse; wired
in driver.rs error branch. Remaining roadmap (discussed, not started):
drift rules (abort+inject on uncited arithmetic), Verifier-as-advisor,
per-model prompt profiles, true child agents for research fan-out.

## HANDOVER — v0.9.25 SHIPPED + LIVE (2026-07-20) — working modes (autonomy dial) + composer model chip
**Tagged v0.9.25 on c707cad; released to public finmodel-releases (Latest).
CI green (run 29757213942); signed NSIS published; endpoint VERIFIED:
latest.json serves 0.9.25 (sig 420), installer HTTP 200 (6,679,794 bytes).
Gates: app-lib 353, UI 195.**

Five working modes picked from a composer chip — Analyst (default), Plan
first (read-only belt via ToolRegistry::agent_schemas_read_only, numbered
plan, one-shot flip back to Analyst after delivery), Goal run + Loop &
refine (Policy::WORKFLOW), Skeptic (adversarial doctrine). Mode module:
src-tauri/src/agent/modes.rs; doctrine layer inserted before the user turn
in LiveDriver::new; mode name recorded on the run row's policy column and
revived by agent_resume (interrupted Goal runs resume as Goal; Plan resumes
read-only). UI: mode chip + menu in ui/js/composer.mjs (getMode/setMode),
send payload in chat.mjs sendViaAgent; Plan is one-shot in the UI too.
Model picker moved inside the input box (short-name chip, full id tooltip).

## HANDOVER — v0.9.24 SHIPPED + LIVE (2026-07-20) — two-face theme + bug hunt
**Tagged v0.9.24 on e823f7e; released to public finmodel-releases (Latest).
CI green (run 29754078854). Signed NSIS published; endpoint VERIFIED:
latest.json serves 0.9.24 (sig 420), installer HTTP 200 (6,677,333 bytes,
byte-exact to the signed local bundle). Includes the bug-hunt commit
923cb51 (recency-first taxonomy selection — Toyota fix) and the theme
commit (Cursor-cream light / OpenCode-dark, --accent-ink AA token; see
ui/CLAUDE.md session block). Human visual pass of both themes still
recommended on next launch (token-only change, jsdom can't see color).**

## HANDOVER — v0.9.23 SHIPPED + LIVE (2026-07-20) — international research + composer polish
**Tagged v0.9.23 on 7be5c36; released to public finmodel-releases (Latest).
CI green all 5 jobs (run 29742579672). Signed NSIS published; endpoint
VERIFIED: latest.json serves 0.9.23 (sig 420), installer HTTP 200
(6,673,057 bytes = the signed local bundle).
Gates all green: app-lib 351 · fm-fetch 59 · fm-research 126 · UI 194.
Live-verified: SAP EUR spread (20-F/IFRS via EDGAR), Fiskars EUR spread
end-to-end via ESEF with NO US listing. EDINET leg is key-gated and
fixture-only (needs a free EDINET key → Settings → Connections; run
live_edinet_toyota with EDINET_API_KEY to verify first time).
Subagent infra note: two 5-agent parallel batches died at ~3m45s with
socket errors (same session had openrouter.ai TLS resets); everything was
executed serially instead.**

What landed (details in src-tauri/CLAUDE.md session block):
1) IFRS + currency through the whole numbers pipeline (spread/LTM/
   quarterly/semi/comps), 20-F/40-F/ESEF/EDINET form acceptance, native
   currency formatting, basis=semi, honest foreign-quarterly message.
2) fm-fetch esef.rs (live-verified) + edinet.rs (key-gated) emitting
   EDGAR-companyfacts-shaped JSON — the one interchange contract.
3) get_financials routing EDGAR→ESEF→EDINET + validate_company_query +
   updated tool description (names/LEI accepted).
4) Verification identities: EPS + EBITDA recompute (NCI stand-down band
   documented as known hole).
5) Research: grounding coverage line on every answer/digest; local
   newswires tiered; operator-smart queries (quotes, site:, -site:;
   NEVER before:/after: — DDG fallback breaks on them).

## HANDOVER — v0.9.22 SHIPPED + LIVE (2026-07-20) — composer multimodal + billing safety
**Tagged v0.9.22 on fd25e15; released to public finmodel-releases (Latest).
CI green all 5 jobs (run 29729750562). Signed NSIS published; endpoint
VERIFIED: latest.json serves 0.9.22 (sig 420), installer HTTP 200
(6,629,539 bytes = the signed local bundle). Local gates: app-lib 344 ·
UI 191 · cargo check --bin clean.**

What's in v0.9.22 (two feature waves on top of v0.9.21):
1) Composer input surface: type-ahead model picker on the pill
   (ui/js/composer.mjs; list_models catalog, 5-min cache + in-flight
   dedupe); attachments via paperclip / OS drag-drop / Ctrl+V paste
   (images ≤5MB ×4, PDF/PPTX/XLSX/DOCX/txt-md-csv-json); backend staging
   commands/attachments.rs (classify + extract via fm-excel calamine,
   fm-pptx inspect, docx zip-XML, plain text 12k cap); multimodal image
   parts flow through seed_agent_messages_with_images → driver → provider.
2) Vision auto-routing + spend guards + polish:
   - agent/model_router.rs route_for_vision: cheapest vision+tools model,
     ≥32k ctx, no :free variants, parseable price ≤ cap (settings
     route_price_cap_usd, default $5/M out); per-TURN override in
     launch_run (agent.rs) — never persisted; model_note → UI line.
     NoneAffordable refuses BEFORE provider call/attachment consumption.
   - CostGuard (agent/driver.rs): budget_usd from settings
     conversation_budget_usd (0=off); charges every stream round
     (accept_stream + manual arms + strong finisher) preferring
     OpenRouter billed usage.cost (usage:{include:true} injected in
     stream_completion_for_agent for OpenRouter only; cost survives the
     SSE usage filter in chat.rs apply_delta); falls back tokens ×
     catalog snapshot; total-only ⇒ total × out-rate (overestimates on
     purpose). Persists per-round via store set_run_usage; conversation
     sum = conversation_spend_usd; finish_run now COALESCEs usage_json.
     request_model refuses to START a round over budget (friendly stop).
   - refine_prompt command (settings.rs, 600-token cap) + sparkle
     refineBtn in composer with undo (hint-action).
   - Global personalization: grounding config.json instructions now has
     Settings UI (read_global/write_global read-modify-write; legacy
     'personalization' alias removed on write). ONE source of truth —
     Settings.global_instructions field was deliberately NOT added.
   - Settings copy humanized: Spending + Personal touch sections; money
     fields reject junk (backend errors, UI validates); 'sees images'
     badges (catalog vision() from architecture.input_modalities with
     modality-string fallback, fm-extract llm.rs).
   - Copy button: .msg-copy absolute→in-flow footer (overlap fix).

Live-network caveat: openrouter.ai TLS handshake was RESET from this
machine all session (SEC egress fine; pre-existing live tests fail
identically) — live vision smoke ships as ignored test:
cargo test --lib live_vision_red_png_mini -- --ignored --nocapture
Not covered: budget inert on non-OpenRouter without catalog prices
(hinted in Settings); one in-flight round can overshoot the cap;
workspace-DB standing_instructions still dormant (project files cover it).

## HANDOVER — v0.9.21 SHIPPED + LIVE (2026-07-20) — the analyst writes the memo
**Tagged v0.9.21 on 7759f09 (docs atop RC 4171a1b); released to public
finmodel-releases (Latest). CI green all 5 jobs after the user fixed the
GitHub Actions billing block. Endpoint VERIFIED: latest.json serves 0.9.21
(sig 420), installer HTTP 200 (6,360,226 bytes = the signed RC bundle).**

(Original staged-state notes below, kept for history.)

## HANDOVER — v0.9.21 was STAGED (2026-07-19) — draft_memo on master; CI was BILLING-BLOCKED
**v0.9.21 NOT TAGGED YET: GitHub Actions is rejecting ALL jobs with
"account payments have failed or your spending limit needs to be increased"
(0 steps, instant failure, rerun identical). User must fix GitHub billing;
then: wait green CI on master head → tag v0.9.21 → publish. A FRESH
SIGNED installer already exists, built at RC 4171a1b (the frozen release
candidate): src-tauri/target/release/bundle/nsis/
finmodel_0.9.21_x64-setup.exe (6,360,226 bytes) + .sig (420) +
latest.json (signature + release URL baked in). If master head still
equals 4171a1b (± docs-only commits), publish THAT bundle as-is;
if feature code moved, rebuild + re-sign first. The older bundle from
7becd04 was overwritten by this build.**

Every CI-equivalent gate ran LOCALLY with exit codes on the target OS:
app-lib 318 (Windows = the app job) · UI 175 · fm-research/fm-fetch/fm-build
green · no Python changes (test/ruff cover the legacy core only).

draft_memo (the drafting leg of research→numbers→prose, goal work):
- agent/memo.rs: collect_evidence (REAL-FIXTURE tested against a live user
  DB's card JSON — tests/fixtures/real_cards.json), section_specs per kind
  (earnings_note / company_profile / deal_summary), validate_slot
  (evidence-only numbers + derived analyst roundings 97690→97.7/97.69;
  slop-phrase ban; decimal-safe sentence caps — a live mini draft was
  wrongly rejected when '.' split $97.7; real-source citations),
  fallback_text (honest fact sentences), render_markdown (deterministic
  scaffold: title/tables/segments/sources).
- tool_draft_memo in chat.rs (14th tool; counts updated): conversation
  cards → evidence → per-slot complete_once on synthesis_model||model →
  validate → retry-with-reason → fallback → .md artifact in out_dir +
  memo card. UI: memo card, Artifacts dock pickup, warm labels.
- LIVE mini smoke (agent::memo::tests::live_memo_slot_mini, --ignored):
  gpt-4.1-mini's first headline VALIDATED — cited, precise, no slop.
Since staging, master also gained (all riding v0.9.21):
- memo::draft_sections = the ONE production drafting loop (tool_draft_memo
  delegates); live gates run the exact production path:
  live_memo_full_mini (earnings note over frozen real-app evidence:
  3/3 model-written, 0 fallbacks) and live_comps_note_mini (3/3).
- comps_note memo kind (peer set / relative positioning / valuation read);
  model + benchmark cards distill into evidence; engine-computed revenue
  ratios + fraction→percent derivations keep honest prose validating.
- Memo → deck: fm-pptx add_prose archetype + write_memo_deck; draft_memo
  deck:true saves a branded PPTX beside the .md; card 'Open deck' action.
- Gates now: app 321 · UI 176 · fm-agent 50 · fm-pptx 6.
Workflow integration (same head): earnings_review/company_brief/
ma_screen/pitch_prep allow draft_memo; plan templates end with the drafting
step; prompt doctrine teaches the kind mapping. NOTE: when billing is fixed,
tag v0.9.21 on the CURRENT master head and REBUILD the installer (the staged
one predates the workflow integration).
GOAL remains ACTIVE (broad, ongoing).

## HANDOVER — v0.9.20 SHIPPED + LIVE (2026-07-19) — live-session diagnosis: careers-page ban
**Tagged `v0.9.20` (0cf5e70); Latest on finmodel-releases; CI green; endpoint
verified (0.9.20, sig 420 clean, installer 200, digest match). Installer was
built at 2ad7238; the tag adds ONLY #[cfg(test)] code (live regression test)
— release binary identical.**

First LIVE-SESSION diagnosis: user reported random company pages in research.
Method: copy finmodel.db+wal from AppData/Roaming/com.finmodel.desktop (NEVER
open the live WAL DB read-write), inspect via bun:sqlite readonly; grasp OCR
of the running window confirmed what the user saw. Findings (Veoneer/Magna
2021 synergies question):
- research READ veoneerin.teamtailor.com/jobs (ranked Company via name-token
  upgrade!), linkedin.com/company/veoneer, ambitionbox.com employee reviews;
- 2 runs ended "selected model could not produce a validated synthesis"
  (gpt-4.1-mini) → raw digest of the junk pages rendered;
- 4 read_filing calls failed on the DELISTED 2021 target (tool reaches the
  latest filing only).
Fixes: BANNED_DOMAINS += teamtailor/greenhouse/lever/myworkdayjobs/glassdoor/
indeed/ambitionbox/comparably/zippia/linkedin; BANNED_PATHS /jobs|/careers|
/vacancies even on the company's own domain; SYSTEM_PROMPT historical-events
doctrine (research not read_filing for past events; deal mode for M&A).
LIVE-verified on the exact failing question: ledger contains zero banned
hosts (live_no_careers_pages, --ignored).
RECOMMEND TO USER: set Research synthesis model (Settings → Connections) —
the failed-synthesis digests are the mini model failing cite-validated
writing; that setting exists for exactly this.
Gates: fm-research 112+13 · app 311 (+1 live ignored).

## HANDOVER — v0.9.19 SHIPPED + LIVE (2026-07-19) — tiering, rebrand, segments
**Tagged `v0.9.19` (280ae2c); Latest on finmodel-releases; CI green on that
sha; endpoint verified (0.9.19, sig 420 clean, installer 200, digest match).**

1. **Stream-handoff tiering (real, every turn)**: with synthesis_model set,
   fast-model rounds stream under "<run>:draft" (UI filters deltas by run
   id — draft prose never paints; ':draft' reaches ONLY ephemeral window
   events, nothing persisted); on final_answer the driver's
   finish_with_strong_model streams the real answer under the real run id.
   Message surgery is pure + tested (driver.rs finisher module: draft pop
   only for plain prose, nudge push/pop symmetry, BYTE-IDENTICAL history
   restore on failure → turn always answers). Finisher prompt preserves
   [n] cite markers verbatim. NOT yet exercised with a live paid call —
   opt-in only, falls back to the fast draft on any failure.
2. **Rebrand**: src-tauri/icon-source.svg (indigo tile, ascending bars,
   trend tick) → cargo tauri icon regenerated ALL platform icons.
3. **Segments (3c)**: fm-fetch/src/segments.rs — XBRL instance parser.
   Rules (from review): contexts with EXACTLY ONE dimension (segments axis)
   — double-tagged product/geo facts excluded (no double-count); counting by
   ELEMENT BLOCKS not substring (closing tags double-counted — caught by
   fixture test); eliminations kept + labeled + sorted last; concept
   precedence mirrors the spread. instance URL = primary doc *_htm.xml;
   edgar::fetch_url_text (verbatim, 25MB cap). LIVE-verified: TSLA FY2025
   Automotive 82.056B / Energy 12.771B. get_financials (annual) attaches
   card.segments + text section; card renders .fin-segments table.
4. **Basis toggle (3b)**: financials_card command (spawn_blocking around
   tool_get_financials); Annual/Quarterly/LTM chips in renderFinancials;
   wireCard delegation swaps the card in place (renderCard on the response).
5. **Memory offers (3a)**: agent_send returns memory_candidate when
   is_durable_preference fires AND no commitment did; chat renders
   approval-gated "Remember it" chip → memory_add command
   (insert_memory kind=preference, confidence 1.0, source user_approved).
   Unattended capture stays OFF (precision doctrine).
Gates (exit codes): app 311 · UI 172 · fm-fetch 53 (+live segment test).

## HANDOVER — v0.9.18 SHIPPED + LIVE (2026-07-19) — P1 complete
**Tagged `v0.9.18` (efd1efe); Latest on finmodel-releases; CI green on that
sha; endpoint verified (0.9.18, sig 420 clean, installer 200, digest match).**

P1 items all done:
- **Resume across restart**: load_conversation returns last_run {id,status}
  (latest_run_for_conversation); chat.mjs shows the existing Paused/Resume
  bar when status=='interrupted' (boot repair marks orphans exactly that).
  Field parity Rust↔JS grep-verified; agent_resume path unchanged from the
  live-verified v0.9.0 flow.
- **Schedules panel**: Settings → Scheduled tab (SETTINGS_TABS + panel +
  loadSchedulesList): prompt · due date · recurrence · status + Cancel via
  schedule_cancel. settings.test.mjs covers list + cancel round-trip.
- **Synthesis model** (honest scope): Settings.synthesis_model (Connections
  field). Used by tool_research's OpenRouterSynthesizer and driver wrap_up.
  NOT the main chat loop — a fast-orchestrator/strong-finisher split needs
  a stream handoff (the fast model's final prose already streamed before we
  know it was final). That redesign is the remaining tiering follow-up.
Gates (exit codes): app 308 · UI 169.

## HANDOVER — v0.9.17 SHIPPED + LIVE (2026-07-19) — scheduled follow-through + absolute grounding
**Tagged `v0.9.17` (17f2d2e); Latest on finmodel-releases; CI green on that
sha; endpoint verified (0.9.17, sig 420 clean, installer 200, digest match).**

Scheduler (Tasks 8.2/8.3) is LIVE:
- agent_send extracts a commitment (agent/commitments.rs, precision-gated)
  and returns it in the response; chat.mjs renders an approval-gated offer
  (.schedule-offer, warm copy via scheduleDueLabel). NOTHING schedules
  without an explicit yes.
- schedule_create / schedules_list / schedule_cancel commands; store gains
  list/get/cancel/rearm + ScheduleRow.
- 60s tick in lib.rs setup → commands::agent::run_due_schedules → the tested
  core sweep_due_schedules(handle, launch) with an INJECTED launcher:
  oneshot→done, daily/weekly→re-armed future-due, failure→15-min backoff,
  TERMINAL failed at 5 attempts (tests prove all transitions vs a real
  store; live glue = two lines calling send_message_inner).
- agent_send refactored into send_message_inner (AppHandle+ActorRegistry
  owned args) so the tick can launch runs without a State wrapper.

Grounding/user asks:
- SYSTEM_PROMPT doctrine: NO company facts from training memory; private
  companies researched by NAME (site+research+news, no public tooling
  assumed); user-pasted URLs read FIRST as source of truth; unsupported →
  say what couldn't be verified.
- fm-research Candidate.pinned: user URLs outrank ALL tiers in
  assemble_ledger (sort (!pinned, rank)); wikipedia ban still wins over a
  pin (tested). App-side pinned_candidates() parses up to 3 URLs from the
  question (fused + generic arms).
Remaining P1: run resume across reload; model tiering; schedules management
UI in Settings (commands exist; list/cancel surface deferred).
Gates (exit codes): app 308 · UI 167 · fm-research 112+13 · fm-fetch 50.

## HANDOVER — v0.9.16 SHIPPED + LIVE (2026-07-19) — research reads what humans read
**Tagged `v0.9.16` (716e50c); Latest on finmodel-releases; CI green on that
sha; endpoint verified (0.9.16, sig 420 clean, installer 200, digest match).**
P0 #1 (PDF) + #2 (transcripts) + international + Roam retry + engine chain:
- PDF ingestion: read path detects .pdf URLs → 25MB-capped download →
  fm_extract::extract_pdf_text (temp file) → select_filing_excerpt. Live:
  Apple FY24-Q4 statements PDF → 4002-char excerpt. Berkshire letters fail
  gracefully (pdf-extract font limits → honest pdf: error → fallback doctrine).
- Transcripts: path contains "transcript" → SourceKind::Primary; earnings
  fused_search + spoken-word plan queries hunt them. Live: TSLA ledger with 4
  transcript carriers ranked Primary.
- Multi-engine search (fm-fetch websearch.rs): DDG HTTP 202 = disguised
  challenge (was parsed as zero-hit success = silent blindness); chain
  DDG → Bing RSS (&format=rss serves organic results past the JS wall) →
  Mojeek; empty results never cached. Live-verified under real blocking.
- International: HKEX/EDINET/TDnet/RNS/Euronext/SEDAR+/ASX/SGX/NSE/BSE in
  REGULATORS + PRIORITY_DOMAINS; edgar_hits==0 → annual-report/interim/
  presentation/English-IR queries; dotted tickers resolve via Quote.name
  (longName||shortName — raw MC.PA searched as Minecraft!); question name
  tokens upgrade the company's own domain to Company tier
  (fm_research::upgrade_company_candidates). Live: MC.PA-only question →
  lvmh.com S1-S5, zero wikipedia.
- Roam live-browser retry: HttpBackend.roam: Option<RoamReader> CLOSURE.
  **NEVER put tauri::AppHandle in HttpBackend/research graph**: it links the
  windowing runtime into the manifest-less lib-test exe → comctl32-v6
  TaskDialogIndirect → STATUS_ENTRYPOINT_NOT_FOUND for the WHOLE suite (cost
  a full cargo clean + PE-import bisect to find). chat.rs builds the closure
  (tool_read_page-style McpManager path). Retry fires ONLY for
  Blocked/Failed sources — user directive: browser when appropriate, not
  everything.
- **Gate on EXIT CODES** (cargo test …; echo EXIT=0): a grep-filtered gate
  hid a load-crashed suite as green this cycle.
P0 #3 DONE (5676a39): formula-fidelity AUDIT PASSED — rendered workbook is
65% formula-driven (767 formulas / 406 hardcoded; hardcoded = assumption
inputs + historical actuals, exactly right). Permanent regression gate:
fm-build/tests/formula_fidelity.rs (fails <30%). Presentation ALSO verified:
units lines on every sheet (sheets/mod.rs:82 "({currency} in millions,
unless noted)", Cover, DCF, comps, sources), accounting number formats
(FMT_NUM parens-negatives/dash-zeros, FMT_PCT, FMT_MULT 0.0x), Sources tab
fills from source_audit + verification, provenance cell notes, freeze panes.
No product change needed.
Next: scheduler wiring (8.2/8.3), run resume, model tiering (P1).
Gates: app 302 · fm-fetch 50 · fm-research 111+13 · UI 165.

## HANDOVER — v0.9.15 SHIPPED + LIVE (2026-07-19) — Evidence dock populated (Task 2.3)
**Tagged `v0.9.15` (277bc4f); Latest on finmodel-releases; CI green; endpoint
verified (0.9.15, sig 420 clean, installer 200, digest match). LIVE-smoked on
the real index.html over http-server + headless Chromium: module graph
(chat→evidence→reader/workbench) loads clean, zero console errors, all three
panels populate; smoke caught + fixed raw 'company'/'regulatory' kind chips
(sourceKindLabel now names all five tiers).**

New ui/js/evidence.mjs — conversation-level dock ledger fed from appendCard
(single funnel for live result_part_added AND history replay):
- Sources: dedup by url (first-seen numbering matches inline cites), rows =
  number · letter avatar · title · publisher · status/kind; click →
  openDock(reader) + openReader. Intake: answer.sources / card.sources,
  research_digest items, deal sources_read, page, filing_doc.
- Valuation: Map ticker → latest model card valuation (implied/current/
  upside/EV/WACC) + last verification verdict.
- Artifacts: xlsx_path/pptx_path cards newest-first; rebuild floats to top
  (dedup by path); click → openPath.
- Reset on newChat + loadConversation (rebuilds from replayed cards).
P0 roadmap next: PDF ingestion in research reads (fm-extract is wired for
tie-out only), earnings-call transcript source, Excel formula-fidelity audit,
then scheduler wiring (8.2/8.3).
Gates: UI 165 · app-lib 299 · fm-research 110+13.

## HANDOVER — v0.9.14 SHIPPED + LIVE (2026-07-19) — primary-source-first research
**Tagged `v0.9.14` (aa89b64); Latest on finmodel-releases; CI green; endpoint
verified (0.9.14, sig 420 clean, installer 200, digest match). LIVE-verified
end-to-end** via `cargo test --lib live_primary_first_research -- --ignored
--nocapture`: plan fired 4 primary-first queries on the real TSLA tariff
question; ir.tesla.com + /press ranked S1/S2 (Company); zero wikipedia; 4/9
read in ~7s (tesla IR bot-blocked — known v0.9.8 fallback case).

Doctrine: company sources → credible press → open web; Wikipedia never.
- adapter.rs: PR distributors (businesswire/prnewswire/globenewswire/
  newsfilecorp/accesswire) → Primary; Company via IR/press/news/media
  subdomains OR corporate-site paths (/investor-relations, /press-release,
  /newsroom, …). KNOWN EDGE: third-party /pressreleases/ paths (globeandmail)
  classify Company — issuer-authored text on a carrier; refine to Primary if
  it bothers.
- collect.rs assemble_ledger: BANNED_DOMAINS hard filter (wikipedia/wikimedia/
  wikidata/fandom/reddit/quora). web.rs WEB_JUNK += wikipedia (search card too).
- research.rs budgets: Standard 4q/10s/180s (default), Deep 8q/16s/420s.
- commands/research.rs: HttpBackend::plan now returns a DETERMINISTIC
  primary-first query set (was None — Standard/Deep never multi-queried!);
  fused_search runs IR + press-release queries before the generic one.
- commands/chat.rs tool_research: depth is the model's call (was forced Quick
  = 1q/3s/30s — the reason users out-researched the app).
Gates: fm-research 110 + eval 13 · app-lib 299 (+1 live ignored) · UI 159.

## HANDOVER — v0.9.13 SHIPPED + LIVE (2026-07-19) — context-resolved research questions
**Tagged `v0.9.13`; Latest on finmodel-releases; CI green; endpoint verified
(0.9.13, sig 420 clean, installer 200, digest match).**
- Root cause: ResearchToolArgs::into_request HARD-OVERWROTE the question with
  the raw user message. A "yes" reply to "want me to check the 10-Q?" became
  the literal research query → Yes Bank / Yes (band) / yesofficial YouTube,
  full deadline burned validating garbage.
- Fix (fm-research/research.rs): the model's `question` arg is the question
  of record (tool calling = context resolution); raw user text is the fallback
  when empty. Deal parties still parsed from user text (parse_ma_query,
  never model-trusted). Stale comments in chat.rs tool_research updated.
- Copy: sourceStatusLabel Thin→"Not much there", Blocked→"Site blocked us";
  digest sub "Collected before I could finish summarizing"; fm-research
  deadline limitation offers to continue (assertions updated in fm-research
  machine.rs + src-tauri commands/research.rs).
Gates: fm-research 109 + eval 13 · app-lib 297 · UI 159.

## HANDOVER — v0.9.12 SHIPPED + LIVE (2026-07-19) — move-picker fix + humane filing cards
**Tagged `v0.9.12`; Latest on finmodel-releases; CI green; endpoint verified
(0.9.12, sig 420 clean, installer 200, digest match).**
- Move picker bug: the <select> click bubbled past the .conv-move branch to the
  .conv-row branch → onSelect loaded the chat → sidebar re-render destroyed the
  open picker ("flicker and it's gone"). Guard at top of the convList click
  handler: closest(".conv-move-sel") → stopPropagation + return.
- filing_doc cards: read_filing (chat.rs) now ships "preview" (240-char
  whitespace-collapsed excerpt head, word-boundary cut). UI: form-aware
  filingFormLabel/filingItemLabel in labels.mjs (8-K sub-items resolve on major
  number); section reads show "Read Item N · Name" + quoted preview, whole-doc
  opens show named chips; char counts gone. CSS .filing-read-line/.filing-preview.
Gates: UI 159 · app-lib 297.

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
