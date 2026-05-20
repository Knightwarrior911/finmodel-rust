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
