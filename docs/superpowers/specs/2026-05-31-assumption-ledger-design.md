# Assumption Ledger — Design Spec

**Date:** 2026-05-31
**Status:** Approved (design); pending implementation plan
**Branch:** `feat/assumption-ledger`

## Purpose

Embody finmodel's product wedge — **trust / auditability** — at the number level.

> **The promise:** every number we show is either (a) traced to an exact filing
> page, (b) computed by a formula whose inputs are themselves traced, or (c) an
> explicitly declared assumption with a written rationale — and we **never
> silently substitute a default.**

Today the engine is full of silent defaults that violate this promise:
`a.get("tax_rate_pct", 0.21)`, `a.get("interest_rate_pct", 0.035)`,
`a.get("dso_days", 45.0)` (`src/assumptions.py`), `fallback_beta = 1.0`
(`src/wacc.py`), per-peer `return de, 0.21` (`src/peers.py`),
`preferred = 0.0` / `investments = 0.0` (`src/dcf.py`),
and `_fetch_market_inputs` swallowing exceptions to `risk_free_rate = 0.045`.
Each is, under a trust wedge, a hallucinated number.

## Requirements (decided in brainstorm)

1. **Enforcement = flag loud, ship anyway.** Untraceable / undeclared numbers
   render visibly (red "unverified") but never block the deliverable. The user
   always sees the truth and decides.
2. **Surface scope = Excel only** for v1. Engine-side tagging is prerequisite
   for all surfaces; PPTX + chat-answer rendering are later waves.
3. **Derive-first.** Inputs computable from extracted actuals (tax rate, cost of
   debt, D&A %, working-capital days) are derived and tagged DERIVED with formula
   lineage. Only purely-forward inputs stay declared assumptions.
4. **Architecture = Approach C (Hybrid).** Kill silent defaults at the source
   (inline derivation + registry + ledger during construction); keep filing/market
   numbers on the proven post-hoc index; unify at render.

## Trust tiers

The ledger encodes five tiers; they drive the Excel font colors.

| Tier | Color | Meaning | Existing infra |
|------|-------|---------|----------------|
| FILING | blue | extracted from a filing; page + bbox | `src/provenance.py` |
| MARKET | blue + link | live yfinance/EDGAR; provider URL | `src/citations.py` |
| DERIVED | gray | computed from tagged inputs; carries formula lineage | new |
| ASSUMPTION | amber | declared forward input + written rationale | new (registry) |
| UNVERIFIED | red | silent fallback fired / can't source — wedge violation | new |

## Architecture (Approach C — Hybrid)

One `SourceLedger` accumulates a tier-tagged record for every non-trivially-sourced
number **during model construction**. Filing/market face numbers keep riding the
existing provenance/citation indexes. At render, the Excel pass reads ledger +
indexes and colors/comments cells.

**Scope boundary:** the 3-statement face numbers are already filing-extracted
(`provenance.py` covers them). The silent-default offenders all live in the
**valuation / assumption inputs** — `assumptions.py`, `wacc.py`, `dcf.py`,
`peers.py`, `fetcher.py` derivations. The ledger's new work concentrates there,
which bounds v1.

### New modules

| Module | Purpose |
|--------|---------|
| `src/source_ledger.py` | `Tier` enum, `LedgerEntry{value, tier, ref}`, `SourceLedger` accumulator (`record_*`, `get`, `to_json`/`from_json`). Persisted into `extraction_cache/{ticker}.json` under `__ledger__`. **v1 actively uses `record_derived / record_assumption / record_unverified`**; `record_filing / record_market` exist for API completeness + a future inline path, but in v1 filing/market resolution is delegated to the post-hoc indexes (see Data flow). |
| `src/derivations.py` | Derive-first pure fns: `effective_tax_rate(is_)`, `cost_of_debt(is_, bs)`, `cash_yield(is_, bs)`, `da_pct(is_)`, `wc_days(is_, bs)`. Each returns `(value \| None, lineage)`; `None` when actuals missing or guard fails. |
| `src/assumption_registry.py` | Declared forward inputs as `Assumption{key, value, rationale, basis}`. The single home for every default. `resolve(key, sector=None) -> Assumption \| None` (None → caller records UNVERIFIED). |

