# Tie-Out Sector-Aware Infra (Wave 0) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Refactor the `tieout/` instrument and `src/extractor.py` to be sector-aware (industrial/bank/insurer) with an extractor that auto-detects sector, while proving the change does not regress the existing 7-name baseline.

**Architecture:** `CANONICAL` becomes `CANONICAL_BY_SECTOR`; `industrial` is value-identical to today so existing immutable ground truth and the baseline metric are unchanged. The ground-truth builder and the gate (`run_tieout._compare`) select the schema by the company's `sector` tag. The extractor gains a deterministic pre-LLM `detect_sector()` and picks a sector-specific system prompt — the instrument never tells the model the sector. A per-company hard-assert registry generalizes the existing `_ATCO_ASSERT`.

**Tech Stack:** Python 3.11, pytest, pdfplumber, the existing `tieout.llm` transport.

**Repo:** `C:\Users\vinit\Documents\financial_model` (git, branch `master`).

**Scope:** Wave 0 only — infrastructure + non-regression proof. Adding diverse names (Waves 1-3) and the extractor improvement loop are separate follow-on plans, deliberately excluded so the loop tunes only on revealed waves.

---

## File Structure

| File | Responsibility | Action |
|---|---|---|
| `tieout/config.py` | `CANONICAL_BY_SECTOR`, per-sector `ABS_KEYS`/`EXCLUDE_KEYS`, `SECTORS`, `BASKET` rows gain `sector` | Modify |
| `tieout/groundtruth.py` | sector-keyed prompt + anchors + data-row; `sector` in GT JSON; `HARD_ASSERTS` registry | Modify |
| `tieout/run_tieout.py` | `_compare` selects schema by `gt["sector"]` | Modify |
| `src/extractor.py` | `detect_sector()`; sector-specific system prompts; wire into `extract_financials_from_pdf` | Modify |
| `tests/test_tieout_sector.py` | unit tests for schema integrity + sector detection + GT sector wiring | Create |
| `tests/test_tieout_no_regression.py` | baseline non-regression guard vs committed `_summary.json` | Create |

Existing importers of the old flat `CANONICAL`/`ABS_KEYS`/`EXCLUDE_KEYS` (`groundtruth.py`, `run_tieout.py`) are updated in the same wave — no back-compat shim (the change is internal to the instrument).

---

## Task 1: Sector-aware config schema

**Files:**
- Modify: `tieout/config.py`
- Test: `tests/test_tieout_sector.py`

- [ ] **Step 1: Write the failing test**

Create `tests/test_tieout_sector.py`:

```python
from tieout import config


# The exact industrial schema as it existed before the refactor — frozen here
# so a refactor that silently drops/renames an industrial key fails loudly.
_INDUSTRIAL_FROZEN = {
    "income_statement": [
        "revenue", "cogs", "gross_profit", "sga", "rd", "da", "ebit",
        "ebita", "interest_expense", "interest_income", "income_tax",
        "net_income",
    ],
    "balance_sheet": [
        "cash", "accounts_receivable", "inventory", "total_current_assets",
        "ppe_net", "goodwill", "intangibles_net", "total_assets",
        "accounts_payable", "long_term_debt", "total_liabilities",
        "total_equity",
    ],
    "cash_flow_statement": [
        "cfo", "capex", "cfi", "dividends_paid", "cff", "net_change_cash",
    ],
}


def test_industrial_schema_value_identical():
    assert config.CANONICAL_BY_SECTOR["industrial"] == _INDUSTRIAL_FROZEN


def test_sectors_present():
    assert set(config.SECTORS) == {"industrial", "bank", "insurer"}
    for s in config.SECTORS:
        assert set(config.CANONICAL_BY_SECTOR[s]) == {
            "income_statement", "balance_sheet", "cash_flow_statement"}


def test_per_sector_abs_and_exclude_keys_exist():
    for s in config.SECTORS:
        assert s in config.ABS_KEYS_BY_SECTOR
        assert s in config.EXCLUDE_KEYS_BY_SECTOR


def test_industrial_abs_exclude_value_identical():
    assert config.ABS_KEYS_BY_SECTOR["industrial"] == {
        "cogs", "sga", "rd", "interest_expense", "income_tax",
        "capex", "dividends_paid"}
    assert config.EXCLUDE_KEYS_BY_SECTOR["industrial"] == {"shares_diluted"}


def test_every_basket_row_has_known_sector():
    for row in config.BASKET:
        assert row["sector"] in config.SECTORS


def test_existing_seven_are_industrial():
    expected = {"ATCO-B.ST", "SAND.ST", "ASML.AS", "NESN.SW",
                "SAP.DE", "NOVO-B.CO", "MC.PA"}
    industrial = {r["ticker"] for r in config.BASKET
                  if r["sector"] == "industrial"}
    assert expected <= industrial
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd /c/Users/vinit/Documents/financial_model && python -m pytest tests/test_tieout_sector.py -v`
Expected: FAIL — `AttributeError: module 'tieout.config' has no attribute 'CANONICAL_BY_SECTOR'`.

