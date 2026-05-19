# Design: Broaden the Tie-Out Basket for Model Financial Accuracy

**Date:** 2026-05-19
**Status:** Approved (user pre-approved all gates)
**Topic:** Expand the immutable `tieout/` instrument from 7 European industrials to a
failure-mode-diverse, multi-sector basket so the extractor's filing-match accuracy
is trustworthy beyond an overfit handful.

---

## 1. Problem

`tieout/` measures whether `src.extractor.extract_financials_from_pdf` reproduces the
numbers printed on a company's annual-report face statements. The current basket is
7 European industrials/consumer/tech names; the EU subset reports ~100% (256/256
cells).

That 100% is **not trustworthy as a general accuracy claim**. It is measured on a
small, structurally similar set (IFRS, industrial, mostly two-up EU layouts). It
says nothing about integrated-report layouts, US GAAP, non-English filings,
Indian lakh/crore formatting, or — most importantly — financial-sector statements
whose structure is alien to the fixed `CANONICAL` schema.

**Goal:** raise *real, generalizable* filing-match accuracy by expanding the
immutable instrument to a diverse, multi-sector basket, then driving the extractor
green against that harder gate without overfitting.

The yardstick is unchanged and explicit: **extracted/reconciled number == number
printed on the source filing's face statements.** Not internal consistency, not
projection realism. Pure extraction + reconcile fidelity.

## 2. Scope

**In scope**
- Sector-aware line-item universe (`industrial`, `bank`, `insurer`).
- Sector auto-detection in the extractor (the model must figure out what it is
  reading; the instrument must not tell it).
- Sector-aware ground-truth generation (anchors, data-row detection, prompt
  keys) — dual-pass agreement mechanism kept unchanged.
- Per-new-company hard self-test asserts (the trust anchor that makes "broader"
  trustworthy, generalizing the existing `_ATCO_ASSERT`).
- A diverse basket selection matrix and the names that satisfy it.
- A waved rollout with anti-overfit guards (no-regression, held-out
  generalization names, no ticker-specific hardcoding in `src/`).
- Sourcing reliability for new pins (curated URL → headed-browser fetch →
  human-drop fallback).

**Out of scope**
- The grounding/research path (`run_grounding.py`, `src/research/qa.py`) — that
  is the *research* accuracy track, deliberately separate.
- Projection / DCF / WACC / comps realism.
- Any change to the dual-pass agreement trust mechanism itself.

## 3. Approach (selected: B — Waved, schema-first)

Rejected alternatives:
- **A — Big-bang:** add all new names + sector schemas + auto-detect at once.
  Fastest to "broad" but regressions become unattributable and revealing every
  filing simultaneously invites overfitting.
- **C — Sector-split instruments:** separate `tieout-bank/` etc. Cleaner schemas
  but fragments the single trustworthy accuracy number the user wants. Rejected.

**Selected — B, waved schema-first:**

- **Wave 0 — Infra, zero new names.** Build sector-aware `CANONICAL`, sector
  auto-detect, sector-aware GT, per-company assert registry. Acceptance: the
  existing 7 names still produce identical GT and the extractor still ties out at
  the current baseline (no regression from infra alone). This proves the
  refactor is safe before any new name is added.
- **Wave 1 — Schema-safe diverse industrials/consumer/tech.** ~5-6 names chosen
  to stress non-sector axes (integrated report, Universal Registration Doc, US
  GAAP 10-K, Indian Ind-AS / lakh-crore, non-English/translated, 3+ year column
  layout). Loop the extractor to green.
- **Wave 2 — Banks.** ~4-5 names. Exercises the `bank` schema + sector
  detection on real bank filings. Loop to green.
- **Wave 3 — Insurers + held-out generalization.** ~2-3 insurers plus 2-3
  generalization names that are **never used to tune the extractor** — they are
  scored once at the end as the honest out-of-sample number.

Each wave: build immutable GT + hand-verified asserts, then run the extractor
loop, subject to the no-regression guard.

## 4. Architecture

### 4.1 Sector-aware line-item universe (`tieout/config.py`, `src/extractor.py`)

