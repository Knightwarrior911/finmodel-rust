# Assumption Ledger Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Tag every valuation/assumption number with a trust tier (filing/market/derived/assumption/unverified) so silent defaults become visible, never hidden — embodying finmodel's trust/auditability wedge.

**Architecture:** Approach C (Hybrid). Three new pure modules (`source_ledger`, `derivations`, `assumption_registry`) plus a `SourceLedger` instance threaded as an optional param through the valuation pipeline. Functions record `DERIVED/ASSUMPTION/UNVERIFIED` entries during construction; cli persists the ledger into the extraction cache under `__ledger__`; the Excel audit pass reads it and applies 5-tier colors + comments, with a red catch-all so nothing escapes.

**Tech Stack:** Python 3.11, dataclasses (no pydantic), pytest, openpyxl. Spec: `docs/superpowers/specs/2026-05-31-assumption-ledger-design.md`.

**Branch:** `feat/assumption-ledger` (already created).

**Conventions observed in this repo:**
- Statements are `dict` keyed `line_item -> [value per period]` (`schemas/financial_data.py`).
- Industrial IS keys: `revenue, cogs, gross_profit, sga, rd, da, ebit, ebita, interest_expense, interest_income, income_tax, net_income`. BS keys include `cash, accounts_receivable, inventory, accounts_payable, long_term_debt, total_assets`. (`tieout/config.py CANONICAL_BY_SECTOR`).
- Effective tax derivation uses `income_tax / (net_income + income_tax)` — both are extracted keys; avoids needing a separate pretax line.
- Tests live in `tests/`, run with `python -m pytest -q`. Current baseline: 183 passed, 6 skipped.
- Regression gates: `python -m tieout.run_tieout` must stay 256/256; full pytest stays green.

---

## Integration approach (read before starting)

The ledger is **one `SourceLedger` instance** created by the caller (cli) and passed as an **optional keyword param** (`ledger: SourceLedger | None = None`) into each modified function. When `ledger is None`, behavior is byte-for-byte the current behavior (backward compatible — this is how existing tests and the tieout path stay green). When a ledger is passed, the function records its tier decisions into it. After the model is built, cli serializes `ledger.to_json()` into `extraction_cache/{ticker}.json` under the `__ledger__` key, then `run_audit` renders it.

Task order: build the 3 leaf modules first (Phase 1, zero behavior change), then wire recording into the pipeline (Phase 2), then render (Phase 3), then gates (Phase 4).

---

## Phase 1 — Foundation modules (no behavior change)

### Task 1: SourceLedger data structure

**Files:**
- Create: `src/source_ledger.py`
- Test: `tests/test_source_ledger.py`

- [ ] **Step 1: Write the failing test**

```python
# tests/test_source_ledger.py
from src.source_ledger import SourceLedger, LedgerEntry, Tier


def test_record_and_get_round_trip():
    led = SourceLedger()
    led.record_derived(
        "wacc", "tax_rate", None, value=0.244,
        formula="income_tax / (net_income + income_tax)",
        inputs=[("income_statement", "income_tax", "2024A")],
    )
    e = led.get("wacc", "tax_rate", None)
    assert e.tier is Tier.DERIVED
    assert e.value == 0.244
    assert e.ref["formula"].startswith("income_tax")


def test_record_assumption_and_unverified():
    led = SourceLedger()
    led.record_assumption("assumptions", "terminal_growth_rate", None,
                          value=0.025, rationale="GDP/inflation proxy", basis="house default")
    led.record_unverified("dcf", "preferred_stock", None,
                          reason="not in extraction schema")
    assert led.get("assumptions", "terminal_growth_rate", None).tier is Tier.ASSUMPTION
    assert led.get("dcf", "preferred_stock", None).tier is Tier.UNVERIFIED


def test_json_round_trip():
    led = SourceLedger()
    led.record_assumption("assumptions", "tax_rate_pct", "2026E",
                          value=0.21, rationale="US statutory", basis="default")
    blob = led.to_json()
    led2 = SourceLedger.from_json(blob)
    e = led2.get("assumptions", "tax_rate_pct", "2026E")
    assert e.value == 0.21 and e.tier is Tier.ASSUMPTION


def test_entries_filtered_by_tier():
    led = SourceLedger()
    led.record_assumption("a", "x", None, value=1.0, rationale="r", basis="b")
    led.record_unverified("a", "y", None, reason="z")
    flagged = led.entries_by_tier(Tier.ASSUMPTION, Tier.UNVERIFIED)
    assert len(flagged) == 2
```

- [ ] **Step 2: Run test to verify it fails**

Run: `python -m pytest tests/test_source_ledger.py -q`
Expected: FAIL — `ModuleNotFoundError: No module named 'src.source_ledger'`

- [ ] **Step 3: Write minimal implementation**

```python
# src/source_ledger.py
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
```

- [ ] **Step 4: Run test to verify it passes**

Run: `python -m pytest tests/test_source_ledger.py -q`
Expected: PASS (4 passed)

- [ ] **Step 5: Commit**

