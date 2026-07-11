# finmodel-rust — Resume / Mission

**Repo split (2026-07-10):** The original Python lives in the separate `finmodel`
repo (github.com/Knightwarrior911/finmodel) and is PARKED — we do NOT touch it.
ALL work now happens here, in `finmodel-rust`
(github.com/Knightwarrior911/finmodel-rust), cloned locally at
`C:/Users/vinit/Documents/finmodel-rust`.

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
- ✅ `fm-excel` writer — **DONE**. Full port of `writer.py` (Cover/Assumptions/IS/BS/CF/Sources) into a cell-model (`model.rs`) + `rust_xlsxwriter` renderer (`render.rs`). New snapshot comparator (`snapshot.rs`) diffs cell-by-cell (value+formula+fill) vs `excel_snapshots`; **0 diffs across all 5 companies** (`tests/snapshot_parity.rs`), calamine round-trip proves render faithful (`tests/roundtrip.rs`). App + CLI rewired to the rich writer via `fm_build::build_workbook_input` + `fm_excel::render`. Old stub `writer.rs`/`compare.rs` removed.
- 🟡 Non-US live extraction (PDF→LLM) validated up to the LLM call but not wired into the app's `build_model` (needs a ticker→company-name map).

### Excel writer — known gaps (parity-complete; product follow-ups)
- ✅ **Number formats** added (`model.rs` FMT_* + `render.rs`; `tests/formats.rs`): drivers/rates render `0.0%`, monetary cells `#,##0`. Verified in `xl/styles.xml`. (Not in the snapshot gate — openpyxl doesn't capture number formats.)
- ✅ **IS body ported (standard sector).** `is_structure.rs` (`ISRow`/`build_standard_is`/`compute_is_row_map`) + full IS body in `sheets/is_stmt.rs` (revenue-growth-driven revenue, margin-driven COGS/GP, EBIT/EBITA/EBITDA buildup, interest→BS debt schedule, tax, EPS). Writer is **parameterized**: `WorkbookInput.is_structure` empty → header-only IS (committed-snapshot gate stays green); populated → full IS + BS/CF reference a **dynamic IS row-map** via `WorkbookInput::is_row()` (falls back to the empty-IS `IS_R` positions). App path (`fm_build`) now emits a full IS.
  - **Oracle + gate:** `tieout/build_full_is_oracle.py` runs the reference `src/` with a built `is_structure`, sourcing historicals from the committed snapshots' `model_output`, and commits `*_full_snapshot.json`. `tests/full_is_parity.rs` diffs the Rust IS/BS/CF against the oracle — **0 diffs across 4 companies** (SAND/ASML/ATCO/NOVO). NESN excluded: null `gross_profit` in its reconstructed historicals crashes the Python reference `_derive_assumptions` (oracle-gen only; Rust is unaffected).
  - **Sector coverage (done):** `build_is_structure(sector,…)` dispatches to `build_{standard,utility,bank,insurance,reit}_is`; `is_stmt.rs` handles the `utility_*` slot formulas; `assumptions.rs` relabels drivers for non-standard sectors. Gated by sector oracles (force each sector on SAND).
  - **XBRL detail (done):** `build_standard_is_detailed` handles revenue segments (`rev_seg_*`), detailed COGS (`cogs_seg_*`), and opex line items (`opex_*`, incl. extra items held-flat + subtracted into EBIT); `is_stmt.rs` emits the segment/sum formulas and the "REVENUE BREAKDOWN BY SEGMENT" memo block (`seg_*`); `apply_filing_labels` overrides labels from `notes.filing_labels`. `fm_build` parses `extraction.notes` (segments/opex/cogs_detail/filing_labels), replicates cli.py's cogs/rd/sga remap, and selects the detailed structure. Gated by a synthetic oracle `SAND_ST_xbrl_full_snapshot.json` (2 segments + cogs_detail + rd/sga + extra opex) — `tests/full_is_parity.rs::xbrl_detail_reproduces_oracle` = **0 diffs on IS/BS/CF**; `fm_build` wiring covered by `test_build_detailed_is_from_notes`. The **entire `is_builder.py` IS logic is now ported**; NESN's standard oracle remains excluded (null `gross_profit` crashes the Python reference during oracle-gen; Rust engine unaffected — its empty-IS committed snapshot still gates green).
- 🟡 **App market inputs are placeholders** — `fm_build::build_workbook_input` passes `risk_free_rate=0.045`, `current_share_price=0.0`, `company=ticker`, `fye="Dec"` (no live market/company-name feed).
- 🟡 **Formula cached results** not written; Excel recalcs on load (`fullCalcOnLoad=1`), but LibreOffice shows 0 in projected columns until manual recalc — add `Cell.cached` (from engine projections) if LibreOffice support is wanted.

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
