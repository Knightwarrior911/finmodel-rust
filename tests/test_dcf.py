"""Tests for src/dcf.py — DCF valuation engine."""
import pytest
from unittest.mock import patch, MagicMock
from schemas.financial_data import (
    ModelOutput, DCFOutput, WACCOutput, AssumptionsBlock, ScenarioInputs,
)
from src.dcf import compute_dcf


def _scenario(name: str, terminal_g: float = 0.025, exit_mult: float = 12.0) -> ScenarioInputs:
    return ScenarioInputs(
        name=name,
        revenue_growth_pct=[0.05] * 5,
        gross_margin_pct=[0.44] * 5,
        sga_pct_rev=[0.065] * 5,
        rd_pct_rev=[0.075] * 5,
        da_pct_rev=[0.03] * 5,
        capex_pct_rev=[0.028] * 5,
        tax_rate_pct=[0.147] * 5,
        interest_rate_pct=[0.035] * 5,
        dso_days=[28.0] * 5,
        dio_days=[6.0] * 5,
        dpo_days=[95.0] * 5,
        dividend_per_share=[0.0] * 5,
        terminal_growth_rate=terminal_g,
        exit_ebitda_multiple=exit_mult,
    )


def _mock_assumptions(active_case: int = 1) -> AssumptionsBlock:
    return AssumptionsBlock(
        proj_periods=["2024E", "2025E", "2026E"],
        active_case=active_case,
        base=_scenario("Base"),
        upside=_scenario("Upside", 0.030, 14.0),
        downside=_scenario("Downside", 0.020, 10.0),
        risk_free_rate=0.043,
        equity_risk_premium=0.055,
        target_de_ratio=0.30,
        cost_of_debt_pretax=0.035,
        current_share_price=180.0,
        shares_diluted=15813.0,
        mid_year_convention=True,
    )


def _mock_wacc(wacc_pct: float | None = None, tax: float = 0.21) -> WACCOutput:
    ke = 0.043 + 1.24 * 0.055
    kd_at = 0.035 * (1 - tax)
    we, wd = 0.963, 0.037
    computed = we * ke + wd * kd_at
    return WACCOutput(
        peers=[],
        median_unlevered_beta=1.0,
        target_levered_beta=1.24,
        target_de_ratio=0.30,
        risk_free_rate=0.043,
        equity_risk_premium=0.055,
        cost_of_equity=ke,
        cost_of_debt_pretax=0.035,
        tax_rate=tax,
        after_tax_cost_of_debt=kd_at,
        target_market_cap=2_500_000.0,
        target_debt=95_281.0,
        target_total_capital=2_595_281.0,
        equity_weight=we,
        debt_weight=wd,
        wacc=wacc_pct if wacc_pct is not None else computed,
    )

# ── shared fixture ────────────────────────────────────────────────────────────

PROJ_OUTPUT = ModelOutput(
    periods=["2021A", "2022A", "2023A", "2024E", "2025E", "2026E"],
    income_statement={
        "revenue":         [365817, 394328, 383285, 402948, 423595, 445274],
        "ebit":            [108949, 119437, 114301, 120016, 126017, 132318],
        "da":              [11284,  11104,  11519,  12100,  12705,  13340],
        "ebitda":          [120233, 130541, 125820, 132116, 138722, 145658],
        "net_income":      [94680,  99803,  96995,  101850, 106943, 112290],
        "interest_expense":[2645,   2830,   3933,   3330,   3330,   3330],
        "shares_diluted":  [16865,  16215,  15813,  15813,  15813,  15813],
    },
    balance_sheet={
        "cash":                [62639,  48304,  61555,  64632,  67864,  71257],
        "accounts_receivable": [26278,  28184,  29508,  31010,  32560,  34188],
        "inventory":           [6580,   4946,   6331,   6647,   6979,   7328],
        "accounts_payable":    [54763,  64115,  62611,  65741,  69028,  72480],
        "long_term_debt":      [109106, 98959,  95281,  95281,  95281,  95281],
        "total_equity":        [63090,  50672,  62146,  65253,  68516,  71942],
        "total_assets":        [351002, 352755, 352583, 370212, 388722, 408158],
    },
    cash_flow_statement={
        "cfo":   [104038, 122151, 110543, 116070, 121874, 127967],
        "capex": [11085,  10708,  10959,  11510,  12086,  12690],
        "cfi":   [-14545, -22354, -3,     -11510, -12086, -12690],
        "cff":   [-93353, -110749,-108488, 0,      0,      0],
        "net_change_cash": [-3860, -10952, 13248, 4560,  4790,  5030],
    },
    schedules={},
    assumptions={
        "revenue_growth_pct": 0.05,
        "gross_margin_pct":   0.44,
        "sga_pct_rev":        0.065,
        "rd_pct_rev":         0.075,
        "da_pct_rev":         0.03,
        "capex_pct_rev":      0.028,
        "tax_rate_pct":       0.147,
        "interest_rate_pct":  0.035,
        "dso_days":           28.0,
        "dpo_days":           95.0,
        "dio_days":           6.0,
        "shares_diluted":     15813.0,
        "dividend_per_share": 0.0,
    },
    converged=True,
    plug_used=False,
)


