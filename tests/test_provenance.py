"""Tests for src.provenance — value normalization + PDF search."""
from __future__ import annotations

from pathlib import Path

import pytest

from src.provenance import (
    CellProvenance,
    normalize_variants,
    locate_value_in_pdf,
    provenance_dict,
    load_provenance,
)


def test_normalize_variants_integer():
    v = normalize_variants(168343)
    # must include comma-grouped, space-grouped, plain
    assert "168,343" in v
    assert "168 343" in v
    assert "168343" in v


def test_normalize_variants_negative():
    v = normalize_variants(-168343)
    assert "(168,343)" in v
    assert "-168,343" in v


def test_normalize_variants_decimal():
    v = normalize_variants(1.5)
    assert any("1.5" == x for x in v) or any(x.endswith("1.5") for x in v)


def test_normalize_variants_none_and_invalid():
    assert normalize_variants(None) == []
    assert normalize_variants("not a number") == []


def test_cellprovenance_roundtrip():
    p = CellProvenance(
        pdf_path="x.pdf", page_index=3, bbox=(1.0, 2.0, 3.0, 4.0),
        raw_text="168,343", label="Revenue", key="income_statement.revenue",
        period="2023", low_confidence=False,
    )
    d = p.to_json()
    assert d["bbox"] == [1.0, 2.0, 3.0, 4.0]
    p2 = CellProvenance.from_json(d)
    assert p2 == p


def test_provenance_dict_nesting():
    a = CellProvenance("x", 0, (0, 0, 1, 1), "100", "Revenue",
                       "income_statement.revenue", "2023")
    b = CellProvenance("x", 1, (0, 0, 1, 1), "200", "Revenue",
                       "income_statement.revenue", "2024")
    d = provenance_dict([a, b])
    assert "income_statement.revenue" in d
    assert "2023" in d["income_statement.revenue"]
    assert "2024" in d["income_statement.revenue"]

    out = load_provenance(d)
    assert len(out) == 2


def _atco_pdf() -> Path:
    p = Path("extraction_cache/ATCO_B_2023_raw.pdf")
    if not p.exists():
        pytest.skip(f"ATCO PDF not present at {p}")
    return p


def test_locate_value_in_pdf_hits_real_atco_revenue():
    """ATCO-B.ST cache says 2023 revenue = 172,664 MSEK. PDF should contain it."""
    import fitz
    pdf = _atco_pdf()
    doc = fitz.open(str(pdf))
    try:
        page_idx, bbox, raw = locate_value_in_pdf(doc, 172664)
    finally:
        doc.close()
    assert page_idx is not None, "should locate 172,664 (revenue) in ATCO 2023 PDF"
    assert bbox is not None
    x0, y0, x1, y1 = bbox
    assert x1 > x0 and y1 > y0


def test_locate_value_in_pdf_miss_returns_none():
    """A value vanishingly unlikely to appear should miss cleanly."""
    import fitz
    pdf = _atco_pdf()
    doc = fitz.open(str(pdf))
    try:
        page_idx, bbox, raw = locate_value_in_pdf(doc, 99999999999, max_pages=5)
    finally:
        doc.close()
    assert page_idx is None
    assert bbox is None


# ── Whole-number matching (no substring false positives) ───────────────────

def _make_pdf(tmp_path, lines: list[str]):
    """Write a tiny one-page PDF with the given text lines. Returns path."""
    import fitz
    doc = fitz.open()
    page = doc.new_page(width=400, height=300)
    y = 50
    for ln in lines:
        page.insert_text((40, y), ln, fontsize=11)
        y += 24
    out = tmp_path / "tiny.pdf"
    doc.save(str(out))
    doc.close()
    return out


def test_locate_value_no_substring_false_positive(tmp_path):
    """Searching 7067 must NOT match inside 67067 (the original F20 bug).

    Page has 'Intangible assets 67 067' and 'R&D 7 067' on separate lines.
    Locating 7067 must return the bbox of the standalone 7 067, whose
    re-extracted text must normalize to exactly 7067 — not 67067.
    """
    import fitz
    pdf = _make_pdf(tmp_path, [
        "Intangible assets 67 067",
        "Research and development 7 067",
    ])
    doc = fitz.open(str(pdf))
    try:
        page_idx, bbox, raw = locate_value_in_pdf(doc, 7067)
        assert page_idx == 0
        assert bbox is not None
        # Re-extract text under the located bbox and assert it is the standalone number
        rect = fitz.Rect(*bbox)
        # widen slightly to capture full glyphs
        rect = fitz.Rect(rect.x0 - 1, rect.y0 - 1, rect.x1 + 1, rect.y1 + 1)
        text = doc[0].get_textbox(rect)
        digits = "".join(ch for ch in text if ch.isdigit())
        assert digits == "7067", f"located wrong number: text={text!r} digits={digits!r}"
    finally:
        doc.close()


def test_locate_value_nordic_space_grouping(tmp_path):
    """172 664 (regular-space grouped) must be locatable as 172664."""
    import fitz
    pdf = _make_pdf(tmp_path, ["Revenues 172 664"])
    doc = fitz.open(str(pdf))
    try:
        page_idx, bbox, raw = locate_value_in_pdf(doc, 172664)
        assert page_idx == 0
        assert bbox is not None
        rect = fitz.Rect(bbox[0] - 1, bbox[1] - 1, bbox[2] + 1, bbox[3] + 1)
        digits = "".join(ch for ch in doc[0].get_textbox(rect) if ch.isdigit())
        assert digits == "172664", f"got {digits!r}"
    finally:
        doc.close()


def test_locate_value_plain_number(tmp_path):
    """A non-grouped 67067 must be locatable and not collide with 7067."""
    import fitz
    pdf = _make_pdf(tmp_path, ["Goodwill 67067", "Other 7067"])
    doc = fitz.open(str(pdf))
    try:
        _, bbox7, _ = locate_value_in_pdf(doc, 7067)
        _, bbox67, _ = locate_value_in_pdf(doc, 67067)
        assert bbox7 is not None and bbox67 is not None
        d7 = "".join(c for c in doc[0].get_textbox(
            fitz.Rect(bbox7[0]-1, bbox7[1]-1, bbox7[2]+1, bbox7[3]+1)) if c.isdigit())
        d67 = "".join(c for c in doc[0].get_textbox(
            fitz.Rect(bbox67[0]-1, bbox67[1]-1, bbox67[2]+1, bbox67[3]+1)) if c.isdigit())
        assert d7 == "7067", d7
        assert d67 == "67067", d67
    finally:
        doc.close()
