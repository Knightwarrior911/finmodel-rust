# Ledger v1 Follow-ups Implementation Plan

> **For agentic workers:** Use superpowers:subagent-driven-development. TDD, checkbox steps.

**Goal:** Close two ledger loose ends, offline-verifiable: (A) carry the Sources & Assumptions appendix into PPTX decks; (B) make the DCF consume `preferred_stock` / `short_term_investments` from the balance sheet when present (tag FILING) instead of always 0/UNVERIFIED.

**Design rationale (embedded):** PPTX runs have no comment surface and DERIVED/ASSUMPTION tiers have no URL, so deck provenance lives in **speaker notes** (clean, useful for a presenter) via the existing `build_sources_report`. For the EV bridge, dcf currently hardcodes `preferred = investments = 0.0`; we read them from `balance_sheet` when present and tag the source accordingly. **Deferred (NOT in this PR):** changing the extractor PROMPT to emit those fields — that touches the tie-out-protected extraction path and requires a supervised `run_tieout` re-extraction (network/LLM). This PR makes the consumer ready; extraction stays untouched.

**Tech Stack:** Python 3.11, python-pptx, pytest. Branch `feat/ledger-followups`. Baseline 231 passed, 6 skipped.

---

## Task 1: PPTX Sources & Assumptions in speaker notes

**Files:** Modify `src/audit_pptx.py` (add a function); Test `tests/test_audit_pptx_sources.py`.

**Context:** `src/sources_report.py` has `build_sources_report(cache: dict) -> str`. `src/audit_pptx.py` already imports json/Path and uses `build_link_indexes`.

- [ ] **Step 1: Write `tests/test_audit_pptx_sources.py`**

```python
import json
from pptx import Presentation
from src.source_ledger import SourceLedger
from src.audit_pptx import annotate_pptx_with_sources


def _deck(tmp_path):
    prs = Presentation()
    prs.slides.add_slide(prs.slide_layouts[5])  # blank-ish layout with title
    p = tmp_path / "d.pptx"; prs.save(p); return p


def _cache(tmp_path):
    led = SourceLedger()
    led.record_assumption("assumptions", "terminal_growth_rate", None,
                          value=0.025, rationale="GDP proxy", basis="house default")
    p = tmp_path / "c.json"
    p.write_text(json.dumps({"__ledger__": led.to_json()}), encoding="utf-8")
    return p


def test_sources_added_to_notes(tmp_path):
    deck = _deck(tmp_path)
    res = annotate_pptx_with_sources(str(deck), cache_path=str(_cache(tmp_path)))
    assert res["notes_added"] == 1
    prs = Presentation(str(deck))
    notes = prs.slides[0].notes_slide.notes_text_frame.text
    assert "Sources & Assumptions" in notes
    assert "terminal_growth_rate" in notes


def test_no_ledger_no_notes(tmp_path):
    deck = _deck(tmp_path)
    cp = tmp_path / "empty.json"; cp.write_text("{}", encoding="utf-8")
    res = annotate_pptx_with_sources(str(deck), cache_path=str(cp))
    assert res["notes_added"] == 0


def test_none_cache_path_safe(tmp_path):
    deck = _deck(tmp_path)
    assert annotate_pptx_with_sources(str(deck), cache_path=None)["notes_added"] == 0
```

- [ ] **Step 2: Run `python -m pytest tests/test_audit_pptx_sources.py -q` — confirm FAIL (no attribute `annotate_pptx_with_sources`).**

- [ ] **Step 3: Add to `src/audit_pptx.py`:**

```python
def annotate_pptx_with_sources(pptx_path, *, cache_path=None) -> dict:
    """Append a Sources & Assumptions block to the deck's first-slide speaker
    notes when the cache has a ledger. Returns {"notes_added": int}. Never
    raises on a normal empty/missing input."""
    out = {"notes_added": 0}
    if cache_path is None:
        return out
    cache = json.loads(Path(cache_path).read_text(encoding="utf-8"))
    if not (cache.get("__ledger__", {}) or {}).get("entries"):
        return out
    from src.sources_report import build_sources_report
    from pptx import Presentation
    report = build_sources_report(cache)
    prs = Presentation(str(pptx_path))
    slides = list(prs.slides)
    if not slides:
        return out
    notes_tf = slides[0].notes_slide.notes_text_frame
    existing = notes_tf.text
    notes_tf.text = (existing + "\n\n" + report) if existing else report
    prs.save(str(pptx_path))
    out["notes_added"] = 1
    return out
```

(`json` and `Path` are already imported at the top of audit_pptx.py — confirm; if not, add them.)

- [ ] **Step 4: Run `python -m pytest tests/test_audit_pptx_sources.py -q` (expect 3 passed), then full suite `python -m pytest -q` (expect 234 passed, 6 skipped).**

- [ ] **Step 5: Commit**

```bash
git add src/audit_pptx.py tests/test_audit_pptx_sources.py
git commit -m "feat(audit): sources & assumptions in pptx speaker notes"
```

---

## Task 2: DCF consumes preferred_stock / short_term_investments

**Files:** Modify `src/dcf.py` (`flag_ev_bridge_gaps` ~line 19-28; the EV-bridge reads ~line 113-124); Test `tests/test_dcf_ledger.py` (append).