- [ ] **Step 3: Rewrite the schema block in `tieout/config.py`**

Replace the current `CANONICAL = { ... }`, `ABS_KEYS = { ... }`, `EXCLUDE_KEYS = { ... }` block (the dict literals defining the industrial universe) with:

```python
SECTORS = ("industrial", "bank", "insurer")

CANONICAL_BY_SECTOR = {
    "industrial": {
        "income_statement": [
            "revenue", "cogs", "gross_profit", "sga", "rd", "da", "ebit",
            "ebita", "interest_expense", "interest_income", "income_tax",
            "net_income",
        ],
        "balance_sheet": [
            "cash", "accounts_receivable", "inventory",
            "total_current_assets", "ppe_net", "goodwill",
            "intangibles_net", "total_assets", "accounts_payable",
            "long_term_debt", "total_liabilities", "total_equity",
        ],
        "cash_flow_statement": [
            "cfo", "capex", "cfi", "dividends_paid", "cff",
            "net_change_cash",
        ],
    },
    "bank": {
        "income_statement": [
            "interest_income", "interest_expense", "net_interest_income",
            "fee_commission_income", "trading_income",
            "total_operating_income", "loan_loss_provisions",
            "operating_expenses", "pretax_income", "income_tax",
            "net_income",
        ],
        "balance_sheet": [
            "cash_and_central_bank", "loans_to_customers",
            "investment_securities", "total_assets", "customer_deposits",
            "debt_securities_issued", "total_liabilities", "total_equity",
        ],
        "cash_flow_statement": [
            "cfo", "cfi", "cff", "net_change_cash",
        ],
    },
    "insurer": {
        "income_statement": [
            "gross_written_premium", "net_earned_premium",
            "net_investment_income", "net_claims_incurred",
            "acquisition_expenses", "operating_expenses", "pretax_income",
            "income_tax", "net_income",
        ],
        "balance_sheet": [
            "investments", "cash", "total_assets",
            "insurance_contract_liabilities", "total_liabilities",
            "total_equity",
        ],
        "cash_flow_statement": [
            "cfo", "cfi", "cff", "net_change_cash",
        ],
    },
}

ABS_KEYS_BY_SECTOR = {
    "industrial": {
        "cogs", "sga", "rd", "interest_expense", "income_tax",
        "capex", "dividends_paid",
    },
    "bank": {
        "interest_expense", "loan_loss_provisions", "operating_expenses",
        "income_tax",
    },
    "insurer": {
        "net_claims_incurred", "acquisition_expenses", "operating_expenses",
        "income_tax",
    },
}

EXCLUDE_KEYS_BY_SECTOR = {
    "industrial": {"shares_diluted"},
    "bank": set(),
    "insurer": set(),
}
```

- [ ] **Step 4: Tag every `BASKET` row with `sector`**

In each of the 7 dicts inside `BASKET`, add `"sector": "industrial",` (all 7 existing names are industrials/consumer/tech). Example for the ATCO row:

```python
    {
        "ticker": "ATCO-B.ST",
        "company": "Atlas Copco AB",
        "currency": "SEK",
        "sector": "industrial",
        "pinned": "annual_report.pdf",
        "search": "Atlas Copco annual report",
    },
```

Repeat the `"sector": "industrial",` line for SAND.ST, ASML.AS, NESN.SW, SAP.DE, NOVO-B.CO, MC.PA.

- [ ] **Step 5: Run test to verify it passes**

Run: `python -m pytest tests/test_tieout_sector.py -v`
Expected: PASS (all 6 tests).

- [ ] **Step 6: Commit**

```bash
git add tieout/config.py tests/test_tieout_sector.py
git commit -m "$(cat <<'EOF'
feat(tieout): sector-aware CANONICAL_BY_SECTOR schema

industrial schema value-identical to prior flat CANONICAL; add bank +
insurer schemas, per-sector ABS/EXCLUDE keys, sector tag on basket rows.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: Sector-keyed ground-truth builder

**Files:**
- Modify: `tieout/groundtruth.py`
- Test: `tests/test_tieout_sector.py`

- [ ] **Step 1: Add failing tests**

Append to `tests/test_tieout_sector.py`:

```python
import inspect
from tieout import groundtruth


def test_build_ground_truth_accepts_sector():
    sig = inspect.signature(groundtruth.build_ground_truth)
    assert "sector" in sig.parameters


def test_hard_asserts_registry_has_atco():
    assert "ATCO-B.ST" in groundtruth.HARD_ASSERTS
    blk = groundtruth.HARD_ASSERTS["ATCO-B.ST"]["income_statement"]
    assert blk["revenue"][2023] == 172664
    assert blk["net_income"][2022] == 23482


def test_bank_income_data_row_matches_net_interest():
    rx = groundtruth.SECTOR_DATA_ROW["bank"]
    assert rx.search("Net interest income 12 345 11 200")
    assert not rx.search("Revenue 12 345 11 200")


def test_industrial_data_row_unchanged():
    rx = groundtruth.SECTOR_DATA_ROW["industrial"]
    assert rx.search("Net sales 172 664 141 325")
```

- [ ] **Step 2: Run to verify it fails**

Run: `python -m pytest tests/test_tieout_sector.py -k "sector or hard_asserts or data_row" -v`
Expected: FAIL — `HARD_ASSERTS` / `SECTOR_DATA_ROW` not defined; `sector` not a parameter.

- [ ] **Step 3: Add sector data-row table and anchors in `tieout/groundtruth.py`**

After the existing `_REVENUE_DATA_ROW` / `_UNIT_HINT` definitions, add:

```python
_BANK_DATA_ROW = re.compile(
    r"(?:net interest income|interest income|interest and similar income)"
    r"\b[^\n]*?\d{3,}[^\n]*?\d{3,}", re.I)
_INSURER_DATA_ROW = re.compile(
    r"(?:gross written premium|net earned premium|gross premium"
    r"|net premium earned|premiums? earned)\b[^\n]*?\d{3,}[^\n]*?\d{3,}",
    re.I)

SECTOR_DATA_ROW = {
    "industrial": _REVENUE_DATA_ROW,
    "bank": _BANK_DATA_ROW,
    "insurer": _INSURER_DATA_ROW,
}

_SECTOR_IS_ANCHORS = {
    "industrial": _IS_ANCHORS,
    "bank": _IS_ANCHORS + ["interest income", "net interest income"],
    "insurer": _IS_ANCHORS + ["insurance revenue", "premiums earned",
                              "gross written premium"],
}
```

- [ ] **Step 4: Make `_find_face_window` sector-aware**

Change the signature and the two regex/anchor uses:

```python
def _find_face_window(pages, sector="industrial"):
    anchors = _SECTOR_IS_ANCHORS[sector]
    data_row = SECTOR_DATA_ROW[sector]
    for i, norm in enumerate(pages):
        low = norm.lower()
        if not any(a in low for a in anchors):
            continue
        if not data_row.search(norm):
            continue
        if len(re.findall(r"\b20[1-3]\d\b", norm)) < 2:
            continue
        window = pages[i: i + 13]
        joined = "\n".join(window)
        best = ""
        for ln in norm.splitlines():
            ys = set(re.findall(r"\b(20[1-3]\d)\b", ln))
            if len(ys) >= 2:
                if _UNIT_HINT.search(ln) or "note" in ln.lower():
                    best = ln
                    break
                if not best:
                    best = ln
        return i, joined, best
    return 0, "\n".join(pages)[:120_000], ""