### Modified modules (kill silent defaults)

| Module | `.get(k, default)` / fallback → |
|--------|---------------------------------|
| `src/assumptions.py` | derive-first → registry → unverified; record each to ledger |
| `src/wacc.py` | beta 1.0 / tax 0.21 → MARKET (beta) / derive (tax) / declare / unverified |
| `src/peers.py` | per-peer tax 0.21 / beta 1.0 → tagged (market/derived/assumption/unverified) |
| `src/dcf.py` | `preferred = 0`, `investments = 0` → filing-or-UNVERIFIED |
| `src/fetcher.py` | interest debt×3.5% / cash×2% → `derivations.py` + ledger |
| `src/audit_pipeline.py` | render pass reads `__ledger__` + indexes → 5-tier color/comment + summary block |

### Ledger entry `ref` shape (tier-specific)

- FILING → `CellProvenance` (reuse)
- MARKET → provider URL string (reuse `citations`)
- DERIVED → `{formula: "income_tax / pretax_income", inputs: [ledger-keys]}`
- ASSUMPTION → `{rationale, basis}`
- UNVERIFIED → `{reason}`

**Ledger key:** `(group, field, period)` — e.g. `("wacc", "cost_of_equity", None)`,
`("assumptions", "tax_rate_pct", "2026E")`.

## Tier resolution & data flow

### Resolution cascade (valuation/assumption inputs)

First match wins:

```
1. derivations.<fn>() returns a value?   -> DERIVED    (lineage = formula + input keys)
2. assumption_registry.resolve(key)?     -> ASSUMPTION (rationale + basis)
3. otherwise                             -> UNVERIFIED (reason)
```

Filing & market numbers do not enter this cascade — they keep their existing paths.

### End to end

1. **Construction time** — derivation layer + registry + every former-fallback
   path write `DERIVED / ASSUMPTION / UNVERIFIED` entries into the inline
   `SourceLedger`. Silent defaults die here: code can no longer `return default`;
   it must record which tier and why.
2. **Filing/market numbers** — unchanged; located post-hoc by
   `audit_pipeline.build_link_indexes` (proven value/market indexes).
3. **Render time** — for each numeric Excel cell, resolve tier by precedence:

```
ledger entry (by label+value)  -> use its tier  (DERIVED/ASSUMPTION/UNVERIFIED, exact)
else filing index match        -> FILING
else market index match        -> MARKET
else                           -> UNVERIFIED (red)   <- catch-all, nothing escapes
```

**The catch-all makes "flag loud" real:** any number with no ledger entry and no
index match goes red. A silent default added later cannot hide — it surfaces red
on first render.

**Ledger↔cell mapping:** render matches a ledger entry to its cell by the same
label-aware (sheet-context + row label + value) matcher the audit pass already
uses. No new matching machinery.

**The "two mechanisms" seam:** inline ledger owns derived/assumption/unverified;
post-hoc indexes own filing/market. They are disjoint sets (inputs vs face
numbers), unified only at render by the precedence ladder. No overlap to reconcile.

## Derivation layer (`src/derivations.py`)

Each fn: pure, returns `(value | None, lineage)`. Derives from extracted historical
actuals (**multi-year average**; last-FY if only one year). A **validity guard**
rejects absurd results → `None` → cascade falls to assumption/unverified.

| Derivation | Formula | Guard | Replaces | `None` when |
|------------|---------|-------|----------|-------------|
| `effective_tax_rate` | `income_tax / pretax_income` | [0, 0.50] | `0.21` (assumptions, wacc, peers) | tax or pretax missing; pretax ≤ 0 |
| `cost_of_debt` | `interest_expense / total_debt` | [0.005, 0.20] | `0.035` (assumptions, fetcher) | interest or debt missing/0 |
| `cash_yield` | `interest_income / cash` | [0, 0.15] | cash×2% (fetcher) | either missing |
| `da_pct` | `da / revenue` (avg) | [0, 0.50] | `0.04` (assumptions) | da or revenue missing |
| `wc_days` (DSO/DIO/DPO) | `AR/rev·365`, `Inv/cogs·365`, `AP/cogs·365` | [0, 365] | `45/60/50` (assumptions) | balances missing |

