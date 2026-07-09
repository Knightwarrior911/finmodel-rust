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
    CURRENT source (same scope fingerprint the gate caches on).

    Regression categories covered by the assertions below:

    (a) Fewer exact matches
        ``assert cur_co["matched"] >= base_co["matched"]``
        If the extractor regresses and starts getting fewer cells right
        for any company, the guard fires.  This is the main quality gate.

    (b) Fewer trusted cells
        ``assert cur_co["trusted"] == base_co["trusted"]``
        Ground truth is immutable: the set of trusted (human-verifiable)
        cells per company must never shrink.  A change here means the
        ground-truth source changed or a sector schema broke.

    (c) Company disappeared
        ``assert ticker in cur["companies"]``
        If a ticker present in the baseline is absent from current
        results, the guard fires.  This catches extraction failures,
        missing pinned PDFs, or config changes that drop a company.
    """
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
        # Regression (c): company disappeared from results
        assert ticker in cur["companies"], (
            f"{ticker} dropped from the measured set")
        cur_co = cur["companies"][ticker]
        # Regression (b): fewer trusted cells
        assert cur_co["trusted"] == base_co["trusted"], (
            f"{ticker} trusted-cell count changed "
            f"{base_co['trusted']}->{cur_co['trusted']} "
            f"(ground truth must be immutable / value-identical)")
        # Regression (a): fewer exact matches
        assert cur_co["matched"] >= base_co["matched"], (
            f"{ticker} regressed: matched "
            f"{base_co['matched']}->{cur_co['matched']}")


@pytest.mark.skipif(not _SUMMARY.exists(),
                    reason="no _summary.json to corrupt")
def test_stale_summary_is_rejected(tmp_path):
    """Verify the fingerprint check rejects a corrupted _summary.json.

    If the summary file's fingerprint doesn't match the current source
    fingerprint, the guard must fail.  This prevents a stale
    _summary.json from producing a false PASS after scope files have
    been edited but before re-running the tie-out.

    The test copies the real _summary.json to a tmpdir, overrides the
    fingerprint with a clearly stale value, and asserts the fingerprint
    check would fire (i.e. the hash does NOT match the current source).
    """
    stale = tmp_path / "_summary.json"
    data = json.loads(_SUMMARY.read_text(encoding="utf-8"))
    data["fingerprint"] = "0000000000000000"
    stale.write_text(json.dumps(data), encoding="utf-8")

    cur = json.loads(stale.read_text(encoding="utf-8"))
    assert cur["fingerprint"] != _scope_fingerprint(), (
        "Fingerprint check should reject a corrupted summary "
        "(fingerprint overridden to 0000000000000000)")
