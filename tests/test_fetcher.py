# financial_model/tests/test_fetcher.py
import json
import pytest
from pathlib import Path
from unittest.mock import patch, MagicMock
from src.fetcher import get_cik, fetch_xbrl_facts, parse_xbrl_to_raw, fetch_us_filing

FIXTURE_DIR = Path(__file__).parent / "fixtures"


def mock_get(url, headers=None):
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
    return mock_resp


def test_get_cik():
    with patch("src.fetcher.requests.get", side_effect=mock_get):
        cik = get_cik("AAPL")
    assert cik == "0000320193"


def test_get_cik_not_found():
    with patch("src.fetcher.requests.get", side_effect=mock_get):
        with pytest.raises(ValueError, match="not found"):
            get_cik("XXXX")


def test_fetch_xbrl_facts_returns_dict():
    with patch("src.fetcher.requests.get", side_effect=mock_get):
        facts = fetch_xbrl_facts("0000320193")
    assert "us-gaap" in facts["facts"]


def test_parse_xbrl_extracts_revenue():
    facts = json.loads((FIXTURE_DIR / "xbrl_facts.json").read_text())
    raw = parse_xbrl_to_raw(facts, periods_historical=2)
    assert "revenue" in raw.income_statement
    assert len(raw.income_statement["revenue"]) == 2
    assert raw.income_statement["revenue"][1] == 383285  # millions


def test_parse_xbrl_converts_to_millions():
    facts = json.loads((FIXTURE_DIR / "xbrl_facts.json").read_text())
    raw = parse_xbrl_to_raw(facts, periods_historical=2)
    # 383285000000 / 1e6 = 383285.0
    assert raw.income_statement["revenue"][1] == pytest.approx(383285.0)