**Guard-fail behavior:** a derived value outside its band (e.g. tax = 80% from a
one-off true-up) is treated as `None`; never emits the absurd number.

### Not derivations — route elsewhere

- **Beta** (`wacc` 1.0, `peers` 1.0) — not computable from a filing. MARKET if
  yfinance returns it; else ASSUMPTION (declared sector-median beta) or UNVERIFIED.
- **Forward inputs** (revenue growth, terminal g, exit multiple, ERP, target D/E)
  — inherently forward → ASSUMPTION (registry).
- **`preferred`, `investments`** (dcf:105,107) — filing balance-sheet items not in
  the extraction schema. **v1: UNVERIFIED** with reason "not in extraction schema";
  expanding the schema is a separate accuracy task, out of scope here.
- **Margin/opex %** (gross margin, SG&A%, R&D%, capex%) already derive from
  historicals in the engine; the default only fires when historicals are absent,
  which routes to registry/unverified.

## Assumptions registry (`src/assumption_registry.py`)

The single file where every default lives, each declared with justification — kills
scattered `.get(k, default)`.

- `Assumption{key, value, rationale, basis}`
- `resolve(key, sector=None) -> Assumption | None`
- Migrates today's hardcodes in, with rationale: ERP 0.055 ("Damodaran long-run
  US equity risk premium"), target D/E 0.30 ("sector-typical capital structure"),
  terminal g 0.025 ("long-run GDP/inflation proxy"), sector-median beta (kills the
  wacc/peers 1.0), the existing `_SECTOR_MULTIPLES` exit multiples (sector-aware).

## Excel rendering (extend `src/audit_pipeline.py`)

5-tier font color + comment, reusing the existing cell-walk + link machinery.

| Tier | Font | Comment |
|------|------|---------|
| FILING | blue | + finmodelaudit page link (existing) |
| MARKET | blue | + provider URL (existing) |
| DERIVED | gray | `Derived: {formula} = {value}; inputs: […]` |
| ASSUMPTION | amber | `Assumption: {rationale} (basis: {basis})` |
| UNVERIFIED | red | `⚠ Unverified: {reason}` |

Plus an **"Assumptions & Flags" summary block** — a labeled row group appended to
the existing Sources sheet — listing every amber + red entry in one place (field,
tier, value, rationale/reason). The at-a-glance trust report a reviewer scans first.

Return dict extends to `{filing, market, derived, assumption, unverified, total}`.

## Error handling

- Derivation exceptions caught → treated as `None` (cascade), never crash the model.
- `SourceLedger` always built (empty is valid).
- **Non-breaking:** the 5-tier render rides the existing `--audit` path. When no
  `__ledger__` is present in the cache, render falls back to current 2-tier
  (filing/market) behavior — existing flows unaffected.

## Testing

- **Unit** — each derivation fn (value / guard-fail / `None` cases, table-driven);
  registry `resolve` (declared / undeclared / sector-aware); `SourceLedger`
  record/get/serialize round-trip; precedence ladder (ledger > filing > market >
  red catch-all).
- **Integration** — build ATCO-B.ST model offline; run render; assert every
  numeric cell receives a tier; assert tax/Kd now DERIVED (not amber/red); assert a
  deliberately-missing field → red.
- **Regression gates** — `python -m tieout.run_tieout` stays **256/256** (derivations
  are downstream of extraction, cannot change it); full `pytest` suite stays green.

## Out of scope (v1)

- PPTX + chat-answer rendering (later waves; tags already travel for them).
- Expanding the extraction schema to add preferred stock / short-term investments.
- Valuation-accuracy instrumentation in the tieout gate (separate spec).
- A full inline `Tagged` value type across the engine (Approach B; revisit if v1
  proves the model).
