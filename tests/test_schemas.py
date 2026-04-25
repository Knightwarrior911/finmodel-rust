from schemas.financial_data import (
    ModelConfig, SourceCitation, ReconciledFinancialData,
    DiscrepancyReport, ModelOutput, VerificationReport
)

def test_model_config_defaults():
    cfg = ModelConfig(ticker="AAPL", company_name="Apple Inc.", domicile="US",
                      currency="USD", fiscal_year_end="Sep")
    assert cfg.periods_historical == 5
    assert cfg.periods_projected == 5
    assert cfg.filing_override is None
    assert cfg.force is False

def test_source_citation_xbrl():
    s = SourceCitation(filing="10-K FY2023", confidence=1.0, xbrl_tag="us-gaap:Revenues")
    assert s.page is None
    assert s.xbrl_tag == "us-gaap:Revenues"

def test_reconciled_financial_data_structure():
    rfd = ReconciledFinancialData(
        ticker="AAPL", company_name="Apple Inc.", currency="USD",
        fiscal_year_end="Sep", periods=["2022A", "2023A"],
        income_statement={"revenue": [394328, 383285]},
        balance_sheet={"total_assets": [352755, 352583]},
        cash_flow_statement={"cfo": [122151, 110543]},
        notes={}, sources={}, flags=[]
    )
    assert rfd.periods == ["2022A", "2023A"]
    assert rfd.income_statement["revenue"][1] == 383285

def test_verification_report_passed():
    vr = VerificationReport(passed=True, critical_failures=[], warnings=[], notes=[], period_checks={})
    assert vr.passed is True