`CANONICAL` becomes `CANONICAL_BY_SECTOR: dict[str, dict]`:

- `industrial` — exactly the current 30 keys (12 IS / 12 BS / 6 CFS). Unchanged
  so existing GT is bit-identical.
- `bank` — IS: `net_interest_income, interest_income, interest_expense,
  fee_commission_income, trading_income, total_operating_income,
  loan_loss_provisions, operating_expenses, pretax_income, income_tax,
  net_income`. BS: `cash_and_central_bank, loans_to_customers,
  investment_securities, total_assets, customer_deposits, debt_securities_issued,
  total_liabilities, total_equity`. CFS: `cfo, cfi, cff, net_change_cash`.
- `insurer` — IS: `gross_written_premium, net_earned_premium,
  net_investment_income, net_claims_incurred, acquisition_expenses,
  operating_expenses, pretax_income, income_tax, net_income`. BS:
  `investments, cash, total_assets, insurance_contract_liabilities,
  total_liabilities, total_equity`. CFS: `cfo, cfi, cff, net_change_cash`.

`ABS_KEYS` and `EXCLUDE_KEYS` become per-sector. Same philosophy: face-statement,
transcribe-only, null if not printed, no derivation.

Each `BASKET` row gains `"sector": "industrial" | "bank" | "insurer"`. The 7
existing rows are tagged `industrial`.

### 4.2 Sector detection in the extractor (`src/extractor.py`)

The extractor **auto-detects** sector from the filing, then selects the schema.
The instrument must NOT pass sector to the model path — "does the model recognise
it is reading a bank" is part of what is under test.

Detection heuristic (cheap, deterministic, pre-LLM): scan extracted face text for
sector signatures — bank: `net interest income`, `loans and advances to
customers`, `due to customers`; insurer: `gross written premium`, `net earned
premium`, `insurance contract liabilities`; else `industrial`. The LLM prompt is
then built from the detected sector's schema. Detection itself becomes a tested
capability (a wrong sector pick will collapse tie-out for that name, surfacing as
a root-cause class).

### 4.3 Sector-aware ground truth (`tieout/groundtruth.py`)

- `_build_prompt` keys-block sourced from `CANONICAL_BY_SECTOR[sector]`; persona
  + sign rules parameterised per sector.
- `_IS_ANCHORS` extended with bank/insurer income-statement phrases.
- `_REVENUE_DATA_ROW` is revenue-centric and will not fire for banks. Add
  sector-specific data-row detectors (bank: an `interest income` /
  `net interest income` line followed by two multi-digit figures; insurer: a
  `premium` line likewise). The face-window finder picks the detector matching
  the detected sector.
- GT builder receives `sector` from the basket row (the *instrument* legitimately
  knows the sector for its own answer key; only the *model path* must not).
- The dual-pass A/B decorrelated agreement mechanism is **unchanged** — it is
  schema-agnostic and is the core trust primitive.

### 4.4 Per-company hard self-test (`tieout/groundtruth.py`)

Generalize `_ATCO_ASSERT` into a registry: `HARD_ASSERTS: dict[ticker, dict]`.
For every new name, 3-5 rock-solid, hand-verified face figures are recorded.
After dual-pass GT is built, the matching assert block must pass or GT generation
fails loudly.

Rationale: diverse names — integrated reports, banks — are expected to have lower
dual-pass agreement. A small hand-verified anchor per company prevents a noisy
agreement set from silently certifying a wrong answer key. This is the mechanism
that makes "broader" *trustworthy*, not just bigger. Asserts live in `tieout/`
and are immutable like the rest of the instrument.

### 4.5 Sourcing reliability (`tieout/pin_filings.py`)

Pin order per new name:
1. Curated direct filing URL (current primary path).
2. If the host bot-blocks scripted GET or serves a CDN stub (SAP/LVMH class):
   fetch once via the existing finmodel headed-browser pipeline, write the PDF to
   `filings/<ticker>/annual_report.pdf`, then it is immutable on disk.
3. Human-drop fallback (already supported): a PDF placed in
   `filings/<ticker>/` of sufficient size is picked up.