```

(Only the first three lines and the function signature change; the body below `window = ...` is unchanged from the current implementation — keep it byte-for-byte so industrial behaviour is identical.)

- [ ] **Step 5: Make `_build_prompt` and `build_ground_truth` sector-aware**

In `_build_prompt`, change the keys block to use the sector schema. Update signature to `_build_prompt(face_text, years, currency_hint, variant, sector="industrial")` and replace the `keys_block` line:

```python
    from tieout.config import CANONICAL_BY_SECTOR, ABS_KEYS_BY_SECTOR
    keys_block = json.dumps(CANONICAL_BY_SECTOR[sector], indent=2)
    abs_keys = sorted(ABS_KEYS_BY_SECTOR[sector])
```

and change the `sign_rule` f-string to interpolate `abs_keys` instead of the old module-level `ABS_KEYS`.

Update the top-of-file import line
`from tieout.config import CANONICAL, ABS_KEYS, EXCLUDE_KEYS, GT_DIR`
to
`from tieout.config import (CANONICAL_BY_SECTOR, ABS_KEYS_BY_SECTOR, EXCLUDE_KEYS_BY_SECTOR, GT_DIR)`.

In `build_ground_truth`, change the signature to:

```python
def build_ground_truth(ticker, company, currency, pdf_path, *,
                        sector="industrial", force=False):
```

Inside it: pass `sector` to `_find_face_window(pages, sector)` and to both `_build_prompt(..., "A", sector)` / `_build_prompt(..., "B", sector)`; replace the `for stmt, keys in CANONICAL.items()` loop header with `for stmt, keys in CANONICAL_BY_SECTOR[sector].items()`; replace `if key in EXCLUDE_KEYS` with `if key in EXCLUDE_KEYS_BY_SECTOR[sector]`; replace `_norm_cell`'s `ABS_KEYS` reference with a sector-bound set passed in (add parameter `abs_keys` to `_norm_cell` and call `_norm_cell(key, ..., abs_keys)` where `abs_keys = ABS_KEYS_BY_SECTOR[sector]`). Add `"sector": sector,` to the `gt` dict that gets written.

- [ ] **Step 6: Generalize `_ATCO_ASSERT` into `HARD_ASSERTS`**

Replace `_ATCO_ASSERT = { ... }` with:

```python
HARD_ASSERTS = {
    "ATCO-B.ST": {
        "income_statement": {
            "revenue": {2023: 172664, 2022: 141325},
            "gross_profit": {2023: 75117, 2022: 59384},
            "ebit": {2023: 37091, 2022: 30216},
            "rd": {2023: 6693, 2022: 5389},
            "net_income": {2023: 28052, 2022: 23482},
        },
    },
}
```

Change `_assert_atco(gt)` to a generic `_assert_hard(gt, ticker)` that looks up `HARD_ASSERTS.get(ticker)` and returns immediately if absent; otherwise runs the same per-cell comparison loop and raises `AssertionError` on mismatch. Replace the `if ticker == "ATCO-B.ST": _assert_atco(gt)` call site with `_assert_hard(gt, ticker)`. Update the `__main__` block to pass `sector=row.get("sector", "industrial")` into `build_ground_truth`.

- [ ] **Step 7: Run tests**

Run: `python -m pytest tests/test_tieout_sector.py -v`
Expected: PASS (all tests including the new GT-sector ones).

- [ ] **Step 8: Commit**

```bash
git add tieout/groundtruth.py tests/test_tieout_sector.py
git commit -m "$(cat <<'EOF'
feat(tieout): sector-keyed ground-truth builder + HARD_ASSERTS registry

