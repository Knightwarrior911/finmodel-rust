# financial_model/src/extractor.py
import json
import os
import re
import subprocess
from urllib.parse import quote, urljoin

import pdfplumber
from pathlib import Path


def _llm_complete(system_text: str, user_text: str, max_tokens: int) -> str:
    """Call the configured LLM provider and return the raw text response.

    Provider selection (checked in order):
      1. FINMODEL_TIEOUT_TRANSPORT=codex → read-only, ephemeral Codex CLI
      2. DEEPSEEK_API_KEY set   → DeepSeek (openai-compatible, ~10x cheaper)
      3. ANTHROPIC_API_KEY set  → Anthropic SDK
      4. Neither set            → Claude Code CLI (active Claude session)

    Override model with FINMODEL_LLM_MODEL or FINMODEL_TIEOUT_MODEL.
    """
    transport = os.environ.get("FINMODEL_TIEOUT_TRANSPORT", "claude").strip().lower()
    if transport == "codex":
        return _llm_complete_via_codex(system_text, user_text)
    if transport != "claude":
        raise ValueError("FINMODEL_TIEOUT_TRANSPORT must be claude or codex")

    deepseek_key = os.environ.get("DEEPSEEK_API_KEY", "").strip()
    anthropic_key = os.environ.get("ANTHROPIC_API_KEY", "").strip()

    if deepseek_key:
        from openai import OpenAI
        model = os.environ.get("FINMODEL_LLM_MODEL", "deepseek-chat")
        client = OpenAI(api_key=deepseek_key, base_url="https://api.deepseek.com")
        resp = client.chat.completions.create(
            model=model,
            messages=[
                {"role": "system", "content": system_text},
                {"role": "user",   "content": user_text},
            ],
            max_tokens=max_tokens,
            temperature=0,
        )
        return resp.choices[0].message.content.strip()

    if anthropic_key:
        import anthropic as _anthropic
        model = os.environ.get("FINMODEL_LLM_MODEL", "claude-sonnet-4-6")
        client = _anthropic.Anthropic()
        resp = client.messages.create(
            model=model,
            max_tokens=max_tokens,
            system=[{"type": "text", "text": system_text, "cache_control": {"type": "ephemeral"}}],
            messages=[{"role": "user", "content": user_text}],
        )
        return resp.content[0].text.strip()

    # Fallback: Claude Code CLI — no API key required, uses active session
    return _llm_complete_via_cli(system_text, user_text)


def _llm_complete_via_cli(system_text: str, user_text: str) -> str:
    """Run a one-shot query through the Claude Code CLI (`claude -p`).

    Uses the active Claude Code session — no ANTHROPIC_API_KEY needed.
    Always pipes user_text via stdin to avoid shell quoting issues on Windows
    (shell=True with a list converts to a string, breaking embedded JSON quotes).
    System prompt is written to a temp file for the same reason.
    """
    import tempfile as _tmp
    import sys as _sys

    # Write system prompt to temp file — avoids shell quoting of embedded quotes
    with _tmp.NamedTemporaryFile(mode="w", suffix=".txt", delete=False, encoding="utf-8") as sf:
        sf.write(system_text)
        sys_file = sf.name

    try:
        # Always pipe user_text via stdin — safest across all content types.
        # The -p task phrase is a stable string with no special characters.
        # Default to opus: the committed tie-out baseline was built with the
        # user's default (opus); sonnet mis-reads some lines (e.g. ATCO ppe_net,
        # gross vs. net). Override with FINMODEL_LLM_MODEL.
        _model = os.environ.get("FINMODEL_LLM_MODEL", "opus")
        claude_args = [
            "--model", _model,
            "--system-prompt-file", sys_file,
            "--output-format", "text",
            "-p", "Process the piped input per the system instructions and return only the requested JSON.",
        ]
        if _sys.platform == "win32":
            # On Windows, claude is a .CMD file — must run via cmd.exe.
            # Use raw bytes + manual UTF-8 decode to avoid cmd.exe codepage corruption.
            proc = subprocess.Popen(
                ["cmd", "/c", "claude"] + claude_args,
                stdin=subprocess.PIPE,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
            )
            stdout_bytes, stderr_bytes = proc.communicate(
                input=user_text.encode("utf-8"), timeout=300
            )
            out_raw = stdout_bytes.decode("utf-8", errors="replace")
            err_raw = stderr_bytes.decode("utf-8", errors="replace")
            rc = proc.returncode
        else:
            result = subprocess.run(
                ["claude"] + claude_args,
                input=user_text,
                capture_output=True,
                text=True,
                timeout=300,
                encoding="utf-8",
                errors="replace",
            )
            out_raw, err_raw, rc = result.stdout, result.stderr, result.returncode
    finally:
        os.unlink(sys_file)

    result = type("R", (), {"returncode": rc, "stdout": out_raw, "stderr": err_raw})()

    if result.returncode != 0:
        raise RuntimeError(
            f"claude CLI error (rc={result.returncode}): {result.stderr[:400]}"
        )
    out = result.stdout.strip()
    # Strip markdown code fences if present
    if out.startswith("```"):
        lines = out.split("\n")
        inner = lines[1:]
        if inner and inner[-1].strip() == "```":
            inner = inner[:-1]
        out = "\n".join(inner).strip()
    return out


