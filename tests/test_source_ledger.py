from src.source_ledger import SourceLedger, LedgerEntry, Tier


def test_record_and_get_round_trip():
    led = SourceLedger()
    led.record_derived(
        "wacc", "tax_rate", None, value=0.244,
        formula="income_tax / (net_income + income_tax)",
        inputs=[("income_statement", "income_tax", "2024A")],
    )
    e = led.get("wacc", "tax_rate", None)
    assert e.tier is Tier.DERIVED
    assert e.value == 0.244
    assert e.ref["formula"].startswith("income_tax")


def test_record_assumption_and_unverified():
    led = SourceLedger()
    led.record_assumption("assumptions", "terminal_growth_rate", None,
                          value=0.025, rationale="GDP/inflation proxy", basis="house default")
    led.record_unverified("dcf", "preferred_stock", None,
                          reason="not in extraction schema")
    assert led.get("assumptions", "terminal_growth_rate", None).tier is Tier.ASSUMPTION
    assert led.get("dcf", "preferred_stock", None).tier is Tier.UNVERIFIED


def test_json_round_trip():
    led = SourceLedger()
    led.record_assumption("assumptions", "tax_rate_pct", "2026E",
                          value=0.21, rationale="US statutory", basis="default")
    blob = led.to_json()
    led2 = SourceLedger.from_json(blob)
    e = led2.get("assumptions", "tax_rate_pct", "2026E")
    assert e.value == 0.21 and e.tier is Tier.ASSUMPTION


def test_entries_filtered_by_tier():
    led = SourceLedger()
    led.record_assumption("a", "x", None, value=1.0, rationale="r", basis="b")
    led.record_unverified("a", "y", None, reason="z")
    flagged = led.entries_by_tier(Tier.ASSUMPTION, Tier.UNVERIFIED)
    assert len(flagged) == 2