build_ground_truth takes sector; sector-specific face anchors/data-row;
_ATCO_ASSERT generalized to a per-company hard-assert registry.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: Sector-aware gate (`run_tieout._compare`)

**Files:**
- Modify: `tieout/run_tieout.py`
- Test: `tests/test_tieout_sector.py`

- [ ] **Step 1: Add failing test**

Append to `tests/test_tieout_sector.py`:

```python
from tieout import run_tieout


def test_compare_uses_sector_schema():
    gt = {
        "years": [2022, 2023],
        "sector": "bank",
        "values": {"income_statement": {
            "net_interest_income": {"2022": 100, "2023": 110}}},
        "citations": {},
    }
    model = {"income_statement": {"net_interest_income": [100, 110]}}
    pct, denom, matched, per_stmt, rows = run_tieout._compare(gt, model)
    assert denom == 2 and matched == 2 and pct == 100.0
```

- [ ] **Step 2: Run to verify it fails**

Run: `python -m pytest tests/test_tieout_sector.py::test_compare_uses_sector_schema -v`
Expected: FAIL — `_compare` iterates the old flat `CANONICAL`, so the bank key is never scored (`denom == 0`).

- [ ] **Step 3: Make `_compare` and `_norm` sector-aware**

Change the import line in `run_tieout.py`
`from tieout.config import (BASKET, CANONICAL, ABS_KEYS, EXCLUDE_KEYS, RESULTS_DIR, ticker_filings_dir)`
to
`from tieout.config import (BASKET, CANONICAL_BY_SECTOR, ABS_KEYS_BY_SECTOR, EXCLUDE_KEYS_BY_SECTOR, RESULTS_DIR, ticker_filings_dir)`.

Replace `_norm(key, v)` with `_norm(key, v, abs_keys)` (use `abs_keys` instead of module `ABS_KEYS`). In `_compare`, derive the schema from the GT:

```python
def _compare(gt: dict, model: dict):
    years = gt["years"]
    sector = gt.get("sector", "industrial")
    canonical = CANONICAL_BY_SECTOR[sector]
    abs_keys = ABS_KEYS_BY_SECTOR[sector]
    exclude_keys = EXCLUDE_KEYS_BY_SECTOR[sector]
    rows, denom, matched = [], 0, 0
    per_stmt = {}
    for stmt, keys in canonical.items():
        s_d = s_m = 0
        gvals = gt["values"].get(stmt, {})
        mvals = model.get(stmt, {}) or {}
        for key in keys:
            if key in exclude_keys:
                continue
            gk = gvals.get(key, {})
            if not gk:
                continue
            mlist = mvals.get(key)
            for y in years:
                gv = gk.get(str(y))
                if gv is None:
                    continue
                denom += 1
                s_d += 1
                mv = None
                if isinstance(mlist, list):
                    idx = years.index(y)
                    if idx < len(mlist):
                        mv = _norm(key, mlist[idx], abs_keys)
                ok = (mv is not None and mv == int(gv))
                if ok:
                    matched += 1
                    s_m += 1
                else:
                    rows.append({
                        "statement": stmt, "key": key, "year": y,
                        "ground_truth": int(gv), "model": mv,
                        "page": gt.get("citations", {}).get(stmt),
                    })
        per_stmt[stmt] = {"trusted": s_d, "matched": s_m,
                          "pct": round(100 * s_m / s_d, 2) if s_d else None}
    pct = round(100 * matched / denom, 2) if denom else None
    return pct, denom, matched, per_stmt, rows
```

In `run(...)`, pass the basket row's sector into GT building: locate the `build_ground_truth(tk, row["company"], row["currency"], str(pdf))` call and change it to
`build_ground_truth(tk, row["company"], row["currency"], str(pdf), sector=row.get("sector", "industrial"))`.

- [ ] **Step 4: Run test to verify it passes**

Run: `python -m pytest tests/test_tieout_sector.py::test_compare_uses_sector_schema -v`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add tieout/run_tieout.py tests/test_tieout_sector.py
git commit -m "$(cat <<'EOF'
feat(tieout): sector-aware compare in the gate

