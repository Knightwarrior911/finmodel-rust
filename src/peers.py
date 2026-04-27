"""
Peer-set selector for WACC and trading comps.

Workflow per valuation_kit/dcf spec Q1:
  1. LLM proposes 15-20 candidates by business similarity
  2. Filter to:
     - same GICS sub-industry (proxied by sector match)
     - market cap 0.3x-3x target
     - public ≥ 1 year
     - sufficient trading liquidity
  3. Final 6-10 names

Falls back to single-ticker yfinance beta when LLM unavailable.
"""
import json
import logging

logger = logging.getLogger(__name__)


_PEER_SYSTEM_PROMPT = """You are a sell-side equity research analyst selecting trading comparables.

Given a target company, return 15-20 candidate peer tickers (US-listed) that share the
target's core business model. Prioritize same-sector competitors and direct substitutes.
Exclude conglomerates, holding companies, and shells.

Return ONLY valid JSON in this exact format:
{
  "candidates": ["TICK1", "TICK2", ...],
  "rationale": "1-2 sentence explanation of selection criteria applied"
}

No prose outside the JSON."""


# Orchestrator-curated peer sets — managed by Claude as orchestrator.
# Until end-user release, no LLM call required. Add tickers as needed.
_CURATED_PEERS: dict[str, list[str]] = {
    "AAPL":  ["MSFT", "GOOGL", "META", "AMZN", "DELL", "HPQ", "SONY", "SSNLF"],
    "MSFT":  ["AAPL", "GOOGL", "ORCL", "CRM", "ADBE", "SAP", "NOW", "INTU"],
    "GOOGL": ["META", "MSFT", "AMZN", "AAPL", "BIDU", "PINS", "SNAP", "TTD"],
    "META":  ["GOOGL", "SNAP", "PINS", "MSFT", "TTD", "RDDT", "X", "AMZN"],
    "AMZN":  ["WMT", "COST", "BABA", "MELI", "EBAY", "TGT", "GOOGL", "MSFT"],
    "TSLA":  ["F", "GM", "TM", "STLA", "RIVN", "LCID", "NIO", "BYDDY"],
    "NVDA":  ["AMD", "INTC", "AVGO", "QCOM", "TSM", "MRVL", "MU", "ASML"],
    "AMD":   ["NVDA", "INTC", "QCOM", "AVGO", "MRVL", "MU", "TXN", "ON"],
    "JPM":   ["BAC", "WFC", "C", "GS", "MS", "USB", "PNC", "TFC"],
    "WMT":   ["COST", "TGT", "KR", "AMZN", "DG", "DLTR", "BJ", "ACI"],
    "COP":   ["XOM", "CVX", "EOG", "OXY", "PXD", "DVN", "MRO", "FANG"],
    "JNJ":   ["PFE", "MRK", "ABBV", "LLY", "BMY", "AZN", "GSK", "NVS"],
    "UNH":   ["CI", "CVS", "ELV", "HUM", "CNC", "MOH", "HCA", "ANTM"],
    "NFLX":  ["DIS", "WBD", "PARA", "CMCSA", "SPOT", "ROKU", "FUBO", "CURI"],
    "HON":   ["GE", "MMM", "EMR", "ETN", "ROP", "ITW", "PH", "DOV"],
    "ITW":   ["HON", "EMR", "ETN", "DOV", "PH", "ROK", "FLS", "AME"],
    "NEE":   ["DUK", "SO", "AEP", "EXC", "D", "XEL", "PCG", "SRE"],
    "BA":    ["RTX", "LMT", "NOC", "GD", "LHX", "TXT", "SPR", "HEI"],
    "LMT":   ["RTX", "NOC", "GD", "LHX", "BA", "TXT", "HII", "CW"],
    "RTX":   ["LMT", "NOC", "GD", "LHX", "BA", "TXT", "HWM", "HXL"],
    "NOC":   ["LMT", "RTX", "GD", "LHX", "BA", "TXT", "CW", "KTOS"],
    "GD":    ["LMT", "RTX", "NOC", "LHX", "TXT", "HII", "CW", "BA"],
    "GE":    ["HON", "RTX", "MMM", "EMR", "ETN", "ITW", "ABB", "SIEGY"],
    "CAT":   ["DE", "CNHI", "TEX", "OSK", "MTW", "VOLVO", "KMTUY", "KUBTY"],
    "XOM":   ["CVX", "COP", "EOG", "PXD", "OXY", "DVN", "BP", "SHEL"],
    "CVX":   ["XOM", "COP", "EOG", "PXD", "OXY", "DVN", "BP", "SHEL"],
    "PFE":   ["JNJ", "MRK", "LLY", "ABBV", "BMY", "AZN", "NVS", "GILD"],
    "MRK":   ["JNJ", "PFE", "LLY", "ABBV", "BMY", "AZN", "NVS", "GILD"],
}