- [ ] **Step 1: Append to `tests/test_dcf_ledger.py`**

```python
def test_preferred_from_filing_tagged_filing():
    from src.source_ledger import SourceLedger, Tier
    from src.dcf import flag_ev_bridge_gaps
    led = SourceLedger()
    flag_ev_bridge_gaps(led, preferred=50.0, investments=30.0,
                        preferred_from_filing=True, investments_from_filing=True)
    assert led.get("dcf", "preferred_stock", None).tier is Tier.FILING
    assert led.get("dcf", "preferred_stock", None).value == 50.0
    assert led.get("dcf", "investments", None).tier is Tier.FILING


def test_absent_still_unverified():
    from src.source_ledger import SourceLedger, Tier
    from src.dcf import flag_ev_bridge_gaps
    led = SourceLedger()
    flag_ev_bridge_gaps(led, preferred=0.0, investments=0.0)   # defaults: not from filing
    assert led.get("dcf", "preferred_stock", None).tier is Tier.UNVERIFIED
```

- [ ] **Step 2: Run `python -m pytest tests/test_dcf_ledger.py -q` — confirm the new `test_preferred_from_filing_tagged_filing` FAILS (TypeError: unexpected kwarg `preferred_from_filing`).**

- [ ] **Step 3: Edit `src/dcf.py`.**

(a) Replace `flag_ev_bridge_gaps` with:

```python
def flag_ev_bridge_gaps(ledger, *, preferred: float, investments: float,
                        preferred_from_filing: bool = False,
                        investments_from_filing: bool = False) -> None:
    """Record EV-bridge items. When a value came from the balance sheet, tag it
    FILING; otherwise it is a schema gap assumed 0 -> UNVERIFIED (so the audit
    pass flags it red)."""
    if ledger is None:
        return
    if preferred_from_filing:
        ledger.record_filing("dcf", "preferred_stock", None, value=preferred,
                             provenance={"note": "balance sheet"})
    else:
        ledger.record_unverified("dcf", "preferred_stock", None, value=preferred,
                                 reason="preferred stock not in extraction schema (assumed 0)")
    if investments_from_filing:
        ledger.record_filing("dcf", "investments", None, value=investments,
                             provenance={"note": "balance sheet"})
    else:
        ledger.record_unverified("dcf", "investments", None, value=investments,
                                 reason="short-term investments not in extraction schema (assumed 0)")
```

(b) Replace the EV-bridge reads (currently `preferred = 0.0` ~line 118 and `investments = 0.0` ~line 120, with the `flag_ev_bridge_gaps(...)` call after). Read from the balance sheet:

```python
    pref_arr = output.balance_sheet.get("preferred_stock")
    preferred = (pref_arr or [0.0])[-1] or 0.0
    inv_arr = output.balance_sheet.get("short_term_investments")
    investments = (inv_arr or [0.0])[-1] or 0.0
    flag_ev_bridge_gaps(ledger, preferred=preferred, investments=investments,
                        preferred_from_filing=pref_arr is not None,
                        investments_from_filing=inv_arr is not None)
```

Leave `nci_balance` and the `net_debt = ...` line unchanged (they already use `preferred`/`investments`). Numeric behavior is identical when the BS lacks those keys (still 0.0); when present, the real value flows in — a strict correctness improvement.

- [ ] **Step 4: Run `python -m pytest tests/test_dcf_ledger.py -q` (expect 4 passed), then full suite `python -m pytest -q` (expect 236 passed, 6 skipped). Confirm existing dcf tests + the prior `test_dcf_ledger` cases still pass.**

- [ ] **Step 5: Commit**

```bash
git add src/dcf.py tests/test_dcf_ledger.py
git commit -m "feat(audit): DCF consumes preferred/investments from balance sheet when present"
```

---

## Task 3: Regression + PR

- [ ] **Step 1:** `python -m pytest -q` → 236 passed, 6 skipped, 0 failed.
- [ ] **Step 2:** `python -m pytest tests/test_tieout_no_regression.py tests/test_tieout_sector.py -q` → green (extraction path untouched).
- [ ] **Step 3:** Push + PR

```bash
git push -u origin feat/ledger-followups
gh pr create --title "Ledger follow-ups: PPTX sources notes + DCF preferred/investments" \
  --body "Two offline-verifiable ledger loose ends. (A) annotate_pptx_with_sources() appends the Sources & Assumptions appendix to deck speaker notes (reuses build_sources_report; gated behind __ledger__). (B) DCF now consumes preferred_stock/short_term_investments from the balance sheet when present (tagged FILING) instead of hardcoded 0/UNVERIFIED — strict correctness improvement, numeric behavior unchanged when absent. DEFERRED (needs supervised run_tieout): the extractor-prompt change to emit those fields — extraction path untouched here. 236 tests; tieout guard green."
```

---

## Self-review notes (author)
- Part A reuses `build_sources_report` (no new report logic); notes is the right surface (runs have no comment API; derived/assumption have no URL).
- Part B is backward-compatible: new `*_from_filing` flags default False → existing `test_dcf_ledger` cases stay UNVERIFIED; numeric output unchanged when BS lacks the keys.
- Extraction path (prompt) deliberately untouched → tie-out unaffected; the prompt change is a documented supervised follow-up.