_compare/_norm select schema, ABS and EXCLUDE keys by gt["sector"];
run() passes basket row sector into ground-truth building.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: Extractor sector auto-detection

**Files:**
- Modify: `src/extractor.py`
- Test: `tests/test_tieout_sector.py`

- [ ] **Step 1: Add failing tests**

Append to `tests/test_tieout_sector.py`:

```python
import src.extractor as ex


def test_detect_sector_bank():
    pages = ["Consolidated income statement",
             "Net interest income 12 345 11 200\n"
             "Loans and advances to customers 998 877"]
    assert ex.detect_sector(pages) == "bank"


def test_detect_sector_insurer():
    pages = ["Consolidated income statement",
             "Gross written premium 5 000 4 800\n"
             "Net claims incurred 3 100 2 900\n"
             "Insurance contract liabilities 9 000"]
    assert ex.detect_sector(pages) == "insurer"


def test_detect_sector_industrial_default():
    pages = ["Consolidated income statement",
             "Net sales 172 664 141 325\nCost of goods sold 97 547"]
    assert ex.detect_sector(pages) == "industrial"
```

- [ ] **Step 2: Run to verify it fails**

Run: `python -m pytest tests/test_tieout_sector.py -k detect_sector -v`
Expected: FAIL — `AttributeError: module 'src.extractor' has no attribute 'detect_sector'`.

- [ ] **Step 3: Implement `detect_sector` in `src/extractor.py`**

Add immediately after `_extract_financial_section` (before `extract_financials_from_pdf`):

```python
_BANK_SIGNATURES = (
    "net interest income", "loans and advances to customers",
    "due to customers", "interest and similar income",
)
_INSURER_SIGNATURES = (
    "gross written premium", "net earned premium",
    "insurance contract liabilities", "net claims incurred",
    "premiums earned",
)


def detect_sector(text_pages: list[str]) -> str:
    """Deterministic pre-LLM sector guess from filing face text.

    bank/insurer require >=2 distinct sector signatures so a passing
    mention in an industrial filing's notes does not misclassify it.
    Default is 'industrial'.
    """
    blob = "\n".join(text_pages[:80]).lower()
    bank_hits = sum(1 for s in _BANK_SIGNATURES if s in blob)
    ins_hits = sum(1 for s in _INSURER_SIGNATURES if s in blob)
    if ins_hits >= 2 and ins_hits >= bank_hits:
        return "insurer"
    if bank_hits >= 2:
        return "bank"
    return "industrial"
```

- [ ] **Step 4: Add sector-specific system prompts**

The current `FINANCIALS_SYSTEM_PROMPT` is the industrial prompt. Keep it byte-for-byte and bind it as the industrial entry; add bank/insurer variants that mirror its rules but swap the JSON `income_statement`/`balance_sheet` key blocks for the bank/insurer keys from `tieout/config.py` (`net_interest_income`, `loans_to_customers`, … / `gross_written_premium`, `insurance_contract_liabilities`, …). After the `FINANCIALS_SYSTEM_PROMPT = """..."""` literal add:

```python
_BANK_SYSTEM_PROMPT = FINANCIALS_SYSTEM_PROMPT.replace(
    '"income_statement": {', '"income_statement": {  // BANK', 1)
# NOTE: replace the industrial IS/BS JSON key lines with the bank keys:
#   income_statement: interest_income, interest_expense,
#     net_interest_income, fee_commission_income, trading_income,
#     total_operating_income, loan_loss_provisions, operating_expenses,
#     pretax_income, income_tax, net_income
#   balance_sheet: cash_and_central_bank, loans_to_customers,
#     investment_securities, total_assets, customer_deposits,
#     debt_securities_issued, total_liabilities, total_equity
#   cash_flow_statement: cfo, cfi, cff, net_change_cash
# Write the full prompt string out explicitly (do not chain .replace());
# keep every non-schema rule line identical to the industrial prompt.

_INSURER_SYSTEM_PROMPT = "..."  # same: industrial rules, insurer key block

_SYSTEM_PROMPT_BY_SECTOR = {
    "industrial": FINANCIALS_SYSTEM_PROMPT,
    "bank": _BANK_SYSTEM_PROMPT,
    "insurer": _INSURER_SYSTEM_PROMPT,
}
```

