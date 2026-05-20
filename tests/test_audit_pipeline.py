"""Tests for src.audit_pipeline — end-to-end snapshot + xlsx annotation."""
from __future__ import annotations

import json
import shutil
from pathlib import Path

import pytest

from src.audit_pipeline import (
    attach_provenance_to_cache,
    generate_snapshots_for_cache,
    annotate_workbook_with_snapshots,
    run_audit,
)


def _atco_pdf() -> Path:
    p = Path("extraction_cache/ATCO_B_2023_raw.pdf")
    if not p.exists():
        pytest.skip(f"ATCO PDF not present at {p}")
    return p


def _atco_cache() -> Path:
    p = Path("extraction_cache/ATCO-B_ST.json")
    if not p.exists():
        pytest.skip(f"ATCO cache not present at {p}")
    return p


def test_attach_provenance_to_cache(tmp_path):
    """Copy cache + run attach; assert __provenance__ key populated."""
    cache_src = _atco_cache()
    pdf = _atco_pdf()
    cache = tmp_path / "ATCO-B_ST.json"
    shutil.copy(cache_src, cache)

    n_located = attach_provenance_to_cache(cache, pdf)
    assert n_located > 0, "should locate at least one value"

    out = json.loads(cache.read_text(encoding="utf-8"))
    assert "__provenance__" in out
    assert "income_statement.revenue" in out["__provenance__"]


def test_generate_snapshots_for_cache(tmp_path):
    cache_src = _atco_cache()
    pdf = _atco_pdf()
    cache = tmp_path / "ATCO-B_ST.json"
    shutil.copy(cache_src, cache)
    attach_provenance_to_cache(cache, pdf)

    out_dir = tmp_path / "snapshots"
    snaps = generate_snapshots_for_cache(cache, out_dir, "ATCO-B.ST")
    assert len(snaps) > 0
    for (key, period), png in snaps.items():
        assert png.exists()
        assert png.stat().st_size > 1000


def test_annotate_workbook_with_snapshots(tmp_path):
    """Build a fake xlsx whose cell values match cached ATCO numbers,
    run annotate, assert hyperlinks added."""
    import openpyxl
    cache_src = _atco_cache()
    pdf = _atco_pdf()
    cache = tmp_path / "ATCO-B_ST.json"
    shutil.copy(cache_src, cache)
    attach_provenance_to_cache(cache, pdf)
    snaps = generate_snapshots_for_cache(cache, tmp_path / "snapshots", "ATCO-B.ST")
    if not snaps:
        pytest.skip("no snapshots rendered — bbox lookup miss")

    # Build a workbook with one of the located values
    sample_cache = json.loads(cache.read_text(encoding="utf-8"))
    rev = sample_cache["income_statement"]["revenue"][0]   # 2023 = 172664
    xlsx = tmp_path / "fake_model.xlsx"
    wb = openpyxl.Workbook()
    ws = wb.active
    ws["A1"] = "Revenue 2023"
    ws["B1"] = rev
    wb.save(str(xlsx))

    annotated = annotate_workbook_with_snapshots(xlsx, snaps, cache_path=cache)
    assert annotated >= 1

    wb2 = openpyxl.load_workbook(str(xlsx))
    cell = wb2.active["B1"]
    assert cell.hyperlink is not None
    assert cell.hyperlink.target.endswith(".png")


def test_run_audit_end_to_end(tmp_path):
    """Run full audit pipeline against ATCO-B.ST."""
    # Stage cache + pdf into tmp_path/extraction_cache/
    cache_dir = tmp_path / "extraction_cache"
    cache_dir.mkdir()
    shutil.copy(_atco_cache(), cache_dir / "ATCO-B_ST.json")
    shutil.copy(_atco_pdf(), cache_dir / "ATCO_B_2023_raw.pdf")

    res = run_audit(
        "ATCO-B.ST",
        cache_dir=cache_dir,
        snapshots_dir=tmp_path / "snapshots",
    )
    assert res["ok"] is True, res
    assert res["values_located"] > 0
    assert res["snapshots_rendered"] > 0
    snap_dir = Path(res["snapshots_dir"])
    assert snap_dir.exists()
    assert any(snap_dir.iterdir())


def test_run_audit_no_pdf_returns_error(tmp_path):
    cache_dir = tmp_path / "extraction_cache"
    cache_dir.mkdir()
    shutil.copy(_atco_cache(), cache_dir / "ATCO-B_ST.json")
    # NB: no PDF copied

    res = run_audit(
        "ATCO-B.ST",
        cache_dir=cache_dir,
        snapshots_dir=tmp_path / "snapshots",
    )
    assert res["ok"] is False
    assert "PDF not found" in res["error"]
