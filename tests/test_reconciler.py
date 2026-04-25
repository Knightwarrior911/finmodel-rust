import json
import pytest
from unittest.mock import patch, MagicMock
from schemas.financial_data import ReconciledFinancialData, DiscrepancyReport, SourceCitation
from src.reconciler import reconcile, check_consistency

MOCK_RAW = ReconciledFinancialData(
    ticker="AAPL", company_name="Apple Inc.", currency="USD",
    fiscal_year_end="Sep", periods=["2022A", "2023A"],
    income_statement={"revenue": [394328, 383285], "net_income": [99803, 96995], "da": [11104, 11519]},
    balance_sheet={"total_assets": [352755, 352583], "total_liabilities": [302083, 290437], "total_equity": [50672, 62146]},
    cash_flow_statement={"cfo": [122151, 110543], "capex": [10708, 10959]},
    notes={"da": {"values": {"2022A": 11104, "2023A": 11519}, "source": "Note 4"}},
    sources={"revenue": [SourceCitation(filing="10-K 2023A", confidence=1.0, xbrl_tag="us-gaap:Revenues")]},
    flags=[]
)

MOCK_RECONCILE_RESPONSE = json.dumps({
    "confirmed": {"da": "matches IS and notes — no discrepancy"},
    "discrepancies": [],
    "notes_merged": {"da": {"2022A": 11104, "2023A": 11519}}
})


def make_mock_client(content: str):
    mock_msg = MagicMock()
    mock_msg.content = [MagicMock(text=content)]
    mock_client = MagicMock()
    mock_client.messages.create.return_value = mock_msg
    return mock_client


def test_check_consistency_bs_balances():
    errors = check_consistency(MOCK_RAW)
    assert errors == []


def test_check_consistency_bs_mismatch():
    bad = ReconciledFinancialData(
        ticker="TEST", company_name="Test", currency="USD", fiscal_year_end="Dec",
        periods=["2023A"],
        income_statement={},
        balance_sheet={"total_assets": [100], "total_liabilities": [60], "total_equity": [30]},  # 90 != 100
        cash_flow_statement={},
        notes={}, sources={}, flags=[]
    )
    errors = check_consistency(bad)
    assert any("balance sheet" in e.lower() for e in errors)


def test_reconcile_returns_reconciled_data():
    with patch("src.reconciler.anthropic.Anthropic", return_value=make_mock_client(MOCK_RECONCILE_RESPONSE)):
        result, report = reconcile(MOCK_RAW)
    assert isinstance(result, ReconciledFinancialData)
    assert isinstance(report, DiscrepancyReport)
    assert result.flags == []


def test_reconcile_notes_merged_into_income_statement():
    with patch("src.reconciler.anthropic.Anthropic", return_value=make_mock_client(MOCK_RECONCILE_RESPONSE)):
        result, _ = reconcile(MOCK_RAW)
    assert "da" in result.income_statement
