import json
from pathlib import Path

import pytest

_REPO = Path(__file__).parent.parent
_BASELINE = _REPO / "tieout" / "results" / "_baseline_wave0.json"


@pytest.mark.skipif(not _BASELINE.exists(),
                    reason="no frozen Wave0 baseline committed")
def test_existing_industrials_do_not_regress():
    """Every company measured in the frozen baseline must still match at
    least as many cells after the sector-aware refactor. Industrial GT is
    immutable and value-identical, so matched/trusted must not drop."""
    base = json.loads(_BASELINE.read_text(encoding="utf-8"))
    cur_path = _REPO / "tieout" / "results" / "_summary.json"
    assert cur_path.exists(), "run `python -m tieout.run_tieout` first"
    cur = json.loads(cur_path.read_text(encoding="utf-8"))
    for tk, b in base["companies"].items():
        assert tk in cur["companies"], f"{tk} dropped from measured set"
        c = cur["companies"][tk]
        assert c["trusted"] == b["trusted"], (
            f"{tk} trusted-cell count changed {b['trusted']}->{c['trusted']} "
            f"(industrial GT must be immutable/value-identical)")
        assert c["matched"] >= b["matched"], (
            f"{tk} regressed: matched {b['matched']}->{c['matched']}")