Implementation note for the engineer: write `_BANK_SYSTEM_PROMPT` and `_INSURER_SYSTEM_PROMPT` as full explicit triple-quoted strings — copy the entire `FINANCIALS_SYSTEM_PROMPT`, then in the copy replace only the `income_statement` and `balance_sheet` key lines with the sector keys listed above and drop `shares_diluted`/`gross_profit`/`cogs`/`inventory` etc. that do not apply. Every rule bullet and the `notes`/`confidence`/`discrepancies` tail stay identical. The `.replace()` snippet above is illustrative only — do not ship a chained-replace; ship literal strings.

- [ ] **Step 5: Wire detection into `extract_financials_from_pdf`**

In `extract_financials_from_pdf`, after `text_pages = [...]` and before `text_chunk = _extract_financial_section(text_pages)`, add:

```python
    sector = detect_sector(text_pages)
    system_prompt = _SYSTEM_PROMPT_BY_SECTOR[sector]
```

Change the extraction call from
`raw = _llm_complete(FINANCIALS_SYSTEM_PROMPT, prompt, max_tokens=8192)`
to
`raw = _llm_complete(system_prompt, prompt, max_tokens=8192)`.

The return tuple is unchanged (`is_dict, bs_dict, cfs_dict, notes, years_found`); downstream consumers and `run_tieout._model_extract` need no change because keys flow through generically.

- [ ] **Step 6: Run tests**

Run: `python -m pytest tests/test_tieout_sector.py -k detect_sector -v`
Expected: PASS (3 detection tests).

- [ ] **Step 7: Commit**

```bash
git add src/extractor.py tests/test_tieout_sector.py
git commit -m "$(cat <<'EOF'
feat(extractor): pre-LLM sector auto-detection + sector system prompts

detect_sector() picks industrial/bank/insurer from face text; extractor
selects the matching schema prompt. Instrument never passes sector to
the model path.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 5: Non-regression guard vs committed baseline

**Files:**
- Create: `tests/test_tieout_no_regression.py`
- Read: `tieout/results/_summary.json` (committed baseline; if absent, generate once with `python -m tieout.run_tieout` BEFORE Task 1 and commit it as the frozen baseline)

- [ ] **Step 1: Freeze the baseline (one-time, do this BEFORE Task 1 if not already committed)**

Run: `python -m tieout.run_tieout --quiet` then
`cp tieout/results/_summary.json tieout/results/_baseline_wave0.json` and
`git add -f tieout/results/_baseline_wave0.json && git commit -m "test(tieout): freeze pre-Wave0 baseline summary"`.
(`-f` because `tieout/results/` is gitignored at runtime; this one snapshot is intentionally tracked as the regression oracle.)

- [ ] **Step 2: Write the regression test**

Create `tests/test_tieout_no_regression.py`:

```python
import json
from pathlib import Path

import pytest

_REPO = Path(__file__).parent.parent
_BASELINE = _REPO / "tieout" / "results" / "_baseline_wave0.json"


@pytest.mark.skipif(not _BASELINE.exists(),
                    reason="no frozen Wave0 baseline committed")
def test_existing_industrials_do_not_regress():
    """Every company measured in the frozen baseline must still match at
    least as many cells after the sector-aware refactor. Industrial GT is
    immutable and value-identical, so matched/trusted must not drop."""
    base = json.loads(_BASELINE.read_text(encoding="utf-8"))
    cur_path = _REPO / "tieout" / "results" / "_summary.json"
    assert cur_path.exists(), "run `python -m tieout.run_tieout` first"
    cur = json.loads(cur_path.read_text(encoding="utf-8"))
    for tk, b in base["companies"].items():
        assert tk in cur["companies"], f"{tk} dropped from measured set"
        c = cur["companies"][tk]
        assert c["trusted"] == b["trusted"], (
            f"{tk} trusted-cell count changed {b['trusted']}->{c['trusted']} "
            f"(industrial GT must be immutable/value-identical)")
        assert c["matched"] >= b["matched"], (
            f"{tk} regressed: matched {b['matched']}->{c['matched']}")
