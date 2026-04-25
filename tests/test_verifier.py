# financial_model/tests/test_verifier.py
from schemas.financial_data import ModelOutput, VerificationReport
from src.verifier import verify

def make_balanced_output(periods=("2023A", "2024E")):
    n = len(periods)
    return ModelOutput(
        periods=list(periods),
        income_statement={
            "revenue": [100.0] * n,
            "net_income": [15.0] * n,
            "ebit": [20.0] * n,
            "da": [5.0] * n,
        },
        balance_sheet={
            "total_assets": [200.0] * n,
            "total_liabilities": [120.0] * n,
            "total_equity": [80.0] * n,
            "cash": [50.0] * n,
            "retained_earnings": [30.0] * n,
        },
        cash_flow_statement={
            "cfo": [20.0] * n,
            "cfi": [-5.0] * n,
            "cff": [-3.0] * n,
            "net_change_cash": [12.0] * n,
        },
        schedules={},
        assumptions={"revenue_growth_pct": 0.05},
        converged=True,
        plug_used=False,
    )


def test_verify_passes_on_balanced_model():
    output = make_balanced_output()
    report = verify(output)
    assert report.passed is True
    assert report.critical_failures == []


def test_verify_fails_bs_mismatch():
    output = make_balanced_output()
    output.balance_sheet["total_assets"] = [999.0, 999.0]  # doesn't balance
    report = verify(output)
    assert report.passed is False
    assert any("balance sheet" in f.lower() for f in report.critical_failures)


def test_verify_warns_negative_revenue():
    output = make_balanced_output()
    output.income_statement["revenue"] = [-10.0, -10.0]
    report = verify(output)
    assert any("negative revenue" in w.lower() for w in report.warnings)


def test_verify_warns_high_leverage():
    output = make_balanced_output()
    output.balance_sheet["total_liabilities"] = [1001.0, 1001.0]
    output.balance_sheet["total_equity"] = [-801.0, -801.0]  # net debt >> EBITDA
    output.income_statement["da"] = [0.0, 0.0]
    report = verify(output)
    assert any("leverage" in w.lower() for w in report.warnings)


def test_verify_notes_plug_used():
    output = make_balanced_output()
    output.plug_used = True
    report = verify(output)
    assert any("plug" in n.lower() for n in report.notes)


def test_verify_fails_cfs_mismatch():
    output = make_balanced_output()
    output.cash_flow_statement["net_change_cash"] = [999.0, 999.0]  # cfo+cfi+cff=12, stated=999
    report = verify(output)
    assert report.passed is False
    assert any("cfs" in f.lower() or "cash flow" in f.lower() for f in report.critical_failures)
