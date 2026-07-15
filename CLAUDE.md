# Finmodel — Financial Model Engine

## HANDOVER — v0.4.0 sellable-feature expansion (built 2026-07-15, unreleased)
**Branch `master`.** v0.3.1 remains the LIVE release; v0.4.0 is code-complete and
green (`finmodel-core` workspace tests + `src-tauri` tests pass) but NOT yet
built/signed/shipped — do the signed NSIS build per `docs/RELEASE_CHECKLIST.md` §6
only after `df -h /c` shows >6G free and the user okays shipping. Seven independent
workstreams landed (all flag/opt-gated, defaults unchanged so every parity oracle
stays byte-identical):
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

## HANDOVER — Chat-first desktop redesign, v0.3.1 LIVE (current, 2026-07-15)
**Branch `master`.** Source `finmodel-rust` PRIVATE; releases → PUBLIC
`finmodel-releases`. **v0.3.1 is the live release** (updater endpoint
`…/finmodel-releases/releases/latest/download/latest.json` verified serving
0.3.1). Disk volatile: `df -h /c` before any `cargo`; a signed release build
needs >6G free — reclaim with `rm -rf src-tauri/target/debug finmodel-core/target/debug`.

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