```bash
git add src/source_ledger.py tests/test_source_ledger.py
git commit -m "feat(ledger): SourceLedger trust-tier accumulator"
```

---

### Task 2: Derivation layer

**Files:**
- Create: `src/derivations.py`
- Test: `tests/test_derivations.py`

Each function takes the relevant statement dict(s) and returns `(value | None, lineage)`,
where `lineage` is `(formula_str, inputs_list)` and `inputs_list` is a list of
`(statement, key, period)` tuples. Returns `(None, None)` when actuals are missing
or the value fails its validity guard. Derivations average across all historical
periods present (the lists may contain `None` — skip those).

- [ ] **Step 1: Write the failing test**

```python
# tests/test_derivations.py
from src import derivations as d


def _is():
    # two historical periods
    return {
        "revenue": [1000.0, 1200.0],
        "income_tax": [100.0, 120.0],
        "net_income": [300.0, 360.0],
        "interest_expense": [40.0, 50.0],
        "interest_income": [5.0, 6.0],
        "da": [80.0, 96.0],
        "cogs": [600.0, 720.0],
    }


def _bs():
    return {
        "long_term_debt": [500.0, 600.0],
        "cash": [200.0, 240.0],
        "accounts_receivable": [150.0, 180.0],
        "inventory": [100.0, 120.0],
        "accounts_payable": [90.0, 108.0],
    }


def test_effective_tax_rate():
    v, (formula, inputs) = d.effective_tax_rate(_is())
    # avg of 100/400 and 120/480 = 0.25 and 0.25 -> 0.25
    assert abs(v - 0.25) < 1e-9
    assert "income_tax" in formula


def test_effective_tax_rate_none_when_missing():
    v, lin = d.effective_tax_rate({"revenue": [1000.0]})
    assert v is None and lin is None


def test_effective_tax_rate_guard_rejects_absurd():
    # tax > pretax -> rate > 0.5 -> guard fails -> None
    bad = {"income_tax": [900.0], "net_income": [100.0]}
    v, lin = d.effective_tax_rate(bad)
    assert v is None


def test_cost_of_debt():
    v, (formula, inputs) = d.cost_of_debt(_is(), _bs())
    # avg of 40/500=0.08 and 50/600=0.0833 -> ~0.0817
    assert 0.08 <= v <= 0.084
    assert "interest_expense" in formula


def test_cash_yield():
    v, _ = d.cash_yield(_is(), _bs())
    # avg of 5/200=0.025 and 6/240=0.025 -> 0.025
    assert abs(v - 0.025) < 1e-9


def test_da_pct():
    v, _ = d.da_pct(_is())
    # avg of 80/1000=0.08 and 96/1200=0.08 -> 0.08
    assert abs(v - 0.08) < 1e-9


def test_wc_days():
    res = d.wc_days(_is(), _bs())
    # res is dict {"dso": (v,lin), "dio": (v,lin), "dpo": (v,lin)}
    dso, _ = res["dso"]
    # avg of 150/1000*365=54.75 and 180/1200*365=54.75 -> 54.75
    assert abs(dso - 54.75) < 1e-6
    dio, _ = res["dio"]
    assert dio is not None
    dpo, _ = res["dpo"]
    assert dpo is not None
```

- [ ] **Step 2: Run test to verify it fails**

Run: `python -m pytest tests/test_derivations.py -q`
Expected: FAIL — `ModuleNotFoundError: No module named 'src.derivations'`

- [ ] **Step 3: Write minimal implementation**

```python
# src/derivations.py
"""Derive-first valuation inputs from extracted historical actuals.

Each function returns (value | None, lineage) where lineage is
(formula_str, inputs_list); inputs_list is a list of (statement, key, period)
tuples. None is returned when required actuals are missing or the derived
value fails its validity guard (in which case the caller falls back to a
declared assumption, then UNVERIFIED).

Values are averaged across all historical periods present; None entries in a
series are skipped. "period" in lineage is recorded as "hist_avg" since the
value summarises the historical window.
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
```

- [ ] **Step 4: Run test to verify it passes**

Run: `python -m pytest tests/test_derivations.py -q`
Expected: PASS (7 passed)

- [ ] **Step 5: Commit**

```bash
git add src/derivations.py tests/test_derivations.py
git commit -m "feat(ledger): derive-first valuation inputs from actuals"
```

---

### Task 3: Assumption registry

**Files:**
- Create: `src/assumption_registry.py`
- Test: `tests/test_assumption_registry.py`

- [ ] **Step 1: Write the failing test**

```python
# tests/test_assumption_registry.py
from src.assumption_registry import resolve, Assumption


def test_resolve_known_global():
    a = resolve("equity_risk_premium")
    assert isinstance(a, Assumption)
    assert a.value == 0.055
    assert a.rationale and a.basis


def test_resolve_unknown_returns_none():
    assert resolve("totally_made_up_key") is None


def test_resolve_sector_beta():
    util = resolve("sector_beta", sector="utility")
    std = resolve("sector_beta", sector="standard")
    assert util.value < std.value          # utilities lower beta than market
    assert util.basis


def test_resolve_sector_exit_multiple():
    a = resolve("exit_ebitda_multiple", sector="bank")
    assert a.value == 12.0
```