def _llm_complete_via_codex(system_text: str, user_text: str) -> str:
    """Run one read-only, ephemeral Codex CLI request for the tie-out gate."""
    import tempfile as _tmp
    import sys as _sys

    with _tmp.NamedTemporaryFile(mode="w", suffix=".txt", delete=False, encoding="utf-8") as answer:
        answer_file = answer.name
    prompt = system_text + (chr(10) * 2) + "Process the following piped input per those instructions and return only the requested JSON." + (chr(10) * 2) + user_text
    try:
        model = (
            os.environ.get("FINMODEL_TIEOUT_MODEL")
            or os.environ.get("FINMODEL_LLM_MODEL")
            or "gpt-5.5"
        )
        args = [
            "exec", "--ephemeral", "--sandbox", "read-only",
            "--skip-git-repo-check", "--model", model,
            "--output-last-message", answer_file, "-",
        ]
        if _sys.platform == "win32":
            proc = subprocess.Popen(
                ["cmd", "/c", "codex"] + args,
                stdin=subprocess.PIPE,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
            )
            _stdout, stderr_bytes = proc.communicate(
                input=prompt.encode("utf-8"), timeout=300
            )
            err = stderr_bytes.decode("utf-8", errors="replace")
            rc = proc.returncode
        else:
            result = subprocess.run(
                ["codex"] + args,
                input=prompt, capture_output=True, text=True, timeout=300,
                encoding="utf-8", errors="replace",
            )
            err, rc = result.stderr, result.returncode
        if rc != 0:
            raise RuntimeError(f"codex CLI error (rc={rc}): {err[:400]}")
        with open(answer_file, encoding="utf-8") as output:
            out = output.read().strip()
    finally:
        try:
            os.unlink(answer_file)
        except FileNotFoundError:
            pass

    fence = chr(96) * 3
    if out.startswith(fence):
        lines = out.splitlines()
        inner = lines[1:]
        if inner and inner[-1].strip() == fence:
            inner = inner[:-1]
        out = chr(10).join(inner).strip()
    return out
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
    prompt = f"Periods in scope: {periods}\n\nFiling text:\n{text}"
    raw = _llm_complete(NOTES_SYSTEM_PROMPT, prompt, max_tokens=4096)
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


