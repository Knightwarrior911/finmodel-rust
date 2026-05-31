"""build_sources_report — a markdown provenance appendix for any finmodel answer.

Pure function over an extraction-cache dict. Summarises every tracked number by
trust tier (filing / market / derived / assumption / unverified) so a chat or
CLI answer can carry its full provenance. No I/O.
"""
from __future__ import annotations

from typing import Any

from src.source_ledger import SourceLedger, Tier


def _fmt_val(v: Any) -> str:
    if isinstance(v, float):
        return f"{v:g}"
    return str(v)


def build_sources_report(cache: dict[str, Any]) -> str:
    led = SourceLedger.from_json((cache or {}).get("__ledger__"))
    entries = led.entries()

    derived, assumptions, unverified, market = [], [], [], []
    for e in entries:
        if e.tier is Tier.DERIVED:
            derived.append(e)
        elif e.tier is Tier.ASSUMPTION:
            assumptions.append(e)
        elif e.tier is Tier.UNVERIFIED:
            unverified.append(e)
        elif e.tier is Tier.MARKET:
            market.append(e)

    lines = ["## Sources & Assumptions"]

    if market:
        items = [f"{e.field} — {e.ref.get('source', 'market')}" for e in market]
        lines.append("**Market data:** " + "; ".join(items))

    if derived:
        items = [f"{e.field} = {e.ref.get('formula', 'computed')}" for e in derived]
        lines.append("**Derived:** " + "; ".join(items))

    if assumptions:
        items = [f"{e.field} {_fmt_val(e.value)} ({e.ref.get('rationale', '')})"
                 for e in assumptions]
        lines.append("**Assumptions:** " + "; ".join(items))

    if unverified:
        items = [f"{e.field} ({e.ref.get('reason', 'no source')})" for e in unverified]
        lines.append("**⚠ Unverified (review):** " + "; ".join(items))

    if len(lines) == 1:
        lines.append("_No tracked numbers for this answer._")
    return "\n\n".join(lines)
