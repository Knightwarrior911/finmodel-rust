"""Tests for src.audit_link — file#page hyperlink builder."""
from __future__ import annotations

from src.audit_link import make_audit_link


def test_pdf_page_link_is_file_uri_with_page(tmp_path):
    pdf = tmp_path / "ATCO_2023.pdf"
    pdf.write_bytes(b"%PDF-1.4")
    link = make_audit_link(str(pdf), page_index=46)   # 0-based -> #page=47
    assert link.startswith("file:///")
    assert link.endswith("#page=47")
    assert "ATCO_2023.pdf" in link


def test_pdf_bare_link_when_no_page(tmp_path):
    pdf = tmp_path / "x.pdf"
    pdf.write_bytes(b"%PDF-1.4")
    link = make_audit_link(str(pdf), page_index=None)
    assert link.startswith("file:///")
    assert "#page=" not in link


def test_page_index_zero_is_page_one(tmp_path):
    pdf = tmp_path / "x.pdf"
    pdf.write_bytes(b"%PDF-1.4")
    link = make_audit_link(str(pdf), page_index=0)
    assert link.endswith("#page=1")


def test_market_data_returns_url():
    link = make_audit_link(None, page_index=None, url="https://finance.example/AAPL")
    assert link == "https://finance.example/AAPL"


def test_no_source_returns_none():
    assert make_audit_link(None, page_index=None) is None
