from src.source_ledger import SourceLedger, Tier
from src.assumptions import resolve_input


def test_resolve_input_derives_when_actuals_present():
    led = SourceLedger()
    is_ = {"income_tax": [100.0, 120.0], "net_income": [300.0, 360.0]}
    bs = {}
    v = resolve_input("tax_rate_pct", is_, bs, sector="standard", ledger=led, period="2026E")
    assert abs(v - 0.25) < 1e-9
    assert led.get("assumptions", "tax_rate_pct", "2026E").tier is Tier.DERIVED


def test_resolve_input_assumption_when_no_actuals():
    led = SourceLedger()
    v = resolve_input("tax_rate_pct", {}, {}, sector="standard", ledger=led, period="2026E")
    assert v == 0.21
    assert led.get("assumptions", "tax_rate_pct", "2026E").tier is Tier.ASSUMPTION


def test_resolve_input_unverified_when_unknown_key():
    led = SourceLedger()
    v = resolve_input("mystery_key", {}, {}, sector="standard", ledger=led, period="2026E")
    assert v is None
    assert led.get("assumptions", "mystery_key", "2026E").tier is Tier.UNVERIFIED


def test_forward_drivers_recorded():
    from src.assumptions import build_assumptions_block
    from src.source_ledger import SourceLedger, Tier

    class _MO:
        periods = ["2024A", "2025A", "2026E", "2027E"]
        assumptions = {"revenue_growth_pct": 0.07, "gross_margin_pct": 0.42,
                       "sga_pct_rev": 0.11, "rd_pct_rev": 0.06,
                       "capex_pct_rev": 0.04, "dividend_per_share": 1.25,
                       "shares_diluted": 500.0}

    led = SourceLedger()
    build_assumptions_block(_MO(), "TEST", sector="standard", ledger=led)
    rg = led.get("assumptions", "revenue_growth_pct", None)
    assert rg is not None and rg.value == 0.07
    assert led.get("assumptions", "shares_diluted", None) is not None