Missing pins do not abort the run (current behavior kept); the run reports them
and continues. Once pinned, the PDF is immutable — sourcing is explicitly NOT
under test.

### 4.6 Reporting (`tieout/run_tieout.py`, `_report.md`)

`_report.md` extended with: per-sector aggregate %, per-axis coverage table,
disagreed-cell census per company, and a regression table vs the immutable
baseline (existing 7 must not drop).

## 5. Basket selection matrix

Failure-mode axes (derived from the 5 known root-cause classes + known
SAP/LVMH gaps + the sector decision). Selection rule: **every axis value covered
by at least one name; minimize total names while maximizing axis coverage.**

| Axis | Values to cover |
|---|---|
| Sector schema | industrial, bank, insurer |
| Filing format | traditional AR, integrated report, Universal Registration Doc, US 10-K, 20-F/40-F |
| Accounting standard | IFRS, US GAAP, Indian Ind-AS, other local GAAP |
| Number formatting | EU space-thousands, comma, Indian lakh/crore, scale (k vs m vs bn), parentheses-negative |
| Layout | single-column, two-up side-by-side, 3+ year columns, landscape |
| Language | English, translated, bilingual/primary-language |
| Fiscal period | calendar, non-calendar, 52/53-week |

Concrete name selection is a planning deliverable (the implementation plan picks
specific tickers and tags each with the axes it stresses, ~15-20 total = 7
existing + ~10-13 new across waves 1-3 including held-outs). Selection is
deferred to the plan so candidate availability/sourcing can be validated per
name.

## 6. Anti-overfit guards (the loop must respect all)

1. `tieout/` is immutable — the improvement loop NEVER edits config, ground
   truth, asserts, or harness. Fixes land only in `src/` (`extractor.py`,
   `reconciler.py`, `fetcher.py`).
2. Each `groundtruth/<ticker>.json` is write-once; never regenerated in-loop.
3. **No-regression:** any extractor change that raises a new name but drops any
   previously-green cell on the existing/earlier-wave names is rejected.
4. **Waved reveal:** the extractor is tuned only on revealed waves; later waves
   (and the Wave-3 held-out names) are scored cold as the generalization number.
5. **No ticker-/filing-specific hardcoding in `src/`** — the established
   immutable-key anti-gaming principle. Fixes must be root-cause classes that
   generalize, not page numbers or company-specific constants.

## 7. Metric & direction (for the autoresearch loop)

- **Metric:** aggregate tie-out % across the full revealed basket (cells matched
  / cells the filing reports), reported overall, per-sector, and per-wave; plus
  the cold held-out % at the end.
- **Direction:** failing cells → cluster into root-cause classes (the way the
  original 5 classes were found) → fix the responsible `src/` module → re-run the
  gate → keep only if metric up and no-regression holds.
- **Verify:** `python -m tieout.run_tieout` last-line aggregate; `_report.md`
  regression table green for all prior waves; full `pytest` (131+ tests) and
  existing tie-out baseline unbroken.

## 8. Testing

- Wave 0 acceptance test: regenerate GT for the existing 7 with the
  sector-aware refactor; assert **value-identical** GT vs the committed immutable
  files — every trusted cell, year set, and currency unchanged (JSON key order
  and `source_pdf` path are not asserted). Proves the refactor is
  non-destructive.
- Sector detection unit tests: synthetic face text for industrial/bank/insurer →
  correct schema selected; ambiguous text → documented tie-break.
- Per-sector GT smoke: one known bank and one known insurer with hand-verified
  asserts must build GT that passes its assert block.
- No-regression gate wired into the loop's keep/discard decision.
- Full existing `pytest` suite stays green throughout.

## 9. Open items deferred to the plan

- Specific ticker list per wave + per-name axis tags + curated source URLs.
- Exact bank/insurer canonical key names (draft above; finalize against 2-3 real
  filings during planning so keys map cleanly to what filings actually print).
- Whether `reconciler.py` needs sector-aware bridges (likely for banks — no
  gross-profit/EV bridge; assess during Wave 2).
