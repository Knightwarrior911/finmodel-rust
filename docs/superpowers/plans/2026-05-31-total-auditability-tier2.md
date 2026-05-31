# Total Auditability (Tier 2) Implementation Plan

> **For agentic workers:** Use superpowers:subagent-driven-development. TDD, checkbox steps.

**Goal:** Make every Excel cell (including formulas) and every chat answer carry visible provenance.

**Architecture:** Part A extends the ledger-gated Excel audit pass to annotate formula cells with their precedent labels + coverage stats. Part B adds a pure `build_sources_report(cache)` markdown generator and appends it to orchestrator answers. All new behavior is gated/guarded so no-ledger and no-ticker paths are byte-identical.

**Tech Stack:** Python 3.11, openpyxl, pytest. Spec: `docs/superpowers/specs/2026-05-31-total-auditability-tier2-design.md`. Branch `feat/total-auditability`.

**Baseline:** 211 passed, 6 skipped. Gates: full pytest green; existing audit tests + no-ledger Excel behavior unchanged.

---

## Task 1: Sources & Assumptions report generator

**Files:** Create `src/sources_report.py`; Create `tests/test_sources_report.py`.

- [ ] **Step 1: Write `tests/test_sources_report.py`**

```python
from src.source_ledger import SourceLedger
from src.sources_report import build_sources_report


def _cache():
    led = SourceLedger()
    led.record_derived("assumptions", "tax_rate_pct", None, value=0.25,
                       formula="income_tax / (net_income + income_tax)", inputs=[])
    led.record_assumption("assumptions", "terminal_growth_rate", None, value=0.025,
                          rationale="Long-run GDP/inflation proxy", basis="house default")
    led.record_unverified("dcf", "preferred_stock", None,
                          reason="not in extraction schema")
    return {"__ledger__": led.to_json()}


def test_report_has_all_sections():
    md = build_sources_report(_cache())
    assert "Sources & Assumptions" in md
    assert "Derived" in md and "income_tax" in md
    assert "Assumptions" in md and "GDP" in md
    assert "Unverified" in md and "preferred_stock" in md


def test_empty_cache_no_error():
    md = build_sources_report({})
    assert isinstance(md, str)
    assert "Sources & Assumptions" in md


def test_unverified_section_absent_when_none():
    led = SourceLedger()
    led.record_assumption("a", "x", None, value=1.0, rationale="r", basis="b")
    md = build_sources_report({"__ledger__": led.to_json()})
    assert "Unverified" not in md
```

- [ ] **Step 2: Run `python -m pytest tests/test_sources_report.py -q` — confirm FAIL (ModuleNotFoundError).**

- [ ] **Step 3: Create `src/sources_report.py`:**

```python
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
```

- [ ] **Step 4: Run `python -m pytest tests/test_sources_report.py -q` — expect 3 passed.**

- [ ] **Step 5: Commit**

```bash
git add src/sources_report.py tests/test_sources_report.py
git commit -m "feat(audit): sources & assumptions report generator"
```

---

## Task 2: Formula-cell lineage in the Excel audit pass

**Files:** Modify `src/audit_pipeline.py` (`annotate_workbook_with_links`); Test `tests/test_formula_lineage.py`.

**Context:** `annotate_workbook_with_links(xlsx_path, *, cache_path=None)` loads the workbook (NOT data_only, so formula cells are strings starting "="), iterates numeric cells applying ledger/filing/market tiers + a red catch-all (all gated behind `ledger_present = bool(cache.get("__ledger__", {}).get("entries"))`). Helpers `_row_label(ws, cell)` and `_label_tokens(text)` exist. The return dict currently includes `linked_page/linked_doc/linked_market/derived/assumption/filing/market/unverified/total`.

- [ ] **Step 1: Write `tests/test_formula_lineage.py`**

```python
import json
import openpyxl
from src.source_ledger import SourceLedger
from src.audit_pipeline import annotate_workbook_with_links


def _wb(tmp_path):
    wb = openpyxl.Workbook()
    ws = wb.active
    ws.title = "IS"
    ws["A1"] = "Revenue"; ws["B1"] = 1000
    ws["A2"] = "COGS"; ws["B2"] = 600
    ws["A3"] = "Gross Profit"; ws["B3"] = "=B1-B2"
    p = tmp_path / "m.xlsx"; wb.save(p); return p


def _cache_with_ledger(tmp_path):
    led = SourceLedger()
    led.record_derived("x", "y", None, value=1.0, formula="f", inputs=[])
    p = tmp_path / "c.json"
    p.write_text(json.dumps({"__ledger__": led.to_json()}), encoding="utf-8")
    return p


def test_formula_cell_gets_lineage_comment(tmp_path):
    xlsx = _wb(tmp_path)
    cp = _cache_with_ledger(tmp_path)
    res = annotate_workbook_with_links(str(xlsx), cache_path=str(cp))
    assert res["derived_formula"] >= 1
    assert "covered_pct" in res
    wb = openpyxl.load_workbook(str(xlsx))
    c = wb["IS"]["B3"]
    assert c.comment is not None
    assert "Computed" in c.comment.text
    assert "Revenue" in c.comment.text and "COGS" in c.comment.text


def test_no_ledger_leaves_formula_uncommented(tmp_path):
    xlsx = _wb(tmp_path)
    cp = tmp_path / "empty.json"; cp.write_text("{}", encoding="utf-8")
    annotate_workbook_with_links(str(xlsx), cache_path=str(cp))
    wb = openpyxl.load_workbook(str(xlsx))
    assert wb["IS"]["B3"].comment is None
```

