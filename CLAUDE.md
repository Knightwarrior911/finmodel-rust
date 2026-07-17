# Finmodel — Financial Model Engine

## HANDOVER — v0.7.1 get_financials, LIVE RELEASE (current, 2026-07-17)
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
