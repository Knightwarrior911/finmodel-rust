import json
from pathlib import Path
from src.source_ledger import SourceLedger
from src.orchestrator import VirtualAnalystOrchestrator


def test_finalize_appends_report():
    cache_dir = Path("extraction_cache"); cache_dir.mkdir(exist_ok=True)
    led = SourceLedger()
    led.record_assumption("assumptions", "terminal_growth_rate", None,
                          value=0.025, rationale="GDP proxy", basis="house default")
    cpath = cache_dir / "ZZTEST.json"
    cpath.write_text(json.dumps({"__ledger__": led.to_json()}), encoding="utf-8")
    try:
        orch = VirtualAnalystOrchestrator.__new__(VirtualAnalystOrchestrator)
        out = orch._finalize("The terminal growth rate is 2.5%.", "ZZTEST")
        assert "Sources & Assumptions" in out
        assert "terminal_growth_rate" in out
    finally:
        cpath.unlink(missing_ok=True)


def test_finalize_no_ticker_unchanged():
    orch = VirtualAnalystOrchestrator.__new__(VirtualAnalystOrchestrator)
    assert orch._finalize("plain answer", "") == "plain answer"


def test_finalize_missing_cache_unchanged():
    orch = VirtualAnalystOrchestrator.__new__(VirtualAnalystOrchestrator)
    assert orch._finalize("plain answer", "NOPE_NO_CACHE") == "plain answer"
