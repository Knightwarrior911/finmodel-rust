# Dynamic IS Line Item Plan — Spec Compliance Phases 1–4

**Goal:** Full compliance with `valuation_kit/3statement/CLAUDE.md` rule:
> "Replicate the company's actual reported line item structure. Do not impose a generic template."

**Status:** All 4 phases complete (Phases 2–4 shipped in commit `9174435`: `_detect_opex_items` in fetcher.py, `_build_bank_is`/`_build_insurance_is`/`_build_reit_is` + `_apply_filing_labels` in is_builder.py). Dynamic IS now uses company-actual XBRL disclosure line items with actual filing labels. **Exception:** §7.4 SaaS metrics template was NOT built — deferred to `docs/MASTER_PLAN.md` Phase 5 (evidence-gated). Phase bodies below are the original specs, retained as implementation reference.

---

## Phase 1 — Dynamic Revenue Segments [DONE]

**Why:** SPEC_methodology §3.1 requires `[Segment lines reproducing company's actual disclosure]`.
AAPL must show Products + Services rows, not a single Revenue line.

**Files to change:**

### `src/fetcher.py`
- In `fetch_us_filing()`: after pulling standard XBRL concepts, also query companyfacts for segment revenue concepts.
- EDGAR companyfacts URL: `https://data.sec.gov/api/xbrl/companyfacts/CIK{cik}.json`
- Look for concepts where `label` contains "Revenue" or "Sales" and is NOT the total (`us-gaap/Revenues`, `us-gaap/RevenueFromContractWithCustomerExcludingAssessedTax`) — i.e., sub-total concepts.
- Common segment concepts: `us-gaap/ProductRevenue`, `us-gaap/ServiceRevenue`, company-specific extension concepts (e.g., `aapl/ProductsNet`, `aapl/ServicesNet`).
- Store result in `raw_data.income_statement["revenue_segments"]` as list of dicts:
  ```python
  [{"label": "Products", "key": "rev_seg_products", "values": [123.0, 234.0, 345.0]}, ...]
  ```
- If no segment data found → empty list (fallback: single Revenue row).

### `src/is_builder.py`
- `build_is_structure()` accepts new param `revenue_segments: list[dict] = None`.
- If segments provided: insert one ISRow per segment (indent=1, row_type="line_item", driver_key=f"rev_seg_{slug}_growth_pct") before Total Revenue.
- Total Revenue row: row_type="subtotal", bold=True. No driver row below it (driver lives under each segment).
- If no segments: keep current single Revenue row with revenue_growth_pct driver.
- Driver key slug: `re.sub(r'[^a-z0-9]', '_', label.lower())`.

### `src/assumptions.py`
- `build_assumptions_block()`: for each segment, generate a growth % driver row (same structure as existing revenue_growth_pct).
- Key: `rev_seg_{slug}_growth_pct`.
- Default value: same as base revenue growth (can be differentiated later).

### `src/engine.py`
- In projection loop: project each segment independently using its own growth driver.
- `revenue_segments[i]["projected_values"] = prev * (1 + driver)`.
- Total revenue = sum of segments.
- If no segments: existing logic unchanged.

### `src/writer.py`
- No structural changes needed — `_write_is()` already iterates ISRows dynamically.
- `_write_is_data_row()`: handle `key.startswith("rev_seg_")` → blue hist, growth proj formula.
- `DRIVER_KEY_TO_ASSUMP_OFFSET` in `is_builder.py`: add entries for each segment driver.

**Test:** `python model.py --ticker AAPL` → IS should show Products / Services rows above Total Revenue.

---

## Phase 2 — Dynamic OpEx Rows from Actual XBRL

**Why:** Spec says "Show all operating expenses" from actual disclosure. Companies report unique items (restructuring, impairment, stock-based comp, etc.) that our archetypes miss or hardcode.

**Approach:**
- In `fetch_us_filing()`: pull ALL XBRL concepts filed under `OperatingExpenses` or tagged as opex-level items.
- Build concept → (label, key, values) mapping.
- Pass to `build_is_structure()` as `opex_items: list[dict]`.
- `is_builder.py`: replace hardcoded `has_rd / has_sga` logic with dynamic opex rows built from `opex_items`.
- Keep fallback: if `opex_items` empty → use current archetype.
- Labels: use XBRL concept's `label` field (human-readable) from companyfacts.

**Risk:** XBRL concept coverage is inconsistent across companies/years. Need robust fallback.

---

## Phase 3 — Complete Bank / Insurance / REIT Sector Templates

**Why:** SPEC_methodology §7.1–7.3 defines exact structure. Current stubs in `is_builder.py` are incomplete.

### Bank (§7.1)
- Replace COGS/Gross Profit with Net Interest Income (Interest Income – Interest Expense).
- Replace OpEx with Non-Interest Expense.
- Add Provision for Loan Losses as separate line.
- Drivers: NIM, efficiency ratio, loan growth, deposit growth, credit cost.
- Fetcher: pull `us-gaap/InterestAndFeeIncomeLoansAndLeases`, `us-gaap/InterestExpense`, `us-gaap/ProvisionForLoanAndLeaseLosses`.

### Insurance (§7.2)
- Revenue: Net Premiums Earned + Net Investment Income.
- COGS equivalent: Net Losses and LAE Incurred + Acquisition Costs.
- BS: Reserves for Losses, Unearned Premium Reserve, Reinsurance Recoverables.
- Drivers: combined ratio, loss ratio, expense ratio, investment yield, premium growth.

### REIT (§7.3)
- Revenue: Rental Revenue + other property-level income.
- Add FFO and AFFO as supplemental memo rows.
- Heavy PP&E: gross + accumulated depreciation.
- Drivers: occupancy rate, same-store NOI growth, cap rate.

### SaaS (§7.4 — not currently in is_builder)
- Track ARR, NRR, GRR.
- Capitalize internal-use software → CapEx + amortization.
- Add to sector routing: SIC codes 7372, 7371.

---

## Phase 4 — Actual Filing Labels

**Why:** We currently hardcode "Cost of Revenues", "Research and development" etc. Spec says actual disclosure.

**Approach:**
- EDGAR companyfacts returns concept labels (e.g., `"Cost of Sales"` for some, `"Cost of Revenues"` for others).
- In `fetch_us_filing()`: store concept → label mapping alongside values.
- `is_builder.py`: `ISRow.label` defaults to our generic label but is overridden if filing label available.
- Fallback chain: filing label → our generic label → concept name.

---

## Architecture Invariants (do not break)

- `ISRow` dataclass in `schemas/financial_data.py` — add fields if needed, don't remove.
- `compute_is_row_map()` in `is_builder.py` — always called first in `_write_is()`.
- `_isr(key)` in `writer.py` — all cross-tab IS refs must use this, never `IS_R[key]` directly.
- `IS_R["circ"]` (row 7) and `IS_R["headers"]` (row 9) stay fixed — never replaced with `_isr()`.
- Validator must pass `status=success, failures=0` after every phase.

---

## Test Tickers Per Phase

| Phase | Primary test | Secondary test | What to verify |
|---|---|---|---|
| 1 | AAPL | MSFT | Products/Services rows visible in IS |
| 2 | AAPL | AMZN | All reported opex lines present, no extras |
| 3 | JPM (bank) | BRK-B (insurance), SPG (REIT) | Sector-specific IS structure |
| 4 | AAPL | Any | Labels match 10-K filing exactly |