FINANCIALS_SYSTEM_PROMPT = """You are a senior financial analyst extracting structured financial data from annual report text.

Extract main income statement, balance sheet, and cash flow statement line items for ALL years present in the report (typically 2-3 comparative years). Also extract key footnote detail.

IMPORTANT RULES:
- All monetary values in MILLIONS (same currency as the filing)
- Arrays: oldest year first, newest year last — same length for every key
- capex: positive number (absolute cash outflow for PP&E purchases)
- income_tax: positive number (absolute tax charge)
- dividends_paid: positive number (absolute cash outflow)
- cfi: SIGNED total (negative = net outflow from investing; typical for industrial/manufacturing companies)
- cff: SIGNED total (negative = net outflow from financing)
- net_change_cash: SIGNED total (positive = increase in cash and equivalents)
- USE ONLY the CONSOLIDATED financial statements — never segment tables, parent-company, or subsidiary statements
- IFRS naming mappings (label in filing → JSON key):
    "Revenue" / "Net revenue" / "Net sales" / "Revenues" → revenue
    "Cost of sales" / "Cost of revenue" / "Cost of goods sold" → cogs
    "Gross profit" → gross_profit
    "Marketing expenses" + "Selling expenses" + "Administrative expenses" / "SG&A" → sga (SUM them if split). EXCLUDE "Distribution expenses" / "Logistics" / "Fulfilment" — those are COGS-type, NOT sga
    "Research and development expenses" / "R&D expenses" → rd
    "Operating profit" / "Operating income" / "EBIT" → ebit
    "EBITA" / "Earnings before interest, taxes and amortisation" → ebita
    "Financial expenses" / "Interest expense" / "Finance costs" → interest_expense
    "Financial income" / "Interest income" / "Finance income" → interest_income
    "Depreciation and amortization" / "D&A" from cash flow statement → da
    "Net cash from investing activities" / "Net cash used in investing activities" / "Cash flow from investment activities" → cfi
    "Net cash from financing activities" / "Net cash used in financing activities" / "Cash flow from financing activities" → cff
    "Net change in cash and cash equivalents" / "Net increase (decrease) in cash" / "Change in cash and cash equivalents" → net_change_cash
- da: take from the cash flow statement add-back line (most reliable source), NOT the income statement
- net_income: the TOTAL "Profit for the year" / "Profit for the period" / "Net profit" for the whole group INCLUDING non-controlling interests — NEVER the "attributable to owners/shareholders of the parent" sub-line
- shares_diluted: weighted average DILUTED shares in MILLIONS — NOT earnings per share
- If gross profit not shown separately and cogs not shown: omit both cogs and gross_profit
- Nordic/European numbers: "168 343" means 168,343 (space = thousands separator)
- If a line item is absent from the filing, omit its key entirely (do not include null or 0)

Return ONLY valid JSON in this exact structure (no prose, no markdown):
{
  "currency": "<3-letter code e.g. SEK, EUR, GBP>",
  "years_found": ["2022", "2023", "2024"],
  "income_statement": {
    "revenue":          [<2022>, <2023>, <2024>],
    "cogs":             [<2022>, <2023>, <2024>],
    "gross_profit":     [<2022>, <2023>, <2024>],
    "sga":              [<2022>, <2023>, <2024>],
    "rd":               [<2022>, <2023>, <2024>],
    "da":               [<2022>, <2023>, <2024>],
    "ebit":             [<2022>, <2023>, <2024>],
    "ebita":            [<2022>, <2023>, <2024>],
    "interest_expense": [<2022>, <2023>, <2024>],
    "interest_income":  [<2022>, <2023>, <2024>],
    "income_tax":       [<2022>, <2023>, <2024>],
    "net_income":       [<2022>, <2023>, <2024>],
    "shares_diluted":   [<2022>, <2023>, <2024>]
  },
  "balance_sheet": {
    "cash":                 [<2022>, <2023>, <2024>],
    "accounts_receivable":  [<2022>, <2023>, <2024>],
    "inventory":            [<2022>, <2023>, <2024>],
    "total_current_assets": [<2022>, <2023>, <2024>],
    "ppe_net":              [<2022>, <2023>, <2024>],
    "goodwill":             [<2022>, <2023>, <2024>],
    "intangibles_net":      [<2022>, <2023>, <2024>],
    "total_assets":         [<2022>, <2023>, <2024>],
    "accounts_payable":     [<2022>, <2023>, <2024>],
    "long_term_debt":       [<2022>, <2023>, <2024>],
    "total_liabilities":    [<2022>, <2023>, <2024>],
    "total_equity":         [<2022>, <2023>, <2024>]
  },
  "cash_flow_statement": {
    "cfo":             [<2022>, <2023>, <2024>],
    "capex":           [<2022>, <2023>, <2024>],
    "cfi":             [<2022>, <2023>, <2024>],
    "dividends_paid":  [<2022>, <2023>, <2024>],
    "cff":             [<2022>, <2023>, <2024>],
    "net_change_cash": [<2022>, <2023>, <2024>]
  },
  "notes": {
    "tax_rate":          {"values": {"2022A": <decimal>, "2023A": <decimal>, "2024A": <decimal>}},
    "debt_maturities":   {"2025": <val>, "2026": <val>, "2027": <val>},
    "sbc_expense":       {"values": {"2022A": <val>, "2023A": <val>, "2024A": <val>}},
    "lease_obligations": {"operating": <val>, "finance": <val>},
    "dso_days": <number or null>,
    "dpo_days": <number or null>,
    "dio_days": <number or null>
  },
  "confidence": <0.0 to 1.0>,
  "discrepancies": ["description of any conflicts or missing items"]
}"""


