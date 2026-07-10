# finmodel — Demo & Smoke-Test Guide

The Rust engine turns a ticker into a projected 3-statement Excel model.

## 2-minute offline smoke test (no key, no network)

From `finmodel-core/`:

```
cargo run --bin fm-cli -- build SAND.ST
```

Expected output:

```
Build pipeline for SAND.ST
  [offline] loaded committed fixture: fm-cli/tests/fixtures/SAND_ST_model.json
  extracted: IS(12) BS(11) CFS(6) across 2 years in SEK
  reconcile -> project...
  assumptions: 13 drivers
  ✓ wrote Excel model -> SAND_ST_model.xlsx
  ✓ wrote projection -> SAND_ST_projection.json
```

Open `SAND_ST_model.xlsx` — 3 sheets (Income Statement, Balance Sheet, Cash Flow),
historical 2023–2024 + projected 2025E–2029E.

**Offline demo tickers** (committed real data): `SAND.ST`, `ASML.AS`, `NOVO-B.CO`,
`NESN.SW`, `ATCO-B.ST`.

## Live run (real extraction from a fresh filing)

Requires an OpenRouter API key (get one at openrouter.ai).

```
# PowerShell
$env:OPENROUTER_API_KEY = "sk-or-..."
cargo run --bin fm-cli -- build AAPL          # US ticker (SEC EDGAR path)
```

- **US tickers** (e.g. `AAPL`, `MSFT`) → SEC EDGAR XBRL, fully live.
- **Non-US tickers** → currently fall back to the committed fixture (the native
  PDF→LLM path is built and validated but not yet wired into `build` — it needs a
  ticker→company-name map). To exercise it directly:
  `cargo run --example extract_sandvik -p fm-extract` (with the key set).

Model selection: `FINMODEL_LLM_MODEL` (default `anthropic/claude-sonnet-4`). The
live model catalog is fetched from OpenRouter — see `fm_extract::list_openrouter_models`.

## What's guaranteed safe

- With **no key**, baseline tickers produce a real Excel model from committed data.
- With a **key**, a non-US ticker that isn't in EDGAR **falls back to the real
  fixture** — it never fabricates placeholder numbers.
- One bad/malformed PDF cannot crash the process (extraction is panic-guarded).

## Status (2026-07-10)

- ✅ Engine (projections, DCF/WACC/comps), Excel writer, native PDF text extraction
  (pure Rust, no Python), PDF discovery, OpenRouter provider + live model listing.
- 🟡 Non-US live extraction: validated end-to-end up to the LLM call; not yet wired
  into `build`.
- ❌ Tauri desktop UI: not started (the OpenRouter key + model picker live here).
