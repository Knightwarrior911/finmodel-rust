# Changelog


## Unreleased

### Added — research/benchmarking subsystem (filings → Excel)
- **Benchmark metric set (16 across 6 dimensions)**: Scale (revenue/EBITDA/net
  income), Growth (YoY + full-window revenue CAGR), Profitability (gross/EBITDA/
  net/FCF margin), Returns (ROE/ROA), Liquidity (current ratio), Leverage (net
  debt / net-debt-to-EBITDA / interest coverage) — all from filings, unit-tested.
- **`fm verify`** now filters snapshots structurally (`model_output` present &&
  not `*_full_*`), so the new gate oracles (adhoc / ev_bridge) never break it.
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