_BANK_SYSTEM_PROMPT = """You are a senior financial analyst extracting structured financial data from annual report text.

Extract main income statement, balance sheet, and cash flow statement line items for ALL years present in the report (typically 2-3 comparative years). Also extract key footnote detail.

IMPORTANT RULES:
- All monetary values in MILLIONS (same currency as the filing)
- Arrays: oldest year first, newest year last — same length for every key
- income_tax: positive number (absolute tax charge)
- cfi: SIGNED total (negative = net outflow from investing)
- cff: SIGNED total (negative = net outflow from financing)
- net_change_cash: SIGNED total (positive = increase in cash and equivalents)
- USE ONLY the CONSOLIDATED financial statements — never segment tables, parent-company, or subsidiary statements
- IFRS naming mappings (label in filing → JSON key):
    "Interest income" / "Interest and similar income" / "Interest and similar revenue" → interest_income
    "Interest expense" / "Interest and similar expense" / "Interest and similar charges" → interest_expense
    "Net interest income" / "Net interest and similar income" → net_interest_income
    "Fee and commission income" / "Net fee and commission income" / "Fees and commissions" → fee_commission_income
    "Net trading income" / "Trading income" / "Net gains on financial instruments at fair value" → trading_income
    "Total operating income" / "Total income" / "Operating income" → total_operating_income
    "Loan loss provisions" / "Impairment losses on loans" / "Credit loss expense" / "Net impairment on financial assets" → loan_loss_provisions
    "Operating expenses" / "Total operating expenses" / "General and administrative expenses" → operating_expenses
    "Profit before tax" / "Profit before income tax" / "Pre-tax profit" → pretax_income
    "Net cash from investing activities" / "Net cash used in investing activities" / "Cash flow from investment activities" → cfi
    "Net cash from financing activities" / "Net cash used in financing activities" / "Cash flow from financing activities" → cff
    "Net change in cash and cash equivalents" / "Net increase (decrease) in cash" / "Change in cash and cash equivalents" → net_change_cash
- net_income: the TOTAL "Profit for the year" / "Profit for the period" / "Net profit" for the whole group INCLUDING non-controlling interests — NEVER the "attributable to owners/shareholders of the parent" sub-line
- Nordic/European numbers: "168 343" means 168,343 (space = thousands separator)
- If a line item is absent from the filing, omit its key entirely (do not include null or 0)

Return ONLY valid JSON in this exact structure (no prose, no markdown):
{
  "currency": "<3-letter code e.g. SEK, EUR, GBP>",
  "years_found": ["2022", "2023", "2024"],
  "income_statement": {
    "interest_income":         [<2022>, <2023>, <2024>],
    "interest_expense":        [<2022>, <2023>, <2024>],
    "net_interest_income":     [<2022>, <2023>, <2024>],
    "fee_commission_income":   [<2022>, <2023>, <2024>],
    "trading_income":          [<2022>, <2023>, <2024>],
    "total_operating_income":  [<2022>, <2023>, <2024>],
    "loan_loss_provisions":    [<2022>, <2023>, <2024>],
    "operating_expenses":      [<2022>, <2023>, <2024>],
    "pretax_income":           [<2022>, <2023>, <2024>],
    "income_tax":              [<2022>, <2023>, <2024>],
    "net_income":              [<2022>, <2023>, <2024>]
  },
  "balance_sheet": {
    "cash_and_central_bank":  [<2022>, <2023>, <2024>],
    "loans_to_customers":     [<2022>, <2023>, <2024>],
    "investment_securities":  [<2022>, <2023>, <2024>],
    "total_assets":           [<2022>, <2023>, <2024>],
    "customer_deposits":      [<2022>, <2023>, <2024>],
    "debt_securities_issued": [<2022>, <2023>, <2024>],
    "total_liabilities":      [<2022>, <2023>, <2024>],
    "total_equity":           [<2022>, <2023>, <2024>]
  },
  "cash_flow_statement": {
    "cfo":             [<2022>, <2023>, <2024>],
    "cfi":             [<2022>, <2023>, <2024>],
    "cff":             [<2022>, <2023>, <2024>],
    "net_change_cash": [<2022>, <2023>, <2024>]
  },
  "notes": {
    "tax_rate":          {"values": {"2022A": <decimal>, "2023A": <decimal>, "2024A": <decimal>}},
    "debt_maturities":   {"2025": <val>, "2026": <val>, "2027": <val>},
    "sbc_expense":       {"values": {"2022A": <val>, "2023A": <val>, "2024A": <val>}},
    "lease_obligations": {"operating": <val>, "finance": <val>},
    "dso_days": <number or null>,
    "dpo_days": <number or null>,
    "dio_days": <number or null>
  },
  "confidence": <0.0 to 1.0>,
  "discrepancies": ["description of any conflicts or missing items"]
}"""


