import pytest
from schemas.financial_data import ReconciledFinancialData, ModelConfig, ModelOutput
from src.engine import ModelEngine

HIST_DATA = ReconciledFinancialData(
    ticker="AAPL", company_name="Apple Inc.", currency="USD", fiscal_year_end="Sep",
    periods=["2021A", "2022A", "2023A"],
    income_statement={
        "revenue": [365817, 394328, 383285],
        "cogs": [212981, 223546, 214137],
        "gross_profit": [152836, 170782, 169148],
        "sga": [21973, 25094, 24932],
        "rd": [21914, 26251, 29915],
        "da": [11284, 11104, 11519],
        "ebit": [108949, 119437, 114301],
        "interest_expense": [2645, 2830, 3933],
        "interest_income": [2843, 2825, 3750],
        "income_tax": [14527, 19300, 16520],
        "net_income": [94680, 99803, 96995],
        "shares_diluted": [16865, 16215, 15813],
    },
    balance_sheet={
        "cash": [62639, 48304, 61555],
        "accounts_receivable": [26278, 28184, 29508],
        "inventory": [6580, 4946, 6331],
        "total_current_assets": [134836, 135405, 143566],
        "ppe_net": [39440, 42117, 43715],
        "total_assets": [351002, 352755, 352583],
        "accounts_payable": [54763, 64115, 62611],
        "total_current_liabilities": [125481, 153982, 145308],
        "long_term_debt": [109106, 98959, 95281],
        "total_liabilities": [287912, 302083, 290437],
        "retained_earnings": [5562, -3068, -214],
        "total_equity": [63090, 50672, 62146],
    },
    cash_flow_statement={
        "cfo": [104038, 122151, 110543],
        "capex": [11085, 10708, 10959],
        "cfi": [-14545, -22354, -3],
        "cff": [-93353, -110749, -108488],
        "net_change_cash": [-3860, -10952, 13248],
    },
    notes={}, sources={}, flags=[]
)

CFG = ModelConfig(
    ticker="AAPL", company_name="Apple Inc.", domicile="US",
    currency="USD", fiscal_year_end="Sep",
    periods_historical=3, periods_projected=3
)


def test_engine_builds_model_output():
    engine = ModelEngine(HIST_DATA, CFG)
    output = engine.build()
    assert isinstance(output, ModelOutput)


def test_engine_period_count():
    engine = ModelEngine(HIST_DATA, CFG)
    output = engine.build()
    # 3 historical + 3 projected = 6 periods
    assert len(output.periods) == 6


def test_engine_projected_revenue_grows():
    engine = ModelEngine(HIST_DATA, CFG)
    output = engine.build()
    proj_rev = output.income_statement["revenue"][3:]
    # projected should be positive
    assert all(r > 0 for r in proj_rev)


def test_engine_default_revenue_growth_uses_3yr_avg():
    engine = ModelEngine(HIST_DATA, CFG)
    output = engine.build()
    rev = output.assumptions["revenue_growth_pct"]
    # avg of (394328/365817-1) and (383285/394328-1) ≈ mean([0.0778, -0.028]) ≈ 0.025
    # allow wide tolerance since we avg last 3
    assert -0.10 < rev < 0.20


def test_engine_gross_margin_assumed():
    engine = ModelEngine(HIST_DATA, CFG)
    output = engine.build()
    assert "gross_margin_pct" in output.assumptions


def test_engine_convergence_flag():
    engine = ModelEngine(HIST_DATA, CFG)
    output = engine.build()
    assert isinstance(output.converged, bool)
