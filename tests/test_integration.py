# financial_model/tests/test_integration.py
import json
import os
import tempfile
from pathlib import Path
from unittest.mock import patch, MagicMock

from src.preflight import run_preflight
from src.fetcher import fetch_us_filing
from src.reconciler import reconcile
from src.engine import ModelEngine
from src.verifier import verify
from src.writer import ExcelWriter

FIXTURE_DIR = Path(__file__).parent / "fixtures"

PREFLIGHT_RESPONSE = json.dumps({
    "ticker": "AAPL", "company_name": "Apple Inc.",
    "domicile": "US", "currency": "USD", "fiscal_year_end": "Sep",
    "periods_historical": 2, "periods_projected": 2, "ambiguity": None
})

RECONCILE_RESPONSE = json.dumps({
    "confirmed": {}, "discrepancies": [], "notes_merged": {}
})


def make_llm_sequence(*responses):
    """Returns a side_effect for _llm_complete that returns each response in order."""
    call_count = [0]
    def _side_effect(system_text, user_text, max_tokens):
        if call_count[0] >= len(responses):
            raise AssertionError(
                f"LLM called {call_count[0] + 1} times but only {len(responses)} responses provided"
            )
        resp = responses[call_count[0]]
        call_count[0] += 1
        return resp
    return _side_effect


def mock_requests_get(url, headers=None, timeout=None):
    mock_resp = MagicMock()
    mock_resp.raise_for_status = MagicMock()
    if "company_tickers" in url:
        mock_resp.json.return_value = {
            "0": {"cik_str": 320193, "ticker": "AAPL", "title": "Apple Inc."}
        }
    elif "companyfacts" in url:
        mock_resp.json.return_value = json.loads(
            (FIXTURE_DIR / "xbrl_facts.json").read_text()
        )
    else:
        raise ValueError(f"Unexpected URL in test: {url}")
    return mock_resp


def test_end_to_end_us_company():
    with tempfile.TemporaryDirectory() as tmpdir:
        out_path = os.path.join(tmpdir, "AAPL_model.xlsx")

        with patch("src.extractor._llm_complete", side_effect=make_llm_sequence(PREFLIGHT_RESPONSE, RECONCILE_RESPONSE)), \
             patch("src.fetcher.requests.get", side_effect=mock_requests_get):

            cfg = run_preflight("AAPL", periods_historical=2, periods_projected=2)
            raw = fetch_us_filing(cfg)
            reconciled, disc_report = reconcile(raw)
            model_out = ModelEngine(reconciled, cfg).build()
            report = verify(model_out)
            assert report.passed or not report.critical_failures, f"Unexpected critical failures: {report.critical_failures}"
            ExcelWriter(model_out, report, cfg.company_name, out_path, sources=reconciled.sources).write()

        assert os.path.exists(out_path)
        assert os.path.getsize(out_path) > 5000

        import openpyxl
        wb = openpyxl.load_workbook(out_path)
        assert set(wb.sheetnames) == {"Cover", "Assumptions", "IS", "BS", "CF", "Sources"}


def test_end_to_end_model_periods():
    with patch("src.extractor._llm_complete", side_effect=make_llm_sequence(PREFLIGHT_RESPONSE, RECONCILE_RESPONSE)), \
         patch("src.fetcher.requests.get", side_effect=mock_requests_get):

        cfg = run_preflight("AAPL", periods_historical=2, periods_projected=2)
        raw = fetch_us_filing(cfg)
        reconciled, _ = reconcile(raw)
        model_out = ModelEngine(reconciled, cfg).build()

    assert len(model_out.periods) == 4  # 2 historical + 2 projected
    assert all(p.endswith("A") for p in model_out.periods[:2])
    assert all(p.endswith("E") for p in model_out.periods[2:])