_INSURER_SYSTEM_PROMPT = """You are a senior financial analyst extracting structured financial data from annual report text.

Extract main income statement, balance sheet, and cash flow statement line items for ALL years present in the report (typically 2-3 comparative years). Also extract key footnote detail.

IMPORTANT RULES:
- All monetary values in MILLIONS (same currency as the filing)
- Arrays: oldest year first, newest year last — same length for every key
- income_tax: positive number (absolute tax charge)
- cfi: SIGNED total (negative = net outflow from investing)
- cff: SIGNED total (negative = net outflow from financing)
- net_change_cash: SIGNED total (positive = increase in cash and equivalents)
- USE ONLY the CONSOLIDATED financial statements — never segment tables, parent-company, or subsidiary statements
- IFRS naming mappings (label in filing → JSON key):
    "Gross written premium" / "Gross written premiums" / "Gross premiums written" → gross_written_premium
    "Net earned premium" / "Net earned premiums" / "Premiums earned, net" / "Net insurance revenue" → net_earned_premium
    "Net investment income" / "Investment income" / "Investment result" → net_investment_income
    "Net claims incurred" / "Claims incurred, net" / "Net insurance claims" / "Insurance service expense" → net_claims_incurred
    "Acquisition expenses" / "Acquisition costs" / "Deferred acquisition costs amortisation" / "Commission expenses" → acquisition_expenses
    "Operating expenses" / "Total operating expenses" / "Administrative expenses" → operating_expenses
    "Profit before tax" / "Profit before income tax" / "Pre-tax profit" → pretax_income
    "Net cash from investing activities" / "Net cash used in investing activities" / "Cash flow from investment activities" → cfi
    "Net cash from financing activities" / "Net cash used in financing activities" / "Cash flow from financing activities" → cff
    "Net change in cash and cash equivalents" / "Net increase (decrease) in cash" / "Change in cash and cash equivalents" → net_change_cash
- net_income: the TOTAL "Profit for the year" / "Profit for the period" / "Net profit" for the whole group INCLUDING non-controlling interests — NEVER the "attributable to owners/shareholders of the parent" sub-line
- Nordic/European numbers: "168 343" means 168,343 (space = thousands separator)
- If a line item is absent from the filing, omit its key entirely (do not include null or 0)

Return ONLY valid JSON in this exact structure (no prose, no markdown):
{
  "currency": "<3-letter code e.g. SEK, EUR, GBP>",
  "years_found": ["2022", "2023", "2024"],
  "income_statement": {
    "gross_written_premium": [<2022>, <2023>, <2024>],
    "net_earned_premium":    [<2022>, <2023>, <2024>],
    "net_investment_income": [<2022>, <2023>, <2024>],
    "net_claims_incurred":   [<2022>, <2023>, <2024>],
    "acquisition_expenses":  [<2022>, <2023>, <2024>],
    "operating_expenses":    [<2022>, <2023>, <2024>],
    "pretax_income":         [<2022>, <2023>, <2024>],
    "income_tax":            [<2022>, <2023>, <2024>],
    "net_income":            [<2022>, <2023>, <2024>]
  },
  "balance_sheet": {
    "investments":                    [<2022>, <2023>, <2024>],
    "cash":                           [<2022>, <2023>, <2024>],
    "total_assets":                   [<2022>, <2023>, <2024>],
    "insurance_contract_liabilities": [<2022>, <2023>, <2024>],
    "total_liabilities":              [<2022>, <2023>, <2024>],
    "total_equity":                   [<2022>, <2023>, <2024>]
  },
  "cash_flow_statement": {
    "cfo":             [<2022>, <2023>, <2024>],
    "cfi":             [<2022>, <2023>, <2024>],
    "cff":             [<2022>, <2023>, <2024>],
    "net_change_cash": [<2022>, <2023>, <2024>]
  },
  "notes": {
    "tax_rate":          {"values": {"2022A": <decimal>, "2023A": <decimal>, "2024A": <decimal>}},
    "debt_maturities":   {"2025": <val>, "2026": <val>, "2027": <val>},
    "sbc_expense":       {"values": {"2022A": <val>, "2023A": <val>, "2024A": <val>}},
    "lease_obligations": {"operating": <val>, "finance": <val>},
    "dso_days": <number or null>,
    "dpo_days": <number or null>,
    "dio_days": <number or null>
  },
  "confidence": <0.0 to 1.0>,
  "discrepancies": ["description of any conflicts or missing items"]
}"""


