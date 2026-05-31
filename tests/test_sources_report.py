from src.source_ledger import SourceLedger
from src.sources_report import build_sources_report


def _cache():
    led = SourceLedger()
    led.record_derived("assumptions", "tax_rate_pct", None, value=0.25,
                       formula="income_tax / (net_income + income_tax)", inputs=[])
    led.record_assumption("assumptions", "terminal_growth_rate", None, value=0.025,
                          rationale="Long-run GDP/inflation proxy", basis="house default")
    led.record_unverified("dcf", "preferred_stock", None,
                          reason="not in extraction schema")
    return {"__ledger__": led.to_json()}


def test_report_has_all_sections():
    md = build_sources_report(_cache())
    assert "Sources & Assumptions" in md
    assert "Derived" in md and "income_tax" in md
    assert "Assumptions" in md and "GDP" in md
    assert "Unverified" in md and "preferred_stock" in md


def test_empty_cache_no_error():
    md = build_sources_report({})
    assert isinstance(md, str)
    assert "Sources & Assumptions" in md


def test_unverified_section_absent_when_none():
    led = SourceLedger()
    led.record_assumption("a", "x", None, value=1.0, rationale="r", basis="b")
    md = build_sources_report({"__ledger__": led.to_json()})
    assert "Unverified" not in md
