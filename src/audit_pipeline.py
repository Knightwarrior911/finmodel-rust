"""Audit pipeline — attach CellProvenance to an extraction cache, render snapshots,
and post-process an existing 3-statement xlsx to add per-cell hyperlinks to those
snapshots.

Public API:
    attach_provenance_to_cache(cache_path, pdf_path) -> int
    generate_snapshots_for_cache(cache_path, out_dir, ticker) -> dict
    annotate_workbook_with_snapshots(xlsx_path, ticker, snap_index) -> int
    run_audit(ticker, *, pdf_path=None, models_dir=Path("models"),
              cache_dir=Path("extraction_cache"),
              snapshots_dir=Path("snapshots")) -> dict

The audit pass is non-invasive: writers and extractors are NOT modified. Provenance
is computed by re-reading the source PDF with PyMuPDF and locating each cached
numeric value via normalize_variants().
"""
from __future__ import annotations

import json
import re
from pathlib import Path
from typing import Any, Iterable, Optional

from .provenance import (
    CellProvenance,
    locate_value_in_pdf,
    provenance_dict,
)
from .snapshot import render_snapshot


# Statement keys we instrument
_STATEMENT_KEYS = ("income_statement", "balance_sheet", "cash_flow_statement")


def _years(cache: dict[str, Any]) -> list[str]:
    ys = cache.get("years_found") or []
    return [str(y) for y in ys]


def _ticker_from_cache_name(cache_path: Path) -> str:
    name = cache_path.stem  # e.g. "ATCO-B_ST"
    return name.replace("_", "-")


def attach_provenance_to_cache(cache_path: str | Path, pdf_path: str | Path) -> int:
    """Locate every numeric value in the cache inside the PDF; write provenance
    back into the cache JSON under `__provenance__`.

    Returns the number of values for which a bbox was found.
    """
    import fitz

    cache_path = Path(cache_path)
    pdf_path = Path(pdf_path)
    cache = json.loads(cache_path.read_text(encoding="utf-8"))
    years = _years(cache)

    doc = fitz.open(str(pdf_path))
    try:
        provenances: list[CellProvenance] = []
        # Per-statement page hint: once we find one value on page N, future
        # lookups in the same statement search page N first (huge speedup —
        # consolidated IS/BS/CFS values cluster on 3-5 pages).
        stmt_hint: dict[str, int] = {}
        for stmt in _STATEMENT_KEYS:
            block = cache.get(stmt) or {}
            for key, values in block.items():
                if not isinstance(values, list):
                    continue
                for idx, val in enumerate(values):
                    if val is None:
                        continue
                    period = years[idx] if idx < len(years) else str(idx)
                    page_idx, bbox, raw = locate_value_in_pdf(
                        doc, val, page_hint=stmt_hint.get(stmt),
                    )
                    if page_idx is not None and stmt not in stmt_hint:
                        stmt_hint[stmt] = page_idx
                    if page_idx is None:
                        provenances.append(CellProvenance(
                            pdf_path=str(pdf_path),
                            page_index=0,
                            bbox=None,
                            raw_text=str(val),
                            label=_humanize(key),
                            key=f"{stmt}.{key}",
                            period=period,
                            low_confidence=True,
                        ))
                    else:
                        provenances.append(CellProvenance(
                            pdf_path=str(pdf_path),
                            page_index=page_idx,
                            bbox=bbox,
                            raw_text=raw or str(val),
                            label=_humanize(key),
                            key=f"{stmt}.{key}",
                            period=period,
                            low_confidence=False,
                        ))
    finally:
        doc.close()

    cache["__provenance__"] = provenance_dict(provenances)
    cache_path.write_text(json.dumps(cache, indent=2), encoding="utf-8")

    return sum(1 for p in provenances if not p.low_confidence)


def _humanize(key: str) -> str:
    return key.replace("_", " ").title()


def generate_snapshots_for_cache(
    cache_path: str | Path,
    out_dir: str | Path,
    ticker: str,
) -> dict[tuple[str, str], Path]:
    """For every CellProvenance with bbox in the cache, render a PNG snapshot.

    Returns a map {(key, period): png_path}.
    """
    cache_path = Path(cache_path)
    cache = json.loads(cache_path.read_text(encoding="utf-8"))
    prov_block = cache.get("__provenance__") or {}

    out: dict[tuple[str, str], Path] = {}
    for full_key, bucket in prov_block.items():
        if not isinstance(bucket, dict):
            continue
        for period, payload in bucket.items():
            if not isinstance(payload, dict):
                continue
            if payload.get("low_confidence"):
                continue
            if payload.get("bbox") is None:
                continue
            prov = CellProvenance.from_json(payload)
            try:
                png = render_snapshot(prov, out_dir, ticker=ticker)
            except FileNotFoundError:
                continue
            out[(full_key, period)] = png
    return out