# INVARIANT: the JSON keys in the three prompts below MUST stay key-exact
# (same names, same order) with tieout.config.CANONICAL_BY_SECTOR[sector].
# Editing a sector schema there requires the matching prompt edit here.
_SYSTEM_PROMPT_BY_SECTOR = {
    "industrial": FINANCIALS_SYSTEM_PROMPT,
    "bank": _BANK_SYSTEM_PROMPT,
    "insurer": _INSURER_SYSTEM_PROMPT,
}


_CACHE_DIR = Path(__file__).parent.parent / "extraction_cache"


def _cache_path(ticker: str) -> Path:
    return _CACHE_DIR / f"{ticker.replace('/', '_').replace('.', '_')}.json"


def _load_cache(ticker: str) -> tuple[dict, dict, dict, dict, list[str]] | None:
    p = _cache_path(ticker)
    if not p.exists():
        return None
    with open(p) as f:
        data = json.load(f)
    return (
        data.get("income_statement", {}),
        data.get("balance_sheet", {}),
        data.get("cash_flow_statement", {}),
        data.get("notes", {}),
        data.get("years_found", []),
    )


def save_extraction_cache(ticker: str, data: dict) -> Path:
    """Write extracted financials JSON to cache. Called externally or after API extraction."""
    _CACHE_DIR.mkdir(exist_ok=True)
    p = _cache_path(ticker)
    with open(p, "w") as f:
        json.dump(data, f, indent=2)
    return p


