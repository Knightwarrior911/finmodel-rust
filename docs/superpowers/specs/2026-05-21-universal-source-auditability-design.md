# Universal Source Auditability — Design

**Date:** 2026-05-21
**Status:** Approved (shape), pending spec review
**Supersedes:** the `--audit` snapshot/PNG pass (`src/snapshot.py`, pre-rendered `snapshots/` pile)

## UPDATE 2026-05-22 — Excel drops `#page`; a one-time launcher is required

Empirically verified on Win11 + Edge: a plain `file:///doc.pdf#page=N` hyperlink does **not** jump to the page when clicked in Excel. Excel parses the link into Address (bare PDF) + SubAddress (`page=N`) and the Windows shell **discards the fragment** when launching the default PDF app, so the PDF always opens at page 1. Edge honours `#page=N` **only** when launched directly with the URL as an argument (`msedge.exe "file:///…#page=N"` → correct page, confirmed). `?query` strings die the same way through the shell.

Therefore the local "exact page" requirement cannot be met by a pure hyperlink. Resolution: a custom **`finmodelaudit:` protocol** handler (`src/audit_open.py`), registered once under HKCU (no admin, nothing running in background). Excel cells link to `finmodelaudit:page=N&path=<percent-encoded-abs-path>` — no `#`, so Excel keeps the whole string. Click → shell runs the handler → handler launches the browser directly at `file:///…#page=N`. The earlier "no install" non-goal is revised: a one-time per-machine registration is now required for page-accurate clicks. Adobe Reader as the default PDF app is an alternative for some users but the target environment is Edge.

---

## Problem

Today finmodel's audit trail is inconsistent and narrow:

- **Inconsistent rendering.** Located numbers get a pre-rendered PNG of the source page with a yellow box; un-located ("low_confidence") numbers fall back to a plain page-level PDF link. Two different click experiences for the same intent.
- **File-pile cost.** One PNG is rendered per located number up front (`snapshots/{TICKER}/{key}_{period}.png`). Hundreds of images per model.
- **Narrow coverage.** Only the 3-statement model (IS/BS/CFS) is wired. DCF, public comps, IFRS bridges, and the PPTX engine emit numbers with no audit trail. The IFRS bridge writer still uses old PDF-URL Col E links.
- **Annual-report-only.** Provenance discovery assumes annual-report PDFs keyed by year-in-filename. Press releases, quarterly/interim reports, and investor presentations aren't modeled.

## Goal

Every number finmodel writes, anywhere, is **auditable on demand**: clicking it opens its source document at the correct page. No pre-rendered image files. No background helper. No install. Works in plain Excel on the user's PC via a `file#page=N` hyperlink emitted at write time.

This applies to **all** finmodel outputs and **all** source document types.

## Non-Goals (this build)

- **No live yellow highlight locally.** Clicking jumps to the correct page; the user reads the number there. (The exact-spot coordinate is still recorded so a future web viewer can highlight — see "Highlight bonus, deferred".)
- **No web product, no server, no helper process, no protocol handler.** 100% local: Excel/PPTX → local PDF files.
- **No new source ingestion.** A number is auditable only if finmodel already ingests and retains its source doc. Broadening ingestion (e.g. fetching investor decks) is separate work; this design makes whatever is ingested linkable.

---

## Architecture

Three layers, each independently testable:

```
producers (writers)            engine                         consumer
─────────────────              ──────                         ────────
3-statement writer  ─┐
public comps        ─┤   ┌─ Citation model ──────┐
DCF inputs          ─┼──>┤  Source-doc registry  ├──> audit_link(citation) ──> file#page=N
IFRS bridge writer  ─┤   │  Page locator          │       (hyperlink string)
PPTX engine         ─┘   └─ Citation registry ───┘
```

### 1. `Citation` model (doc-type-agnostic)

Replaces `CellProvenance`. One record per sourced number.

| field | meaning |
|-------|---------|
| `source_id` | stable id of the source document (see registry) |
| `doc_type` | `annual_report` \| `interim_report` \| `quarterly_report` \| `press_release` \| `investor_presentation` \| `market_data` \| `computed` \| `other` |
| `page` | 1-based page number for paginated docs; `None` for non-paginated sources |
| `bbox` | `(x0,y0,x1,y1)` exact spot, optional — recorded when found, used only by the deferred web highlight |
| `value` | the numeric value as written |
| `label` | line-item / cell label (e.g. "Revenue") |
| `period` | period tag (e.g. "2023A"), optional |
| `confidence` | `located` (page found) \| `page_only` (doc known, page not pinned) \| `unlocated` |

Notes:
- `bbox` is best-effort and never required. Page is the contract.
- `market_data` and `computed` carry no page; they resolve differently (below).

### 2. Source-document registry

A per-run record of every source doc finmodel touched, so a `source_id` resolves to a real file/URL plus metadata.

