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
- 🔴 `fm-excel` writer — **STUB** (bare header+data dump). THIS is the mission.
- 🟡 Non-US live extraction (PDF→LLM) validated up to the LLM call but not wired into the app's `build_model` (needs a ticker→company-name map).

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