- [ ] **Step 2: Run test to verify it fails**

Run: `python -m pytest tests/test_assumption_registry.py -q`
Expected: FAIL — `ModuleNotFoundError: No module named 'src.assumption_registry'`

- [ ] **Step 3: Write minimal implementation**

```python
# src/assumption_registry.py
"""The single declared home for every forward-looking default in finmodel.

No module may invent a default inline (`x.get(k, 0.21)`); instead it asks the
registry, which returns an Assumption carrying the value AND its written
rationale + basis. A key the registry does not know returns None, and the
caller must then record UNVERIFIED rather than silently substitute.
"""
from __future__ import annotations

from dataclasses import dataclass
from typing import Optional


@dataclass
class Assumption:
    key: str
    value: float
    rationale: str
    basis: str


# Global (non-sector) declared assumptions.
_GLOBAL: dict[str, Assumption] = {
    "equity_risk_premium": Assumption(
        "equity_risk_premium", 0.055,
        "Long-run US equity risk premium", "Damodaran historical ERP"),
    "target_de_ratio": Assumption(
        "target_de_ratio", 0.30,
        "Sector-typical target capital structure", "house default"),
    "terminal_growth_rate": Assumption(
        "terminal_growth_rate", 0.025,
        "Long-run nominal GDP / inflation proxy", "house default"),
    "risk_free_rate": Assumption(
        "risk_free_rate", 0.045,
        "10Y Treasury proxy when live fetch unavailable", "fallback"),
    # Forward driver fallbacks used only when historicals are absent.
    "revenue_growth_pct": Assumption(
        "revenue_growth_pct", 0.05, "Generic forward growth when no history", "fallback"),
    "gross_margin_pct": Assumption(
        "gross_margin_pct", 0.30, "Generic margin when no history", "fallback"),
    "sga_pct_rev": Assumption("sga_pct_rev", 0.10, "Generic SG&A% when no history", "fallback"),
    "rd_pct_rev": Assumption("rd_pct_rev", 0.05, "Generic R&D% when no history", "fallback"),
    "da_pct_rev": Assumption("da_pct_rev", 0.04, "Generic D&A% when no history", "fallback"),
    "capex_pct_rev": Assumption("capex_pct_rev", 0.05, "Generic capex% when no history", "fallback"),
    "tax_rate_pct": Assumption(
        "tax_rate_pct", 0.21, "US statutory corporate rate", "fallback when not derivable"),
    "interest_rate_pct": Assumption(
        "interest_rate_pct", 0.035, "Generic pre-tax cost of debt", "fallback when not derivable"),
    "dso_days": Assumption("dso_days", 45.0, "Generic receivable days", "fallback"),
    "dio_days": Assumption("dio_days", 60.0, "Generic inventory days", "fallback"),
    "dpo_days": Assumption("dpo_days", 50.0, "Generic payable days", "fallback"),
    "dividend_per_share": Assumption("dividend_per_share", 0.0, "No dividend assumed", "house default"),
}

# Sector-median levered beta (kills the 1.0 fallback in wacc.py / peers.py).
_SECTOR_BETA: dict[str, float] = {
    "standard": 1.10, "utility": 0.60, "bank": 1.15,
    "insurance": 0.95, "reit": 0.85,
}

# Mirrors assumptions.py _SECTOR_MULTIPLES base column (sector -> base exit mult).
_SECTOR_EXIT_MULT: dict[str, float] = {
    "standard": 16.0, "utility": 14.0, "bank": 12.0,
    "insurance": 12.0, "reit": 16.0,
}


def resolve(key: str, sector: Optional[str] = None) -> Optional[Assumption]:
    if key == "sector_beta":
        s = sector or "standard"
        if s in _SECTOR_BETA:
            return Assumption("sector_beta", _SECTOR_BETA[s],
                              f"Sector-median levered beta ({s})", "house sector table")
        return None
    if key == "exit_ebitda_multiple":
        s = sector or "standard"
        if s in _SECTOR_EXIT_MULT:
            return Assumption("exit_ebitda_multiple", _SECTOR_EXIT_MULT[s],
                              f"Sector-typical exit EBITDA multiple ({s})", "house sector table")
        return None
    return _GLOBAL.get(key)
```

- [ ] **Step 4: Run test to verify it passes**

Run: `python -m pytest tests/test_assumption_registry.py -q`
Expected: PASS (4 passed)

- [ ] **Step 5: Commit**

```bash
git add src/assumption_registry.py tests/test_assumption_registry.py
git commit -m "feat(ledger): declared assumption registry"
```

---

## Phase 2 — Wire recording into the pipeline (kill silent defaults)

> Every modified function gains `ledger: SourceLedger | None = None`. When None, behavior is unchanged (keeps existing tests + tieout green). When provided, it records the tier decision. Import locally to avoid import cycles: `from src.source_ledger import SourceLedger`.

### Task 4: wacc.py — tag tax rate, beta, and the clamp

