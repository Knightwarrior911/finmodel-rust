# financial_model/src/extractor.py
import json
import anthropic
import pdfplumber
import fitz  # pymupdf
from pathlib import Path

NOTES_SYSTEM_PROMPT = """You are a senior financial analyst extracting data from company filing text.
Extract ALL financial data found: D&A schedules, debt maturities, tax rates, working capital details,
CapEx breakdown, SBC expense, lease obligations, segment data, and any other quantitative footnote data.

For EVERY line item: collect ALL mentions across the text, cross-check for consistency, then return
the authoritative value. If mentions conflict, flag the discrepancy.

Return JSON:
{
  "da": {"values": {"2023A": <millions>, ...}, "source": "Note X"},
  "tax_rate": {"values": {"2023A": <decimal e.g. 0.146>, ...}, "source": "Note X"},
  "debt_maturities": {"2024": <millions>, "2025": <millions>, ...},
  "revenue_breakdown": {"segment_name": {"period": value, ...}, ...},
  "capex_split": {"maintenance": <millions>, "growth": <millions>},
  "sbc_expense": {"values": {"2023A": <millions>, ...}},
  "lease_obligations": {"operating": <millions>, "finance": <millions>},
  "dso_days": <number or null>,
  "dpo_days": <number or null>,
  "dio_days": <number or null>,
  "discrepancies": ["description of any value conflicts found"],
  "confidence": <0.0 to 1.0>
}
Return ONLY valid JSON. Use millions as unit. Omit keys where data not present."""


def extract_notes_from_text(text: str, periods: list[str]) -> dict:
    client = anthropic.Anthropic()
    prompt = f"Periods in scope: {periods}\n\nFiling text:\n{text}"
    response = client.messages.create(
        model="claude-sonnet-4-6",
        max_tokens=4096,
        system=[{"type": "text", "text": NOTES_SYSTEM_PROMPT, "cache_control": {"type": "ephemeral"}}],
        messages=[{"role": "user", "content": prompt}],
    )
    raw = response.content[0].text.strip()
    return json.loads(raw)


def extract_notes_from_pdf_vision(pdf_path: str, periods: list[str]) -> dict:
    doc = fitz.open(pdf_path)
    pages_text = []
    for page in doc:
        pages_text.append(page.get_text())
    full_text = "\n".join(pages_text)
    doc.close()
    return extract_notes_from_text(full_text, periods)


def extract_notes_from_pdf(pdf_path: str, periods: list[str]) -> dict:
    with pdfplumber.open(pdf_path) as pdf:
        text_pages = [p.extract_text() or "" for p in pdf.pages]
    full_text = "\n".join(text_pages)

    result = extract_notes_from_text(full_text, periods)
    confidence = result.get("confidence", 0.0)

    if confidence < 0.75:
        result = extract_notes_from_pdf_vision(pdf_path, periods)

    return result


def scrape_ir_page_for_pdfs(ticker: str, company_name: str) -> list[str]:
    import requests
    from bs4 import BeautifulSoup

    client = anthropic.Anthropic()
    search_resp = client.messages.create(
        model="claude-sonnet-4-6",
        max_tokens=256,
        messages=[{
            "role": "user",
            "content": f"Return ONLY a URL for the investor relations page of {company_name} ({ticker}). No prose, just the URL."
        }],
    )
    ir_url = search_resp.content[0].text.strip()

    headers = {"User-Agent": "Mozilla/5.0 (compatible; FinancialModelBot/1.0)"}
    resp = requests.get(ir_url, headers=headers, timeout=15)
    soup = BeautifulSoup(resp.text, "lxml")

    pdf_links = []
    for a in soup.find_all("a", href=True):
        href = a["href"]
        text = a.get_text(strip=True).lower()
        if href.endswith(".pdf") and any(kw in text for kw in ["annual", "report", "20-f", "results"]):
            if not href.startswith("http"):
                from urllib.parse import urljoin
                href = urljoin(ir_url, href)
            pdf_links.append(href)

    return pdf_links[:6]  # limit to most recent filings
