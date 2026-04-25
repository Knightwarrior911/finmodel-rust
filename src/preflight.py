import json
import anthropic
from schemas.financial_data import ModelConfig

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
