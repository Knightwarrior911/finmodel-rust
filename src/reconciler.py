import json
import anthropic
from copy import deepcopy
from schemas.financial_data import ReconciledFinancialData, DiscrepancyReport, SourceCitation

RECONCILE_SYSTEM = """You are a senior financial analyst reconciling data extracted from company filings.
You receive: (1) main statement data from XBRL/PDF, (2) notes data extracted separately.

Rules:
- For every line item, find ALL mentions across both sources
- Cross-check: D&A on IS must match CFS add-back and notes schedule; total debt must match maturity sum; segment revenues must sum to consolidated; net income must equal EPS × shares
- Never silently pick a value if sources conflict — flag the discrepancy
- Source hierarchy: footnote schedule > main statement > MD&A
- Return JSON: {"confirmed": {item: note}, "discrepancies": ["description", ...], "notes_merged": {item: {period: value}}}
Return ONLY valid JSON."""

TOLERANCE_PCT = 0.02  # 2% tolerance for cross-check consistency


def check_consistency(data: ReconciledFinancialData) -> list[str]:
    errors = []
    bs = data.balance_sheet
    for i, period in enumerate(data.periods):
        assets = bs.get("total_assets", [None] * (i + 1))[i]
        liab = bs.get("total_liabilities", [None] * (i + 1))[i]
        equity = bs.get("total_equity", [None] * (i + 1))[i]
        rnci = (bs.get("redeemable_nci") or [None] * (i + 1))[i] or 0.0
        if assets is None or liab is None or equity is None:
            continue
        diff = abs(assets - (liab + equity + rnci))
        if diff > assets * TOLERANCE_PCT:
            errors.append(
                f"Balance sheet mismatch {period}: assets={assets:.0f}, L+E+RNCI={liab + equity + rnci:.0f}, diff={diff:.0f}"
            )

    is_data = data.income_statement
    for i, period in enumerate(data.periods):
        da_is = is_data.get("da", [None] * (i + 1))[i]
        da_notes = (data.notes.get("da") or {}).get("values", {}).get(period)
        if da_is and da_notes:
            diff = abs(da_is - da_notes)
            if diff > da_is * TOLERANCE_PCT:
                errors.append(f"D&A mismatch {period}: IS={da_is}, notes={da_notes}")

    return errors


def _merge_notes(data: ReconciledFinancialData, notes_merged: dict) -> ReconciledFinancialData:
    """Merge reconciled notes values back into statement dicts using note as authoritative source."""
    for item, period_values in notes_merged.items():
        if not isinstance(period_values, dict):
            continue
        vals = [period_values.get(p) for p in data.periods]
        if any(v is not None for v in vals):
            existing = data.income_statement.get(item, [None] * len(data.periods))
            data.income_statement[item] = [
                v if v is not None else existing[i]
                for i, v in enumerate(vals)
            ]
    return data


def reconcile(data: ReconciledFinancialData) -> tuple[ReconciledFinancialData, DiscrepancyReport]:
    data = deepcopy(data)  # don't mutate caller's object
    consistency_errors = check_consistency(data)

    # Skip LLM reconciliation when there are no notes to cross-check against.
    # This covers --direct mode (EDGAR-only, no PDF footnotes) and empty note sets.
    if not data.notes:
        data.flags = consistency_errors
        return data, DiscrepancyReport(items=consistency_errors)

    context = {
        "periods": data.periods,
        "income_statement": data.income_statement,
        "balance_sheet": data.balance_sheet,
        "cash_flow_statement": data.cash_flow_statement,
        "notes": data.notes,
    }

    from src.extractor import _llm_complete
    raw = _llm_complete(RECONCILE_SYSTEM, json.dumps(context, default=str), max_tokens=4096)
    try:
        result_json = json.loads(raw)
    except json.JSONDecodeError as e:
        raise ValueError(f"Reconciler LLM returned invalid JSON: {e}\nRaw: {raw[:200]}") from e

    discrepancies = result_json.get("discrepancies", []) + consistency_errors
    notes_merged = result_json.get("notes_merged", {})

    data = _merge_notes(data, notes_merged)
    data.flags = discrepancies

    return data, DiscrepancyReport(items=discrepancies)
