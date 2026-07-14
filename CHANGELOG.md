# Changelog


## Unreleased

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
