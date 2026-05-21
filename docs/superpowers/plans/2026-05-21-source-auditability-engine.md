# Source Auditability Engine (Plan #1) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement task-by-task. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Replace pre-rendered PNG snapshots with write-time `file#page=N` hyperlinks on the 3-statement model, retiring `snapshot.py`. Proves the universal-auditability approach end-to-end.

**Architecture:** Keep the existing page locator (`src/provenance.py`). Add a pure `audit_link` helper that turns a located page into a `file:///…#page=N` URI. Repurpose the audit post-pass (`src/audit_pipeline.py`) to attach those links to numeric cells instead of rendering/annotating PNGs.

**Tech Stack:** Python, PyMuPDF (locator, unchanged), openpyxl (cell hyperlinks), pytest.

**Scope:** 3-statement Excel only. Comps/DCF/IFRS-bridge/PPTX are follow-on plans reusing `audit_link`.

---

### Task 1: `audit_link` helper

**Files:**
- Create: `src/audit_link.py`
- Test: `tests/test_audit_link.py`

- [ ] **Step 1: Write failing tests**

```python
# tests/test_audit_link.py
from pathlib import Path
from src.audit_link import make_audit_link

def test_pdf_page_link_is_file_uri_with_page(tmp_path):
    pdf = tmp_path / "ATCO_2023.pdf"; pdf.write_bytes(b"%PDF-1.4")
    link = make_audit_link(str(pdf), page_index=46)   # 0-based -> #page=47
    assert link.startswith("file:///")
    assert link.endswith("#page=47")
    assert "ATCO_2023.pdf" in link

def test_pdf_bare_link_when_no_page(tmp_path):
    pdf = tmp_path / "x.pdf"; pdf.write_bytes(b"%PDF-1.4")
    link = make_audit_link(str(pdf), page_index=None)
    assert link.startswith("file:///")
    assert "#page=" not in link

def test_market_data_returns_url():
    link = make_audit_link(None, page_index=None, url="https://finance.example/AAPL")
    assert link == "https://finance.example/AAPL"

def test_no_source_returns_none():
    assert make_audit_link(None, page_index=None) is None
```

- [ ] **Step 2: Run, expect fail** — `pytest tests/test_audit_link.py -q` → ImportError.

- [ ] **Step 3: Implement**

```python
# src/audit_link.py
"""Build click-to-source hyperlink strings (no rendering, no files).

A located PDF citation becomes a file:///…#page=N URI that modern viewers
(Edge default on Win11, Adobe, browsers) open at the right page. Non-PDF
sources (market data) pass through their URL. Page is the contract; bbox is
recorded elsewhere for a future highlight and not used here.
"""
from __future__ import annotations
from pathlib import Path
from typing import Optional


def make_audit_link(
    pdf_path: Optional[str],
    *,
    page_index: Optional[int] = None,
    url: Optional[str] = None,
) -> Optional[str]:
    """Return a hyperlink string for a sourced number, or None.

    - pdf_path + page_index -> file:///abs/doc.pdf#page=(page_index+1)
    - pdf_path only         -> file:///abs/doc.pdf  (opens page 1)
    - url (market_data)     -> url unchanged
    - nothing               -> None
    """
    if pdf_path:
        uri = Path(pdf_path).resolve().as_uri()   # file:///C:/...
        if page_index is not None and page_index >= 0:
            return f"{uri}#page={page_index + 1}"
        return uri
    if url:
        return url
    return None
```

- [ ] **Step 4: Run, expect pass** — `pytest tests/test_audit_link.py -q`.
- [ ] **Step 5: Commit** — `git add src/audit_link.py tests/test_audit_link.py && git commit -m "feat(audit): file#page hyperlink builder"`.

---

### Task 2: Repurpose audit pipeline to link source pages

**Files:**
- Modify: `src/audit_pipeline.py` (drop snapshot generation; replace `annotate_workbook_with_snapshots` with `annotate_workbook_with_links`; rewrite `run_audit`)
- Modify: `tests/test_audit_pipeline.py`

Keep `attach_provenance_to_cache` unchanged (it already records `page_index` per value). Replace the PNG steps.

- [ ] **Step 1: Rewrite the annotator** — replace `generate_snapshots_for_cache` + `annotate_workbook_with_snapshots` with:

