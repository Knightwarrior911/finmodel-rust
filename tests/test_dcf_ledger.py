from src.source_ledger import SourceLedger, Tier
from src.dcf import flag_ev_bridge_gaps


def test_preferred_and_investments_flagged_unverified():
    led = SourceLedger()
    flag_ev_bridge_gaps(led, preferred=0.0, investments=0.0)
    assert led.get("dcf", "preferred_stock", None).tier is Tier.UNVERIFIED
    assert led.get("dcf", "investments", None).tier is Tier.UNVERIFIED


def test_flag_ev_bridge_gaps_noop_when_no_ledger():
    # Must not raise when ledger is None.
    flag_ev_bridge_gaps(None, preferred=0.0, investments=0.0)
