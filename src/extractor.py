# financial_model/src/extractor.py
import json
import subprocess
from urllib.parse import quote, urljoin

import anthropic
import pdfplumber
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
    try:
        return json.loads(raw)
    except json.JSONDecodeError as e:
        raise ValueError(f"Extractor LLM returned invalid JSON: {e}\nRaw: {raw[:200]}") from e


def extract_notes_from_pdf(pdf_path: str, periods: list[str]) -> dict:
    with pdfplumber.open(pdf_path) as pdf:
        text_pages = [p.extract_text() or "" for p in pdf.pages]
    full_text = "\n".join(text_pages)

    result = extract_notes_from_text(full_text, periods)

    if result.get("confidence", 1.0) < 0.75:
        result.setdefault("discrepancies", []).append(
            "Low extraction confidence — PDF may be image-based or poorly structured"
        )

    return result


def _find_ir_url_via_browser(company_name: str, ticker: str) -> str:
    """Use actionbook browser (extension mode) to find the company IR page URL."""
    session = "fm_ir_search"
    query = f"{company_name} {ticker} investor relations annual report"
    search_url = f"https://www.google.com/search?q={quote(query)}"

    # Start headed browser (extension mode)
    subprocess.run(
        ["actionbook", "browser", "start", "--set-session-id", session, "--headed"],
        capture_output=True, check=False
    )
    subprocess.run(
        ["actionbook", "browser", "goto", search_url, "--session", session, "--tab", "t1"],
        check=True
    )
    result = subprocess.run(
        ["actionbook", "browser", "text", "--session", session, "--tab", "t1"],
        capture_output=True, text=True
    )
    subprocess.run(
        ["actionbook", "browser", "close", "--session", session],
        capture_output=True, check=False
    )

    page_text = result.stdout[:4000]
    client = anthropic.Anthropic()
    resp = client.messages.create(
        model="claude-sonnet-4-6",
        max_tokens=256,
        messages=[{
            "role": "user",
            "content": (
                f"From these Google search results for '{company_name} investor relations', "
                f"extract ONLY the URL of the official investor relations or annual reports page. "
                f"Return just the URL, no prose.\n\n{page_text}"
            )
        }]
    )
    return resp.content[0].text.strip()


def _scrape_pdfs_from_url(ir_url: str) -> list[str]:
    import requests
    from bs4 import BeautifulSoup

    headers = {"User-Agent": "Mozilla/5.0 (compatible; FinancialModelBot/1.0)"}
    resp = requests.get(ir_url, headers=headers, timeout=15)
    soup = BeautifulSoup(resp.text, "lxml")

    pdf_links = []
    for a in soup.find_all("a", href=True):
        href = a["href"]
        text = a.get_text(strip=True).lower()
        if href.endswith(".pdf") and any(kw in text for kw in ["annual", "report", "20-f", "results"]):
            if not href.startswith("http"):
                href = urljoin(ir_url, href)
            pdf_links.append(href)

    return pdf_links[:6]


def scrape_ir_page_for_pdfs(
    ticker: str, company_name: str, ir_url: str | None = None
) -> list[str]:
    """Find and return annual report PDF URLs from a company's IR page.

    If ir_url is provided, scrapes it directly.
    Otherwise uses actionbook browser to discover the IR page via Google search.
    """
    if ir_url:
        return _scrape_pdfs_from_url(ir_url)
    ir_url = _find_ir_url_via_browser(company_name, ticker)
    return _scrape_pdfs_from_url(ir_url)
