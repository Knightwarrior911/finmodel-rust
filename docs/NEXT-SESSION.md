# finmodel-rust — Resume / Mission

**Repo split (2026-07-10):** The original Python lives in the separate `finmodel`
repo (github.com/Knightwarrior911/finmodel) and is PARKED — we do NOT touch it.
ALL work now happens here, in `finmodel-rust`
(github.com/Knightwarrior911/finmodel-rust), cloned locally at
`C:/Users/vinit/Documents/finmodel-rust`.


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
  in `docs/RELEASE_CHECKLIST.md` §6. **Remaining to go live:** publish the first
  GitHub Release (upload `-setup.exe` + a `latest.json`); rebrand the pdf-panda
  placeholder icons in `src-tauri/icons/`.
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
