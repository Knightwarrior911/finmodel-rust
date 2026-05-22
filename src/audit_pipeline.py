"""Audit pipeline — attach CellProvenance to an extraction cache, then post-process
an existing 3-statement xlsx to add a `file#page` source hyperlink on each numeric
cell (click-to-source audit trail, no rendered images).

Public API:
    attach_provenance_to_cache(cache_path, pdf_path) -> int
    annotate_workbook_with_links(xlsx_path, *, cache_path) -> dict
    run_audit(ticker, *, pdf_path=None, xlsx_path=None,
              models_dir=Path("models"),
              cache_dir=Path("extraction_cache")) -> dict

The audit pass is non-invasive: writers and extractors are NOT modified. Provenance
is computed by re-reading the source PDF with PyMuPDF and locating each cached
numeric value via normalize_variants(). Located values link to their exact source
page (#page=N); page-unlocated values with a known source doc link to the doc.
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
from .audit_open import build_uri


# Statement keys we instrument
_STATEMENT_KEYS = ("income_statement", "balance_sheet", "cash_flow_statement")


def _years(cache: dict[str, Any]) -> list[str]:
    ys = cache.get("years_found") or []
    return [str(y) for y in ys]


def _ticker_from_cache_name(cache_path: Path) -> str:
    name = cache_path.stem  # e.g. "ATCO-B_ST"
    return name.replace("_", "-")


def attach_provenance_to_cache(
    cache_path: str | Path,
    pdf_path: str | Path | None = None,
    *,
    pdf_for_period: dict[str, str] | None = None,
) -> int:
    """Locate every numeric value in the cache inside its source PDF; write
    provenance back into the cache JSON under `__provenance__`.

    Each period (year) is searched in the PDF that actually contains it:
      - pdf_for_period maps "2023" -> path; falls back to pdf_path otherwise.
    A value not found in its period's PDF is stored low_confidence (no bbox)
    rather than risking a wrong-number match.

    Returns the count of values for which a bbox was located.
    """
    import fitz

    cache_path = Path(cache_path)
    cache = json.loads(cache_path.read_text(encoding="utf-8"))
    years = _years(cache)
    pdf_for_period = dict(pdf_for_period or {})
    default_pdf = str(pdf_path) if pdf_path else None

    # Open each distinct PDF once.
    doc_cache: dict[str, Any] = {}

    def _get_doc(path: str | None):
        if not path:
            return None
        if path not in doc_cache:
            try:
                doc_cache[path] = fitz.open(path)
            except Exception:
                doc_cache[path] = None
        return doc_cache[path]

    try:
        provenances: list[CellProvenance] = []
        # Per-(statement, pdf) page hint for speed.
        stmt_hint: dict[tuple[str, str], int] = {}
        for stmt in _STATEMENT_KEYS:
            block = cache.get(stmt) or {}
            for key, values in block.items():
                if not isinstance(values, list):
                    continue
                for idx, val in enumerate(values):
                    if val is None:
                        continue
                    period = years[idx] if idx < len(years) else str(idx)
                    mapped = pdf_for_period.get(period)
                    # Year-mapped mode: search ONLY the period's own report (a
                    # value can't legitimately appear in a different year's
                    # report, and value-only search across the wrong report
                    # yields coincidental false positives). Single-report mode
                    # (no year mapping): use the default PDF for all periods.
                    if pdf_for_period:
                        search_pdf = mapped
                    else:
                        search_pdf = default_pdf
                    doc = _get_doc(search_pdf)
                    page_idx = bbox = raw = None
                    if doc is not None:
                        hk = (stmt, search_pdf)
                        page_idx, bbox, raw = locate_value_in_pdf(
                            doc, val, page_hint=stmt_hint.get(hk),
                        )
                        if page_idx is not None and hk not in stmt_hint:
                            stmt_hint[hk] = page_idx
                    # Citation PDF: where it was found, else the period's mapped
                    # report. Never cite a fallback default for an UNMAPPED
                    # period (that would point at the wrong year's report).
                    if page_idx is not None:
                        cite_pdf = search_pdf
                    else:
                        cite_pdf = mapped or ""
                    provenances.append(CellProvenance(
                        pdf_path=cite_pdf or "",
                        page_index=page_idx if page_idx is not None else 0,
                        bbox=bbox,
                        raw_text=raw or str(val),
                        label=_humanize(key),
                        key=f"{stmt}.{key}",
                        period=period,
                        low_confidence=(page_idx is None),
                    ))
    finally:
        for d in doc_cache.values():
            if d is not None:
                d.close()

    cache["__provenance__"] = provenance_dict(provenances)
    cache_path.write_text(json.dumps(cache, indent=2), encoding="utf-8")

    return sum(1 for p in provenances if not p.low_confidence)


def _humanize(key: str) -> str:
    return key.replace("_", " ").title()


# Cell-value matching for openpyxl post-pass
_NUM_RE = re.compile(r"^-?\d{1,3}(?:[, ]\d{3})*(?:\.\d+)?$")


def _label_tokens(s: str) -> set[str]:
    """Lowercase alpha tokens (len>=3) for fuzzy label matching."""
    return {t for t in re.split(r"[^a-z]+", str(s).lower()) if len(t) >= 3}


def _row_label(ws, cell) -> str:
    """Find the line-item label for a numeric cell: the rightmost non-empty
    string cell to its left on the same row."""
    label = ""
    for c in range(1, cell.column):
        v = ws.cell(row=cell.row, column=c).value
        if isinstance(v, str) and v.strip():
            label = v.strip()
    return label


def annotate_workbook_with_links(
    xlsx_path: str | Path,
    *,
    cache_path: Optional[str | Path] = None,
) -> dict[str, int]:
    """Open an xlsx and, for each numeric cell whose value matches a filing-sourced
    provenance value, attach a `file#page` hyperlink to that number's source.

    Matching is LABEL-AWARE: when a value collides across line items, the cell's
    row label is used to disambiguate (token overlap with the provenance label).

    - Located value (page known)        -> finmodelaudit:page=N&path=<pdf>.
    - Page-unlocated value but source PDF exists -> finmodelaudit page 1.

    The finmodelaudit: scheme is used (not a raw file#page link) because Excel
    drops the #page fragment when the shell opens a local PDF; the registered
    handler (src/audit_open.py) re-launches the browser directly at the page.

    Returns {"linked_page": n, "linked_doc": n, "total": n}.
    """
    import openpyxl

    if cache_path is None:
        return {"linked_page": 0, "linked_doc": 0, "total": 0}
    cache = json.loads(Path(cache_path).read_text(encoding="utf-8"))
    prov_block = cache.get("__provenance__") or {}
    years = _years(cache)

    # value -> list of candidate dicts
    value_index: dict[float, list[dict[str, Any]]] = {}
    for full_key, bucket in prov_block.items():
        if not isinstance(bucket, dict):
            continue
        stmt, _, key = full_key.partition(".")
        for period, payload in bucket.items():
            if not isinstance(payload, dict):
                continue
            arr = cache.get(stmt, {}).get(key, [])
            try:
                val = arr[years.index(period)] if period in years else arr[int(period)]
            except Exception:
                val = None
            if val is None:
                continue
            value_index.setdefault(float(val), []).append({
                "full_key": full_key,
                "period": period,
                "pdf": payload.get("pdf_path") or "",
                "page_index": payload.get("page_index"),
                "label_tokens": _label_tokens(payload.get("label", "")),
                "low_confidence": bool(payload.get("low_confidence")),
            })

    wb = openpyxl.load_workbook(str(xlsx_path))
    linked_page = linked_doc = 0
    for ws in wb.worksheets:
        for row in ws.iter_rows():
            for cell in row:
                v = cell.value
                if not isinstance(v, (int, float)) or isinstance(v, bool):
                    continue
                cands = value_index.get(float(v))
                if not cands:
                    continue
                # Disambiguate by row-label token overlap
                row_tokens = _label_tokens(_row_label(ws, cell))
                best = max(
                    cands,
                    key=lambda c: len(c["label_tokens"] & row_tokens),
                )
                # Require a label match when multiple candidates collide, to
                # avoid linking a coincidental value (e.g. a computed cell).
                if len(cands) > 1 and not (best["label_tokens"] & row_tokens):
                    continue
                if not best["pdf"] or not Path(best["pdf"]).exists():
                    continue
                page_idx = None if best["low_confidence"] else best["page_index"]
                page_1based = (page_idx + 1) if page_idx is not None else None
                link = build_uri(best["pdf"], page_1based)
                cell.hyperlink = link
                where = f"page {page_idx + 1}" if page_idx is not None else "source doc"
                cell.comment = openpyxl.comments.Comment(
                    f"Source: {best['full_key']} {best['period']} ({where})",
                    "audit",
                )
                if page_idx is not None:
                    linked_page += 1
                else:
                    linked_doc += 1
    wb.save(str(xlsx_path))
    return {
        "linked_page": linked_page,
        "linked_doc": linked_doc,
        "total": linked_page + linked_doc,
    }


def run_audit(
    ticker: str,
    *,
    pdf_path: Optional[str | Path] = None,
    xlsx_path: Optional[str | Path] = None,
    models_dir: str | Path = "models",
    cache_dir: str | Path = "extraction_cache",
) -> dict[str, Any]:
    """Full audit pass for a ticker.

    1. Locate cache JSON (extraction_cache/{TICKER}.json with dot->underscore)
    2. Discover source PDFs per period (year in filename) + a default PDF
    3. Attach provenance to cache (each period searched in its own report)
    4. If xlsx_path provided, attach file#page source hyperlinks (label-aware)
    5. Report honest coverage (located / low_confidence / per-period)
    """
    cache_dir = Path(cache_dir)
    models_dir = Path(models_dir)

    cache_name = ticker.replace(".", "_").replace("-", "_") + ".json"
    cache_path = cache_dir / cache_name
    if not cache_path.exists():
        cache_path = cache_dir / (ticker.replace(".", "_") + ".json")
    if not cache_path.exists():
        return {"ok": False, "error": f"cache not found for {ticker}"}

    cache = json.loads(cache_path.read_text(encoding="utf-8"))
    years = _years(cache)

    # Default PDF: explicit arg, else first ticker-matching PDF in cache_dir
    if pdf_path is None:
        stems = ticker.replace(".", "_").replace("-", "_").split("_")
        for cand in cache_dir.glob("*.pdf"):
            if any(s.lower() in cand.name.lower() for s in stems if len(s) > 2):
                pdf_path = cand
                break

    # Per-period PDF: map each year to a PDF whose filename contains that year.
    # The 2023 report carries FY2023 (+FY2022); the 2025 report carries FY2025
    # (+FY2024) — so a value is only findable in the report that reports it.
    pdfs = list(cache_dir.glob("*.pdf"))
    pdf_for_period: dict[str, str] = {}
    for yr in years:
        # Primary: a report whose filename names this exact year.
        match = next((c for c in pdfs if yr in c.name), None)
        # Comparative fallback: the next year's report carries this year as its
        # prior-period comparative column (e.g. the 2025 report shows FY2024).
        if match is None and yr.isdigit():
            nxt = str(int(yr) + 1)
            match = next((c for c in pdfs if nxt in c.name), None)
        if match is not None:
            pdf_for_period[yr] = str(match)

    if pdf_path is None and not pdf_for_period:
        return {"ok": False, "error": f"PDF not found for {ticker}",
                "cache_path": str(cache_path)}

    n_located = attach_provenance_to_cache(
        cache_path, pdf_path, pdf_for_period=pdf_for_period,
    )

    # Coverage stats
    cache = json.loads(cache_path.read_text(encoding="utf-8"))
    prov = cache.get("__provenance__") or {}
    total = low = 0
    per_period_located: dict[str, int] = {}
    # In year-mapped mode, any period without its own report can't be linked.
    if pdf_for_period:
        missing_period_pdfs = [y for y in years if y not in pdf_for_period]
    else:
        missing_period_pdfs = [] if pdf_path else list(years)
    for _k, bucket in prov.items():
        for period, payload in bucket.items():
            total += 1
            if payload.get("low_confidence"):
                low += 1
            else:
                per_period_located[period] = per_period_located.get(period, 0) + 1

    result: dict[str, Any] = {
        "ok": True,
        "ticker": ticker,
        "cache_path": str(cache_path),
        "pdf_path": str(pdf_path) if pdf_path else None,
        "pdf_for_period": pdf_for_period,
        "values_total": total,
        "values_located": n_located,
        "values_low_confidence": low,
        "coverage_pct": round(100.0 * n_located / total, 1) if total else 0.0,
        "located_by_period": per_period_located,
        "missing_period_pdfs": missing_period_pdfs,
    }

    if xlsx_path:
        result["annotated"] = annotate_workbook_with_links(
            xlsx_path, cache_path=cache_path,
        )
        result["xlsx_path"] = str(xlsx_path)

    return result
