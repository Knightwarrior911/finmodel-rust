# Workflow contract validation (Phase A pre-code gate)

Purpose: before implementing `WorkflowSpec` (Phase F), validate that the six
embedded workflow contracts express **at least five representative real
IB-VP tasks** using only the fixed `WorkflowSpec` field set — and designate the
two golden fixtures. Outcome: the field set is sufficient; **only spec content
is revised, never the core architecture** (per the plan's Phase A work item).

`WorkflowSpec` fields under test (from the plan's Finance workflow layer):
allowed/required tools · source policy · confidentiality · user-input schema ·
workflow-sheet controls · expected output parts/artifacts · per-workflow
round/token/deadline/child/concurrency budgets · plan template · claim/artifact
completion checks · approval policy · assumptions · disclaimer.

Tool names are the registered capabilities from the plan: `build_model`,
`benchmark_peers`, `get_news`, `get_quote`, `list_filings`, `read_filing`,
`analyze_pdf`, `web_search`, `read_page`, `research_deal`, `research`, plus the
artifact wrappers over `fm-build`/`fm-excel`/`fm-pptx`.

## Representative VP tasks → workflow mapping

| # | Real VP ask (verbatim-style) | Workflow |
|---|---|---|
| T1 | "One-pager on Company X for tomorrow's client call." | Company/sector brief |
| T2 | "NVDA just reported — beat/miss vs the prior period + guidance changes, with a cited variance table." | **Earnings review (GOLDEN)** |
| T3 | "Trading comps for the semis peer set, EV/EBITDA + P/E, as of today; Excel." | **Trading comps (GOLDEN)** |
| T4 | "Quick DCF on Company Y, base/bear/bull; give me the workbook." | DCF / 3-statement |
| T5 | "Screen precedent medtech deals > $1bn announced this year." | M&A / deal screen |
| T6 | "Prep the Project Falcon management-meeting deck." | Pitch / meeting prep |
| T7 | "Latest news + share reaction on Company Z after the downgrade." | Company brief (news-led variant) |

Seven tasks exercised (≥5 required); each is expressible with the field set below.

## Per-workflow contract validation

### 1. Company/sector brief  — covers T1, T7
- required tools: `research` (wraps `ResearchMachine`), `read_page`; allowed:
  `web_search`, `get_news`, `list_filings`, `read_filing`, `get_quote`.
- source policy: primary-source ledger required for every material figure;
  search snippets discover but cannot solely support a figure.
- confidentiality: inherits workspace tier.
- user-input schema: `{ entity (required), as_of?, focus_areas[]? }`.
- outputs: `report` part + optional report artifact; sources group.
- budgets: WORKFLOW policy (12 rounds / 30 min); ≤12 read-only children.
- completion checks: every claim has a citation; report has ≥1 primary source.
- approval: none (read-only + LocalCreate report).
- **Fit: OK.** T7 is the same spec with news as the lead required tool — a spec
  content variation, not a new field.

### 2. Earnings review  — covers T2  **[GOLDEN FIXTURE]**
- required tools: `list_filings` + `read_filing` (latest period), `get_news`
  (guidance), `research`/`web_search` (consensus context), `get_quote`.
- source policy: period-aware; issuer fiscal calendar; latest restatement wins;
  variance table is claim-verified (entity/value/unit/scale/currency/period).
- user-input schema: `{ ticker (required), period? (default latest), peer_prior? }`.
- outputs: `result` variance table part + `sources` group; optional Excel.
- budgets: WORKFLOW; verification REQUIRED (numeric-finance turn).
- completion checks: every variance cell traces to a filing/consensus source;
  fiscal period matches the issuer calendar; prior-period comparison present.
- approval: none unless an Excel export leaves the output root.
- **Fit: OK.** Chosen golden because it stresses period normalization + claim
  verification end-to-end — the earliest value-proving slice (Phase C).

### 3. Trading comps  — covers T3  **[GOLDEN FIXTURE]**
- required tools: `benchmark_peers`, `get_quote`, `list_filings`; Excel artifact
  wrapper on request.
- source policy: typed peer set; explicit as-of date; units/currency normalized;
  FX conversions require a cited rate/date.
- user-input schema: `{ tickers[] (required), multiples[]?, as_of?, to_usd? }`.
- outputs: one typed peer `result` table (entity/value/period/unit/locator per
  cell) + optional Excel artifact.
- budgets: WORKFLOW; up to 12 read-only children (10-peer set + headroom),
  ≤4 concurrent.
- completion checks: peer count matches input (no silent truncation); every
  metric cell verified; as-of date on the table.
- approval: none for a new immutable Excel version in the output root.
- **Fit: OK.** Chosen golden because it stresses the one-parent/many-children
  scheduler and per-cell claim verification (Phase F acceptance).

### 4. DCF / 3-statement  — covers T4
- required tools: `build_model` (+ current extraction/verification); artifact
  wrapper over `fm-build`/`fm-excel`.
- source policy: separated assumptions; immutable versioned workbook; no LLM
  arithmetic — the engine computes.
- user-input schema: `{ ticker (required), case? (base|bear|bull), overrides? }`.
- outputs: model `result` card + immutable workbook artifact (new version).
- budgets: WORKFLOW; verification REQUIRED.
- completion checks: balance identity holds; DCF/WACC verification passes;
  workbook published atomically.
- approval: new version auto-runs (LocalCreate); **in-place overwrite is a
  separate LocalOverwrite action requiring approval**.
- **Fit: OK.**

### 5. M&A / deal screen  — covers T5
- required tools: `research_deal`, `get_news`, `web_search`, `read_page`.
- source policy: each precedent carries announcement date + status; primary or
  reputable-press evidence.
- user-input schema: `{ sector/theme (required), min_size?, since?, status? }`.
- outputs: `result` precedent table + sources group.
- budgets: WORKFLOW; read-only children per candidate.
- completion checks: every row has announcement date + status + source.
- approval: none (read-only).
- **Fit: OK.**

### 6. Pitch / meeting prep  — covers T6
- required tools: company/deal research tools + `fm-pptx` deck wrapper.
- source policy: deck figures inherit claim verification from their source cards.
- user-input schema: `{ deal/company (required), sections[]?, audience? }`.
- outputs: report/deck artifact via **one write-capable parent executor**;
  children are read-only.
- budgets: WORKFLOW; one write-capable parent, ≤12 read-only children.
- completion checks: deck artifact published atomically; every figure cited.
- approval: new deck version auto-runs; overwrite/export approved.
- **Fit: OK.**

## Conclusion

All seven representative tasks map onto the six workflows using only the fixed
`WorkflowSpec` field set; no new architectural field is required. **Golden
fixtures: Earnings review (§2) and Trading comps (§3)** — they exercise,
respectively, period/claim normalization and the one-parent/many-children
scheduler with per-cell verification, and are the two slices validated earliest
(Phase C earnings slice; Phase F comps acceptance). Revisions during Phase F are
confined to spec content (tool lists, schemas, budgets, templates), per the plan.
