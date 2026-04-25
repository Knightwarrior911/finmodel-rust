import json
import pytest
from unittest.mock import patch, MagicMock
from src.preflight import run_preflight
from schemas.financial_data import ModelConfig


MOCK_LLM_RESPONSE = json.dumps({
    "ticker": "AAPL",
    "company_name": "Apple Inc.",
    "domicile": "US",
    "currency": "USD",
    "fiscal_year_end": "Sep",
    "periods_historical": 5,
    "periods_projected": 5,
    "ambiguity": None
})


def make_mock_client(content: str):
    mock_msg = MagicMock()
    mock_msg.content = [MagicMock(text=content)]
    mock_client = MagicMock()
    mock_client.messages.create.return_value = mock_msg
    return mock_client


def test_preflight_returns_model_config():
    with patch("src.preflight.anthropic.Anthropic", return_value=make_mock_client(MOCK_LLM_RESPONSE)):
        cfg = run_preflight("AAPL")
    assert isinstance(cfg, ModelConfig)
    assert cfg.ticker == "AAPL"
    assert cfg.domicile == "US"
    assert cfg.currency == "USD"


def test_preflight_non_us():
    resp = json.dumps({
        "ticker": "7203.T", "company_name": "Toyota Motor Corporation",
        "domicile": "non-US", "currency": "JPY", "fiscal_year_end": "Mar",
        "periods_historical": 5, "periods_projected": 5, "ambiguity": None
    })
    with patch("src.preflight.anthropic.Anthropic", return_value=make_mock_client(resp)):
        cfg = run_preflight("Toyota")
    assert cfg.domicile == "non-US"
    assert cfg.currency == "JPY"


def test_preflight_raises_on_ambiguity():
    resp = json.dumps({
        "ticker": None, "company_name": None, "domicile": None,
        "currency": None, "fiscal_year_end": None,
        "periods_historical": 5, "periods_projected": 5,
        "ambiguity": "Did you mean HSBC London (HSBA.L) or HSBC HK (0005.HK)?"
    })
    with patch("src.preflight.anthropic.Anthropic", return_value=make_mock_client(resp)):
        with pytest.raises(ValueError, match="Ambiguous"):
            run_preflight("HSBC")
