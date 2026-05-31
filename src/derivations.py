"""Derive-first valuation inputs from extracted historical actuals.

Each function returns (value | None, lineage) where lineage is
(formula_str, inputs_list); inputs_list is a list of (statement, key, period)
tuples. None is returned when required actuals are missing or the derived
value fails its validity guard (in which case the caller falls back to a
declared assumption, then UNVERIFIED).

Values are averaged across all historical periods present; None entries in a
series are skipped.
"""
from __future__ import annotations

from typing import Optional


def _clean_pairs(*series: list) -> list[tuple]:
    """Zip series, dropping any tuple where any element is None or non-numeric."""
    out = []
    n = min((len(s) for s in series), default=0)
    for i in range(n):
        row = [s[i] for s in series]
        if any(x is None for x in row):
            continue
        try:
            row = [float(x) for x in row]
        except (TypeError, ValueError):
            continue
        out.append(tuple(row))
    return out


def _avg(vals: list[float]) -> Optional[float]:
    return sum(vals) / len(vals) if vals else None


def effective_tax_rate(is_: dict):
    pairs = _clean_pairs(is_.get("income_tax", []), is_.get("net_income", []))
    rates = []
    for tax, ni in pairs:
        pretax = ni + tax
        if pretax <= 0:
            continue
        rates.append(tax / pretax)
    v = _avg(rates)
    if v is None or not (0.0 <= v <= 0.50):
        return None, None
    return v, ("income_tax / (net_income + income_tax)",
               [("income_statement", "income_tax", "hist_avg"),
                ("income_statement", "net_income", "hist_avg")])


def cost_of_debt(is_: dict, bs: dict):
    pairs = _clean_pairs(is_.get("interest_expense", []), bs.get("long_term_debt", []))
    rates = [ie / debt for ie, debt in pairs if debt > 0]
    v = _avg(rates)
    if v is None or not (0.005 <= v <= 0.20):
        return None, None
    return v, ("interest_expense / long_term_debt",
               [("income_statement", "interest_expense", "hist_avg"),
                ("balance_sheet", "long_term_debt", "hist_avg")])


def cash_yield(is_: dict, bs: dict):
    pairs = _clean_pairs(is_.get("interest_income", []), bs.get("cash", []))
    rates = [ii / cash for ii, cash in pairs if cash > 0]
    v = _avg(rates)
    if v is None or not (0.0 <= v <= 0.15):
        return None, None
    return v, ("interest_income / cash",
               [("income_statement", "interest_income", "hist_avg"),
                ("balance_sheet", "cash", "hist_avg")])


def da_pct(is_: dict):
    pairs = _clean_pairs(is_.get("da", []), is_.get("revenue", []))
    rates = [da / rev for da, rev in pairs if rev > 0]
    v = _avg(rates)
    if v is None or not (0.0 <= v <= 0.50):
        return None, None
    return v, ("da / revenue",
               [("income_statement", "da", "hist_avg"),
                ("income_statement", "revenue", "hist_avg")])


def _days(numer: list, denom: list, numer_key, denom_key, numer_stmt, denom_stmt):
    pairs = _clean_pairs(numer, denom)
    vals = [n / d * 365.0 for n, d in pairs if d > 0]
    v = _avg(vals)
    if v is None or not (0.0 <= v <= 365.0):
        return None, None
    return v, (f"{numer_key} / {denom_key} * 365",
               [(numer_stmt, numer_key, "hist_avg"), (denom_stmt, denom_key, "hist_avg")])


def wc_days(is_: dict, bs: dict) -> dict:
    return {
        "dso": _days(bs.get("accounts_receivable", []), is_.get("revenue", []),
                     "accounts_receivable", "revenue", "balance_sheet", "income_statement"),
        "dio": _days(bs.get("inventory", []), is_.get("cogs", []),
                     "inventory", "cogs", "balance_sheet", "income_statement"),
        "dpo": _days(bs.get("accounts_payable", []), is_.get("cogs", []),
                     "accounts_payable", "cogs", "balance_sheet", "income_statement"),
    }
