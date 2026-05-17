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
  "fiscal_year_end": "MM-DD format e.g. 12-31 for December, 09-30 for September, 03-31 for March",
  "periods_historical": 3,
  "periods_projected": 5,
  "ambiguity": null or "clarification question if ticker is ambiguous"
}
Return ONLY valid JSON. No prose.

Exchange suffix rules — use EXACTLY as given:
- .ST = Stockholm (Nasdaq Stockholm), e.g. ATCO-B.ST = Atlas Copco B-share
- .L  = London Stock Exchange
- .DE = Xetra (Frankfurt)
- .PA = Euronext Paris
- .T  = Tokyo Stock Exchange
- No suffix = US (NYSE/NASDAQ)

IMPORTANT: The ticker suffix tells you the exchange. ATCO-B.ST is NOT AstraZeneca (AZN). Respect the suffix."""

_MONTH_ABR = {
    1: "Jan", 2: "Feb", 3: "Mar", 4: "Apr", 5: "May", 6: "Jun",
    7: "Jul", 8: "Aug", 9: "Sep", 10: "Oct", 11: "Nov", 12: "Dec",
}

_MONTH_TO_NUM = {v.lower(): k for k, v in _MONTH_ABR.items()}
_MONTH_TO_LAST_DAY = {1:31,2:28,3:31,4:30,5:31,6:30,7:31,8:31,9:30,10:31,11:30,12:31}


def _normalize_fy_end(raw: str) -> str:
    """Coerce any fiscal_year_end format to MM-DD.

    Accepts: '12-31', 'Dec-31', 'Dec', 'december', '12/31'
    """
    if not raw:
        return "12-31"
    raw = raw.strip().replace("/", "-")
    # Already MM-DD
    parts = raw.split("-")
    if len(parts) == 2:
        try:
            m, d = int(parts[0]), int(parts[1])
            return f"{m:02d}-{d:02d}"
        except ValueError:
            # e.g. 'Dec-31'
            m_num = _MONTH_TO_NUM.get(parts[0].lower()[:3])
            if m_num:
                try:
                    return f"{m_num:02d}-{int(parts[1]):02d}"
                except ValueError:
                    return f"{m_num:02d}-{_MONTH_TO_LAST_DAY[m_num]:02d}"
    # Just a month name e.g. 'Dec'
    m_num = _MONTH_TO_NUM.get(raw.lower()[:3])
    if m_num:
        return f"{m_num:02d}-{_MONTH_TO_LAST_DAY[m_num]:02d}"
    return "12-31"


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


def _derive_sic_and_sector(cik: str) -> tuple[int, str]:
    """Fetch SIC code from EDGAR submissions API and map to sector."""
    try:
        resp = requests.get(
            f"https://data.sec.gov/submissions/CIK{cik}.json",
            headers=EDGAR_HEADERS, timeout=10,
        )
        if resp.status_code != 200:
            return 0, "standard"
        data = resp.json()
        sic = int(data.get("sic") or 0)
    except Exception:
        return 0, "standard"

    if 4900 <= sic <= 4999:
        sector = "utility"
    elif 6000 <= sic <= 6299:
        sector = "bank"
    elif 6311 <= sic <= 6411:
        sector = "insurance"
    elif 6500 <= sic <= 6599 or sic == 6798:
        sector = "reit"
    else:
        sector = "standard"
    return sic, sector


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
    periods_historical: int = 3,
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
    sic, sector = _derive_sic_and_sector(cik)
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
        sic=sic,
        sector=sector,
    )


def run_preflight(
    user_input: str,
    periods_historical: int = 3,
    periods_projected: int = 5,
    filing_override: str | None = None,
    force: bool = False,
) -> ModelConfig:
    from src.extractor import _llm_complete
    raw = _llm_complete(SYSTEM_PROMPT, f"Company or ticker: {user_input}", max_tokens=512)
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
        fiscal_year_end=_normalize_fy_end(data.get("fiscal_year_end", "12-31")),
        periods_historical=periods_historical,
        periods_projected=periods_projected,
        filing_override=filing_override,
        force=force,
    )