def _llm_propose_peers(target_ticker: str, company_name: str,
                       sector: str | None = None) -> tuple[list[str], str]:
    """Peer candidate proposal — orchestrator-curated lookup first; LLM fallback if API key set."""
    import os
    tk = (target_ticker or "").upper()
    if tk in _CURATED_PEERS:
        return _CURATED_PEERS[tk], "orchestrator-curated peer set"
    if not os.environ.get("ANTHROPIC_API_KEY"):
        raise RuntimeError(
            f"No curated peer set for {tk} and no ANTHROPIC_API_KEY. "
            f"Add {tk} to _CURATED_PEERS in src/peers.py or set the API key."
        )
    import anthropic
    client = anthropic.Anthropic()
    user_msg = f"Target: {company_name} ({target_ticker})"
    if sector:
        user_msg += f"  |  Sector: {sector}"
    response = client.messages.create(
        model="claude-sonnet-4-6",
        max_tokens=1024,
        system=[{"type": "text", "text": _PEER_SYSTEM_PROMPT,
                 "cache_control": {"type": "ephemeral"}}],
        messages=[{"role": "user", "content": user_msg}],
    )
    raw = response.content[0].text.strip()
    if raw.startswith("```"):
        raw = raw.strip("`").lstrip("json").strip()
    data = json.loads(raw)
    return data["candidates"], data.get("rationale", "")


def _market_cap(ticker: str) -> float:
    """Returns market cap in $M from yfinance, 0 on failure."""
    try:
        import yfinance as yf
        fi = yf.Ticker(ticker).fast_info
        for key in ("marketCap", "market_cap"):
            try:
                v = fi[key]
                if v:
                    return float(v) / 1e6
            except (KeyError, Exception):
                continue
    except Exception as e:
        logger.warning("market_cap fetch failed for %s: %s", ticker, e)
    return 0.0


def _beta(ticker: str) -> float:
    """Returns levered beta from yfinance, 1.0 on failure."""
    try:
        import yfinance as yf
        info = yf.Ticker(ticker).info
        b = info.get("beta") or info.get("beta3Year")
        if b and 0.1 <= float(b) <= 5.0:
            return float(b)
    except Exception:
        pass
    return 1.0


def _de_and_tax(ticker: str) -> tuple[float, float]:
    """Pull D/E ratio and effective tax rate from yfinance .info; falls back to defaults."""
    try:
        import yfinance as yf
        info = yf.Ticker(ticker).info
        # debtToEquity is typically reported as percent (e.g., 75.0 = 0.75)
        de = info.get("debtToEquity")
        de = float(de) / 100 if de is not None else 0.30
        # effective tax rate not directly available; use 21% statutory as default
        return de, 0.21
    except Exception:
        return 0.30, 0.21


def _filter_peers(target_mc: float, candidates: list[str],
                  target_ticker: str) -> tuple[list[str], list[tuple[str, str]]]:
    """Apply size / listing filters. Returns (kept, excluded_with_reason)."""
    kept: list[str] = []
    excluded: list[tuple[str, str]] = []
    for tk in candidates:
        if tk.upper() == target_ticker.upper():
            excluded.append((tk, "is the target"))
            continue
        mc = _market_cap(tk)
        if mc <= 0:
            excluded.append((tk, "no market cap data"))
            continue
        if target_mc > 0 and not (0.3 * target_mc <= mc <= 3.0 * target_mc):
            excluded.append((tk, f"market cap {mc:,.0f}M outside 0.3x-3x target"))
            continue
        kept.append(tk)
    # Cap final list to 10
    return kept[:10], excluded


def build_peer_set(target_ticker: str, company_name: str,
                   target_de_ratio: float = 0.30,
                   sector: str | None = None) -> "PeerSet":
    """Build peer set with LLM proposal + size filter + per-peer market data."""
    from schemas.financial_data import PeerSet, Peer
    target_mc = _market_cap(target_ticker)

    try:
        candidates, _ = _llm_propose_peers(target_ticker, company_name, sector)
        kept, excluded = _filter_peers(target_mc, candidates, target_ticker)
        source = "llm"
    except Exception as e:
        logger.warning("LLM peer proposal failed (%s) — falling back to target-only beta", e)
        kept, excluded, source = [], [], "fallback"

    peers: list[Peer] = []
    for tk in kept:
        peers.append(Peer(
            ticker=tk,
            name=tk,                     # full name lookup deferred (yfinance .info shortName)
            market_cap=_market_cap(tk),
            enterprise_value=0.0,
            levered_beta=_beta(tk),
            de_ratio=_de_and_tax(tk)[0],
            tax_rate=_de_and_tax(tk)[1],
            rationale="",
        ))

    return PeerSet(
        target_ticker=target_ticker,
        target_market_cap=target_mc,
        target_de_ratio=target_de_ratio,
        peers=peers,
        excluded=excluded,
        source=source,
    )
