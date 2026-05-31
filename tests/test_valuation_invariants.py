import math
from types import SimpleNamespace
from src.valuation_invariants import check_valuation


def _good_wacc(**over):
    d = dict(tax_rate=0.25, cost_of_equity=0.095, risk_free_rate=0.04,
             cost_of_debt_pretax=0.05, after_tax_cost_of_debt=0.0375,
             equity_weight=0.8, debt_weight=0.2, wacc=0.085,
             target_levered_beta=1.1)
    d.update(over)
    return SimpleNamespace(**d)


def _good_dcf(**over):
    d = dict(tax_rate=0.25, wacc=0.085, wacc_minus_g=0.06, tv_growth_rate=0.025,
             equity_weight=0.8, debt_weight=0.2,
             discount_factors=[0.96, 0.88, 0.81, 0.74, 0.68],
             fcff_proj=[100.0, 110.0, 121.0, 133.0, 146.0],
             enterprise_value=5000.0, net_debt=1000.0, equity_value=4000.0,
             shares_diluted=100.0, implied_price=40.0,
             tv_pct_of_ev=0.7, current_share_price=35.0, upside_downside_pct=0.14,
             pv_tv=3000.0, pv_fcfs=2000.0)
    d.update(over)
    return SimpleNamespace(**d)


def test_good_model_passes():
    r = check_valuation(_good_wacc(), _good_dcf())
    assert r.passed is True
    assert r.critical == []


def test_wacc_below_growth_is_critical():
    r = check_valuation(_good_wacc(), _good_dcf(wacc_minus_g=-0.01, tv_growth_rate=0.10, wacc=0.085))
    assert r.passed is False
    assert any("terminal growth" in c for c in r.critical)


def test_negative_beta_implied_by_ke_below_rf():
    r = check_valuation(_good_wacc(cost_of_equity=0.03, risk_free_rate=0.04), _good_dcf())
    assert any("risk_free_rate" in c for c in r.critical)


def test_weights_dont_sum_to_one():
    r = check_valuation(_good_wacc(equity_weight=0.9, debt_weight=0.2), _good_dcf())
    assert any("weights" in c for c in r.critical)


def test_nonpositive_ev():
    r = check_valuation(_good_wacc(), _good_dcf(enterprise_value=-10.0))
    assert any("enterprise_value" in c for c in r.critical)


def test_ev_bridge_identity_violation():
    r = check_valuation(_good_wacc(), _good_dcf(equity_value=999.0))
    assert any("equity_value" in c for c in r.critical)


def test_tax_shield_negative():
    r = check_valuation(_good_wacc(after_tax_cost_of_debt=0.06, cost_of_debt_pretax=0.05), _good_dcf())
    assert any("after_tax_cost_of_debt" in c for c in r.critical)


def test_discount_factors_not_decreasing():
    r = check_valuation(_good_wacc(), _good_dcf(discount_factors=[0.9, 0.95, 0.8]))
    assert any("discount factors" in c for c in r.critical)


def test_nan_in_fcff():
    r = check_valuation(_good_wacc(), _good_dcf(fcff_proj=[100.0, float("nan")]))
    assert any("fcff_proj" in c for c in r.critical)


def test_tv_share_warning():
    r = check_valuation(_good_wacc(), _good_dcf(tv_pct_of_ev=0.99))
    assert r.passed is True
    assert any("TV is" in w for w in r.warnings)


def test_extreme_upside_warning():
    r = check_valuation(_good_wacc(), _good_dcf(upside_downside_pct=3.5))
    assert any("upside" in w.lower() for w in r.warnings)


def test_missing_attrs_does_not_raise():
    r = check_valuation(SimpleNamespace(), SimpleNamespace())
    assert isinstance(r.critical, list)
