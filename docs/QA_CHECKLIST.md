# QA Checklist — Client-Ready Deliverable

> Human-readable companion to `scripts/qa_checklist.py`. Run the script to
> automate, or step through manually below.

## 1. Branding (fonts / colors match `config/branding.yaml`)

| Check | Detail |
|-------|--------|
| Cover title font | Should be Arial (or `font_display` from branding) |
| Cover title colours | White text on ink-color (`#0F1632`) background bar |
| Subtitle colour | Uses primary blue (`#255BE3`) |
| Body labels | Arial, ink-color (`#0F1632`) |
| Tab colours | Follow brand palette (Cover=`#255BE3`, IS/BS/CF=`#E6EBED`, Sources=`#D3DADD`, etc.) |
| Section headers | Sand background (`#EAE0D3`) with ink-color bold text |

**Acceptance:** All key surfaces carry the expected brand font + colour.

---

## 2. No UNVERIFIED RED cells left unexplained

The audit pass colours `UNVERIFIED` trust-tier cells in red (`#C00000`).
Every such cell **must** carry a cell comment explaining why it is unverified
(e.g. "⚠ Unverified: no source" or a specific reason from the SourceLedger).

**What to check:**
- Scan every sheet for red-font numeric cells
- Each red cell must have an attached comment containing the word "Unverified"
- Cells without a comment or with a silent red font are a FAIL

**Acceptance:** Zero unexplained red cells.

---

## 3. Required sheets present

All six core sheets must exist in the workbook:

- **Cover** — company name, valuation overview, key metrics
- **Assumptions** — toggle + scenario blocks + shared inputs
- **IS** — Income Statement
- **BS** — Balance Sheet
- **CF** — Cash Flow Statement
- **Sources** — provenance, audit trail, verification report

**Optional sheets** (DCF, WACC, Sensitivities, Comps Peers, Comps Summary)
may also be present and are not checked off.

**Acceptance:** `Cover` + `Assumptions` + `IS` + `BS` + `CF` + `Sources` all present.

---

## 4. Sanity checks / warning indicators

Look for blockers in the Verification Report and any warning indicators:

| What to scan | Example |
|--------------|---------|
| Sources sheet: `Status: FAILED` | Model verification did not pass |
| Sources sheet: `Critical Failures` | Hard formula / structural errors |
| Sources sheet: `Plug was used to balance BS` | Placeholder filled a gap |
| Sources sheet: `Warnings:` section | Non-blocking but noteworthy |
| Any sheet: `⚠` character in cells | Inline warning flags |

**Acceptance:** Status `PASSED`, no critical failures, no unaddressed warnings.

---

## Summary

All four sections must pass for a client-ready deliverable. The automated script
(`scripts/qa_checklist.py`) prints a `PASS`/`FAIL` per section and exits 0
(all pass) or 1 (any fail).
