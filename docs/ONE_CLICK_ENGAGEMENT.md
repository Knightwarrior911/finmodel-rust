# One-Click Engagement Flow

The engagement flow wraps the entire model pipeline into a single command,
packaging deliverables into a dated client folder.

## Usage

```bash
# Basic — model only
python scripts/run_engagement.py --ticker AAPL

# Specify a custom output root
python scripts/run_engagement.py --ticker MSFT --output-dir ./client_deliverables/

# With PowerPoint summary deck (requires python-pptx)
python scripts/run_engagement.py --ticker AAPL --deck
```

## What it does

1.  **Creates a dated client folder**

        engagements/<TICKER>/<YYYY-MM-DD>/

2.  **Runs the full pipeline** via `python -m src.cli` with `--direct` (skips
    LLM preflight so no API key is required for US-ticker lookups).

3.  **Packages deliverables** into the folder:

    | File | Description |
    |---|---|
    | `<TICKER>_model.xlsx` | The financial model workbook |
    | `<TICKER>_Summary.pptx` | PowerPoint summary deck (only with `--deck`) |
    | `branding.yaml` | Copy of the project branding palette |
    | `extraction_cache.json` | Raw extraction provenance cache |
    | `SOURCES.md` | Human-readable sources & assumptions appendix |

## Folder structure

```
engagements/
  AAPL/
    2026-07-10/
      AAPL_model.xlsx
      AAPL_model_Summary.pptx    (if --deck)
      branding.yaml
      extraction_cache.json
      SOURCES.md
  MSFT/
    2026-07-10/
      ...
```

## Notes

- The `--direct` flag is always passed to the pipeline, so it operates in
  orchestrator mode (no LLM required for US tickers). Reconciliation that
  needs an LLM will degrade gracefully.
- Deck generation (`--deck`) depends on `python-pptx` and may fail if the
  library is not installed. If it fails the engagement still succeeds; the
  error is printed but does not halt the flow.
- Each run creates a new timestamped folder. Previous runs for the same
  ticker are preserved under their own date.