| field | meaning |
|-------|---------|
| `source_id` | stable key |
| `path` | local file path (for paginated docs we can open) |
| `url` | optional canonical URL (provider page for market data; doc URL if hosted) |
| `doc_type` | as above |
| `period` | the period the doc reports |
| `title` | human label (e.g. "Atlas Copco Annual Report 2023") |

This generalizes today's "year-in-filename" discovery in `run_audit`. Discovery still bootstraps from files in `extraction_cache/` but the registry is the source of truth and is doc-type-aware.

### 3. Page locator

Generalizes the existing whole-number search in `src/provenance.py` (`locate_value_in_pdf`, `_iter_page_numbers`). Requirements change:

- **Page is the goal, bbox is a bonus.** When the exact number can't be pinned to a bbox, fall back to locating the **statement/section page** (anchor-based, reusing the extractor's section finder) → still `page_only`, still a useful link. This raises coverage versus the bbox-required approach.
- Per-period doc selection: each period's value is searched only in the doc that reports it (keeps today's correctness fix against wrong-year matches).
- Decimals/derived subtotals that never appear verbatim → `page_only` against their statement page, or `unlocated` if even the page can't be inferred.

### 4. `audit_link(citation)` — the one link-maker

Pure function. Given a citation + the source registry, returns the hyperlink string:

- paginated doc with `page` → `file:///abs/path/doc.pdf#page=N`
- `market_data` → the provider `url` (no page)
- `computed` → link to a designated primary-input citation's link, or no link
- `unlocated` with known doc → `file:///abs/path/doc.pdf` (page 1)

Absolute `file:///` paths so Windows openers (Edge default on Win11, Adobe) honor `#page`. (Adobe-only soft bonus: optionally append `&search=<value>` to trigger find-on-open; ignored by other viewers.)

### 5. Write-time emission (per the user's intent)

Producers attach the link **as they write the number**, not as a re-open post-pass.

- `writer.py` uses **xlsxwriter** → emit the hyperlink at cell-write time. (Implementation detail for the plan: xlsxwriter attaches URLs via `write_url`; keeping the cell numeric while carrying a link is an xlsxwriter mechanic to settle in the plan, e.g. write_url with a numeric display + number format, or a cell-comment carrying the link as fallback.)
- The openpyxl re-open post-pass (`annotate_workbook_with_snapshots`) is retired for the 3-statement path. A post-pass annotator may remain only as a bridge for already-built xlsx during migration, then be removed.

---

## Rollout (ordered)

1. **Engine** — `Citation`, source-doc registry, generalized page locator, `audit_link`. Unit-tested in isolation.
2. **3-statement writer** — emit links at write time. Proves the path end-to-end on ATCO-B.ST. Retire the PNG pass.
3. **Public comps** (`src/public_comps.py`, `peers.py`) — peer multiples are `market_data` → provider links.
4. **DCF** (`src/dcf.py`, `assumptions.py`, `wacc.py`) — filing-sourced inputs get filing citations; computed outputs link to a primary input or carry none.
5. **IFRS bridge writer** (`src/research/output_writer.py`) — replace old Col E PDF-URL links with `audit_link`.
6. **PPTX engine** — numbers in generated slides carry the same links.

Each step after (1) is small: register citations + call `audit_link`.

## Removed

- `src/snapshot.py` (PNG rendering) and the `snapshots/` output tree.
- The "located→PNG vs low_confidence→PDF" split in the annotator.
- `--audit`'s snapshot-generation step (the flag stays, now meaning "compute citations + emit links"; default behavior moves toward always-on since links are cheap).

## Honest limits

- Local links assume source docs stay where finmodel saved them (`extraction_cache/`). Moving the model to a PC without those files breaks links. (A future hosted/web mode would fix portability — out of scope.)
- `#page` needs a modern viewer (Edge / Adobe / browser). Basic viewers open page 1.
- A number is auditable only if its source doc was ingested + retained. New doc types (investor decks, press releases) become linkable automatically once ingested, but ingesting them is separate work.
- `computed` and `market_data` numbers have no highlighted page by nature — they link to an input source or provider, honestly labeled.

## Highlight bonus, deferred

`bbox` is still recorded whenever found. If/when finmodel gains a hosted PDF viewer, that viewer can draw the yellow box on demand using the stored bbox — no pre-rendered files, no helper. Recorded now so the data exists; not built now.

## Testing

- **Engine units:** Citation (de)serialization; registry resolution; page locator on `ATCO_B_2023_raw.pdf` (known: 2023 revenue 172,664 → page 2); `audit_link` output shapes for each `doc_type`; `page_only` fallback path; `unlocated` path.
- **Writer integration:** build ATCO-B.ST model, assert numeric cells carry `file#page` hyperlinks to the correct per-period doc; assert no `snapshots/` produced.
- **Regression:** full suite stays green (currently 151). Retiring snapshot tests is expected; replace with link-emission tests.

## Open questions (resolved)

- *Highlight locally?* No — page-jump only; bbox kept for future web highlight.
- *Helper/server?* No — fully local hyperlinks.
- *Scope?* All finmodel outputs, all source doc types, phased rollout above.
