# Changelog

## v0.9.13 — 2026-07-19 — "Yes" no longer researches Yes Bank

- **Follow-up answers stop becoming search queries.** Replying "yes" to
  "want me to check the 10-Q?" used to send the literal word "yes" to the
  research engine — which dutifully returned Yes Bank and the prog-rock band,
  then burned the whole research window validating them. The research question
  of record is now the model's context-resolved ask; your raw message is only
  the fallback when the model passes nothing.
- **Source statuses speak plainly.** "Thin" → "Not much there";
  "Blocked" → "Site blocked us"; the digest header and the out-of-time
  note now say what happened and offer to continue.

## v0.9.12 — 2026-07-19 — Move-to-project works, filing cards earn their place

- **The move-to-project picker no longer vanishes on click.** Clicking the
  dropdown fell through to the row underneath, which loaded that conversation
  and re-rendered the sidebar — destroying the open picker (the flicker). The
  picker now owns its clicks.
- **Filing cards show what was read, not byte counts.** "Excerpt ready · 574
  characters" is gone. A section read is named in plain English ("Read Item 2
  · Financial information") with the opening lines of the actual text quoted
  beneath; a whole-document open lists its contents by name ("Item 9 ·
  Financial statements and exhibits"). Form codes carry their plain names
  ("8-K · Current report"), and the SEC link says where it goes.

## v0.9.11 — 2026-07-19 — The analyst works until the job is done

- **No more "step budget" dead ends.** The per-turn work quotas (10–12 rounds —
  in practice barely five tool calls, since a round was charged on both the
  model reply and the tool completion) are gone. Ceilings still exist but only
  as runaway guards — hundreds of steps, hours of wall clock, sized so no
  legitimate task ever hits them. The analyst now works like a colleague:
  until the job is done, or you press Stop or Pause.
- **If a guard ever trips, you still get an answer.** The wrap-up pass now
  actually makes one final no-tools model call over the evidence gathered.
  Previously it silently persisted stale text — which is why a stopped run
  could end with "ask me to continue" and nothing else.
- **Targeted questions stay targeted.** "Did Tesla say anything about tariffs
  or China competition?" is answered directly from the filing and the news —
  it is no longer escalated into a full five-step earnings review producing
  deliverables nobody asked for.
- **More parallel headroom.** Interactive turns can fan out subagents; workflow
  missions get room for up to 32 children.

## v0.9.10 — 2026-07-19 — The analyst talks like a colleague, not a debugger

- **Tool activity as a story.** Live checks read as calm colleague narration —
  “Working through this” / “How I checked this” — instead of snake_case tool ids
  or schema-speak. Shared warm approval vocabulary (“Go ahead”, “Not this time”,
  “Save as a new version”) across chat, parts, and activity.
- **Mission chrome folds into the trail.** Phase/plan/verify no longer compete
  as a second status strip; progress stays polite for screen readers while the
  thinking panel carries the only visible status story.
- **Result cards demote schema.** Research, deal, quote, page, and verification
  cards use soft human status language and readable facts — no JSON dumps or raw
  enum labels.
- **Sources feel like cites, not ids.** Numbered inline citation pills plus an
  always-visible Sources strip (number · letter avatar · title · publisher). No
  external favicon fetches (CSP stays `img-src 'self' data:`).
- **One indigo voice + light motion.** Activity badges retire traffic-light
  green/red chrome for accent/neutral tokens. CSS-only entrances and micro-
  feedback (~120–220ms), compositor-friendly, with `prefers-reduced-motion`
  honored — no animation runtime.

## v0.9.9 — 2026-07-19 — The thinking trail becomes an instrument trace

- **Activity ledger redesign.** The boxed grey "Thinking process" panel is now
  a quiet timeline: a hairline rail with small state nodes (indigo running,
  green done, red failed), tool steps in a single calm voice, and measured
  durations in mono/tabular (0.4s, 1.2s) stamped as each step completes —
  precision as the aesthetic.
- **Motion that means something.** The generic spinning circle is gone; the
  live step breathes with a single accent dot (1.4s ease), new steps slide in
  over 220ms, and reduced-motion turns all of it off. State is signaled once,
  on the rail node, instead of three times (colored icon + tick + word).
- Screen-reader step status preserved via visually-hidden text.

## v0.9.8 — 2026-07-19 — Blocked sources get a fallback, not a shrug

- **The analyst no longer gives up on bot-protected websites.** When a page
  blocks automated readers (tesla.com and most large corporate sites do), the
  tool result now carries the fallback playbook — research synthesis, SEC
  filings (DEF 14A proxy / 10-K executive-officer sections for management
  questions), news — and the analyst is instructed to try them immediately
  instead of asking permission to continue.
- **Source-fallback doctrine in the system prompt.** Blocked, empty, or
  unavailable sources trigger the next-best source automatically, matching how
  a human analyst works.
- Tip: configuring the Roam browser (Settings → Connections → "Roam MCP
  command") lets read_page drive a real browser through bot protection.

## v0.9.7 — 2026-07-19 — The interface grows up

- **Quieter, more professional visual register.** The oversized centered
  greeting becomes a calm left-aligned workbench opener; capsule pills across
  the app (example chips, mission chip, badges, skill states) become quiet
  rectangles; the New chat button trades its saturated block fill for a soft
  indigo that fills on hover; the composer and message bubbles tighten their
  corners. Surfaces that don't float no longer cast shadows, per the design
  system's Overlay-Only rule.
- **No more emoji glyphs.** The memory pin badge is a proper vector glyph.

## v0.9.6 — 2026-07-19 — Three bases, credit metrics, and a roomier run budget

- **Quarterly and LTM bases.** `get_financials` now takes `basis`:
  `annual` (default), `quarterly` (last 8 fiscal quarters, with Q4 derived
  as FY − Q1..Q3 and marked), or `ltm` (trailing twelve months — the real
  comps basis, stitched FY + interim − prior interim with staleness guards).
- **Credit metrics in the spread.** Interest expense, D&A, and short-term debt
  join the annual spread; EBITDA, total debt, leverage (debt/EBITDA), interest
  coverage, and net cash/(debt) are pre-computed deterministically. Discontinued
  XBRL tags no longer shadow current ones (most-recent-data tag wins).
- **Segment routing.** Segment revenue/profit tables live in the 10-K item 8
  segment note (not XBRL company facts); the filing reader and skills now route
  segment questions there explicitly.
- **Run budget raised to 10 rounds** (from 8) for interactive turns — multi-company
  questions kept binding on rounds even with the one-call spread.

## v0.9.5 — 2026-07-19 — get_financials becomes a real analyst spread

- **Multi-year spread in one call.** `get_financials` now returns up to 6
  fiscal years (default 3) of annual data: income statement, balance sheet
  (cash, total assets, long-term debt, equity), cash flow (CFO, capex), diluted
  EPS, 10-K cover-page shares outstanding, and weighted-average diluted shares.
  Restatements are handled correctly — the latest filing wins per period.
- **Derived metrics computed by the app, not the model.** Revenue growth YoY,
  gross/operating/net margins, free cash flow, and net cash/(debt) are
  calculated deterministically from the reported figures and handed to the
  analyst pre-computed — eliminating LLM arithmetic as an error source and
  cutting a 3-question tool hunt to a single call within the round budget.
- **Spread card.** The financials card renders the full multi-year table with
  per-year columns; derived rows are visually set apart. Older single-year
  cards in past conversations still render.

## v0.9.4 — 2026-07-19 — Budget stops end with an answer, and shares outstanding is a first-class figure

- **A budget-limited turn now wraps up instead of dying.** When a run exhausts
  its step or token budget mid-task, the analyst gets one final no-tools
  synthesis pass to answer from the evidence it already gathered — then the run
  still ends as "Budget reached" (partial). Previously the run stopped cold
  after its last tool call, leaving the question unanswered. Time-budget
  exhaustion still stops immediately.
- **No more raw JSON in chat.** The fallback message for a turn that ended
  without streaming text used to print the internal stop payload verbatim
  (`{"detail":"rounds","kind":"budget"}`). Every terminal now renders a human
  sentence telling you what happened and what to do next.
- **get_financials now reports share counts.** Shares outstanding from the
  10-K cover page (dei: EntityCommonStockSharesOutstanding — the exact
  disclosed number, e.g. Tesla FY2025: 3,752,431,984), with the balance-sheet
  count as fallback, plus weighted-average diluted shares as a separate labeled
  row so the two are never conflated. Asking "how many shares does Tesla have?"
  is now a one-tool answer instead of a filing hunt.

## v0.9.3 — 2026-07-19 — Settings gets sections, skills get an editor

- **Settings is now sectioned.** The settings dialog is organized into four
  tabs — **General · Connections · Memory · Skills** — with the same keyboard
  vocabulary as the evidence dock (←/→/Home/End move between tabs). No more
  one long scroll mixing API keys with skill playbooks; the dialog is wider
  (780px) so editing surfaces have room.
- **Skills are viewable and editable.** Every skill row now has an Edit button
  that opens the full SKILL.md in an inline editor — view the playbook, change
  the steps, save. Renaming a skill in its frontmatter moves the file. Rows
  show how often the analyst has used each skill, and stale/archived skills
  wear a proper badge. "Save as skill" drafts from chat now land directly on
  the Skills section.
- **Design context committed.** PRODUCT.md and DESIGN.md now codify the app's
  design system ("The Patient Analyst": warm neutrals, one indigo accent,
  hairlines, mono numbers) so future features are held to a written spec.

## v0.9.2 — 2026-07-19 — The analyst ships with a skill library

- **13 built-in skills.** The skill library is no longer empty: the app now
  bundles eight investment-banking / financial-analysis playbooks
  (`dcf-valuation`, `comparable-companies`, `precedent-transactions`,
  `earnings-analysis`, `ma-accretion-dilution`, `lbo-screen`,
  `company-profile`, `credit-analysis`) and five workflow skills
  (`planner`, `orchestrator`, `task-executor`, `reviewer`,
  `verification-loop`). Each is grounded in the analyst's real tools and
  demands cited, recomputed numbers. They seed into the skills folder once at
  first launch — your edits are never overwritten and deleting one is sticky.
- **Skill usage now actually counts.** Using a skill via `use_skill` records
  the use (use count + last-used), so skills you rely on no longer age out to
  stale/archived after 30/90 days, and the Settings list shows real use counts.
  Hand-dropped skill files are auto-registered on first use.

## v0.9.1 — 2026-07-19 — The Evidence dock replaces the analyst pop-up

- **Evidence dock.** The right-side reader is now a tabbed deal binder —
  **Model · Valuation · Sources · Artifacts · Reader**. It docks beside the
  analyst on wide screens, slides in as an overlay on laptops, and becomes a
  bottom sheet / full-screen drawer on smaller windows — always without a
  horizontal scrollbar or a hidden composer. Toggle it with Ctrl/⌘+J, jump to a
  tab with Ctrl/⌘+1–5, and move between tabs with the arrow keys.
- **Model tools moved into the dock.** The EV bridge, IFRS bridge, and tie-out
  tools — previously a separate pop-up — now live in the dock's Model tab, so
  your evidence and tools sit beside the conversation instead of covering it. The
  old Analyst-tools modal is gone.
- **Keyboard + accessibility.** Every dock action is keyboard-reachable, focus
  returns to whatever opened the dock, plan steps are arrow-navigable, status
  stays glyph-plus-text, and reduced-motion is honored.

## v0.9.0 — 2026-07-18 — The analyst shows its plan, checks its own math, and can be paused

- **Live mission plan + status header.** When the analyst runs a structured
  workflow (e.g. "do an earnings review for NVDA"), the steps now appear as a live
  checklist — each turns from pending to running to done as the work actually
  happens — and a status header shows the workflow, current phase, step progress
  (e.g. "5/5 steps"), and the verification result at a glance. Watch the mission
  progress instead of staring at a spinner. Verified live end-to-end.
- **Numbers cross-checked, not just echoed.** Verification now recomputes an
  accounting identity (gross profit = revenue − cost of revenue) from the reported
  figures; if they don't reconcile, the run is marked partial instead of showing a
  green "verified" badge. A consistent NVIDIA income statement still verifies 6/6.
- **Steadier long conversations.** Older turns are assembled and compacted through
  one consistent context builder, and a request that would overflow the model's
  context is pruned and retried once before failing visibly.
- **Resilience + housekeeping.** Transient provider hiccups (rate limits, brief
  outages) get one automatic retry; long-unused saved skills age out of the default
  set (still restorable) so the assistant's toolbox stays relevant.
- **Cleaner sidebar.** Removed the confusing "Personal" dropdown (it had nothing to
  switch to — use Projects to group chats). Conversation rows are now clean single
  lines — the full title with a compact time on the right, and edit/move/delete
  appear on hover — instead of cramped, mid-word-clipped two-line blocks. The
  "Temporary chat" toggle stays for chats you don't want saved.
- **"Move to project" reads and moves correctly.** The picker now opens preselected
  to the chat's current project (so a chat already in a folder shows that folder, not
  a misleading "No project"), and when you have no projects yet it says so instead of
  offering an empty, dead-end menu. Clicking away closes the picker cleanly.
- **Pause a run and pick it back up.** During a run the command bar now shows a
  Pause button next to Stop: Pause ends the run at the next safe checkpoint as a
  *resumable* interrupt (distinct from Stop, which is final), and a "Resume" action
  then relaunches it from that checkpoint without redoing completed work.
- **Sharper UI colours.** Fixed a set of interface elements (activity rows, task
  badges, workspace/memory banners, approval cards) that were silently rendering
  with the wrong colour because their style variable was never defined — they now
  use the correct light/dark theme colours.

294 lib + 130 UI + engine/research gates green. (Behind the scenes: result cards now
flow on one durable event path, several agent capabilities moved from tested-but-dormant
to live; a few large items — the rest of the UI event-path cutover, scheduled follow-ups,
and the signed installer — remain.)

### Also in this cycle — Verified numbers, live

The analyst now shows its work on the figures it reports:

- **Reported financials render as a table.** A `get_financials` result used to
  show only the bare word "financials" (the card had no renderer). It now renders
  the company, fiscal year, and every line item — revenue, cost of revenue, gross
  profit, operating income, net income, diluted EPS — with a SEC EDGAR link.
  Verified live: NVIDIA FY2024 renders the full income statement.
- **A verification badge on material numbers.** Financial turns now run a
  verification pass over the figures pulled from SEC EDGAR and show a
  **Verified N/N** card ("6 of 6 material figures verified against SEC EDGAR
  XBRL"). Every reported number is extracted as a source-tagged claim and checked
  against its filing value before the run is badged. Verified live end-to-end.
  (This proves each number is sourced; catching a *restated* figure via
  independent recompute is the next step.)
- **Fix: wrong fiscal-year label.** A comparative figure shown inside a later
  10-K is tagged by SEC with the *later* filing's fiscal year, so NVIDIA's
  FY2024 numbers were labelled "FY2026". The card now labels by the period the
  figures actually cover — **FY2024 · period ended 2024-01-28**.

290 lib + 127 UI + engine/research gates green.

## v0.8.6 — Skills (drop-in playbooks + self-evolution)

A decentralized skills system, in the SKILL.md format (agentskills.io-compatible):

- **Drop-in skills.** A skill is a Markdown file (`<config>/skills/<name>.md`) with
  YAML frontmatter (`name` + `description`) and a body of steps. Manage them in
  Settings → Skills (add / view / delete), or drop files in by hand.
- **Discovery + progressive disclosure.** The catalog (names + descriptions) is
  injected into the system prompt; when a request matches, the agent calls the new
  `use_skill` tool to load that skill's full steps and follow them — so a growing
  library never bloats the prompt. Verified live: a saved skill fired on a matching
  question and its steps were followed.
- **Self-evolution.** After a multi-step turn, a "Save as skill" action asks the
  model to abstract what it just did into a reusable, generalized SKILL.md draft
  (specifics like tickers/years turned into instructions); you review and save it.
  Runs through your configured provider. Verified live: produced a valid, generalized
  draft from a two-company comparison.

208+ backend (223 lib) + 116 UI green.

## v0.8.5 — Design polish

A craft pass on the interface (no feature changes), guided by a product-register
design review:

- **Commanding hero.** The empty-state headline now uses a proper display scale
  with tight tracking, so the first screen reads intentional, not plain.
- **Consistent iconography.** Replaced the ad-hoc emoji (folders, gear, move,
  per-tool thinking-step glyphs, parallel-fan-out) with crisp mono line-SVGs that
  match the existing icon set — the biggest single "looks unpolished" fix.
- **Refined surfaces.** Softer off-white canvas (no pure #fff), elevated composer
  with a rounded field + focus ring, refined suggestion chips with a subtle hover
  lift, and a shared ease-out motion curve for calmer, premium transitions.

Verified in light and dark themes. 217 lib + 116 UI green.

## v0.8.4 — Project folders

Group related chats into projects, each with its own shared context:

- **Project folders in the sidebar.** A **New project** button next to New chat;
  conversations nest under collapsible folders (loose chats stay ungrouped). Move
  any chat into a project from its 📁 action.
- **Project settings & grounding.** The ⚙ on a folder opens a modal to name the
  project and set **system instructions** that apply to *every* chat in it —
  e.g. "Benchmark Tesla against Ford," "Report revenue in USD billions." Stored
  as an editable `projects/<id>/finmodel.md`, chained after your global rules.
- **Project dashboard.** Opening a folder shows a center view: the project name,
  its chats, and **+ New chat in project** (which starts a chat already grounded
  in that project's rules from its first message).

Backed by a `project_id` column on conversations + a `projects` table (schema v2,
auto-migrated). Verified live: a project's grounding was applied inside its chats
and absent in loose ones. 217 lib + 47 fetch + 116 UI green.

## v0.8.3 — Grounding layers (personalization + project rules)

Two configuration layers are now chained onto the system prompt before every
turn, so the analyst carries standing context automatically:

- **Global personalization** (`config.json` in the app config dir): user-level
  rules applied to *every* chat — e.g. "Always format tables in Markdown,"
  "Prefer revenue shown in USD," "Keep responses concise." Set/read via the
  `grounding_set_global` / `grounding_get_global` commands.
- **Project workspace grounding** (`workspaces/<id>/finmodel.md`, falling back to
  `claude.md`): rules unique to one project folder — e.g. "Benchmark NVDA against
  AMD/INTC," "Data source: 2025 10-K." Applied right after the global layer for
  chats in that workspace.
- **Real-time "Thinking process" trace.** Each turn now shows a collapsible panel
  logging every tool step live — icon + active label ("Fetching financials…") and
  a status that flips from "In progress" to ✓ Success / ✗ Failed — with the result
  cards below and a step count. It auto-collapses when the turn finishes, so you can
  watch the agent work in real time and re-open the trace afterward.

Order is always `base prompt → global → project` (a project refines, never
silently contradicts, your global preferences). Workspace ids are validated
against path traversal before any file read/write. Verified live: a global rule
made the model prefix its reply exactly as instructed. 217 lib + 116 UI green.

## v0.8.2 — Watch the subagents work

- **Live task tray for parallel work.** When the analyst fans out independent
  lookups (e.g. per-company financials), each one is now a real child subagent
  (`SubagentPool`) and shows as its own live row in the task tray — "get_financials
  · AAPL", "· MSFT", "· GOOGL" — running, then clearing as each finishes. Combined
  with the fan-out banner from v0.8.1, you can both see the concurrency and track
  each unit of work. Verified live: a three-company revenue + net-income compare
  spawned three subagents in one wave and answered with the full table.

This completes milestone 4 (subagent fan-out surfaced + task tray). Remaining on
the roadmap: automatic (unattended) memory capture, still gated on its precision
dataset. 208 lib + 116 UI green.

## v0.8.1 — See the parallel work

- **Fan-out is now visible.** When the analyst runs several independent lookups
  at once (e.g. per-company financials), the transcript shows a live "Running N
  tasks in parallel…" banner that resolves to "⚡ N tasks ran in parallel", so
  you can see the concurrency instead of just a stack of tool rows. Verified
  live: a three-company revenue comparison (Apple / Microsoft / Google 2025) ran
  all three `get_financials` calls in one wave and reported the ranking.

Still a stretch: a dedicated task tray for *child-subagent* runs (separate agent
turns), and automatic memory capture (pending its precision dataset). 208 lib +
114 UI green.

## v0.8.0 — More like an analyst you talk to

Agentic-experience upgrades toward "talking to a capable analyst":

- **Multi-step follow-through.** Ask a compound question ("compare Apple and
  Microsoft 2025 — revenue and net income, who earns more") and the agent runs
  every needed tool and delivers the full comparison + verdict, instead of doing
  one step and asking "want me to continue?". Verified live.
- **Live progress.** The status line names what the agent is doing —
  "Fetching financials…", "Searching the web…", "Writing the answer…",
  "Checking the figures…" — per tool and phase.
- **Parallel tool fan-out.** Independent calls (e.g. per-company financials) can
  run in one turn and execute concurrently (tool-capable models).
- **Memory drawer.** Settings → Saved memories lists what the analyst remembers
  and lets you delete anything it got wrong (verified: list + delete, DB-backed).
- Prompt now asks the agent to state a one-line plan before multi-step work
  (honored by stronger models; concise models may skip it — the progress stream
  still shows the steps).

Not yet: automatic (unattended) memory capture stays off pending its precision
dataset; full subagent orchestration UI is a future milestone. 208 lib + 114 UI green.

## v0.7.2 — Any provider, full income statement

- **Bring your own provider.** Settings now has a **Provider** dropdown — use
  your own key with OpenRouter (default), OpenAI, xAI/Grok, Anthropic, Google
  Gemini, DeepSeek, Groq, Mistral, Together, Fireworks, Cerebras, Moonshot, or
  any custom OpenAI-compatible endpoint. The chat stream, capability probe, and
  model list all follow the configured provider; existing OpenRouter users are
  unaffected. (No subscription/OAuth logins — own-key only, so no ToS/account
  risk.)
- **Full income statement** from `get_financials`: revenue, cost of revenue,
  gross profit, operating income, net income, and diluted EPS — pulled from SEC
  XBRL with confirmed tag coverage (e.g. TSLA FY2025: revenue $94.83B, gross
  profit $17.09B, operating income $4.36B, net income $3.79B, EPS $1.08).

## v0.7.1 — Just answer the number (get_financials)

Asking "what were Tesla's 2025 sales" made the app read *risk factors*, decide
the figure was "undisclosed," and punt with "want me to build a model?" — when
the exact number is in the filing. Root cause: the agent could only *read prose*
or *build a model*; it had no way to just fetch a reported figure, so it flailed.

New **`get_financials`** tool pulls exact annual figures (revenue/sales, net
income, gross profit, operating income, diluted EPS) straight from SEC EDGAR
XBRL company facts — deterministic and citable, not scraped prose. The system
prompt now routes reported-figure questions here and tells the assistant to
answer the number directly. Verified live in the app: *"What were Tesla's sales
for 2025?"* → **"Tesla's sales (revenue) for fiscal year 2025 were $94.83
billion, according to its annual report filed with the SEC."** (US filers; for
foreign filers it still routes to build_model.) 208 backend tests green.

## v0.7.0 — Memory, faster tools, sharper routing, smoother UI

Five improvements, all verified live in the running app:

- **Memory is now a real feature.** Say `remember: <fact>` (or `note:`, `save to
  memory:`) and it's saved to the workspace (secrets/paths/questions rejected by
  a precision gate); a "Memory saved · N" pill confirms it. Later turns recall
  relevant notes via scoped full-text search and use them in the answer —
  verified: after saving "I prefer revenue in USD millions", a later revenue
  question answered in USD millions unprompted. Automatic (unattended) capture
  stays off pending its quality gate; this is explicit manual save + recall.
- **Parallel tool calls.** Independent read-only tools (e.g. a peer set's
  per-ticker fetches) now run concurrently — capped at 4 in flight — instead of
  one-at-a-time, cutting latency on multi-tool turns.
- **Sharper tool routing.** A question for a specific reported figure
  (revenue/sales, net income, EPS) now routes to research (cited) or a model
  build instead of scraping narrative filing sections — the exact failure from
  the earlier "Tesla 2025 sales" turn. Verified: it now runs research and builds
  a real TSLA model.
- **Live auto-scroll.** The transcript follows a streaming response instead of
  freezing after the first big chunk; scroll up to read and it releases, return
  to the bottom and it re-engages.
- **UI polish.** Elevated composer focus ring, defined message bubbles, a
  clearer "Memory saved" pill, and improved reading rhythm — refining the
  existing editorial-finance aesthetic (not a redesign).

Engine reuse: parallel execution, compact tool summaries, and durable event
patterns draw on the concepts studied from Oh My Pi and Grok Build (reimplemented
in Rust/JS, no upstream code). 208 backend + 114 UI tests green.

## v0.6.1 — Fix: reading 10-K filings

`read_filing` (e.g. "what were Tesla's 2025 sales from the annual report") kept
returning "Item 7/8 not available" or "not yet filed" for filings that plainly
exist. Cause: the filing fetcher reused the web-article text extractor, which
only reads `<h*>/<p>/<li>` and stops after 20 KB — but real 10-Ks lay their
sections out in `<div>/<span>/<table>`, with Item 7/8 sitting megabytes into the
document, so no item was ever found. Filings now use a dedicated extractor that
reads the whole document (including tables) with section headings preserved.
Verified live: Tesla's 10-K now yields every item (1–16, including the MD&A and
the financial statements). Web search / read-page are unaffected.

## v0.6.0 — Agentic analyst engine (unified agent loop, live)

First shipped release on the rebuilt engine. The desktop app now runs entirely
on the unified, workspace-scoped `agent_send` loop: streaming turns, tool
calling (build models, trading comps, research with citations, quotes, filings),
multi-turn memory, structured result cards, Approve/Deny on risky actions, and a
no-key demo fallback. Conversations are SQLite-backed (list/load/rename/delete);
model tool-capability is auto-detected on save. The legacy keyed/routed JSON
chat engine has been fully removed (not just disabled) — ~2400 lines of dead
code deleted, clean build, 205 backend + 114 UI tests green. See the Phase A–G
entries below for the full rebuild history.

## Pre-v0.6.0 — Agentic analyst cutover (Phases A–B: contracts, SQLite, unified actor loop)

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

### Phase G — legacy source deleted (dead-code cutover complete)

With the runtime cutover verified, the now-unreachable legacy source was
removed from `commands/chat.rs` (3900 → 1620 lines): the `chat_send`/
`chat_cancel`/`chat_send_blocking` commands, the LLM turn loop, intent routing
(`route_intent` + the `Intent` enum), JSON persistence (`Conversation`/
`ChatMsg` + `read/write_conversation`), the research/fallback turn helpers, and
the test-only `validate_tool_args` island (the `*Args` structs, `require_*`,
`dec_err`, `error_card`). The genuinely-shared `emit`/`emit_chat` (used by the
agent's streaming path) were kept. `StreamOutcome::Partial` was collapsed into
`Failed` so a mid-stream network failure surfaces as `Err` instead of returning
truncated content as a completed answer. Cleared incidental pre-existing
warnings (duplicate `#[test]`, unused `mem_db`, reserved `ok` validator,
unenforced `SubagentPool.budget` doc). Build is clean; 205 lib + 114 UI tests
green. Only the signed installer + 7-day rollback rehearsal remain (need the
minisign key).

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

Remaining before the release tag: only the signed installer + 7-day rollback
rehearsal (needs the minisign key). The legacy-source deletion is now done (see
the dead-code cutover entry above); the runtime cutover was already complete.

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
