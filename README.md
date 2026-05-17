# finmodel

Builds an integrated **3-statement financial model** (Income Statement, Balance Sheet, Cash Flow) plus DCF valuation and trading comps from a company's primary filings, and emits a formula-driven Excel workbook.

Works for **US companies** (via SEC EDGAR) and **non-US / IFRS companies** (via annual-report PDF extraction — no EDGAR coverage required).

---

## What it does

```
filing (SEC XBRL or annual-report PDF)
        │
        ▼
  extract  →  reconcile  →  engine  →  Excel writer  →  {TICKER}_model.xlsx
        │                                   │
        │                                   ├── DCF (3 scenarios)
        │                                   └── trading comps
        │
        └── verified by the tie-out harness (tieout/) — every historical
            number must match the source filing exactly
```

- **Extraction** (`src/extractor.py`, `src/fetcher.py`) — pulls IS/BS/CFS line items. US: SEC XBRL. Non-US: locates and reads the consolidated face statements out of the annual-report PDF.
- **Reconciliation** (`src/reconciler.py`) — merges statement data with footnote detail, cross-checks internal consistency.
- **Engine** (`src/engine.py`) — projects forward, builds the linked 3-statement model.
- **Writer** (`src/writer.py`) — emits a formula-driven Excel workbook (Rogo-standard layout, colour-coded inputs/formulas/cross-refs).
- **Valuation** (`src/dcf.py`, `src/public_comps.py`, `src/wacc.py`) — 3-scenario DCF, trading comps, EV bridge.
- **Orchestrator** (`src/orchestrator.py`) — natural-language entry point (`--ask`).

## Quick start

```bash
pip install -r requirements.txt

# US company (SEC EDGAR, no API key needed)
python -m src.cli --ticker AAPL

# Non-US / IFRS company (reads the annual-report PDF)
python -m src.cli --ticker ATCO-B.ST

# Supply a local filing directly
python -m src.cli --ticker NESN.SW --filing /path/to/report.pdf
```

Output: `{TICKER}_model.xlsx` in the working directory (`.` → `_`, e.g. `ATCO-B.ST` → `ATCO-B_ST_model.xlsx`).

### LLM provider

Extraction uses one of, in priority order:
1. `DEEPSEEK_API_KEY` — DeepSeek (cheapest)
2. `ANTHROPIC_API_KEY` — Anthropic SDK
3. neither set — the `claude` CLI subprocess (uses the active Claude Code session, no key)

Override the model with `FINMODEL_LLM_MODEL`. Put keys in a local `.env` (gitignored).

## Accuracy: the tie-out harness (`tieout/`)

The internal verifiers only check *self-consistency* (the balance sheet balances, cash flow ties). They do **not** check that an extracted number equals the number printed in the filing. The `tieout/` package is an independent measuring instrument that does exactly that.

For each company it:
1. Pins one source annual-report PDF (`tieout/filings/<TICKER>/`).
2. Builds an **independent ground-truth answer key** — two decorrelated `claude` transcription passes; a cell is trusted only if both agree (`tieout/groundtruth/<TICKER>.json`). Column-aware PDF rendering handles side-by-side IFRS statements.
3. Runs the real extractor on the same PDF and compares **every** historical IS/BS/CFS cell, exact integer at reporting unit.

```bash
python -m tieout.run_tieout            # full basket
python -m tieout.run_tieout --only ATCO-B.ST
# → tieout/results/_report.md  (per-line model vs filing, with page refs)
```

The harness is deliberately isolated from the extraction code so the accuracy metric cannot be gamed.

### Result

Non-US extraction was hardened from a **67% → 100%** exact filing tie-out across a 5-company European basket (Atlas Copco, Sandvik, ASML, Nestlé, Novo Nordisk — SEK/EUR/CHF/DKK, 256/256 historical cells). Five structural root causes were fixed in `src/extractor.py`:

1. The section selector anchored on the table of contents instead of the real statements (fatal for large reports).
2. The three face statements are non-contiguous in big filings — each must be located independently.
3. IFRS `net_income` must be total profit incl. non-controlling interests, not the attributable-to-parent sub-line.
4. `sga` must exclude distribution/logistics (those are COGS-type).
5. The LLM occasionally wraps JSON in prose — the parser now salvages the object.

See `autoresearch-results.tsv` (local, gitignored) for the iteration ledger.

### Known limitations

- Integrated-report layouts (e.g. SAP) can defeat the statement finder.
- Image/scanned PDFs (no extractable text) are unsupported.
- Banks/insurers (different statement structure), unusual label wording, and currency/number formats not yet seen may need verification.
- New companies should be run through the tie-out harness to prove accuracy rather than assumed correct.

## Project layout

```
src/            extraction, reconciliation, engine, writer, valuation, orchestrator
tieout/         independent filing-accuracy harness (immutable measuring instrument)
tests/          pytest suite (131 tests)
config/         sector / assumption configuration
docs/           design notes
```

## Tests

```bash
python -m pytest tests/ -q          # 131 tests
```

## Not committed

`.env`, downloaded filings (`extraction_cache/`, `tieout/filings/`), generated workbooks (`*.xlsx`), runtime output (`tieout/results/`) and the iteration ledger are gitignored — copyrighted filings and large binaries stay out of the repo.