```

- [ ] **Step 3: Run the gate, then the regression test**

Run:
```
python -m tieout.run_tieout --quiet
python -m pytest tests/test_tieout_no_regression.py -v
```
Expected: PASS — refactor did not change any industrial company's `trusted` count and did not lower `matched`. (Ground truth is read from immutable cache, never regenerated, so `trusted` must be exactly equal.)

If `trusted` changed for any company: the refactor altered the industrial schema or GT path — STOP and fix (Task 1/2 broke value-identity) before proceeding.

- [ ] **Step 4: Full suite green**

Run: `python -m pytest -q`
Expected: all pre-existing tests (131+) plus the new sector + no-regression tests PASS. No regressions.

- [ ] **Step 5: Commit**

```bash
git add tests/test_tieout_no_regression.py
git commit -m "$(cat <<'EOF'
test(tieout): Wave 0 non-regression guard vs frozen baseline

Asserts the sector-aware refactor leaves every existing industrial
company's trusted-cell count unchanged and matched count not lowered.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Self-Review

**Spec coverage:**
- §4.1 sector-aware universe → Task 1.
- §4.2 extractor sector auto-detect (instrument must not tell model) → Task 4 (detection is pre-LLM in `src/extractor.py`; `run_tieout` never passes sector to `_model_extract`).
- §4.3 sector-aware GT (anchors, data-row, prompt; dual-pass unchanged) → Task 2.
- §4.4 per-company hard self-test registry → Task 2 Step 6 (`HARD_ASSERTS`).
- §4.6 reporting → deferred: per-sector report columns are cosmetic and only meaningful once non-industrial names exist (Wave 2+); not in Wave 0 scope. Noted as a Wave 2 follow-on.
- §6 anti-overfit guard (no-regression; tieout immutable; GT write-once) → Task 5 + the design's immutability rule (loop edits only `src/`).
- §8 Wave 0 acceptance (value-identical GT, full pytest green) → Task 1 frozen-schema test + Task 5 (`trusted` count must be exactly equal) + Task 5 Step 4.
- §4.5 sourcing reliability → not exercised in Wave 0 (no new names pinned); lands in Wave 1 plan. Correctly out of scope here.

**Placeholder scan:** Task 4 Step 4 intentionally instructs writing two full literal prompt strings rather than embedding ~90 lines of duplicated prompt text twice in this plan; the exact key sets to substitute are fully enumerated, the transformation is unambiguous (copy industrial prompt, swap only the IS/BS key blocks to the listed sector keys, keep all rule lines), and the no-chained-replace constraint is explicit. This is a bounded, fully-specified instruction, not a "figure it out" placeholder.

**Type consistency:** `build_ground_truth(..., *, sector=...)` (Task 2) is called with `sector=row.get("sector","industrial")` in `run_tieout.run` (Task 3) and in the `__main__` block (Task 2 Step 6). `_compare` reads `gt["sector"]` (Task 3), written by `build_ground_truth` (Task 2 Step 5). `detect_sector` (Task 4) returns one of `config.SECTORS` (Task 1). `HARD_ASSERTS` keys are tickers; `_assert_hard(gt, ticker)` no-ops when absent — consistent across Task 2.

---

## Out of Scope (follow-on plans)

- **Wave 1** — add ~5-6 schema-safe diverse industrials + extractor improvement loop.
- **Wave 2** — add banks; per-sector report columns (§4.6); assess sector-aware `reconciler.py` bridges.
- **Wave 3** — add insurers + cold held-out generalization names; final gate.
- **Sourcing hardening** (§4.5) — headed-browser pin fallback for bot-blocked hosts, exercised when Wave 1 names are pinned.