**Files:**
- Modify: `src/wacc.py` (`compute_wacc`, lines 29-98)
- Test: `tests/test_wacc_ledger.py`

Current signature ends `..., target_tax_rate: float = 0.21, target_de_ratio=None, fallback_beta: float = 1.0`. Current beta fallback (lines 53-56) and clamp (line 79) record nothing.

- [ ] **Step 1: Write the failing test**

```python
# tests/test_wacc_ledger.py
from src.source_ledger import SourceLedger, Tier
from src.wacc import compute_wacc
from schemas.financial_data import PeerSet


def test_no_peers_records_beta_assumption():
    led = SourceLedger()
    ps = PeerSet(target_ticker="X", target_market_cap=1000.0, target_de_ratio=0.3, peers=[])
    compute_wacc(ps, target_market_cap=1000.0, target_debt=200.0,
                 risk_free_rate=0.04, equity_risk_premium=0.055,
                 cost_of_debt_pretax=0.05, target_tax_rate=0.244,
                 sector="utility", ledger=led)
    beta_entry = led.get("wacc", "median_unlevered_beta", None)
    assert beta_entry is not None
    assert beta_entry.tier is Tier.ASSUMPTION          # fell back to sector beta
    clamp = led.get("wacc", "wacc", None)
    assert clamp is not None                            # wacc value always recorded


def test_ledger_none_is_unchanged():
    ps = PeerSet(target_ticker="X", target_market_cap=1000.0, target_de_ratio=0.3, peers=[])
    out = compute_wacc(ps, 1000.0, 200.0, 0.04, 0.055, 0.05)   # no ledger kwarg
    assert out.wacc >= 0.05
```

- [ ] **Step 2: Run test to verify it fails**

Run: `python -m pytest tests/test_wacc_ledger.py -q`
Expected: FAIL — `TypeError: compute_wacc() got an unexpected keyword argument 'sector'`

- [ ] **Step 3: Write minimal implementation**

Change the signature (line 29-39) to add `sector: str = "standard"` and `ledger=None` keyword params:

```python
def compute_wacc(
    peer_set,
    target_market_cap: float,
    target_debt: float,
    risk_free_rate: float,
    equity_risk_premium: float,
    cost_of_debt_pretax: float,
    target_tax_rate: float = 0.21,
    target_de_ratio: float | None = None,
    fallback_beta: float = 1.0,
    sector: str = "standard",
    ledger=None,
):
```

Replace the no-peers fallback block (lines 47-56) with registry-backed, ledger-recorded logic:

```python
    from src.assumption_registry import resolve as _resolve

    if peer_set.peers:
        unlevered_betas = [
            _unlever_beta(p.levered_beta, p.de_ratio, p.tax_rate)
            for p in peer_set.peers
        ]
        median_bu = statistics.median(unlevered_betas)
        if ledger is not None:
            ledger.record_derived(
                "wacc", "median_unlevered_beta", None, value=round(median_bu, 4),
                formula="median(unlever(peer.beta))",
                inputs=[("peers", p.ticker, None) for p in peer_set.peers],
            )
    else:
        a = _resolve("sector_beta", sector=sector)
        median_bu = a.value if a else fallback_beta
        logger.warning("No peers in set — using sector-median beta %.2f", median_bu)
        if ledger is not None:
            if a:
                ledger.record_assumption("wacc", "median_unlevered_beta", None,
                                         value=round(median_bu, 4),
                                         rationale=a.rationale, basis=a.basis)
            else:
                ledger.record_unverified("wacc", "median_unlevered_beta", None,
                                         value=round(median_bu, 4),
                                         reason="no peers and no sector beta declared")
```

After the clamp (line 79), record the final wacc and the tax rate tier. Insert before the `return WACCOutput(...)`:

```python
    if ledger is not None:
        ledger.record_derived(
            "wacc", "wacc", None, value=round(wacc, 4),
            formula="We*Ke + Wd*Kd*(1-t)",
            inputs=[("wacc", "cost_of_equity", None), ("wacc", "after_tax_cost_of_debt", None)],
        )
```

> Note: tax-rate tiering is recorded by the caller (assumptions.py / cli) which knows whether `target_tax_rate` was derived or assumed; `compute_wacc` only consumes it.

- [ ] **Step 4: Run tests**

Run: `python -m pytest tests/test_wacc_ledger.py -q`
Expected: PASS (2 passed)

- [ ] **Step 5: Commit**

```bash
git add src/wacc.py tests/test_wacc_ledger.py
git commit -m "feat(ledger): tag beta fallback + wacc in compute_wacc"
```

---

### Task 5: peers.py — tag per-peer tax rate and beta

**Files:**
- Modify: `src/peers.py` (`_beta` line 148-158, `_de_and_tax` line 161-172, `build_peer_set` to thread ledger)
- Test: `tests/test_peers_ledger.py`

`_de_and_tax` currently returns `(de, 0.21)` always — the 0.21 is the silent default to expose. `_beta` returns 1.0 on failure.

- [ ] **Step 1: Write the failing test**