- [ ] **Step 2: Run `python -m pytest tests/test_formula_lineage.py -q` — confirm FAIL (KeyError 'derived_formula' or comment None).**

- [ ] **Step 3: Implement in `src/audit_pipeline.py`.**

Add module-level helpers near the other helpers:

```python
import re as _re

_CELL_REF = _re.compile(
    r"(?:'(?P<q>[^']+)'|(?P<s>[A-Za-z_][A-Za-z0-9_]*))!)?\$?(?P<col>[A-Z]{1,3})\$?(?P<row>\d+)"
)


def _formula_refs(formula: str):
    """Yield (sheet_name_or_None, 'COLROW') for each cell ref in a formula."""
    for m in _CELL_REF.finditer(formula or ""):
        sheet = m.group("q") or m.group("s")
        yield sheet, f"{m.group('col')}{m.group('row')}"


def _formula_lineage_labels(wb, ws, formula: str, limit: int = 6):
    """Resolve a formula's cell refs to unique precedent row-labels."""
    labels = []
    for sheet, coord in _formula_refs(formula):
        target_ws = ws
        if sheet:
            try:
                target_ws = wb[sheet]
            except KeyError:
                continue
        try:
            ref_cell = target_ws[coord]
        except (ValueError, KeyError):
            continue
        lbl = _row_label(target_ws, ref_cell)
        if lbl and lbl not in labels:
            labels.append(lbl)
        if len(labels) >= limit:
            break
    return labels
```

In `annotate_workbook_with_links`, after the existing numeric-cell loop completes (still inside the function, only when `ledger_present`), add a formula-annotation pass and a `derived_formula` counter (initialize `derived_formula = 0` with the other counters):

```python
    if ledger_present:
        for ws in wb.worksheets:
            for row in ws.iter_rows():
                for cell in row:
                    v = cell.value
                    if not (isinstance(v, str) and v.startswith("=")):
                        continue
                    if cell.comment is not None:   # don't double-annotate
                        continue
                    labels = _formula_lineage_labels(wb, ws, v)
                    txt = f"Computed: {v}"
                    if labels:
                        txt += "\nfrom: " + ", ".join(labels)
                    cell.comment = openpyxl.comments.Comment(txt, "audit")
                    derived_formula += 1
```

Extend the return dict: add `"derived_formula": derived_formula`, and a coverage block:

```python
    numeric_total = linked_page + linked_doc + linked_market + derived + assumption + unverified
    formula_total = derived_formula
    covered = filing + market + derived + assumption + derived_formula \
              + linked_page + linked_doc + linked_market
    denom = numeric_total + formula_total
    result["derived_formula"] = derived_formula
    result["covered_pct"] = round(100.0 * covered / denom, 1) if denom else 0.0
```

