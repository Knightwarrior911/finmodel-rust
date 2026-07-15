"""Oracle for the Phase-7 regex extractor port (fm-extract/src/report.rs).

Loads the REAL pure functions from src/research/browser_pipeline.py (stubbing
the browser-only imports so no anti-bot deps are needed), downloads the curated
pinned annual-report PDFs from tieout.config.BASKET, extracts + normalizes text
exactly as the Python `extract_text()` does, then runs the Python
`extract_financials()` to produce the golden JSON.

Commits, per company that downloads:
  tieout/groundtruth/report_text/<TICKER>.txt      (normalized filing text)
  tieout/groundtruth/report_extract/<TICKER>.json  (extract_financials golden)

The Rust parity test (report::tests::parity_vs_python_golden) reads the SAME
committed text, runs the Rust extractor, and tolerance-diffs field-by-field.

Run:  py -3 tieout/build_report_extract_oracle.py [TICKER ...]
Deterministic once the text fixtures are committed — re-running only refreshes
them from source. Discovery/download is NOT the thing under test.
"""
import dataclasses
import importlib.util
import json
import re
import sys
import types
from pathlib import Path

import fitz  # PyMuPDF
import requests

ROOT = Path(__file__).resolve().parent.parent
SRC = ROOT / "src" / "research" / "browser_pipeline.py"
TEXT_DIR = ROOT / "tieout" / "groundtruth" / "report_text"
GOLD_DIR = ROOT / "tieout" / "groundtruth" / "report_extract"

# Companies to pin for the gate (subset of the basket — direct consolidated PDFs).
DEFAULT_TICKERS = ["SAND.ST", "BAS.DE"]
YEAR = "2024"

_UA = {
    "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) "
    "AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0 Safari/537.36",
    "Accept": "application/pdf,application/octet-stream,*/*",
}


def _load_pipeline_module():
    """Load browser_pipeline.py with the browser-only imports stubbed out."""
    for name in (
        "src",
        "src.browser",
        "src.browser.session",
        "src.browser.navigation",
        "src.browser.extraction",
        "src.browser.llm_navigator",
    ):
        if name not in sys.modules:
            sys.modules[name] = types.ModuleType(name)
    # Attach the class names the module imports at top level.
    sys.modules["src.browser.session"].BrowserSession = object
    sys.modules["src.browser.navigation"].BrowserNav = object
    sys.modules["src.browser.extraction"].BrowserExtract = object
    sys.modules["src.browser.llm_navigator"].LLMNavigator = object

    spec = importlib.util.spec_from_file_location("_bp_pure", SRC)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def _normalize(text: str) -> str:
    """Byte-for-byte port of the normalization in `extract_text()`."""
    text = re.sub(r"(\d)[\u202f\xa0\u2009\u2007 ](\d{3})(?!\d)", r"\1,\2", text)
    text = re.sub(r"(\d)[\u202f\xa0\u2009\u2007 ](\d{3})(?!\d)", r"\1,\2", text)
    return text.replace("\u2212", "-")


def _download(url: str, dest: Path) -> bool:
    try:
        r = requests.get(url, headers=_UA, timeout=90, stream=True)
        r.raise_for_status()
        it = r.iter_content(65536)
        first = next(it, b"")
        if not first.startswith(b"%PDF"):
            print(f"  [dl] not a PDF: {url[:80]}", file=sys.stderr)
            return False
        with open(dest, "wb") as f:
            f.write(first)
            for chunk in it:
                f.write(chunk)
        if dest.stat().st_size < 200_000:
            dest.unlink(missing_ok=True)
            print("  [dl] too small", file=sys.stderr)
            return False
        return True
    except Exception as e:  # noqa: BLE001
        print(f"  [dl] failed: {e}", file=sys.stderr)
        return False


def main(tickers):
    from tieout.config import BASKET, ticker_filings_dir

    bp_mod = _load_pipeline_module()
    BrowserPipeline = bp_mod.BrowserPipeline
    bp = BrowserPipeline.__new__(BrowserPipeline)  # skip __init__ (browser glue)

    TEXT_DIR.mkdir(parents=True, exist_ok=True)
    GOLD_DIR.mkdir(parents=True, exist_ok=True)

    by_ticker = {r["ticker"]: r for r in BASKET}
    ok = []
    for t in tickers:
        row = by_ticker.get(t)
        if not row or not row.get("url"):
            print(f"{t}: no curated URL in BASKET; skipping", file=sys.stderr)
            continue
        safe = t.replace(".", "_")
        pdf_path = ticker_filings_dir(t) / "annual_report.pdf"
        if not (pdf_path.exists() and pdf_path.stat().st_size > 200_000):
            print(f"{t}: downloading {row['url'][:80]}", file=sys.stderr)
            if not _download(row["url"], pdf_path):
                print(f"{t}: DOWNLOAD FAILED (skipping)", file=sys.stderr)
                continue

        pdf = fitz.open(pdf_path)
        raw = "".join(p.get_text() for p in pdf)
        pages = pdf.page_count
        pdf.close()
        text = _normalize(raw)

        fin = bp.extract_financials(text, row["company"], YEAR, pdf_url="")
        golden = dataclasses.asdict(fin)

        (TEXT_DIR / f"{safe}.txt").write_text(text, encoding="utf-8", newline="")
        (GOLD_DIR / f"{safe}.json").write_text(
            json.dumps(golden, indent=2, ensure_ascii=False), encoding="utf-8", newline=""
        )
        nfound = sum(
            1
            for k, v in golden.items()
            if isinstance(v, float) and v is not None
        )
        print(
            f"{t}: OK ({pages}p, {len(text):,} chars, {nfound} numeric fields) "
            f"-> {safe}.txt / {safe}.json"
        )
        ok.append(t)

    print(f"\nPinned {len(ok)}/{len(tickers)}: {ok}")
    return 0 if ok else 1


if __name__ == "__main__":
    args = sys.argv[1:] or DEFAULT_TICKERS
    sys.exit(main(args))