```python
# tests/test_peers_ledger.py
from src import peers


def test_de_and_tax_flags_default_tax(monkeypatch):
    # Force the yfinance path to raise so we hit the fallback branch.
    class _Boom:
        def __init__(self, *a, **k): raise RuntimeError("offline")
    monkeypatch.setattr(peers, "_de_and_tax_source", lambda tk: (_ for _ in ()).throw(RuntimeError()), raising=False)
    de, tax, tax_is_default = peers._de_and_tax_tagged("FAKE")
    assert tax == 0.21
    assert tax_is_default is True
```

- [ ] **Step 2: Run test to verify it fails**

Run: `python -m pytest tests/test_peers_ledger.py -q`
Expected: FAIL — `AttributeError: module 'src.peers' has no attribute '_de_and_tax_tagged'`

- [ ] **Step 3: Write minimal implementation**

Add a tagged wrapper that reports whether the tax rate is the silent default (so callers can record the tier). Keep `_de_and_tax` for backward compatibility; add:

```python
def _de_and_tax_tagged(ticker: str) -> tuple[float, float, bool]:
    """Like _de_and_tax but also returns tax_is_default: True when 0.21 was
    substituted because no effective rate was available (always True today,
    since yfinance does not expose effective tax rate)."""
    de, tax = _de_and_tax(ticker)
    tax_is_default = (tax == 0.21)
    return de, tax, tax_is_default
```

> This makes the silent default observable without changing the numeric result.
> The Peer objects keep `tax_rate=0.21`; the tiering is recorded by the WACC
> path via the registry (peers feed WACC). No behavior change to peer math.

- [ ] **Step 4: Run tests**

Run: `python -m pytest tests/test_peers_ledger.py -q`
Expected: PASS (1 passed)

- [ ] **Step 5: Commit**

```bash
git add src/peers.py tests/test_peers_ledger.py
git commit -m "feat(ledger): expose peer default-tax as observable flag"
```

---

### Task 6: assumptions.py — derive-first, registry fallback, ledger recording

**Files:**
- Modify: `src/assumptions.py` (`_build_scenario` lines 18-40, `build_assumptions_block` lines 66-124, `_fetch_market_inputs` lines 43-63)
- Test: `tests/test_assumptions_ledger.py`

This is the core silent-default site. Today `_build_scenario` does `a.get("tax_rate_pct", 0.21)` etc. New behavior: `build_assumptions_block` accepts `reconciled` (the `ReconciledFinancialData` carrying `income_statement`/`balance_sheet`) plus `ledger`, derives tax/Kd/cash-yield/da_pct/wc-days via `src.derivations`, falls back to registry, records each tier. When `reconciled is None` (e.g. cli `_Stub` path) it degrades to registry/unverified.

- [ ] **Step 1: Write the failing test**

```python
# tests/test_assumptions_ledger.py
from src.source_ledger import SourceLedger, Tier
from src.assumptions import resolve_input


def test_resolve_input_derives_when_actuals_present():
    led = SourceLedger()
    is_ = {"income_tax": [100.0, 120.0], "net_income": [300.0, 360.0]}
    bs = {}
    v = resolve_input("tax_rate_pct", is_, bs, sector="standard", ledger=led, period="2026E")
    assert abs(v - 0.25) < 1e-9
    assert led.get("assumptions", "tax_rate_pct", "2026E").tier is Tier.DERIVED


def test_resolve_input_assumption_when_no_actuals():
    led = SourceLedger()
    v = resolve_input("tax_rate_pct", {}, {}, sector="standard", ledger=led, period="2026E")
    assert v == 0.21
    assert led.get("assumptions", "tax_rate_pct", "2026E").tier is Tier.ASSUMPTION


def test_resolve_input_unverified_when_unknown_key():
    led = SourceLedger()
    v = resolve_input("mystery_key", {}, {}, sector="standard", ledger=led, period="2026E")
    assert v is None
    assert led.get("assumptions", "mystery_key", "2026E").tier is Tier.UNVERIFIED
```

- [ ] **Step 2: Run test to verify it fails**

Run: `python -m pytest tests/test_assumptions_ledger.py -q`
Expected: FAIL — `ImportError: cannot import name 'resolve_input'`

- [ ] **Step 3: Write minimal implementation**

Add a single resolution helper to `src/assumptions.py` that implements the cascade (derive -> registry -> unverified) and records the tier. Map each driver key to its derivation:

```python
from src import derivations as _d
from src.assumption_registry import resolve as _resolve_assumption


# driver key -> callable(is_, bs) -> (value|None, lineage)
def _derive_for(key, is_, bs):
    if key == "tax_rate_pct":
        return _d.effective_tax_rate(is_)
    if key == "interest_rate_pct":
        return _d.cost_of_debt(is_, bs)
    if key == "da_pct_rev":
        return _d.da_pct(is_)
    if key == "dso_days":
        return _d.wc_days(is_, bs)["dso"]
    if key == "dio_days":
        return _d.wc_days(is_, bs)["dio"]
    if key == "dpo_days":
        return _d.wc_days(is_, bs)["dpo"]
    return None, None


def resolve_input(key, is_, bs, *, sector="standard", ledger=None, period=None):
    """Derive-first -> registry assumption -> UNVERIFIED. Records tier when a
    ledger is given. Returns the resolved value (or None if wholly unknown)."""
    value, lineage = _derive_for(key, is_ or {}, bs or {})
    if value is not None:
        if ledger is not None:
            formula, inputs = lineage
            ledger.record_derived("assumptions", key, period, value=round(value, 6),
                                  formula=formula, inputs=inputs)
        return value
    a = _resolve_assumption(key, sector=sector)
    if a is not None:
        if ledger is not None:
            ledger.record_assumption("assumptions", key, period, value=a.value,
                                     rationale=a.rationale, basis=a.basis)
        return a.value
    if ledger is not None:
        ledger.record_unverified("assumptions", key, period,
                                 reason=f"no derivation and no declared assumption for '{key}'")
    return None
```

> Wiring `_build_scenario`/`build_assumptions_block` to call `resolve_input`
> (passing the reconciled statements + ledger) is done in Task 8 (cli
> integration), so the numeric pathway and ledger are connected end-to-end
> there. This task delivers and unit-tests the cascade in isolation.

- [ ] **Step 4: Run tests**

Run: `python -m pytest tests/test_assumptions_ledger.py -q`
Expected: PASS (3 passed)

- [ ] **Step 5: Commit**

```bash
git add src/assumptions.py tests/test_assumptions_ledger.py
git commit -m "feat(ledger): derive-first resolve_input cascade for assumptions"
```

---

### Task 7: dcf.py — flag preferred & investments as UNVERIFIED

**Files:**
- Modify: `src/dcf.py` (lines 104-110: `preferred = 0.0`, `investments = 0.0`)
- Test: `tests/test_dcf_ledger.py`

- [ ] **Step 1: Write the failing test**

```python
# tests/test_dcf_ledger.py
from src.source_ledger import SourceLedger, Tier
from src.dcf import flag_ev_bridge_gaps


def test_preferred_and_investments_flagged_unverified():
    led = SourceLedger()
    flag_ev_bridge_gaps(led, preferred=0.0, investments=0.0)
    assert led.get("dcf", "preferred_stock", None).tier is Tier.UNVERIFIED
    assert led.get("dcf", "investments", None).tier is Tier.UNVERIFIED
```

- [ ] **Step 2: Run test to verify it fails**

Run: `python -m pytest tests/test_dcf_ledger.py -q`
Expected: FAIL — `ImportError: cannot import name 'flag_ev_bridge_gaps'`

- [ ] **Step 3: Write minimal implementation**

Add a small helper to `src/dcf.py` and call it where `preferred`/`investments` are set (after line 107). Keep the numeric `0.0` (no schema change in v1) but record the tier:

```python
def flag_ev_bridge_gaps(ledger, *, preferred: float, investments: float) -> None:
    """Record EV-bridge items that are hardcoded to 0 because they are not in
    the extraction schema, so the audit pass renders them UNVERIFIED instead of
    silently trusting a zero."""
    if ledger is None:
        return
    ledger.record_unverified("dcf", "preferred_stock", None, value=preferred,
                             reason="preferred stock not in extraction schema (assumed 0)")
    ledger.record_unverified("dcf", "investments", None, value=investments,
                             reason="short-term investments not in extraction schema (assumed 0)")
```

In `compute_dcf` add `ledger=None` to the signature and call `flag_ev_bridge_gaps(ledger, preferred=preferred, investments=investments)` right after the two assignments at lines 105/107.

- [ ] **Step 4: Run tests**

Run: `python -m pytest tests/test_dcf_ledger.py -q`
Expected: PASS (1 passed)

- [ ] **Step 5: Commit**

```bash
git add src/dcf.py tests/test_dcf_ledger.py
git commit -m "feat(ledger): flag preferred/investments EV-bridge gaps"
```

---

### Task 8: cli.py — thread one ledger through the build and persist to cache

**Files:**
- Modify: `src/cli.py` (assumptions build line 253; wacc build line 321; audit line 415)
- Modify: `src/assumptions.py` (`build_assumptions_block` to accept + use `reconciled` + `ledger`, calling `resolve_input` per driver)
- Test: `tests/test_cli_ledger_integration.py` (offline, cache-level)

- [ ] **Step 1: Write the failing test**

```python
# tests/test_cli_ledger_integration.py
import json
from pathlib import Path
from src.source_ledger import SourceLedger, Tier


def test_ledger_persists_into_cache(tmp_path):
    # Simulate the cli persist step: a built ledger is written under __ledger__.
    led = SourceLedger()
    led.record_assumption("assumptions", "terminal_growth_rate", None,
                          value=0.025, rationale="GDP proxy", basis="house default")
    cache = {"income_statement": {}, "__ledger__": led.to_json()}
    p = tmp_path / "cache.json"
    p.write_text(json.dumps(cache), encoding="utf-8")

    loaded = json.loads(p.read_text(encoding="utf-8"))
    led2 = SourceLedger.from_json(loaded["__ledger__"])
    assert led2.get("assumptions", "terminal_growth_rate", None).tier is Tier.ASSUMPTION
```