(Adapt `result` to however the function names its return dict; ensure all existing keys remain. If counters like `filing`/`market` aren't separately tracked, reuse `linked_page+linked_doc` for filing and `linked_market` for market in the coverage sum — do not double count. The exact coverage arithmetic is secondary; the REQUIRED outcomes are: `derived_formula` key present and > 0 for the test workbook, `covered_pct` key present, and formula cells commented.)

Keep the whole block gated behind `ledger_present`. Save the workbook as the function already does.

- [ ] **Step 4: Run `python -m pytest tests/test_formula_lineage.py -q` (expect 2 passed), THEN full suite `python -m pytest -q` (expect 213 passed + prior, 6 skipped). Existing audit tests MUST still pass; no-ledger behavior unchanged.**

- [ ] **Step 5: Commit**

```bash
git add src/audit_pipeline.py tests/test_formula_lineage.py
git commit -m "feat(audit): formula-cell lineage comments + coverage stats"
```

---

## Task 3: Append sources report to orchestrator answers

**Files:** Modify `src/orchestrator.py` (`VirtualAnalystOrchestrator.run`); Test `tests/test_orchestrator_sources.py`.

**Context:** `run(self, query, ticker="", company="", max_iterations=10)` returns final text at the `end_turn` branch (~line 2040) and at the fallback (~line 2096). Add a `_finalize(answer, ticker)` helper that appends the sources report when a ticker cache with a ledger exists, and route both return points through it.

- [ ] **Step 1: Write `tests/test_orchestrator_sources.py`**

```python
import json
from pathlib import Path
from src.source_ledger import SourceLedger
from src.orchestrator import VirtualAnalystOrchestrator


def test_finalize_appends_report(tmp_path, monkeypatch):
    # Point the cache dir at tmp by writing extraction_cache/<TICKER>.json
    cache_dir = Path("extraction_cache"); cache_dir.mkdir(exist_ok=True)
    led = SourceLedger()
    led.record_assumption("assumptions", "terminal_growth_rate", None,
                          value=0.025, rationale="GDP proxy", basis="house default")
    cpath = cache_dir / "ZZTEST.json"
    cpath.write_text(json.dumps({"__ledger__": led.to_json()}), encoding="utf-8")
    try:
        orch = VirtualAnalystOrchestrator.__new__(VirtualAnalystOrchestrator)
        out = orch._finalize("The terminal growth rate is 2.5%.", "ZZTEST")
        assert "Sources & Assumptions" in out
        assert "terminal_growth_rate" in out
    finally:
        cpath.unlink(missing_ok=True)


def test_finalize_no_ticker_unchanged():
    orch = VirtualAnalystOrchestrator.__new__(VirtualAnalystOrchestrator)
    assert orch._finalize("plain answer", "") == "plain answer"


def test_finalize_missing_cache_unchanged():
    orch = VirtualAnalystOrchestrator.__new__(VirtualAnalystOrchestrator)
    assert orch._finalize("plain answer", "NOPE_NO_CACHE") == "plain answer"
```

- [ ] **Step 2: Run `python -m pytest tests/test_orchestrator_sources.py -q` — confirm FAIL (no attribute `_finalize`).**

- [ ] **Step 3: Implement in `src/orchestrator.py`.**

Add the helper method to `VirtualAnalystOrchestrator`:

```python
    def _finalize(self, answer: str, ticker: str) -> str:
        """Append a Sources & Assumptions provenance appendix when a ticker
        cache with a ledger exists. Never raises — returns answer unchanged on
        any failure or when there is nothing to cite."""
        if not ticker:
            return answer
        try:
            import json
            from pathlib import Path
            from src.sources_report import build_sources_report
            cdir = Path("extraction_cache")
            cpath = cdir / (ticker.replace(".", "_").replace("-", "_") + ".json")
            if not cpath.exists():
                cpath = cdir / (ticker.replace(".", "_") + ".json")
            if not cpath.exists():
                cpath = cdir / (ticker + ".json")
            if not cpath.exists():
                return answer
            cache = json.loads(cpath.read_text(encoding="utf-8"))
            if not (cache.get("__ledger__", {}) or {}).get("entries"):
                return answer
            return answer + "\n\n---\n" + build_sources_report(cache)
        except Exception:
            return answer
```

Route the two return points through it:
- The `end_turn` branch (~line 2040) currently does `return next((b.text ...), "Analysis complete.")`. Wrap: `answer = next((b.text ...), "Analysis complete."); return self._finalize(answer, ticker)`.
- The fallback (~line 2096-2100): capture the chosen text into `answer` and `return self._finalize(answer, ticker)` instead of `return block.text` / `return "Analysis complete."`.

Do not change any other behavior.

- [ ] **Step 4: Run `python -m pytest tests/test_orchestrator_sources.py -q` (expect 3 passed), THEN full suite `python -m pytest -q` (expect 216 passed, 6 skipped). No regressions. NOTE: importing orchestrator requires `anthropic` to be installed (it is — the suite already imports it elsewhere); the test uses `__new__` to avoid constructing the Anthropic client.**

- [ ] **Step 5: Commit**

```bash
git add src/orchestrator.py tests/test_orchestrator_sources.py
git commit -m "feat(audit): append sources report to orchestrator answers"
```

---

## Task 4: Regression + PR

- [ ] **Step 1: Full suite** — `python -m pytest -q` → expect 216 passed, 6 skipped, 0 failed.
- [ ] **Step 2: Tie-out guard** — `python -m pytest tests/test_tieout_no_regression.py tests/test_tieout_sector.py -q` → green (extraction path untouched).
- [ ] **Step 3: Push + PR**

```bash
git push -u origin feat/total-auditability
gh pr create --title "Total auditability (Tier 2): formula lineage + sources appendix" \
  --body "Implements docs/superpowers/specs/2026-05-31-total-auditability-tier2-design.md. Part A: formula cells get Computed:/from: lineage comments + coverage% in the Excel audit pass (gated behind __ledger__). Part B: build_sources_report() markdown appendix appended to orchestrator answers when a ticker cache has a ledger. Full pytest green; existing audit + no-ledger behavior unchanged."
```

---

## Self-review notes (author)
- Part A ↔ spec "computed-cell lineage"; Part B (Task 1 generator + Task 3 wiring) ↔ spec "Sources appendix". Coverage% delivered in Task 2.
- All new behavior gated (`ledger_present`) or guarded (try/except + ticker/cache checks) → no-ledger Excel and no-ticker chat paths byte-identical.
- Deliberately NOT doing inline-prose number citation (brittle); the structured appendix is the v1, per spec.
