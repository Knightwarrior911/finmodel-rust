import json
from src.source_ledger import SourceLedger, Tier


def test_ledger_persists_into_cache(tmp_path):
    led = SourceLedger()
    led.record_assumption("assumptions", "terminal_growth_rate", None,
                          value=0.025, rationale="GDP proxy", basis="house default")
    cache = {"income_statement": {}, "__ledger__": led.to_json()}
    p = tmp_path / "cache.json"
    p.write_text(json.dumps(cache), encoding="utf-8")
    loaded = json.loads(p.read_text(encoding="utf-8"))
    led2 = SourceLedger.from_json(loaded["__ledger__"])
    assert led2.get("assumptions", "terminal_growth_rate", None).tier is Tier.ASSUMPTION


def test_build_assumptions_block_populates_ledger():
    from src.assumptions import build_assumptions_block
    from src.source_ledger import SourceLedger, Tier

    class _Recon:
        income_statement = {"income_tax": [100.0, 120.0], "net_income": [300.0, 360.0],
                            "interest_expense": [40.0, 50.0], "da": [80.0, 96.0],
                            "revenue": [1000.0, 1200.0], "cogs": [600.0, 720.0]}
        balance_sheet = {"long_term_debt": [500.0, 600.0], "cash": [200.0, 240.0],
                         "accounts_receivable": [150.0, 180.0], "inventory": [100.0, 120.0],
                         "accounts_payable": [90.0, 108.0]}

    class _MO:
        periods = ["2024A", "2025A", "2026E", "2027E"]
        assumptions = {}

    led = SourceLedger()
    build_assumptions_block(_MO(), "TEST", sector="standard",
                            reconciled=_Recon(), ledger=led)
    tax = led.get("assumptions", "tax_rate_pct", None)
    assert tax is not None and tax.tier is Tier.DERIVED
