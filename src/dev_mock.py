"""
Development mock for LLM calls. Set FINMODEL_DEV_MOCK=1 to activate.
Stubs anthropic.Anthropic with realistic Atlas Copco (ATCO-B) responses
so the full pipeline runs end-to-end without an API key.
"""
import json
import os


def is_active() -> bool:
    return os.environ.get("FINMODEL_DEV_MOCK", "").strip() == "1"


# --- Canned responses ----------------------------------------------------------

_PREFLIGHT_ATCO = {
    "ticker": "ATCO-B.ST",
    "company_name": "Atlas Copco AB",
    "domicile": "SE",
    "currency": "SEK",
    "fiscal_year_end": "12-31",
    "sector": "standard",
    "sic": None,
    "ambiguity": None,
}

# Atlas Copco 2022-2024 IFRS data (SEK millions, rounded)
# Sources: published annual reports
_FINANCIALS_ATCO = {
    "currency": "SEK",
    "years_found": ["2022", "2023", "2024"],
    "income_statement": {
        "revenue":          [124528, 168343, 178200],
        "sga":              [30200,  40800,  43100],
        "da":               [6800,   9100,   9800],
        "ebit":             [26350,  35640,  37200],
        "interest_expense": [1200,   2100,   2400],
        "interest_income":  [400,    900,    1100],
        "income_tax":       [5900,   8400,   8800],
        "net_income":       [19400,  25800,  27000],
        "shares_diluted":   [4090,   4085,   4080],
    },
    "balance_sheet": {
        "cash":                 [20100, 18300, 22400],
        "accounts_receivable":  [28500, 35200, 36800],
        "inventory":            [19800, 25100, 24300],
        "total_current_assets": [74600, 86900, 91200],
        "ppe_net":              [22400, 28900, 30100],
        "goodwill":             [68300, 92500, 95200],
        "intangibles_net":      [18200, 24100, 23500],
        "total_assets":         [196000, 248000, 258000],
        "accounts_payable":     [13200, 17100, 17800],
        "long_term_debt":       [28500, 42300, 40100],
        "total_liabilities":    [112000, 148000, 152000],
        "total_equity":         [84000,  100000, 106000],
    },
    "cash_flow_statement": {
        "cfo":            [25200, 32100, 35400],
        "capex":          [5800,  8200,  8900],
        "dividends_paid": [9800,  12500, 13800],
    },
    "notes": {
        "tax_rate":          {"values": {"2022A": 0.233, "2023A": 0.245, "2024A": 0.246}},
        "lease_obligations": {"operating": 8200, "finance": 1100},
        "dso_days": 62,
        "dpo_days": 45,
        "dio_days": 52,
    },
    "confidence": 0.92,
    "discrepancies": [],
}


# --- Mock Anthropic client -----------------------------------------------------

class _MockMessage:
    def __init__(self, text: str):
        self.content = [_MockBlock(text)]
        self.stop_reason = "end_turn"


class _MockBlock:
    def __init__(self, text: str):
        self.text = text
        self.type = "text"


class _MockMessages:
    def create(self, *, model, max_tokens, messages, system=None, **kwargs):
        # Detect call site by max_tokens: preflight=512, extractor=8192
        if max_tokens <= 512:
            return _MockMessage(json.dumps(_PREFLIGHT_ATCO))
        return _MockMessage(json.dumps(_FINANCIALS_ATCO))


class MockAnthropicClient:
    messages = _MockMessages()


def patch_anthropic():
    """Monkey-patch anthropic.Anthropic to return MockAnthropicClient."""
    import anthropic
    anthropic.Anthropic = lambda **kw: MockAnthropicClient()
    print("[DEV MOCK] LLM calls stubbed with Atlas Copco (ATCO-B) canned data.")
