import json
from pathlib import Path

import pytest

from tieout.run_tieout import _scope_fingerprint

_REPO = Path(__file__).parent.parent
_BASELINE = _REPO / "tieout" / "results" / "_baseline_wave0.json"
_SUMMARY = _REPO / "tieout" / "results" / "_summary.json"


@pytest.mark.skipif(not _BASELINE.exists(),
                    reason="no frozen Wave0 baseline committed (local dev "
                           "only — the committed oracle must not be deleted)")
def test_existing_basket_does_not_regress():
    """No-regression guard for every company in the frozen pre-Wave0
    baseline. All 7 basket names use the industrial schema (config sector
    tag), and their ground truth is immutable + value-identical across the
    sector-aware refactor, so per-company trusted counts must be exactly
    equal and matched must not drop.

    The fingerprint check makes a stale _summary.json a hard failure
    instead of a false PASS: _summary.json must have been produced by the
    CURRENT source (same scope fingerprint the gate caches on)."""
    assert _SUMMARY.exists(), (
        "tieout/results/_summary.json missing — run "
        "`python -m tieout.run_tieout` first")
    base = json.loads(_BASELINE.read_text(encoding="utf-8"))
    cur = json.loads(_SUMMARY.read_text(encoding="utf-8"))

    assert cur.get("fingerprint") == _scope_fingerprint(), (
        "tieout/results/_summary.json is stale — its fingerprint "
        f"{cur.get('fingerprint')!r} != current source fingerprint "
        f"{_scope_fingerprint()!r}. Re-run `python -m tieout.run_tieout` "
        "before this test.")

    for ticker, base_co in base["companies"].items():
        assert ticker in cur["companies"], (
            f"{ticker} dropped from the measured set")
        cur_co = cur["companies"][ticker]
        assert cur_co["trusted"] == base_co["trusted"], (
            f"{ticker} trusted-cell count changed "
            f"{base_co['trusted']}->{cur_co['trusted']} "
            f"(ground truth must be immutable / value-identical)")
        assert cur_co["matched"] >= base_co["matched"], (
            f"{ticker} regressed: matched "
            f"{base_co['matched']}->{cur_co['matched']}")