- [ ] **Step 2: Run test to verify it fails (then it will pass once schema names align)**

Run: `python -m pytest tests/test_cli_ledger_integration.py -q`
Expected: PASS already at struct level (asserts the persist contract). If it fails, fix `to_json`/`from_json` key alignment.

- [ ] **Step 3: Wire the real cli path**

In `src/cli.py`:
1. Near the top of the build, create `from src.source_ledger import SourceLedger; ledger = SourceLedger()`.
2. Line 253 — change `build_assumptions_block(_Stub(), cfg.ticker, sector=cfg.sector)` to pass the reconciled financials + ledger: `build_assumptions_block(model_output, cfg.ticker, sector=cfg.sector, reconciled=reconciled, ledger=ledger)`. (Use whatever `ReconciledFinancialData` variable is in scope; if only `_Stub` exists on this branch, pass `reconciled=None` — the cascade then records ASSUMPTION/UNVERIFIED, still correct.)
3. Line 321 — add `sector=cfg.sector, ledger=ledger` to the `compute_wacc(...)` call.
4. After `run_audit`/before it, persist: read the cache json, set `cache["__ledger__"] = ledger.to_json()`, write it back, so `annotate_workbook_with_links` (which reads the cache) sees it.

In `src/assumptions.py` `build_assumptions_block`, add params `reconciled=None, ledger=None`. Where `_build_scenario` currently reads `a.get("tax_rate_pct", 0.21)` etc., for the derivable keys call `resolve_input(key, is_, bs, sector=sector, ledger=ledger, period=<each proj period>)` and use the returned value (fall back to the existing `a.get(...)` numeric only if `resolve_input` returns None). `is_`/`bs` come from `reconciled.income_statement`/`reconciled.balance_sheet` when `reconciled` is provided, else `{}`.

- [ ] **Step 4: Run the full suite (no regression)**

Run: `python -m pytest -q`
Expected: 183+ passed, 6 skipped, 0 failed (new tests add to the pass count).

- [ ] **Step 5: Commit**

```bash
git add src/cli.py src/assumptions.py tests/test_cli_ledger_integration.py
git commit -m "feat(ledger): thread ledger through cli build + persist to cache"
```

---

## Phase 3 — Excel rendering

### Task 9: audit_pipeline.py — 5-tier colors, comments, summary block

**Files:**
- Modify: `src/audit_pipeline.py` (`build_link_indexes` line 168, `annotate_workbook_with_links` line 216)
- Test: `tests/test_ledger_render.py`

Extend the existing cell-walk in `annotate_workbook_with_links`. Add a ledger index built from `cache["__ledger__"]`, checked with HIGHEST precedence (ledger > filing > market), apply a font color per tier + a comment, and after the walk apply a red catch-all to any numeric cell not matched. Add an "Assumptions & Flags" summary block to the Sources sheet.

- [ ] **Step 1: Write the failing test**

```python
# tests/test_ledger_render.py
import json
from pathlib import Path
import openpyxl
from src.source_ledger import SourceLedger
from src.audit_pipeline import annotate_workbook_with_links


def _wb(tmp_path):
    wb = openpyxl.Workbook()
    ws = wb.active
    ws.title = "WACC"
    ws["A1"] = "Tax Rate"; ws["B1"] = 0.25
    ws["A2"] = "Terminal Growth"; ws["B2"] = 0.025
    ws["A3"] = "Mystery"; ws["B3"] = 0.99
    p = tmp_path / "m.xlsx"; wb.save(p); return p


def test_tiers_colour_cells(tmp_path):
    led = SourceLedger()
    led.record_derived("wacc", "tax_rate", None, value=0.25,
                       formula="income_tax/(ni+tax)", inputs=[])
    led.record_assumption("wacc", "terminal_growth", None, value=0.025,
                          rationale="GDP proxy", basis="house default")
    cache = {"__ledger__": led.to_json()}
    cp = tmp_path / "cache.json"; cp.write_text(json.dumps(cache), encoding="utf-8")

    xlsx = _wb(tmp_path)
    res = annotate_workbook_with_links(str(xlsx), cache_path=str(cp))
    assert res["derived"] >= 1
    assert res["assumption"] >= 1
    assert res["unverified"] >= 1          # B3 (0.99) is the red catch-all

    wb = openpyxl.load_workbook(str(xlsx))
    ws = wb["WACC"]
    # Derived cell has a comment mentioning the formula
    assert ws["B1"].comment is not None and "Derived" in ws["B1"].comment.text
    assert ws["B3"].comment is not None and "Unverified" in ws["B3"].comment.text
```

- [ ] **Step 2: Run test to verify it fails**

Run: `python -m pytest tests/test_ledger_render.py -q`
Expected: FAIL — `KeyError: 'derived'` (return dict lacks the new tier keys)

- [ ] **Step 3: Write minimal implementation**

In `src/audit_pipeline.py`:

a) Add a ledger-index builder (label-aware, keyed by value):

