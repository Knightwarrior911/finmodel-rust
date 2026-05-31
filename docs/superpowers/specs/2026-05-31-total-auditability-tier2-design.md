# Total Auditability (Tier 2) — Design Spec

**Date:** 2026-05-31
**Status:** Approved-to-build (continuation of the trust-wedge roadmap, under standing pre-approval)
**Branch:** `feat/total-auditability`
**Builds on:** the assumption ledger (`docs/superpowers/specs/2026-05-31-assumption-ledger-design.md`, merged to master).

## Purpose

Close the auditability gap so the trust promise holds for EVERY number on EVERY surface, not just sourced inputs:

> Every number is traceable — sourced to a filing, computed from named precedents, or a declared assumption — and the user can SEE the full provenance of an answer at a glance.

Two gaps remain after the ledger shipped:
1. **Excel computed cells are invisible to the audit pass.** `annotate_workbook_with_links` loads the workbook without `data_only`, so formula cells are strings (`"=B5*C5"`) and get skipped — no tier, no comment. These derived subtotals/multiples/projections were the ~26% "uncovered" remainder. They ARE auditable (the formula names its precedents) but nothing surfaces that.
2. **Chat/CLI answers carry no provenance.** The orchestrator returns free-form text with numbers and no sources.

## Requirements / scope decisions

- **Part A — Excel computed-cell lineage.** Classify formula cells as DERIVED-by-precedent: add a comment naming the formula + its precedent row-labels; count them so coverage reporting reaches ~100% (every numeric/formula cell is sourced, derived, assumption, or red).
- **Part B — Sources & Assumptions appendix for chat/CLI.** A deterministic markdown appendix built from the cache's ledger/provenance/citations, appended to orchestrator answers. **NOT** fragile inline-number-in-prose matching — a structured appendix is robust and testable.
- **Non-breaking + gated:** all new Excel behavior stays behind `__ledger__` presence (no-ledger render byte-identical, as before). Appendix only appends when a ticker cache with `__ledger__` exists.
- **No recoloring of formula cells:** keep the repo's black=formula convention; Part A is comment + count only.
- **Out of scope:** PPTX lineage; inline-prose number citation; valuation-accuracy in the tie-out gate (separate workstream).

## Part A — Computed-cell formula lineage (`src/audit_pipeline.py`)

Extend `annotate_workbook_with_links` (only when `ledger_present`). After the existing numeric-cell loop, walk FORMULA cells:

- A formula cell: `isinstance(cell.value, str) and cell.value.startswith("=")`.
- Parse cell references from the formula with a regex covering optional sheet qualifier and `$` anchors, e.g. `(?:'[^']+'|[A-Za-z0-9_]+)!)?\$?[A-Z]{1,3}\$?\d+`. Extract (sheet, col, row); same-sheet refs resolve on `ws`, cross-sheet on the named sheet.
- For each referenced cell, resolve its row label via the existing `_row_label(ws, cell)` helper (look up the referenced cell object). Collect unique non-empty labels (cap at ~6 to keep comments readable).
- Set the cell comment: `Computed: {formula}` and a second line `from: {label1}, {label2}, …` when labels resolve. Do NOT change the font.
- Increment a new counter `derived_formula`.
- Skip a formula cell if it already has a comment from the ledger/filing pass (don't double-annotate).

**Coverage:** extend the return dict to include `derived_formula` and a `coverage` block:
`{"numeric_total", "formula_total", "filing", "market", "derived", "assumption", "derived_formula", "unverified", "covered_pct"}` where `covered_pct = (filing+market+derived+assumption+derived_formula) / (numeric_total+formula_total)` — formula cells now count as covered (auditable via named precedents), so honest coverage approaches 100% and the only "gaps" are red UNVERIFIED inputs.

Existing keys (`linked_page/linked_doc/linked_market/total`) keep their meaning. No-ledger path unchanged.

## Part B — Sources & Assumptions appendix

### New module `src/sources_report.py`

`build_sources_report(cache: dict) -> str` — returns a markdown block. Reads:
- `__ledger__` (via `SourceLedger.from_json`) → Derived / Assumptions / Unverified sections.
- filing + market indexes (via `audit_pipeline.build_link_indexes(cache)`) → Filing-sourced / Market sections (key → page / provider URL).

Sections (omit a section when empty):
```
## Sources & Assumptions
**Filing-sourced:** Revenue (p.3), EBIT (p.3), …
**Market data:** beta — Yahoo Finance, risk-free rate — ^TNX
**Derived:** tax rate = income_tax/(net_income+income_tax); cost of debt = interest_expense/long_term_debt
**Assumptions:** terminal growth 2.5% (long-run GDP proxy); ERP 5.5% (Damodaran)
**⚠ Unverified (review):** preferred stock (not in extraction schema); …
```
Pure function over the cache dict — no I/O, fully unit-testable.

### Wire into the orchestrator (`src/orchestrator.py`)

`VirtualAnalystOrchestrator.run(...)` already takes `ticker`. After the final answer text is determined (the `end_turn` return at ~line 2040 and the fallback at ~line 2096), if `ticker` is set and `extraction_cache/{ticker}.json` exists and contains a non-empty `__ledger__`, append `"\n\n---\n" + build_sources_report(cache)` to the answer. Factor the return into a small helper `_finalize(answer, ticker)` so both exit points share it. Guard everything in try/except → the appendix never breaks an answer. When no ticker/cache/ledger, the answer is unchanged.

## Error handling

- Formula-ref parsing wrapped so a malformed formula is skipped (no crash); the cell just gets no lineage comment.
- `build_sources_report` tolerates missing cache keys (empty sections).
- Orchestrator appendix in try/except; failure → original answer returned.

## Testing

- **Part A unit** (`tests/test_formula_lineage.py`): build a workbook with `B3 = "=B1-B2"` where B1="Revenue", B2="COGS"; cache with a non-empty `__ledger__`; run `annotate_workbook_with_links`; assert B3 comment starts "Computed:" and names "Revenue"/"COGS"; assert return dict has `derived_formula >= 1` and `covered_pct`. Plus a no-ledger test asserting formula cells get no comment (unchanged behavior).
- **Part B unit** (`tests/test_sources_report.py`): synthetic cache with `__ledger__` (one derived, one assumption, one unverified) → assert the markdown contains "Derived", "Assumptions", "Unverified" and the rationale text. Empty cache → returns a short/empty-section string without error.
- **Regression:** full `pytest` green (currently 211 passed, 6 skipped); existing audit tests unchanged; no-ledger Excel behavior byte-identical.

## Out of scope (follow-ups)

- PPTX computed-cell lineage.
- Inline-in-prose number citations (the appendix supersedes for v1).
- Valuation-accuracy instrumentation in the tie-out gate.
- A web viewer that renders the sources report interactively.