```python
def annotate_workbook_with_links(
    xlsx_path, *, cache_path,
) -> dict[str, int]:
    """Attach a file#page hyperlink to each numeric cell whose value matches a
    filing-sourced provenance value. Label-aware disambiguation (unchanged)."""
    import openpyxl
    from .audit_link import make_audit_link

    cache = json.loads(Path(cache_path).read_text(encoding="utf-8"))
    prov_block = cache.get("__provenance__") or {}
    years = _years(cache)

    value_index: dict[float, list[dict]] = {}
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
                "full_key": full_key, "period": period,
                "pdf": payload.get("pdf_path") or "",
                "page_index": payload.get("page_index"),
                "low_confidence": bool(payload.get("low_confidence")),
                "label_tokens": _label_tokens(payload.get("label", "")),
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
                row_tokens = _label_tokens(_row_label(ws, cell))
                best = max(cands, key=lambda c: len(c["label_tokens"] & row_tokens))
                if len(cands) > 1 and not (best["label_tokens"] & row_tokens):
                    continue
                if not best["pdf"] or not Path(best["pdf"]).exists():
                    continue
                page_idx = None if best["low_confidence"] else best["page_index"]
                link = make_audit_link(best["pdf"], page_index=page_idx)
                if not link:
                    continue
                cell.hyperlink = link
                where = f"page {page_idx + 1}" if page_idx is not None else "source doc"
                cell.comment = openpyxl.comments.Comment(
                    f"Source: {best['full_key']} {best['period']} ({where})", "audit")
                if page_idx is not None:
                    linked_page += 1
                else:
                    linked_doc += 1
    wb.save(str(xlsx_path))
    return {"linked_page": linked_page, "linked_doc": linked_doc,
            "total": linked_page + linked_doc}
```

- [ ] **Step 2: Rewrite `run_audit`** — drop `snapshots_dir`, snapshot rendering, and snapshot stats; call `annotate_workbook_with_links` when `xlsx_path` given. Keep PDF discovery + coverage stats (`values_total/located/low_confidence/coverage_pct/located_by_period/missing_period_pdfs`). Replace `snapshots_rendered`/`snapshots_dir` keys with `annotated` dict from the linker.

- [ ] **Step 3: Update imports** — remove `from .snapshot import render_snapshot`.

- [ ] **Step 4: Update tests** — in `tests/test_audit_pipeline.py`: drop `generate_snapshots_for_cache` + `test_generate_snapshots_for_cache`; rewrite `test_annotate…` to assert `cell.hyperlink.target` contains `#page=` and starts with `file:///`; rewrite `test_run_audit_end_to_end` to assert `res["annotated"]["total"] > 0` (no snapshot keys).

- [ ] **Step 5: Run** — `pytest tests/test_audit_pipeline.py -q` → pass.
- [ ] **Step 6: Commit**.

---

### Task 3: CLI wording + call

**Files:** Modify `src/cli.py:401-427`, `:55-56`.

- [ ] **Step 1** — change `--audit` help to "attach file#page source hyperlinks to numeric cells (clickable audit trail)"; remove `--audit-pdf` reference to snapshots wording.
- [ ] **Step 2** — update the block: header text "Audit: linking source pages…"; call `run_audit(cfg.ticker, pdf_path=args.audit_pdf, xlsx_path=out_path)` (no snapshots_dir); print `located/total`, `coverage_pct`, and `annotated` page/doc counts; drop snapshot lines.
- [ ] **Step 3: Commit**.

---

### Task 4: Remove snapshot module

**Files:** Delete `src/snapshot.py`, `tests/test_snapshot.py`.

- [ ] **Step 1** — `git rm src/snapshot.py tests/test_snapshot.py`.
- [ ] **Step 2** — grep for stragglers: `grep -rn "snapshot" src/ tests/` → none except comments. Fix any.
- [ ] **Step 3: Commit**.

---

### Task 5: Full verification + ATCO demo

- [ ] **Step 1** — `pytest -q` → full suite green (snapshot tests gone; link tests in).
- [ ] **Step 2** — rebuild demo offline:
  `python -m src.cli --ticker ATCO-B.ST --no-dcf --no-comps` then run audit via `python -c "from src.audit_pipeline import run_audit; print(run_audit('ATCO-B.ST', xlsx_path='models/ATCO_B_ST_model.xlsx'))"`.
- [ ] **Step 3: Verify links** — open `models/ATCO_B_ST_model.xlsx` with openpyxl; assert ≥1 cell whose `hyperlink.target` matches `file:///…#page=\d+`; print 5 sample (cell, value, target). Confirm no `snapshots/` dir created.
- [ ] **Step 4** — report coverage numbers + sample links to the user (click-through is viewer-dependent; user verifies one click).

## Self-Review
- **Spec coverage:** engine (Citation→provenance reuse, audit_link), 3-statement wiring, PNG removal, honest caveats — covered. Comps/DCF/bridge/PPTX explicitly deferred to follow-on plans (spec rollout steps 3-6).
- **Placeholders:** none.
- **Type consistency:** `make_audit_link(pdf_path, *, page_index, url)` used identically in Task 1 and Task 2. `annotate_workbook_with_links` returns `{linked_page, linked_doc, total}` consumed by Task 2 run_audit + Task 3 CLI.
