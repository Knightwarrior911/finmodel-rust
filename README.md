# finmodel

A virtual financial analyst: turns a ticker or filing into institutional-grade modeling, valuation, research, and presentation output. It builds an integrated **3-statement model**, runs **valuation and IFRS adjustments**, performs **autonomous research**, and produces both **Excel workbooks and PowerPoint decks** — driven directly or through a natural-language orchestrator.

Works for **US companies** (SEC EDGAR / XBRL) and **non-US / IFRS companies** (annual-report PDF extraction — no EDGAR coverage required).

## Capabilities

**Modeling & valuation**
- Integrated 3-statement model (IS / BS / CFS), formula-driven Excel, colour-coded inputs vs formulas vs cross-refs
- 3-scenario DCF, WACC build, trading comps, peer margin/trajectory comparisons
- Enterprise-value bridge and **IFRS adjustment bridges** (e.g. IFRS-16 leases, US-GAAP↔IFRS reconciliation)
- Quality layer: reconciliation, internal verifier, Excel validator, recalculation verification loop, value/format audits

**Research (autonomous)**
- SEC EDGAR + web + headed-browser pipeline for non-US filing discovery
- Market data, news, and M&A **deal synthesis**
- Ad-hoc research → structured Excel output (event logs, bridges, peer/trajectory sheets)

**Presentation**
- Generates PowerPoint decks from model/research output
- Full programmatic deck editing — ~40 tools: text/image edits, slide management, theme recolor, shape move/resize/align/distribute, fills/lines, textboxes, table column/row swap & move, emphasis, footnotes, vision-inspect + render-reflect

**Interfaces**
- Direct CLI per stage, a single-tool mode (`--tool`), and a natural-language **orchestrator** (`--ask`) that plans and chains the ~40 registered tools

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

# Model + PowerPoint deck
python -m src.cli --ticker ATCO-B.ST --deck

# Natural-language orchestrator (plans + chains the ~40 tools)
python -m src.cli --ask "Build a DCF on MSFT and a peer comp deck"

# Invoke a single tool directly (no LLM/API key needed)
python -m src.cli --tool run_public_comps --tool-args '{"ticker":"AAPL"}'
```

Output: `{TICKER}_model.xlsx` in the working directory (`.` → `_`, e.g. `ATCO-B.ST` → `ATCO-B_ST_model.xlsx`); decks and research sheets alongside it.

Useful flags: `--periods-historical/-projected`, `--no-dcf`, `--no-comps`, `--ir-url` (non-US filing page), `--direct` (EDGAR-only, no LLM), `--llm`, `--output`.

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
src/             extraction, reconciliation, engine, Excel writer, valuation
                 (dcf/wacc/comps/peers), EV & IFRS bridges, quality layer,
                 ~40-tool NL orchestrator
src/research/    autonomous research: SEC EDGAR, browser pipeline, market
                 data, news, M&A deal synthesis, IFRS/US-GAAP leases,
                 ad-hoc Excel output, PPTX build / edit / inspect / render
tieout/          independent filing-accuracy harness (immutable instrument)
tests/           pytest suite (131 tests)
config/          sector / assumption configuration
docs/            design notes
```

## Tests

```bash
python -m pytest tests/ -q          # 131 tests
```

## Not committed

`.env`, downloaded filings (`extraction_cache/`, `tieout/filings/`), generated workbooks (`*.xlsx`), runtime output (`tieout/results/`) and the iteration ledger are gitignored — copyrighted filings and large binaries stay out of the repo.
