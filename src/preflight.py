import json
import anthropic
import requests
from schemas.financial_data import ModelConfig

EDGAR_HEADERS = {"User-Agent": "FinancialModelBot vinit.paul@gmail.com"}

SYSTEM_PROMPT = """You are a financial analyst tool. Given a company name or ticker, return JSON with:
{
  "ticker": "exchange-specific ticker (e.g. AAPL, HSBA.L, 7203.T)",
  "company_name": "full legal company name",
  "domicile": "US" or "non-US",
  "currency": "reporting currency ISO code",
  "fiscal_year_end": "month abbreviation e.g. Dec, Sep, Mar",
  "periods_historical": 5,
  "periods_projected": 5,
  "ambiguity": null or "clarification question if ticker is ambiguous"
}
Return ONLY valid JSON. No prose."""

_MONTH_ABR = {
    1: "Jan", 2: "Feb", 3: "Mar", 4: "Apr", 5: "May", 6: "Jun",
    7: "Jul", 8: "Aug", 9: "Sep", 10: "Oct", 11: "Nov", 12: "Dec",
}


def _edgar_preflight(ticker: str) -> tuple[str, str] | None:
    """Look up ticker in EDGAR. Returns (company_name, cik) or None if not found."""
    resp = requests.get(
        "https://www.sec.gov/files/company_tickers.json", headers=EDGAR_HEADERS, timeout=10
    )
    resp.raise_for_status()
    for entry in resp.json().values():
        if entry["ticker"] == ticker.upper():
            return entry["title"], str(entry["cik_str"]).zfill(10)
    return None


def _derive_fy_end_from_xbrl(cik: str) -> str:
    """Derive fiscal year end month from EDGAR XBRL data."""
    resp = requests.get(
        f"https://data.sec.gov/api/xbrl/companyfacts/CIK{cik}.json",
        headers=EDGAR_HEADERS, timeout=15,
    )
    if resp.status_code != 200:
        return "Dec"
    gaap = resp.json().get("facts", {}).get("us-gaap", {})
    # Use revenue tag to find FY end dates
    for tag in ["RevenueFromContractWithCustomerExcludingAssessedTax", "Revenues", "SalesRevenueNet"]:
        if tag not in gaap:
            continue
        entries = gaap[tag].get("units", {}).get("USD", [])
        annual = [e for e in entries if e.get("form") == "10-K" and e.get("fp") == "FY"]
        if annual:
            # Most recent FY end month
            latest = sorted(annual, key=lambda e: e["end"])[-1]
            month = int(latest["end"][5:7])
            return _MONTH_ABR.get(month, "Dec")
    return "Dec"


def run_preflight_direct(
    ticker: str,
    periods_historical: int = 5,
    periods_projected: int = 5,
    filing_override: str | None = None,
    force: bool = False,
) -> ModelConfig:
    """Preflight using EDGAR directly — no API key required. US tickers only."""
    result = _edgar_preflight(ticker)
    if result is None:
        raise ValueError(
            f"Ticker '{ticker}' not found in EDGAR. "
            f"--direct only works for US tickers. Remove --direct for non-US companies."
        )
    company_name, cik = result
    fiscal_year_end = _derive_fy_end_from_xbrl(cik)
    return ModelConfig(
        ticker=ticker.upper(),
        company_name=company_name,
        domicile="US",
        currency="USD",
        fiscal_year_end=fiscal_year_end,
        periods_historical=periods_historical,
        periods_projected=periods_projected,
        filing_override=filing_override,
        force=force,
    )


def run_preflight(
    user_input: str,
    periods_historical: int = 5,
    periods_projected: int = 5,
    filing_override: str | None = None,
    force: bool = False,
) -> ModelConfig:
    client = anthropic.Anthropic()
    response = client.messages.create(
        model="claude-sonnet-4-6",
        max_tokens=512,
        system=[{"type": "text", "text": SYSTEM_PROMPT, "cache_control": {"type": "ephemeral"}}],
        messages=[{"role": "user", "content": f"Company or ticker: {user_input}"}],
    )
    raw = response.content[0].text.strip()
    try:
        data = json.loads(raw)
    except json.JSONDecodeError as e:
        raise ValueError(f"Pre-flight LLM returned non-JSON: {raw}") from e

    if data.get("ambiguity"):
        raise ValueError(f"Ambiguous ticker — {data['ambiguity']}")

    return ModelConfig(
        ticker=data["ticker"],
        company_name=data["company_name"],
        domicile=data["domicile"],
        currency=data["currency"],
        fiscal_year_end=data["fiscal_year_end"],
        periods_historical=periods_historical,
        periods_projected=periods_projected,
        filing_override=filing_override,
        force=force,
    )
