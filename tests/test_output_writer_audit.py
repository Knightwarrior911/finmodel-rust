"""Tests for ResearchExcelWriter._audit_url — page-accurate bridge source links."""
from __future__ import annotations

from src.research.output_writer import ResearchExcelWriter


def _make_pdf(tmp_path, lines):
    import fitz
    doc = fitz.open()
    page = doc.new_page(width=400, height=300)
    y = 50
    for ln in lines:
        page.insert_text((40, y), ln, fontsize=11)
        y += 24
    out = tmp_path / "filing.pdf"
    doc.save(str(out))
    doc.close()
    return out


def test_audit_url_page_accurate(tmp_path):
    pdf = _make_pdf(tmp_path, ["Revenue 172 664", "Other 1 234"])
    w = ResearchExcelWriter(output_dir=str(tmp_path))
    w._source_pdf = str(pdf)
    link = w._audit_url(172664)
    assert link.startswith("finmodelaudit:")
    assert "page=1" in link          # single-page pdf -> page 1
    assert "filing.pdf" in link


def test_audit_url_empty_without_source():
    w = ResearchExcelWriter(output_dir=".")
    assert w._audit_url(172664) == ""


def test_audit_url_zero_and_none_skip(tmp_path):
    pdf = _make_pdf(tmp_path, ["X 5"])
    w = ResearchExcelWriter(output_dir=str(tmp_path))
    w._source_pdf = str(pdf)
    assert w._audit_url(0) == ""
    assert w._audit_url(None) == ""