# ── helpers ───────────────────────────────────────────────────────────────────

def _mock_dcf(output=PROJ_OUTPUT, ticker="AAPL", wacc_pct: float | None = None, **kw):
    """Run compute_dcf with prebuilt WACCOutput + AssumptionsBlock (no network).
    When wacc_pct is None, WACC is computed from components (we×ke + wd×kd_at)."""
    return compute_dcf(output, ticker, _mock_wacc(wacc_pct), _mock_assumptions(), **kw)


# ── tests ─────────────────────────────────────────────────────────────────────

def test_returns_dcf_output():
    result = _mock_dcf()
    assert isinstance(result, DCFOutput)


def test_wacc_in_reasonable_range():
    result = _mock_dcf()
    assert 0.05 <= result.wacc <= 0.25


def test_wacc_components_sum():
    result = _mock_dcf()
    computed = result.equity_weight * result.cost_of_equity + result.debt_weight * result.after_tax_cost_of_debt
    assert abs(result.wacc - computed) < 1e-6


def test_capm_formula():
    result = _mock_dcf()
    expected = result.risk_free_rate + result.beta * result.equity_risk_premium
    assert abs(result.cost_of_equity - expected) < 1e-6


def test_after_tax_cost_of_debt():
    result = _mock_dcf()
    expected = result.cost_of_debt_pretax * (1 - result.tax_rate)
    assert abs(result.after_tax_cost_of_debt - expected) < 1e-6


def test_fcff_count_matches_proj_periods():
    result = _mock_dcf()
    assert len(result.fcff_proj) == 3
    assert len(result.proj_periods) == 3


def test_pv_fcfs_is_sum_of_pvs():
    result = _mock_dcf()
    # Mid-year discounting: 1/(1+wacc)^(t-0.5)
    manual = sum(f / (1 + result.wacc) ** ((i + 1) - 0.5)
                 for i, f in enumerate(result.fcff_proj))
    assert abs(result.pv_fcfs - manual) < 0.5


def test_tv_ebitda_method():
    # tv_method=1 selects EBITDA multiple; multiple comes from assumptions (12.0 default)
    result = _mock_dcf(tv_method=1)
    assert abs(result.tv_selected - result.tv_ebitda) < 0.1
    assert abs(result.tv_ebitda - result.terminal_ebitda * 12.0) < 0.1


def test_tv_gordon_method():
    result = _mock_dcf(tv_method=2)
    assert abs(result.tv_selected - result.tv_gordon) < 0.1


def test_ev_bridge():
    result = _mock_dcf()
    assert abs(result.enterprise_value - (result.pv_fcfs + result.pv_tv)) < 0.1
    assert abs(result.net_debt - (result.total_debt - result.cash)) < 0.1
    assert abs(result.equity_value - (result.enterprise_value - result.net_debt)) < 0.1


def test_implied_price_positive_for_healthy_company():
    result = _mock_dcf()
    # AAPL should have positive implied price
    assert result.implied_price > 0


def test_implied_price_formula():
    result = _mock_dcf()
    expected = result.equity_value / result.shares_diluted
    assert abs(result.implied_price - expected) < 0.01


def test_sensitivity_ebitda_shape():
    result = _mock_dcf()
    assert len(result.sensitivity_ebitda) == 5
    assert all(len(row) == 5 for row in result.sensitivity_ebitda)


def test_sensitivity_gordon_shape():
    result = _mock_dcf()
    assert len(result.sensitivity_gordon) == 5
    assert all(len(row) == 5 for row in result.sensitivity_gordon)


def test_sensitivity_monotone_wacc():
    result = _mock_dcf()
    # Higher WACC → lower price (all else equal)
    prices = [row[2] for row in result.sensitivity_ebitda]   # center column
    assert prices[0] > prices[-1], "Price should fall as WACC rises"


def test_sensitivity_monotone_multiple():
    result = _mock_dcf()
    # Higher multiple → higher price (all else equal)
    center_wacc_row = result.sensitivity_ebitda[2]
    assert center_wacc_row[0] < center_wacc_row[-1], "Price should rise with multiple"


def test_sensitivity_base_cell_at_center():
    result = _mock_dcf()
    # wacc_range is rounded to 4 dp; center element should equal wacc within rounding
    assert abs(result.wacc_range[2] - result.wacc) < 1e-4


def test_zero_shares_returns_zero_price():
    """Zero shares should not crash — implied price returns 0."""
    import copy
    out = copy.deepcopy(PROJ_OUTPUT)
    out.income_statement["shares_diluted"] = [0, 0, 0, 0, 0, 0]
    asmp = _mock_assumptions()
    asmp.shares_diluted = 0  # also zero out fallback
    result = compute_dcf(out, "AAPL", _mock_wacc(), asmp)
    assert result.implied_price == 0.0


def test_wacc_clamped_when_input_low():
    """Low WACC input should still produce WACC ≥ 5% in WACCOutput."""
    # WACC clamping now happens in src.wacc.compute_wacc, not src.dcf.
    # Verify dcf passes through whatever WACC it's given.
    result = compute_dcf(PROJ_OUTPUT, "AAPL", _mock_wacc(wacc_pct=0.05),
                         _mock_assumptions())
    assert result.wacc == 0.05
