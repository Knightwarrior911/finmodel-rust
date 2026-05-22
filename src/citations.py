"""Market-data citations — link source for numbers that come from data providers
(yfinance / EDGAR) rather than the company's own filing PDFs.

Filing numbers get `finmodelaudit:` page links (see audit_open.py). But comps,
peer betas, market cap, risk-free rate, etc. are fetched live from data
providers and have no PDF page. Those are cited to the provider's quote page
(a plain https URL, which Excel opens directly — no fragment, no handler).

The audit post-pass (audit_pipeline.annotate_workbook_with_links) matches a
numeric cell's value against these citations and attaches the URL.

Public API:
    MarketCitation                       — one value -> provider URL
    collect_market_citations(...)        — build them from the output objects
    citations_payload(cites)             — JSON list stored under cache __citations__
    persist_citations(cache_path, cites) — write the block into the cache JSON
"""
from __future__ import annotations

import json
from dataclasses import dataclass, asdict
from datetime import date
from pathlib import Path
from typing import Any, Iterable, Optional
from urllib.parse import quote

YAHOO_QUOTE = "https://finance.yahoo.com/quote/{}"


def yahoo_url(ticker: str) -> str:
    return YAHOO_QUOTE.format(quote(str(ticker), safe=""))


@dataclass
class MarketCitation:
    value: float
    label: str          # e.g. "MSFT market cap", "Risk-free rate (10Y)"
    url: str            # provider page (https)
    source: str         # "yfinance" | "yfinance/edgar" | "assumption"
    as_of: str          # ISO date

    def to_json(self) -> dict[str, Any]:
        return asdict(self)

    @classmethod
    def from_json(cls, d: dict[str, Any]) -> "MarketCitation":
        return cls(
            value=float(d["value"]), label=d["label"], url=d["url"],
            source=d.get("source", ""), as_of=d.get("as_of", ""),
        )


def _peer_fields(p: Any) -> list[tuple[float, str]]:
    """(value, suffix) market-data fields on a PublicCompPeer."""
    out: list[tuple[float, str]] = []
    for attr, suffix in [
        ("share_price", "share price"), ("market_cap", "market cap"),
        ("shares_diluted", "diluted shares"), ("total_debt", "total debt"),
        ("cash", "cash"), ("week52_high", "52-week high"),
        ("week52_low", "52-week low"), ("ltm_revenue", "LTM revenue"),
        ("ltm_ebitda", "LTM EBITDA"), ("ltm_ebit", "LTM EBIT"),
        ("ltm_net_income", "LTM net income"), ("ltm_eps_diluted", "LTM EPS"),
    ]:
        v = getattr(p, attr, None)
        if v:
            out.append((float(v), suffix))
    return out


def collect_market_citations(
    *,
    public_comps: Any = None,
    peer_set: Any = None,
    wacc: Any = None,
    assumptions: Any = None,
    target_ticker: Optional[str] = None,
    as_of: Optional[str] = None,
) -> list[MarketCitation]:
    """Build market-data citations from already-computed output objects.

    Does NOT fetch anything — it reads the values the producers already returned,
    so the cited value matches what was written to the workbook.
    """
    today = as_of or date.today().isoformat()
    cites: list[MarketCitation] = []

    # Comps peers — market data + LTM stats (yfinance / EDGAR).
    if public_comps is not None:
        for p in getattr(public_comps, "peers", []) or []:
            url = yahoo_url(p.ticker)
            for val, suffix in _peer_fields(p):
                cites.append(MarketCitation(
                    val, f"{p.ticker} {suffix}", url, "yfinance/edgar", today))

    # WACC peer set — levered beta, D/E, market cap (yfinance).
    if peer_set is not None:
        for p in getattr(peer_set, "peers", []) or []:
            url = yahoo_url(p.ticker)
            for attr, suffix in [("levered_beta", "levered beta"),
                                 ("de_ratio", "D/E ratio"),
                                 ("market_cap", "market cap")]:
                v = getattr(p, attr, None)
                if v:
                    cites.append(MarketCitation(
                        float(v), f"{p.ticker} {suffix}", url, "yfinance", today))

    # Target-level market inputs.
    tgt = target_ticker or getattr(peer_set, "target_ticker", None) \
        or getattr(public_comps, "target_ticker", None)
    if wacc is not None:
        if getattr(wacc, "risk_free_rate", 0):
            cites.append(MarketCitation(
                float(wacc.risk_free_rate), "Risk-free rate (10Y Treasury, ^TNX)",
                yahoo_url("^TNX"), "yfinance", today))
        if tgt and getattr(wacc, "target_market_cap", 0):
            cites.append(MarketCitation(
                float(wacc.target_market_cap), f"{tgt} market cap",
                yahoo_url(tgt), "yfinance", today))
    if assumptions is not None and tgt:
        price = getattr(assumptions, "current_price", 0) \
            or getattr(assumptions, "current_share_price", 0)
        if price:
            cites.append(MarketCitation(
                float(price), f"{tgt} current share price",
                yahoo_url(tgt), "yfinance", today))

    # De-duplicate identical (value,label,url).
    seen: set[tuple] = set()
    uniq: list[MarketCitation] = []
    for c in cites:
        k = (round(c.value, 6), c.label, c.url)
        if k not in seen:
            seen.add(k)
            uniq.append(c)
    return uniq


def citations_payload(cites: Iterable[MarketCitation]) -> list[dict[str, Any]]:
    return [c.to_json() for c in cites]


def load_citations(payload: Any) -> list[MarketCitation]:
    out: list[MarketCitation] = []
    for d in payload or []:
        if isinstance(d, dict):
            try:
                out.append(MarketCitation.from_json(d))
            except (KeyError, ValueError, TypeError):
                continue
    return out


def persist_citations(cache_path: str | Path, cites: Iterable[MarketCitation]) -> int:
    """Write the citations list into the cache JSON under `__citations__`.
    Returns the count written."""
    cache_path = Path(cache_path)
    cache = json.loads(cache_path.read_text(encoding="utf-8"))
    payload = citations_payload(cites)
    cache["__citations__"] = payload
    cache_path.write_text(json.dumps(cache, indent=2), encoding="utf-8")
    return len(payload)
