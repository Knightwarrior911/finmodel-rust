from src.source_ledger import SourceLedger, Tier
from src.wacc import compute_wacc
from schemas.financial_data import PeerSet


def test_no_peers_records_beta_assumption():
    led = SourceLedger()
    ps = PeerSet(target_ticker="X", target_market_cap=1000.0, target_de_ratio=0.3, peers=[])
    compute_wacc(ps, target_market_cap=1000.0, target_debt=200.0,
                 risk_free_rate=0.04, equity_risk_premium=0.055,
                 cost_of_debt_pretax=0.05, target_tax_rate=0.244,
                 sector="utility", ledger=led)
    beta_entry = led.get("wacc", "median_unlevered_beta", None)
    assert beta_entry is not None
    assert beta_entry.tier is Tier.ASSUMPTION
    clamp = led.get("wacc", "wacc", None)
    assert clamp is not None


def test_ledger_none_is_unchanged():
    ps = PeerSet(target_ticker="X", target_market_cap=1000.0, target_de_ratio=0.3, peers=[])
    out = compute_wacc(ps, 1000.0, 200.0, 0.04, 0.055, 0.05)
    assert out.wacc >= 0.05