def _extract_financial_section(text_pages: list[str], notes_window: int = 30) -> str:
    """Return the text of the consolidated financial statements section only.

    Strategy:
    1. Find the page where the IS/BS/CFS face statements start (anchor page).
    2. Return anchor page + next `notes_window` pages (covers statements + key notes).
    3. Falls back to first 150K chars of the full report if no anchor found.
    """
    # Headers that unambiguously mark the start of the financial statements section.
    _ANCHORS = [
        "consolidated income statement",
        "consolidated statement of profit",
        "consolidated statement of comprehensive income",
        "consolidated balance sheet",
        "consolidated statement of financial position",
        "consolidated statement of cash flow",
    ]

    # A real face statement page has an anchor phrase AND an actual revenue
    # DATA row (revenue synonym followed by >=2 multi-digit figures, allowing
    # European space/nbsp thousands). The contents/TOC page mentions the
    # phrase but has no data row — anchoring there (the old behaviour) fed the
    # LLM the table of contents instead of the statements for large reports.
    _REV_ROW = re.compile(
        r"(?:revenues?|net sales|net revenue|net turnover|turnover"
        r"|total revenue|sales revenue|net sales revenue)\b"
        r"[^\n]*?\d[\d   ]{2,}[^\n]*?\d[\d   ]{2,}", re.I)

    # The three face statements are often NOT contiguous (large reports
    # interleave dozens of note pages between them, or the IS+notes alone
    # exceed the char cap before the BS/CFS are reached). Locate EACH face
    # independently by its own header + a numeric data row, then concatenate
    # the focused slices so all three faces always reach the LLM.
    _FACE = {
        "is": (("consolidated income statement",
                "consolidated statement of profit",
                "consolidated statement of operations",
                "income statement", "statement of income",
                "statement of operations", "statement of profit or loss",
                "profit and loss account"),
               _REV_ROW),
        "bs": (("consolidated balance sheet",
                "consolidated statement of financial position",
                "balance sheet", "statement of financial position"),
               re.compile(r"total (?:assets|equity)\b[^\n]*?"
                          r"\d[\d   ]{2,}", re.I)),
        "cf": (("consolidated statement of cash flow",
                "consolidated cash flow statement",
                "statement of cash flows", "cash flow statement"),
               re.compile(r"(?:operating activities|net cash)\b[^\n]*?"
                          r"\d[\d   ]{2,}", re.I)),
    }

    slices: dict[int, str] = {}
    for phrases, data_row in _FACE.values():
        for i, page_text in enumerate(text_pages):
            t = page_text.lower()
            if (any(p in t for p in phrases)
                    and data_row.search(page_text)
                    and len(re.findall(r"\b20\d\d\b", page_text)) >= 2):
                for j in range(i, min(i + 4, len(text_pages))):
                    slices[j] = text_pages[j]
                break

    if slices:
        ordered = "\n".join(slices[k] for k in sorted(slices))
        if len(ordered) >= 3_000:
            return ordered[:150_000]

    # Fallback 1: first bare anchor-phrase page + window
    for i, page_text in enumerate(text_pages):
        if any(a in page_text.lower() for a in _ANCHORS):
            result = "\n".join(text_pages[i: i + notes_window])
            if len(result) >= 5_000:
                return result[:150_000]
            break

    # Fallback 2: head of full report
    return "\n".join(text_pages)[:150_000]


