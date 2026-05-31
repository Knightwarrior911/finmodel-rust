from src.assumption_registry import resolve, Assumption


def test_resolve_known_global():
    a = resolve("equity_risk_premium")
    assert isinstance(a, Assumption)
    assert a.value == 0.055
    assert a.rationale and a.basis


def test_resolve_unknown_returns_none():
    assert resolve("totally_made_up_key") is None


def test_resolve_sector_beta():
    util = resolve("sector_beta", sector="utility")
    std = resolve("sector_beta", sector="standard")
    assert util.value < std.value
    assert util.basis


def test_resolve_sector_exit_multiple():
    a = resolve("exit_ebitda_multiple", sector="bank")
    assert a.value == 12.0
