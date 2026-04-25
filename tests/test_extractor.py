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


LOW_CONFIDENCE_NOTES = json.dumps({
    "da": {"values": {"2023A": 100}, "source": "Note 4"},
    "confidence": 0.5
})


def test_extract_notes_from_pdf_low_confidence_adds_flag(tmp_path):
    fake_pdf = tmp_path / "report.pdf"
    fake_pdf.write_bytes(b"%PDF-1.4 fake")

    with patch("src.extractor.pdfplumber.open") as mock_pdf, \
         patch("src.extractor.anthropic.Anthropic", return_value=make_mock_client(LOW_CONFIDENCE_NOTES)):
        mock_pdf.return_value.__enter__.return_value.pages = [MagicMock(extract_text=lambda: "some text")]
        result = extract_notes_from_pdf(str(fake_pdf), periods=["2023A"])

    assert any("confidence" in d.lower() for d in result.get("discrepancies", []))


def test_scrape_ir_page_for_pdfs_uses_provided_url():
    from src.extractor import scrape_ir_page_for_pdfs
    import requests as req_module

    fake_html = b"""<html><body>
        <a href="https://example.com/annual_report_2024.pdf">Annual Report 2024</a>
        <a href="https://example.com/other.pdf">Other</a>
    </body></html>"""

    mock_resp = MagicMock()
    mock_resp.text = fake_html.decode()
    mock_resp.raise_for_status = MagicMock()

    with patch("src.extractor.subprocess.run"), \
         patch("requests.get", return_value=mock_resp):
        urls = scrape_ir_page_for_pdfs("HSBA.L", "HSBC", ir_url="https://example.com/investors/")

    assert "https://example.com/annual_report_2024.pdf" in urls
