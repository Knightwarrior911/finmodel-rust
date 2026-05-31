"""SourceLedger — per-number trust-tier record for finmodel outputs.

Every valuation/assumption number records which tier it belongs to and why:
FILING (extracted), MARKET (live provider), DERIVED (computed from tagged
inputs), ASSUMPTION (declared forward input), UNVERIFIED (no source / silent
default fired). The ledger is built during model construction and persisted
into extraction_cache/{ticker}.json under "__ledger__"; the Excel audit pass
reads it to colour + comment cells.

Key = (group, field, period). period may be None for non-period scalars.
"""
from __future__ import annotations

from dataclasses import dataclass, field
from enum import Enum
from typing import Any, Optional


class Tier(str, Enum):
    FILING = "filing"
    MARKET = "market"
    DERIVED = "derived"
    ASSUMPTION = "assumption"
    UNVERIFIED = "unverified"


@dataclass
class LedgerEntry:
    group: str
    field: str
    period: Optional[str]
    value: Optional[float]
    tier: Tier
    ref: dict[str, Any] = field(default_factory=dict)

    def to_json(self) -> dict[str, Any]:
        return {
            "group": self.group, "field": self.field, "period": self.period,
            "value": self.value, "tier": self.tier.value, "ref": self.ref,
        }

    @classmethod
    def from_json(cls, d: dict[str, Any]) -> "LedgerEntry":
        return cls(
            group=d["group"], field=d["field"], period=d.get("period"),
            value=d.get("value"), tier=Tier(d["tier"]), ref=d.get("ref") or {},
        )


def _k(group: str, fieldname: str, period: Optional[str]) -> str:
    return f"{group}|{fieldname}|{period if period is not None else ''}"


class SourceLedger:
    def __init__(self) -> None:
        self._entries: dict[str, LedgerEntry] = {}

    def _put(self, group, fieldname, period, value, tier, ref) -> None:
        self._entries[_k(group, fieldname, period)] = LedgerEntry(
            group=group, field=fieldname, period=period, value=value, tier=tier, ref=ref,
        )

    def record_filing(self, group, fieldname, period, *, value, provenance: dict) -> None:
        self._put(group, fieldname, period, value, Tier.FILING, dict(provenance))

    def record_market(self, group, fieldname, period, *, value, url, source) -> None:
        self._put(group, fieldname, period, value, Tier.MARKET, {"url": url, "source": source})

    def record_derived(self, group, fieldname, period, *, value, formula, inputs) -> None:
        self._put(group, fieldname, period, value, Tier.DERIVED,
                  {"formula": formula, "inputs": [list(i) for i in inputs]})

    def record_assumption(self, group, fieldname, period, *, value, rationale, basis) -> None:
        self._put(group, fieldname, period, value, Tier.ASSUMPTION,
                  {"rationale": rationale, "basis": basis})

    def record_unverified(self, group, fieldname, period, *, reason, value=None) -> None:
        self._put(group, fieldname, period, value, Tier.UNVERIFIED, {"reason": reason})

    def get(self, group, fieldname, period) -> Optional[LedgerEntry]:
        return self._entries.get(_k(group, fieldname, period))

    def entries(self) -> list[LedgerEntry]:
        return list(self._entries.values())

    def entries_by_tier(self, *tiers: Tier) -> list[LedgerEntry]:
        s = set(tiers)
        return [e for e in self._entries.values() if e.tier in s]

    def to_json(self) -> dict[str, Any]:
        return {"entries": [e.to_json() for e in self._entries.values()]}

    @classmethod
    def from_json(cls, blob: Optional[dict[str, Any]]) -> "SourceLedger":
        led = cls()
        for d in (blob or {}).get("entries", []):
            e = LedgerEntry.from_json(d)
            led._entries[_k(e.group, e.field, e.period)] = e
        return led