_BANK_SIGNATURES = (
    "net interest income", "loans and advances to customers",
    "due to customers", "interest and similar income",
)
_INSURER_SIGNATURES = (
    "gross written premium", "net earned premium",
    "insurance contract liabilities", "net claims incurred",
    "premiums earned",
)


def detect_sector(text_pages: list[str]) -> str:
    """Deterministic pre-LLM sector guess from filing face text.

    bank/insurer require >=2 distinct sector signatures so a passing
    mention in an industrial filing's notes does not misclassify it.
    Default is 'industrial'.
    """
    blob = "\n".join(text_pages[:80]).lower()  # primary statements live in the report front-half
    bank_hits = sum(1 for s in _BANK_SIGNATURES if s in blob)
    ins_hits = sum(1 for s in _INSURER_SIGNATURES if s in blob)
    if ins_hits >= 2 and ins_hits >= bank_hits:
        return "insurer"
    if bank_hits >= 2:
        return "bank"
    return "industrial"


def extract_financials_from_pdf(
    pdf_path: str, periods: list[str], ticker: str = ""
) -> tuple[dict, dict, dict, dict, list[str]]:
    """Extract IS/BS/CF statements + footnotes from a non-US annual report PDF.

    Returns: (income_statement, balance_sheet, cash_flow_statement, notes, years_found)
    Each statement dict maps key → list of floats, oldest period first.

    Checks extraction_cache/{ticker}.json first — if found, skips API call entirely.
    """
    # Cache hit — bypass API entirely
    if ticker:
        cached = _load_cache(ticker)
        if cached is not None:
            print(f"[extraction cache] loaded {_cache_path(ticker).name}")
            return cached

    with pdfplumber.open(pdf_path) as pdf:
        text_pages = [p.extract_text() or "" for p in pdf.pages]

    sector = detect_sector(text_pages)
    system_prompt = _SYSTEM_PROMPT_BY_SECTOR[sector]

    text_chunk = _extract_financial_section(text_pages)

    years = [p[:4] for p in periods]  # ["2023A","2024A"] → ["2023","2024"]

    prompt = (
        f"Extract data for these years (oldest first): {years}\n"
        f"Return arrays of length {len(years)} for every key.\n\n"
        f"Annual report text:\n{text_chunk}"
    )
    raw = _llm_complete(system_prompt, prompt, max_tokens=8192)
    if raw.startswith("```"):
        raw = raw.split("```")[1]
        if raw.startswith("json"):
            raw = raw[4:]
    try:
        data = json.loads(raw)
    except json.JSONDecodeError:
        # The model sometimes wraps the JSON in narrative prose ("Consolidated
        # only. Primary statements ...\n{ ... }"). Salvage the outermost JSON
        # object before failing — otherwise the whole company is dropped.
        a, b = raw.find("{"), raw.rfind("}")
        try:
            data = json.loads(raw[a:b + 1]) if a != -1 and b > a else None
        except json.JSONDecodeError:
            data = None
        if data is None:
            raise ValueError(
                f"Financials extractor returned invalid JSON\nRaw: {raw[:300]}")

    is_dict     = data.get("income_statement", {})
    bs_dict     = data.get("balance_sheet", {})
    cfs_dict    = data.get("cash_flow_statement", {})
    notes       = data.get("notes", {})
    years_found = data.get("years_found", [p[:4] for p in periods])

    # Truncate to len(periods) in case model returned more years than requested
    n = len(periods)
    for d in (is_dict, bs_dict, cfs_dict):
        for k in list(d.keys()):
            if isinstance(d[k], list):
                d[k] = d[k][:n]

    return is_dict, bs_dict, cfs_dict, notes, years_found


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
