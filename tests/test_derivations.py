from src import derivations as d


def _is():
    return {
        "revenue": [1000.0, 1200.0],
        "income_tax": [100.0, 120.0],
        "net_income": [300.0, 360.0],
        "interest_expense": [40.0, 50.0],
        "interest_income": [5.0, 6.0],
        "da": [80.0, 96.0],
        "cogs": [600.0, 720.0],
    }


def _bs():
    return {
        "long_term_debt": [500.0, 600.0],
        "cash": [200.0, 240.0],
        "accounts_receivable": [150.0, 180.0],
        "inventory": [100.0, 120.0],
        "accounts_payable": [90.0, 108.0],
    }


def test_effective_tax_rate():
    v, (formula, inputs) = d.effective_tax_rate(_is())
    assert abs(v - 0.25) < 1e-9
    assert "income_tax" in formula


def test_effective_tax_rate_none_when_missing():
    v, lin = d.effective_tax_rate({"revenue": [1000.0]})
    assert v is None and lin is None


def test_effective_tax_rate_guard_rejects_absurd():
    bad = {"income_tax": [900.0], "net_income": [100.0]}
    v, lin = d.effective_tax_rate(bad)
    assert v is None


def test_cost_of_debt():
    v, (formula, inputs) = d.cost_of_debt(_is(), _bs())
    assert 0.08 <= v <= 0.084
    assert "interest_expense" in formula


def test_cash_yield():
    v, _ = d.cash_yield(_is(), _bs())
    assert abs(v - 0.025) < 1e-9


def test_da_pct():
    v, _ = d.da_pct(_is())
    assert abs(v - 0.08) < 1e-9


def test_wc_days():
    res = d.wc_days(_is(), _bs())
    dso, _ = res["dso"]
    assert abs(dso - 54.75) < 1e-6
    dio, _ = res["dio"]
    assert dio is not None
    dpo, _ = res["dpo"]
    assert dpo is not None
