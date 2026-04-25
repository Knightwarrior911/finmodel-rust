# financial_model/tests/test_extractor.py
import json
import pytest
from pathlib import Path
from unittest.mock import patch, MagicMock
from src.extractor import extract_notes_from_text, extract_notes_from_pdf

FIXTURE_DIR = Path(__file__).parent / "fixtures"
EXCERPT = (FIXTURE_DIR / "tenk_excerpt.txt").read_text()

MOCK_LLM_NOTES = json.dumps({
    "da": {"values": {"2022A": 11104, "2023A": 11519}, "source": "Note 4"},
    "tax_rate": {"values": {"2022A": 0.162, "2023A": 0.146}, "source": "Note 6"},
    "debt_maturities": {
        "2024": 9972, "2025": 10867, "2026": 9908, "2027": 9647, "thereafter": 66178
    },
    "revenue_breakdown": {
        "product": {"2023A": 298085}, "services": {"2023A": 85200}
    },
    "confidence": 0.95
})


def make_mock_client(content: str):
    mock_msg = MagicMock()
    mock_msg.content = [MagicMock(text=content)]
    mock_client = MagicMock()
    mock_client.messages.create.return_value = mock_msg
    return mock_client


def test_extract_notes_from_text_returns_dict():
    with patch("src.extractor.anthropic.Anthropic", return_value=make_mock_client(MOCK_LLM_NOTES)):
        result = extract_notes_from_text(EXCERPT, periods=["2022A", "2023A"])
    assert "da" in result
    assert result["da"]["values"]["2023A"] == 11519


def test_extract_notes_confidence_field():
    with patch("src.extractor.anthropic.Anthropic", return_value=make_mock_client(MOCK_LLM_NOTES)):
        result = extract_notes_from_text(EXCERPT, periods=["2022A", "2023A"])
    assert result["confidence"] == pytest.approx(0.95)