```python
def build_ledger_index(cache):
    """value(float) -> list of {group, field, period, tier, ref, label_tokens}."""
    from src.source_ledger import SourceLedger
    led = SourceLedger.from_json(cache.get("__ledger__"))
    idx = {}
    for e in led.entries():
        if e.value is None:
            continue
        idx.setdefault(round(float(e.value), 6), []).append({
            "group": e.group, "field": e.field, "period": e.period,
            "tier": e.tier.value, "ref": e.ref,
            "label_tokens": _label_tokens(e.field.replace("_", " ")),
        })
    return idx
```

b) Define a tier→font-color map and comment builder near the top of the module:

```python
import openpyxl
from openpyxl.styles import Font

_TIER_COLOR = {
    "filing": "0000FF",     # blue
    "market": "0000FF",     # blue
    "derived": "595959",    # gray
    "assumption": "C55A11", # amber
    "unverified": "C00000", # red
}

def _comment_for(tier, ref, value):
    if tier == "derived":
        return f"Derived: {ref.get('formula','')} = {value}"
    if tier == "assumption":
        return f"Assumption: {ref.get('rationale','')} (basis: {ref.get('basis','')})"
    if tier == "unverified":
        return f"⚠ Unverified: {ref.get('reason','')}"
    return None
```

c) In `annotate_workbook_with_links`, build the ledger index, extend the per-cell loop so ledger lookup runs FIRST (precedence), colour the cell font + set comment, increment per-tier counters; after the existing filing/market checks, any unmatched numeric cell gets the red catch-all (color + "Unverified: no source"). Extend the return dict to `{"linked_page","linked_doc","linked_market","derived","assumption","filing","market","unverified","total"}`.

d) Append an "Assumptions & Flags" block: create/append rows to the `Sources` sheet (create it if absent) listing every `entries_by_tier(Tier.ASSUMPTION, Tier.UNVERIFIED)` row as `field | tier | value | rationale-or-reason`.

> The executing agent must READ the current body of `annotate_workbook_with_links`
> (lines 216-300) and splice the ledger check at the top of the matching block,
> preserving the existing filing/market logic for cells the ledger doesn't own.

- [ ] **Step 4: Run tests**

Run: `python -m pytest tests/test_ledger_render.py -q`
Expected: PASS (1 passed)

- [ ] **Step 5: Commit**

```bash
git add src/audit_pipeline.py tests/test_ledger_render.py
git commit -m "feat(ledger): 5-tier Excel rendering + flags summary block"
```

---

## Phase 4 — Regression gates + integration

### Task 10: full regression + ATCO integration sanity

**Files:** none new (verification only)

- [ ] **Step 1: Full pytest**

Run: `python -m pytest -q`
Expected: all prior + new tests pass; 0 failed.

- [ ] **Step 2: Tie-out accuracy gate unchanged**

Run: `python -m tieout.run_tieout --quiet`
Expected: last stdout line reports 100% / 256 cells (derivations are downstream of extraction, so extraction accuracy is unchanged). If it regresses, a derivation accidentally mutated extracted data — revert and isolate.

- [ ] **Step 3: ATCO end-to-end ledger sanity (offline)**

Run:
```bash
python -m src.cli --ticker ATCO-B.ST --no-dcf --no-comps
python -c "import json; c=json.load(open('extraction_cache/ATCO_B_ST.json', encoding='utf-8')); print('ledger entries:', len(c.get('__ledger__',{}).get('entries',[])))"
```
Expected: `__ledger__` present with >0 entries; tax_rate / interest_rate entries tiered DERIVED (ATCO has the actuals). If they're ASSUMPTION, check that `reconciled` statements were passed into `build_assumptions_block`.

- [ ] **Step 4: Commit any fixups, then push + open PR**

```bash
git push -u origin feat/assumption-ledger
gh pr create --title "Assumption ledger: tier every number, kill silent defaults" \
  --body "Implements docs/superpowers/specs/2026-05-31-assumption-ledger-design.md. 5-tier source ledger (filing/market/derived/assumption/unverified). Derive-first for tax/Kd/D&A/WC-days; declared assumption registry; red catch-all in the Excel audit pass. Regression: tieout 256/256 + full pytest green."
```

---

## Self-review notes (author)

- **Spec coverage:** new modules (Tasks 1-3) ↔ spec "New modules"; kill-defaults (Tasks 4-8) ↔ spec "Modified modules" + derive-first; render (Task 9) ↔ spec "Excel rendering" + catch-all + summary block; gates (Task 10) ↔ spec "Testing". Beta→sector registry covers wacc/peers 1.0; preferred/investments→UNVERIFIED covered (Task 7).
- **Backward compatibility:** every modified function defaults `ledger=None` → existing tests + tieout unaffected, satisfying the non-breaking requirement.
- **Known follow-ups (out of v1 scope, per spec):** fetcher.py interest derivation wiring (the `resolve_input` cascade already covers `interest_rate_pct`; the fetcher-internal debt×3.5%/cash×2% fills are a separate derivation site — add in a Phase 2.5 task if ATCO sanity shows fetcher values overriding the ledger); PPTX + chat rendering; extraction-schema expansion for preferred/investments.
