"""Tests for src.audit_open — finmodelaudit: URI build/parse (no launch, no registry)."""
from __future__ import annotations

from src.audit_open import build_uri, parse_uri, SCHEME


def test_build_uri_has_no_hash(tmp_path):
    pdf = tmp_path / "ATCO 2023.pdf"
    pdf.write_bytes(b"%PDF-1.4")
    uri = build_uri(str(pdf), 3)
    assert uri.startswith(f"{SCHEME}:")
    assert "#" not in uri          # Excel splits hyperlinks on '#'; must avoid it
    assert "page=3" in uri


def test_build_parse_roundtrip(tmp_path):
    pdf = tmp_path / "x.pdf"
    pdf.write_bytes(b"%PDF-1.4")
    uri = build_uri(str(pdf), 47)
    path, page = parse_uri(uri)
    assert path == str(pdf.resolve())
    assert page == 47


def test_build_uri_none_page_is_one(tmp_path):
    pdf = tmp_path / "x.pdf"
    pdf.write_bytes(b"%PDF-1.4")
    _, page = parse_uri(build_uri(str(pdf), None))
    assert page == 1


def test_parse_uri_tolerates_authority(tmp_path):
    pdf = tmp_path / "x.pdf"
    pdf.write_bytes(b"%PDF-1.4")
    uri = build_uri(str(pdf), 5).replace(f"{SCHEME}:", f"{SCHEME}://")
    path, page = parse_uri(uri)
    assert path == str(pdf.resolve())
    assert page == 5