# Cell-value matching for openpyxl post-pass
_NUM_RE = re.compile(r"^-?\d{1,3}(?:[, ]\d{3})*(?:\.\d+)?$")


def annotate_workbook_with_snapshots(
    xlsx_path: str | Path,
    snap_index: dict[tuple[str, str], Path],
    *,
    cache_path: Optional[str | Path] = None,
) -> int:
    """Open an xlsx, find every cell whose numeric value matches a provenance value,
    and attach an Excel hyperlink to the corresponding snapshot PNG.

    Returns the number of cells annotated.
    """
    import openpyxl

    if cache_path is None:
        return 0
    cache = json.loads(Path(cache_path).read_text(encoding="utf-8"))
    prov_block = cache.get("__provenance__") or {}

    # Build value → list of (key, period, png) lookup
    value_index: dict[float, list[tuple[str, str, Path]]] = {}
    for full_key, bucket in prov_block.items():
        if not isinstance(bucket, dict):
            continue
        for period, payload in bucket.items():
            if not isinstance(payload, dict):
                continue
            png = snap_index.get((full_key, period))
            if png is None:
                continue
            try:
                # Pull the numeric value from the cache statements
                stmt, key = full_key.split(".", 1)
                arr = cache.get(stmt, {}).get(key, [])
                years = _years(cache)
                if period in years:
                    val = arr[years.index(period)]
                else:
                    try:
                        val = arr[int(period)]
                    except Exception:
                        val = None
                if val is None:
                    continue
                value_index.setdefault(float(val), []).append((full_key, period, png))
            except Exception:
                continue

    wb = openpyxl.load_workbook(str(xlsx_path))
    annotated = 0
    for ws in wb.worksheets:
        for row in ws.iter_rows():
            for cell in row:
                v = cell.value
                if isinstance(v, (int, float)) and not isinstance(v, bool):
                    matches = value_index.get(float(v))
                    if matches:
                        full_key, period, png = matches[0]
                        cell.hyperlink = str(png)
                        cell.comment = openpyxl.comments.Comment(
                            f"Source: {full_key} {period}\n{png.name}",
                            "audit",
                        )
                        annotated += 1
    wb.save(str(xlsx_path))
    return annotated


def run_audit(
    ticker: str,
    *,
    pdf_path: Optional[str | Path] = None,
    xlsx_path: Optional[str | Path] = None,
    models_dir: str | Path = "models",
    cache_dir: str | Path = "extraction_cache",
    snapshots_dir: str | Path = "snapshots",
) -> dict[str, Any]:
    """Full audit pass for a ticker.

    1. Locate cache JSON (extraction_cache/{TICKER}.json with dot→underscore)
    2. Locate PDF (explicit pdf_path arg, else search extraction_cache/)
    3. Attach provenance to cache
    4. Render snapshots
    5. If xlsx_path provided, annotate workbook
    """
    cache_dir = Path(cache_dir)
    models_dir = Path(models_dir)
    snap_root = Path(snapshots_dir)

    cache_name = ticker.replace(".", "_").replace("-", "_") + ".json"
    cache_path = cache_dir / cache_name
    if not cache_path.exists():
        # try with hyphens preserved
        cache_path = cache_dir / (ticker.replace(".", "_") + ".json")
    if not cache_path.exists():
        return {"ok": False, "error": f"cache not found for {ticker}"}

    if pdf_path is None:
        # Auto-discover any pdf in extraction_cache that mentions the ticker stem
        stems = ticker.replace(".", "_").replace("-", "_").split("_")
        for cand in cache_dir.glob("*.pdf"):
            if any(s.lower() in cand.name.lower() for s in stems if len(s) > 2):
                pdf_path = cand
                break
    if pdf_path is None or not Path(pdf_path).exists():
        return {"ok": False, "error": f"PDF not found for {ticker}", "cache_path": str(cache_path)}

    n_located = attach_provenance_to_cache(cache_path, pdf_path)
    snap_index = generate_snapshots_for_cache(cache_path, snap_root, ticker)

    result: dict[str, Any] = {
        "ok": True,
        "ticker": ticker,
        "cache_path": str(cache_path),
        "pdf_path": str(pdf_path),
        "values_located": n_located,
        "snapshots_rendered": len(snap_index),
        "snapshots_dir": str(snap_root / ticker),
    }

    if xlsx_path:
        result["annotated_cells"] = annotate_workbook_with_snapshots(
            xlsx_path, snap_index, cache_path=cache_path,
        )
        result["xlsx_path"] = str(xlsx_path)

    return result
