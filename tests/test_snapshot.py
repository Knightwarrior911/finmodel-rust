"""Tests for src.snapshot — yellow highlight rendering."""
from __future__ import annotations

from pathlib import Path

import pytest

from src.provenance import CellProvenance, locate_value_in_pdf
from src.snapshot import render_snapshot


def _atco_pdf() -> Path:
    p = Path("extraction_cache/ATCO_B_2023_raw.pdf")
    if not p.exists():
        pytest.skip(f"ATCO PDF not present at {p}")
    return p


def _yellow_pixel_count(png_path: Path) -> int:
    """Count pixels that are predominantly yellow (R,G high, B low)."""
    from PIL import Image
    img = Image.open(png_path).convert("RGB")
    px = img.load()
    w, h = img.size
    n = 0
    for y in range(h):
        for x in range(w):
            r, g, b = px[x, y]
            if r > 200 and g > 200 and b < 150:
                n += 1
    return n


def test_render_snapshot_produces_png_with_yellow(tmp_path):
    """Render snapshot for ATCO revenue 172,664 → PNG with yellow pixels in bbox."""
    import fitz
    pdf = _atco_pdf()
    doc = fitz.open(str(pdf))
    try:
        page_idx, bbox, raw = locate_value_in_pdf(doc, 172664)
    finally:
        doc.close()
    assert page_idx is not None, "precondition: ATCO revenue must be locatable"

    prov = CellProvenance(
        pdf_path=str(pdf),
        page_index=page_idx,
        bbox=bbox,
        raw_text=raw or "172,664",
        label="Revenue",
        key="income_statement.revenue",
        period="2023",
    )
    out = render_snapshot(prov, tmp_path, ticker="ATCO-B.ST")
    assert out.exists()
    assert out.suffix == ".png"
    assert out.stat().st_size > 1000   # not empty

    yellow = _yellow_pixel_count(out)
    assert yellow > 50, f"expected >50 yellow pixels in highlight; got {yellow}"


def test_render_snapshot_low_confidence_no_highlight(tmp_path):
    """If bbox is None (low confidence), render page anyway, no yellow."""
    pdf = _atco_pdf()
    prov = CellProvenance(
        pdf_path=str(pdf),
        page_index=2,
        bbox=None,
        raw_text="N/A",
        label="Revenue",
        key="income_statement.revenue",
        period="2023",
        low_confidence=True,
    )
    out = render_snapshot(prov, tmp_path, ticker="ATCO-B.ST")
    assert out.exists()
    # No highlight → far fewer yellow pixels (white pages may have some near-yellow)
    yellow = _yellow_pixel_count(out)
    assert yellow < 5000, f"expected ~no yellow without bbox; got {yellow}"


def test_render_snapshot_missing_pdf_raises(tmp_path):
    prov = CellProvenance(
        pdf_path=str(tmp_path / "nope.pdf"),
        page_index=0, bbox=(0, 0, 10, 10),
        raw_text="x", label="x", key="x.x", period="2023",
    )
    with pytest.raises(FileNotFoundError):
        render_snapshot(prov, tmp_path, ticker="X")


def test_render_snapshot_page_index_out_of_range(tmp_path):
    pdf = _atco_pdf()
    prov = CellProvenance(
        pdf_path=str(pdf),
        page_index=99999, bbox=None,
        raw_text="x", label="x", key="x.x", period="2023",
    )
    with pytest.raises(IndexError):
        render_snapshot(prov, tmp_path, ticker="X")
