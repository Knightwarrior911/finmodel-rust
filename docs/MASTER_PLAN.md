# FINMODEL MASTER PLAN

**Date:** 2026-07-03
**Owner:** Vinit (solo founder)
**Strategy spec:** `docs/superpowers/specs/2026-07-03-master-plan-design.md` (read that first — it holds the *why*; this document holds the *what and in which order*)
**Supersedes:** `docs/FINMODEL_PRODUCTION_PROMPT.md` Parts 5–6 (kept for reference only)

---

## How to read this document

You are a finance professional, not an engineer. This plan is written so that:

- **You** make the decisions marked 👤 (they need finance judgment or your time/money).
- **AI coding agents** execute everything marked as a task, one task per working session,
  each with its own acceptance test. You never need to write code (you will occasionally
  paste a command into a terminal when a gate asks you to — the gate text tells you
  exactly what to type and what a pass looks like).
- Every technical term is explained in plain words the first time it appears.
- Each phase ends with a **gate**: a plain-English check you can verify yourself.
  If the gate fails, the phase is not done, no matter what the agents report.

**Effort units:** one *session* = one focused AI-agent working session (roughly half a
day including review). Calendar estimates assume part-time attention and will stretch —
that is fine; the plan is sequenced by dependency, not by dates.

---

## Build-first amendment (👤 approved 2026-07-03)

The founder's directive: **build the product to fully-functional first; distribution and
selling are parked until then.** Effect on this plan:

- **Build track (active now):** Phase 0 → Phase 1 → Phase 2E (deliverable-polish
  engineering only) → Phase 3. "Functionally ready" = all four of those gates passed —
  ending with 2 of 3 strangers able to install, activate, and build a model unaided.
- **Parked (needs 👤 "go"):** Phase 2S (selling engagements), Phase 4 (seats, pricing,
  landing page, LinkedIn), Phase 5 (everything). The Dodo licensing work inside Phase 3
  still gets built — an activation system is product functionality — but no product goes
  on sale until the founder unparks selling.
- Rough build-track total: ~55–75 agent sessions (≈ 5–8 months part-time, faster
  full-time).

## Rust amendment (👤 approved 2026-07-03)

The founder's directive: **the shipped app is pure Tauri/Rust — no Python inside the
product.** Effect on this plan:

- A new **Phase R (Rust port)** is inserted after Phase 0: the Python pipeline is ported
  to a Rust engine, module by module, with one non-negotiable gate — **the Rust engine
  must reproduce the committed accuracy baseline exactly (cell-for-cell, 7 companies)** before it is
  called done. The existing Python code stays in the repo as the *reference
  implementation* and dev-side "answer key"; it never ships and the customer never sees
  it.
- Phase 1's sector waves (banks, insurers, held-out set) execute **on the Rust engine**
  after parity — so the expensive accuracy work is done once, in the language that ships.
- Phase 0 shrinks: the writer-monolith split (old Workstream 0.4) is **dropped** —
  pointless to refactor Python code that is being ported; the port itself produces the
  modular structure. Baseline verification, CI, and failure-honesty stay (they guard the
  reference implementation the port is measured against).
- Phase 3 simplifies substantially: no Python sidecar, no PyInstaller, no packaging
  spike — the riskiest items in the old Phase 3 disappear. One Rust binary, same
  architecture as the founder's four shipped apps.
- Assets reused from the founder's other repos: PDF text extraction (PDF Panda /
  pdfium), PowerPoint automation (Decko's native COM approach, Windows-first), licensing
  + updater (Snitch), `rust_xlsxwriter` for formula-driven Excel.
- Revised build-track total: ~75–105 agent sessions (≈ 7–10 months part-time). The port
  adds upfront work but removes the sidecar-packaging risk entirely and avoids
  maintaining two engines forever.

## The one-paragraph strategy

Finish the engine's accuracy story so it is provably right across sectors (that *is* the
product for a trust-driven buyer), while you use the tool in your own finance work and
sell 2–3 paid model-building engagements to boutiques. Then wrap the engine in a Windows
desktop app (Tauri, the toolkit behind your four shipped apps) that turns a ticker into a
client-ready model in minutes, license it through Dodo Payments at ~$99/seat/month, and
sell the first 10 seats through your finance network and trust-building content. Web
version, enterprise features, and extra data sources are all deferred until real
customers ask.

**Buyer:** boutique valuation shops, small M&A advisories, fractional CFOs, independent
analysts.
**Wedge vs Rogo (in selling order):** hours of modeling work compressed to minutes →
every number auditable to its source page → output is a real formula-driven Excel model,
not a chat answer → non-US (IFRS) filing coverage → and for the compliance-conscious:
data never leaves the analyst's laptop.
**Budget:** < $50/month pre-revenue (no servers; customers bring their own AI key).

---

## Current state (verified against the code, 2026-07-03)

What exists and works:

- A Python command-line tool (`python -m src.cli`) that turns a ticker or a filing PDF
  into a formula-driven 3-statement Excel model with DCF, comps, and an optional
  PowerPoint deck. 40+ orchestrator tools behind a natural-language `--ask` mode.
- 235 tests across 39 files, all local (no CI — meaning no server automatically runs the
  tests when code changes; today someone must remember to run them).
- The **tie-out harness** (`tieout/`): an independent accuracy instrument that compares
  extracted numbers cell-by-cell against ground truth. A frozen baseline is committed
  (`tieout/results/_baseline_wave0.json`): **339/350 cells (96.86%) across 7 industrial
  basket companies** (Wave 1 task 1.1.0: SAP.DE replaced by BASF after its integrated
  report defeated face-window detection; MC.PA pinned + added). Ground truth committed
  per company (immutable). A pytest guard
  (`tests/test_tieout_no_regression.py`) already fails if a new run regresses against
  that baseline. The two gaps: the guard is not wired into any CI, and the accuracy
  claim covers one sector in one region.
- **Dynamic IS Phases 1–4 are implemented** (commit `9174435`): revenue segments,
  dynamic OpEx rows from XBRL, bank/insurance/REIT income-statement templates in
  `is_builder.py`, and company-actual filing labels. (The old prompt doc listed these
  as "planned"; the code says otherwise. What was *not* built: the SaaS metrics
  template — deferred to Phase 5.)
- The **audit stack** (your real moat): source hyperlinks from Excel cells to filing
  pages, a 5-tier trust ledger on every number, structural sanity checks on every DCF
  (automated rules like "discount rate must exceed long-term growth" that block
  mathematically impossible outputs), and a provenance appendix.

What does not exist (despite what the old prompt doc implied):

- **No web app of any kind** (the "basic Streamlit app" mentioned in the old doc is not
  in the code).
- No installer, no packaging (`pip install` doesn't even work — there is no
  `pyproject.toml`, the file that tells Python how to install a project).
- No payments, licensing, CI, Docker, releases, or changelog.
- Insurers and REITs get a correct income-statement *layout* (via `is_builder.py`), but
  the **projection engine** (`engine.py`) has no insurance or REIT mode — forward-year
  assumptions fall back to industrial defaults, so projections for those sectors are
  structurally wrong. Banks have an engine mode but zero tie-out validation.
- No tests for bank/insurer extraction paths despite schemas existing on paper.

Known structural weak points:

- `src/writer.py` is a **3,615-line monolith** (one giant file doing all Excel output).
  Every future feature touches it; it is the most likely thing to break.
- `src/reconciler.py` (94 lines) has a deterministic `check_consistency()` covering
  assets = liabilities + equity and a D&A cross-check, but everything beyond that
  (footnote merging, multi-source confirmation) is delegated to the LLM (the AI model).
  More accounting identities should be enforced by plain code.
- The LLM is called directly (`anthropic` imports hardcoded in 5+ files, DeepSeek via
  if/else branches) — there is no single provider interface, which blocks the desktop
  app's "choose your provider" settings screen until fixed.

---

## Phase map and dependencies

```
Phase 0: Safety Net ──► Phase R: Rust Port ──► Phase 1: Accuracy Claim ──► Phase 3: Desktop v1 ──► Phase 4: Seats (PARKED) ──► Phase 5 (PARKED)
(guards the answer key)  (parity gate:          (waves run on the      ▲   (pure Rust/Tauri)
                          339/350 vs baseline)   Rust engine)          │
                                 │                    │                │ learnings feed in
                                 └────────────────────▼ (parallel)     │
                                          Phase 2: Dogfood (+ Services PARKED) ┘
```

- Phase 0 blocks everything (no safe changes without CI running the guards).
- Phase R blocks Phase 1: sector waves are built once, on the engine that ships.
- Phase 2 dogfooding starts as soon as the Rust engine passes parity + Wave 1; its
  learnings feed Phase 3's scope. (Selling half parked per build-first amendment.)
- Phase 3 depends on Phase 1 because the desktop app's whole pitch is the accuracy story.
- Phase 5 items are **forbidden** until Phase 4 produces customer evidence.

---

# PHASE 0 — SAFETY NET

**Objective:** make the codebase safe for AI agents to modify aggressively, and make the
accuracy claim regression-proof *automatically*.
**Why first:** every later phase edits core files. The baseline and its pytest guard
exist, but nothing runs them automatically — an agent could quietly break extraction
accuracy and nobody would know until a client sees a wrong number. For a product whose
entire pitch is "provably correct," that is fatal. Post-Rust-amendment, Phase 0's job is
narrower: lock down the *reference implementation* the port will be measured against.
**Total effort:** ~5–8 sessions (≈ 1.5–2.5 weeks part-time).

### Workstream 0.1 — Verify and harden the accuracy baseline (P0, 1 session)

*The baseline file and guard test already exist (see Current state). This workstream
confirms they still hold on today's code and documents the two skipped companies.*

| # | Task | Acceptance |
|---|---|---|
| 0.1.1 | Re-run the tie-out harness against current `master`; confirm the result still matches the committed `_baseline_wave0.json` (5 companies, 256/256). If drift is found, stop and diagnose before anything else in this plan proceeds. Document SAP.DE and MC.PA skip reasons in `tieout/README` and mark closing them as part of Wave 1 (Task 1.1.0). | A dated run log committed; baseline unchanged or drift explained and resolved. |
| 0.1.2 | Review `tests/test_tieout_no_regression.py`: confirm it fails loudly on (a) fewer exact matches, (b) fewer trusted cells, (c) a company disappearing from results. Extend if any of the three is uncovered. | Deliberately corrupting one extraction mapping in a scratch branch makes the guard fail. |

### Workstream 0.2 — Continuous Integration (P0, 1–2 sessions)

*CI = a free GitHub robot that runs the tests automatically on every change, so nothing
broken can merge unnoticed. A "pull request" (PR) is a proposed change awaiting checks.*

| # | Task | Acceptance |
|---|---|---|
| 0.2.1 | GitHub Actions workflow: run full `pytest` suite (including the tie-out no-regression guard, which reads committed files and needs no API keys) on every pull request and push to `master`. Use `FINMODEL_DEV_MOCK=1` for tests that would otherwise call an LLM. | A PR with a deliberately failing test shows a red ✗ on GitHub; reverting shows green ✓. |
| 0.2.2 | Add `ruff` linting (an automatic code-style checker) as a second CI job; fix or explicitly ignore existing warnings. | CI green on `master`. |
| 0.2.3 | Document the manual pre-release ritual in `docs/RELEASE_CHECKLIST.md`: full live tie-out run (with API keys), invariant spot-check, version bump steps. | Checklist exists and is referenced from README. |

### Workstream 0.3 — Make it installable (P0, 1 session)

| # | Task | Acceptance |
|---|---|---|
| 0.3.1 | Add `pyproject.toml` (name, version 0.1.0, dependencies from `requirements.txt`, console entry point `finmodel`). | `pip install -e .` works in a fresh virtual environment; `finmodel --help` runs. |
| 0.3.2 | Start `CHANGELOG.md`; tag current state `v0.1.0`. | Tag visible on GitHub. |

### Workstream 0.4 — ~~Split the writer monolith~~ (DROPPED — Rust amendment)

*Refactoring Python code that Phase R replaces is wasted work. The characterization
snapshot idea survives, repurposed: it becomes the Excel-output parity fixture for the
Rust port (see R.5).*

### Workstream 0.5 — Characterization snapshots as port fixtures (P0, 1–2 sessions)

| # | Task | Acceptance |
|---|---|---|
| 0.5.1 | Generate models for the 5 baseline companies + 1 US ticker from cached data; record every sheet's cell values + formulas + tier colors + hyperlinks to snapshot files (the "answer key" the Rust writer must reproduce). | Snapshots committed under `tests/snapshots/`; a compare script exists that diffs any new .xlsx against them cell-by-cell. |

### Workstream 0.6 — Extraction failure honesty (P1, 1–2 sessions)

*Built in the Python reference first because Phase 2 dogfooding may run on it while the
port is in flight; the behavior spec carries into the Rust extractor (R.2).*

| # | Task | Acceptance |
|---|---|---|
| 0.6.1 | Distinguish three outcomes end-to-end: extraction **succeeded**, **succeeded with low confidence** (< 0.75 → currently flagged but still used), and **failed** (garbage/empty LLM output → currently can slip through). Failed runs must stop the pipeline with a clear message, not produce a model. | Tests simulate each outcome; a "failed" extraction never reaches the Excel writer. |

**PHASE 0 GATE (you can check this yourself):** open the GitHub repo page — the Actions
tab shows green check-marks on recent commits; the snapshot "answer key" files exist in
the repo; the baseline verification log says PASS.

**Revised Phase 0 total: ~5–8 sessions (≈ 1.5–2.5 weeks part-time).**

---

# PHASE R — RUST PORT (parity-gated)

**Objective:** a Rust engine (`finmodel-core`) that reproduces the Python reference
implementation's outputs **exactly** — proven by the same tie-out baseline and Excel
snapshots — so everything after this phase is built once, in the language that ships.
**Why Rust (founder decision):** single fast binary, no bundled Python, same stack as
the four shipped apps (PDF Panda, Snitch, Decko, Pluck). Honest speed note: the slowest
step in a model build is the LLM call + filing download (network), which Rust does not
speed up — what Rust buys is instant app startup, a snappy UI, a small installer, and
the death of the Python-packaging risk. CPU-side steps (PDF parsing, Excel writing) do
get genuinely faster.
**Depends on:** Phase 0 (verified baseline + snapshots = the port's acceptance tests).
**Total effort:** ~30–45 sessions (≈ 2.5–4 months part-time). The big rock. Sequenced so
each module proves parity before the next starts.

### Architecture

- Cargo workspace `finmodel-core` inside this repo (or sibling dir 👤): crates mirroring
  the Python modules — `fm-fetch` (EDGAR XBRL + filing download), `fm-extract` (PDF text
  via pdfium, LLM extraction), `fm-recon` (deterministic identities + LLM second
  opinion), `fm-engine` (projections), `fm-ledger` (5-tier trust + derivation cascade),
  `fm-value` (DCF/WACC/comps + sanity invariants), `fm-excel` (rust_xlsxwriter,
  formula-driven, tier colors, source hyperlinks), `fm-cli` (thin CLI for testing).
- **LLM provider abstraction from day one** (one trait, DeepSeek + Anthropic + local
  implementations) — absorbs old Phase 3 task 3.1.
- **Prompts are ported verbatim.** The extraction prompts and the 350+ XBRL tag map are
  the crown jewels; they copy across unchanged. The port re-implements plumbing, not
  judgment.
- **PPTX decks:** not re-implemented in Rust in this phase. Deck generation reuses the
  Decko approach (native COM automation of PowerPoint, Windows) as a later Phase 3
  workstream; the Python pptx code stays reference-only.
- **Orchestrator (`--ask`, 40 tools):** NOT ported in v1 — the desktop app's four screens
  drive the pipeline directly. The NL orchestrator moves to the Phase 5 menu.
- **Python's ongoing role:** reference implementation + tie-out instrument, dev-only.
  The tie-out harness gets an adapter to score any engine that emits the standard
  extraction JSON (R.1), so Python and Rust are graded by the same examiner.

### Workstreams (each gate = parity, not "looks right")

| # | Workstream | Effort | Parity gate |
|---|---|---|---|
| R.1 | Tie-out adapter: harness scores an external engine via JSON output; wire Python reference through it first (proves the adapter itself). | 2–3 | Python-via-adapter reproduces the committed baseline cell-for-cell (339/350, 7 companies). |
| R.2 | `fm-fetch` + `fm-extract`: EDGAR XBRL pull, PDF text (pdfium), LLM extraction with ported prompts, failure-honesty semantics from 0.6. | 8–12 | Rust extraction JSON matches Python's on all baseline companies + cached US tickers, cell-for-cell. |
| R.3 | `fm-recon` + `fm-engine` + `fm-ledger`: identities, projections, derive-first cascade, assumption registry. | 6–9 | Projected statements + ledger tiers match Python reference outputs on fixtures. |
| R.4 | `fm-value`: DCF, WACC, comps, EV bridge, all 11 sanity invariants. | 5–7 | Valuation outputs match Python to the cent on fixtures; invariants fire on the same seeded errors. |
| R.5 | `fm-excel`: formula-driven workbook via rust_xlsxwriter — formulas, tier colors, source hyperlinks, sources appendix. | 7–10 | Cell-by-cell diff vs the Phase 0.5 snapshots: values + formulas + links identical (formatting may differ only where 👤 approves). |
| R.6 | Full-pipeline parity run + CI: `fm-cli` builds every baseline company end-to-end; tie-out (via R.1 adapter) + snapshot diff run in CI for the Rust engine. | 2–4 | **THE GATE: Rust engine reproduces the committed baseline cell-for-cell (339/350 across 7 companies) and passes all snapshot diffs, in CI.** |

**PHASE R GATE (you can check this yourself):** the agent hands you two Excel files for
the same company — one from the old Python pipeline, one from the Rust engine. You open
both; every number, formula, color, and click-to-filing link behaves identically. CI
shows the Rust parity job green.

**Risks:** (a) rust_xlsxwriter feature gaps vs Python xlsxwriter (both by the same
author; formulas are supported — but any gap found becomes a 👤 formatting-compromise
decision, never a silent change). (b) PDF text extraction differences (pdfplumber vs
pdfium) can shift extraction inputs — R.2's cell-for-cell gate catches this early.
(c) Scope temptation: porting the orchestrator or pptx tools "while we're at it" is
forbidden — parity scope is frozen to what the baseline measures.

---

# PHASE 1 — MAKE THE ACCURACY CLAIM REAL (on the Rust engine)

**Objective:** turn "100% on 5 European industrials" into a **published, reproducible
accuracy table across ≥15 companies in ≥3 sectors, including companies the system has
never seen**. This is the product. Boutiques buy trust; this phase manufactures it.
**Rust amendment:** all wave work (extraction fixes, engine modes, writer layouts)
happens in the Rust engine; the Python reference is consulted, not extended. Ground
truth building (`groundtruth.py`) stays Python — it is instrument, not product.
**Depends on:** Phase R parity gate.
**Total effort:** ~21–30 sessions (≈ 2–3.5 months part-time). Second-largest phase —
deliberately, because you chose quality-first.

### Workstream 1.1 — Tie-out Wave 1: fix the skips + industrial diversity (P0, 4–6 sessions)

| # | Task | Acceptance |
|---|---|---|
| 1.1.0 | Close the two existing basket gaps: rebuild SAP.DE ground truth (came back empty) and pin the MC.PA PDF (discovery failed). This makes the original 7-company claim honest before expanding it. | Both companies measured in the tie-out report (or a documented decision to replace them in the basket). |
| 1.1.1 | 👤 + agent: pick 5 diverse additions — suggest 2 US large-caps (via EDGAR, the SEC's free filings database — a different code path than the European PDF pipeline), 1 UK (GBP), 1 Japan or India (different number/scale conventions — this is deliberately the hardest test), 1 mid-cap with a messy PDF. You approve the list (finance judgment: pick names you know cold, ideally ones relevant to your own work). | Basket list updated in `tieout/config.py`. |
| 1.1.2 | Build dual-pass ground truth for each (existing `groundtruth.py` flow — "dual-pass" = the true numbers are extracted twice independently and must agree before they count as truth). | Ground truth files committed, marked immutable. |
| 1.1.3 | Run the extraction improvement loop until the wave passes; every fix gets a regression test; the committed baseline updates **only** via explicit, human-approved commits. | Wave 1 accuracy = 100% face-statement tie-out, or documented, understood exceptions. |

### Workstream 1.2 — Tie-out Wave 2: banks (P0, 6–8 sessions)

*Banks report fundamentally differently (net interest margin instead of gross margin;
4-line cash flow). An engine bank mode and extraction prompts exist but have **zero**
validated companies.*

| # | Task | Acceptance |
|---|---|---|
| 1.2.1 | 👤 pick 2 banks (suggest 1 US, e.g. a large regional; 1 European/Asian). | Basket updated. |
| 1.2.2 | Ground truth; extend bank cash-flow canonical keys where the current 4-key skeleton proves too thin. | Ground truth committed. |
| 1.2.3 | Fix extraction prompts + `engine.py` bank mode + bank writer layout until tie-out passes; per-sector accuracy column added to the report. | Bank wave passes; industrial baseline untouched. |

### Workstream 1.3 — Tie-out Wave 3: insurers + held-out set (P0, 6–8 sessions)

| # | Task | Acceptance |
|---|---|---|
| 1.3.1 | 👤 pick 2 insurers; ground truth. | Committed. |
| 1.3.2 | **Build the engine's insurance projection mode.** The income-statement *layout* already exists (`is_builder.py`); what's missing is projection logic — how premiums, claims, and reserves roll forward. Today insurers project on industrial defaults, which is wrong. Same gap check for REITs (decide 👤: include REIT mode now or defer to Phase 5 — it is not needed for the boutique wedge unless your clients touch real estate). | Insurer wave passes tie-out; `tests/test_engine_insurance.py` green. |
| 1.3.3 | **Cold held-out test:** 3–4 companies across sectors, never used during any improvement loop, run exactly once. This is the honest generalization number you will publish. Agents are forbidden from opening these filings during improvement loops. | One-shot result recorded in `docs/ACCURACY.md`, warts and all. |

### Workstream 1.4 — Re-validate Dynamic IS across the new basket (P1, 1–2 sessions)

*Dynamic IS Phases 2–4 are already implemented (commit `9174435`) — this workstream
exists because "implemented" ≠ "validated on banks/insurers at tie-out standard."*

| # | Task | Acceptance |
|---|---|---|
| 1.4.1 | Run the dynamic-IS paths (OpEx detection, sector templates, filing labels) across every Wave 1–3 company; fix what breaks; add regression tests for each fix. | Sector-correct IS structure confirmed on all basket companies; no hardcoded-label fallbacks on companies that disclose labels. |

### Workstream 1.5 — Close the audit gaps (P1, 3–4 sessions)

| # | Task | Acceptance |
|---|---|---|
| 1.5.1 | Source hyperlinks + tier colors on the Comps and DCF sheets (the `audit_link.py` / `audit_pipeline.py` infrastructure exists but the DCF/Comps writer sections never call it). Note: if an IFRS-bridge sheet turns out not to exist as its own tab, creating it is a separate 👤 scope decision, not silently added. | Click-through works on a sample model for every number-bearing sheet. |
| 1.5.2 | PPTX decks: source notes on every number-bearing slide (extend existing `audit_pptx.py`). | Speaker notes carry provenance on generated decks. |
| 1.5.3 | Wire fetcher interest heuristics (`debt × 3.5%`, `cash × 2%`) through the derivation cascade so they surface as DERIVED-with-basis instead of naked defaults. | Ledger shows the basis; red catch-all count drops on test companies. |
| 1.5.4 | Add `preferred_stock` + `short_term_investments` to extraction (currently silently 0 in the EV bridge); supervised tie-out re-run. | Fields extracted on companies that report them; EV bridge test updated. |

### Workstream 1.6 — Publish the accuracy story (P1, 1–2 sessions)

| # | Task | Acceptance |
|---|---|---|
| 1.6.1 | `docs/ACCURACY.md`: methodology (dual-pass ground truth, exact-integer comparison), full per-company per-sector table, held-out result, **and an explicit "currently validated" scope statement** (e.g. "European IFRS + US EDGAR filings; Japanese/Indian filings: 1 company validated" — never claim broader non-US coverage than the table shows). | 👤 you read it and would show it to a client without embarrassment. |

**PHASE 1 GATE:** `docs/ACCURACY.md` shows ≥15 companies, ≥3 sectors, includes a one-shot
held-out set, and the CI guard from Phase 0 now protects all of it (meaning: the pytest
tie-out guard reads the expanded baseline, so any regression turns GitHub's checks red).

**Risks:** (a) Banks/insurers may expose deep extraction assumptions — budget says 6–8
sessions each, could double; that is acceptable at quality-first pace. (b) Ground-truth
cost: dual-pass runs go through the `claude` CLI fallback (subscription, no per-token
API bill) — keep API usage for spot checks. (c) The Japan/India pick in Wave 1 is the
most likely to fail — treat failure as information (it bounds your marketing claims),
not as a blocker.

---

# PHASE 2 — DOGFOOD + SERVICES BRIDGE (parallel track)

**Build-first amendment:** this phase is split. **2E (engineering, Workstream 2.1) stays
in the build track** — client-ready output quality is product functionality regardless of
who sells anything. **2S (selling, Workstream 2.2) is PARKED** until 👤 unparks it; the
dogfooding half of 2.2 (you using the tool on your own work and logging defects) is
encouraged any time — it is the cheapest test lab available.

**Objective:** real deliverable quality and real defect data, using the asset you
already have: **you work in finance**.
**Starts:** as soon as Wave 1 (1.1) passes.
**Effort:** ~2–3 engineering sessions + 👤 your ongoing professional time.

### Workstream 2.1 (2E) — Engagement-grade output (P0, 2–3 sessions)

| # | Task | Acceptance |
|---|---|---|
| 2.1.1 | Deliverable QA checklist: an agent-runnable + human checklist for "client-ready" (branding applied from `config/branding.yaml`, no UNVERIFIED reds unexplained, all sanity checks passing, deck footnotes present). | Checklist doc + script in repo. |
| 2.1.2 | One-command engagement flow: ticker/PDF in → branded Excel + deck + sources appendix in a dated client folder. | Single command produces the full folder on a test name. |
| 2.1.3 | Disclaimer baseline: "not investment advice / no warranty" text embedded in Excel cover sheet and deck footer; agent drafts a short ToS from a reputable template. ⚠️ Before the first paid engagement, have the ToS/disclaimer reviewed by a lawyer or a reputable legal-template service — liability for financial outputs is a real business risk, and this plan does not constitute legal advice. | Disclaimers render in outputs; reviewed ToS exists. |

### Workstream 2.2 (2S) — 👤 Sell 2–3 engagements (PARKED until 👤 unparks selling)

- Offer: "institutional-grade 3-statement model + DCF + deck for [company], every number
  source-linked" at a fixed fee you consider fair for your market.
- Targets: your warm finance contacts; boutiques you already know.
- **Capture everything:** every manual fix you make to a deliverable is a defect ticket;
  every client question is feature evidence. Agents turn the log into
  `docs/ENGAGEMENT_LEARNINGS.md` after each engagement.
- **Product willingness-to-pay probe (important):** an engagement client paying for
  *your time* is not yet evidence anyone will pay for *the tool*. So, after each
  engagement, make the standing offer: "the software that built this — 50% off for 6
  months when the desktop version ships." Record accept / decline / counter in the
  learnings doc. This is the earliest real signal for the Phase 4 price hypothesis.

**PHASE 2 GATE:** ≥2 paid engagements delivered; learnings doc exists; the desktop
pre-order offer was made to every engagement client and the responses are recorded.

**Risks:** services can eat all your time and stall the product — cap at 3 engagements
before Phase 3 ships, then let the product take over. Client confidentiality: engagement
files never enter the repo (agents enforce via `.gitignore` rules — already the pattern
for copyrighted filings).

---

# PHASE 3 — FINMODEL DESKTOP v1

**Objective:** a Windows installer a boutique analyst can download, activate, and use
without you. **Pure Tauri 2 / Rust** — the app links the `finmodel-core` crates from
Phase R directly. No Python, no sidecar, no bundled interpreter. Same architecture as
your four shipped apps (installers, signing, auto-update, and Dodo licensing patterns
all exist in your other repos).
**Depends on:** Phase 1 gate (the accuracy story is the sales pitch), Phase 2 learnings
(what analysts actually touch).
**Effort:** ~16–23 sessions (≈ 1.5–2.5 months part-time — the Rust amendment removed
the sidecar, packaging spike, and provider-abstraction work from this phase).

### Architecture

- **Engine:** `finmodel-core` crates compiled into the app. Long pipeline runs execute
  on background threads (tokio) with progress events streamed to the UI — the standard
  Tauri pattern from PDF Panda.
- **Scope boundary:** v1 ships the US/EDGAR path + local-PDF path (drag-and-drop a
  filing — what an analyst with a PDF in hand does anyway). Non-US filing
  *auto-discovery* (the Python browser pipeline) is not ported; it lives on as a
  dev-side research tool and a Phase 5 menu item.
- **Deck generation:** Decko-pattern native PowerPoint COM automation (Windows), reusing
  that repo's approach; if PowerPoint isn't installed, the app produces Excel only and
  says so plainly.
- **Key storage:** Windows Credential Manager via keyring — same pattern as Snitch.
- **Data location:** everything under the user's `Documents/Finmodel/` — models, caches,
  filings. Nothing leaves the machine except calls to the LLM provider *the user
  configured with their own key*, and license checks to Dodo.
- **Four screens only (v1):**
  1. **New Model** — ticker or PDF drop, sector auto-detected + overridable, key
     assumptions form (pre-filled from the derive-first cascade, each field showing its
     trust tier), Build button.
  2. **Progress** — live pipeline steps with plain-language status and cost meter.
  3. **Library** — past runs, open-in-Excel / open-deck buttons, re-run with new
     assumptions, and **export/import of a model bundle** (a single `.finmodel` file
     containing the model + assumptions + source links, so one analyst can hand a model
     to a colleague for review — the boutique "partner reviews the model" workflow,
     without any server).
  4. **Settings** — LLM provider + API key (with a "test key" button and a guided
     "get a DeepSeek key in 5 minutes" walkthrough), license status, updates.

### Workstreams

| # | Workstream | Effort | Key acceptance |
|---|---|---|---|
| 3.1 | **Tauri shell + 4 screens** (P0): app skeleton, engine wired on background threads with streamed progress, all four screens, `.finmodel` export/import bundle. | 8–10 sessions | All four screens + bundle round-trip work end-to-end on a dev machine (real ticker → real Excel). |
| 3.2 | **Deck generation via COM** (P1): port the Decko-pattern PowerPoint automation for the summary deck; graceful Excel-only fallback when PowerPoint absent. | 2–3 sessions | Deck generated on a machine with Office; clean fallback message without. |
| 3.3 | **Licensing + trial via Dodo** (P0): 14-day trial, per-seat subscription, offline-tolerant license cache (7-day grace), reuse Snitch's Dodo patterns and India-MoR setup. 👤 you create the product entries in the Dodo dashboard (agent gives you a click-by-click guide). | 3–4 sessions | Buy → activate → revoke → grace-period flows all tested in Dodo test mode. |
| 3.4 | **Installer, signing, auto-update** (P0): NSIS installer + updater (Snitch/PDF Panda pattern), signed; releases via GitHub. Smoke test on a clean Windows VM (a fresh virtual machine — the agent sets it up and records the run for you to watch). | 2–3 sessions | Fresh VM: install → build a model → auto-update v0.9→v1.0 works. |
| 3.5 | **First-run experience** (P1): guided setup (key, first model on a suggested ticker), sample model bundled so the app demos even before a key exists. | 1–2 sessions | 👤 you watch one finance friend complete first-run without you speaking. |
| 3.6 | **Crash/error reporting, local logs** (P1): errors readable by non-engineers, "copy diagnostic" button that packages recent logs + app state (no client documents) into a single file the user can email you (no auto-telemetry in v1 — privacy is the pitch). | 1 session | Simulated engine crash produces a friendly message + diagnostic file. |

**PHASE 3 GATE (the only gate that matters):** someone who is not you — ideally a
Phase 2 client or finance friend — downloads the installer, activates a trial, adds
their own key, builds a model of a company they choose, and opens the Excel. **Zero
founder assistance.** Repeat with 3 people; 2 of 3 must succeed unaided.

**Risks:** (a) Antivirus false positives on new binaries — mitigated by code signing
(pattern known from Snitch). (b) Scope creep on screens — the four-screen list is a hard
wall; anything else goes to the Phase 5 menu. (c) COM/PowerPoint fragility — contained
by the Excel-only fallback; the deck is P1, not gating.

---

# PHASE 4 — FIRST 10 BOUTIQUE SEATS (PARKED — build-first amendment)

*Entire phase waits for 👤 "go" after the product is functionally ready. Kept in full so
nothing has to be re-planned when selling unparks.*

**Objective:** 10 paying seats (or 3 boutique team deals) **with real retention**.
Everything here is evidence-gathering for Phase 5.
**Depends on:** Phase 3 gate.
**Effort:** ~4–5 engineering sessions + 👤 sustained founder time (this phase is mostly *you*).

### Workstream 4.1 — Offer + pricing test (👤 + 1 session)

- Launch price hypothesis: **$99/seat/month, or $948/year (=$79/mo)**, 14-day trial
  (extendable to 30 by email — generous extensions beat lost trials), no free tier
  (boutiques don't need free; free attracts the wrong crowd and support load).
- Design-partner deal for the first 3 firms: 50% off for 6 months in exchange for a
  monthly feedback call and permission to fix-on-their-workflow. Engagement clients who
  accepted the Phase 2 pre-order offer convert first.
- 👤 decision: final numbers. Agents wire whatever you pick into Dodo.

### Workstream 4.2 — Trust collateral (2–3 sessions + 👤 review)

| Asset | Notes |
|---|---|
| Landing page (one page, static, GitHub Pages — agent deploys a private preview link for 👤 sign-off before it goes public) | Pitch in selling order: hours → minutes (time saved) → every number defends itself (screenshot of click-to-filing) → real Excel output → accuracy table from `docs/ACCURACY.md` → "your data never leaves your laptop" → pricing. |
| Demo video (5 min) | Ticker → finished model → click a number → the filing opens at the page. That moment is the whole sale. |
| `docs/ACCURACY.md` public version | The methodology post doubles as the "why trust us" page. Scope claims stay exactly as validated (see 1.6.1). |
| One written case study from Phase 2 | Anonymized client engagement. |

### Workstream 4.3 — 👤 Distribution (your time, 90-day commitment)

Ranked for a finance-credible founder with zero audience:

1. **Warm network first** — every Phase 2 client and finance contact gets a personal
   demo. Target: 5 of the 10 seats from here.
2. **LinkedIn** — you post as a finance professional showing the time-saved and
   auditability moments (screen recordings, accuracy table). 2 posts/week for 90 days.
   Support channel, not a miracle: its job is credibility when warm intros check you
   out, plus slow inbound.
3. **Finance communities** — targeted, not spray: valuation/modeling groups, CFA
   communities, r/SecurityAnalysis-type spaces where boutique analysts actually are.
4. Product Hunt / Hacker News — optional, later; they reach developers, not boutiques.

### Workstream 4.4 — Feedback engine (1 session)

- In-app "request/report" button → GitHub issues (private repo) with the diagnostic file
  attached (logs + app state only, never client documents). Weekly triage ritual: every
  request tagged to a Phase 5 candidate. Support via email only (founder inbox) — no
  ticket systems at this scale.

**PHASE 4 GATE:** 10 paid seats or 3 team deals, of which **at least 3 seats came from
outside your warm network** (inbound, community, or referral-of-referral), **at least
half have been active past day 60**, and a written ranked list of what customers asked
for exists (this list *is* Phase 5's input). Network-only sales prove your friends like
you; the outside-seats qualifier proves a market exists.

**Risks:** (a) Nobody buys → the engagement clients from Phase 2 are your diagnosis
panel; pivot options preserved (services-heavy, individual-analyst pricing, or open-core
distribution) — decide with data, not upfront. (b) LLM key friction (boutiques may balk
at creating API-provider accounts) → the guided key walkthrough (3.7) is the first
defense; if ≥3 prospects still stall on it, revisit bundled credits (a small hosted
proxy with prepaid credits) as a fast-follow — it is the one cut item most likely to
return early.

---

# PHASE 5 — DEEPEN THE WEDGE (evidence-gated menu)

**Rule:** nothing in this phase starts until Phase 4's ranked customer-request list
exists. Each item below ships only if ≥3 paying customers asked for it (or one team deal
is conditional on it). Pre-planning is limited to the one-line notes here — deliberately,
because building any of these on speculation is how the old prompt doc went wrong.

| Candidate | Trigger to build | Rough effort |
|---|---|---|
| Workflow prompt library (top 5 recurring asks as one-click flows: company profile, earnings comp, meeting prep, precedent transactions, SOTP — sum-of-the-parts valuation) | Recurring `--ask` patterns in feedback | 3–5 sessions each |
| Basket / peer-group analysis (auto-peers via SIC code, side-by-side comps, percentile ranks — `peers.py` foundation exists) | Boutiques comparing 5+ names weekly | 6–10 sessions |
| Merger model / LBO modules (`kb/ma.py`, `kb/lbo.py` already hold the domain logic scaffolding) | A team deal asks | 10–15 sessions each |
| REIT projection mode (layout exists; projection deferred from 1.3.2 if 👤 chose to) | Real-estate clients appear | 4–6 sessions |
| SaaS-metrics IS template (ARR/NRR — the one Dynamic IS piece never built) | Software-sector clients ask | 3–4 sessions |
| macOS build (CI-based, PDF Panda roadmap pattern) | ≥3 Mac prospects lost | 4–6 sessions |
| Bundled LLM credits (hosted proxy + prepaid metering) | Key friction kills ≥3 sales | 5–8 sessions |
| Non-US filing auto-discovery in desktop (the excluded browser pipeline, rethought) | Desktop users repeatedly ask for it vs drag-and-drop | Re-scope then |
| Open-core release of the engine (trust + distribution play) | Organic inbound asks to inspect the engine; or growth stalls and distribution is the constraint | 2–3 sessions + ongoing community cost |
| Hosted API / web viewer | A customer offers to pay for it | Re-scope then |

---

## Cross-cutting rules (apply to every phase)

1. **The execution loop for every task:** agent writes test → implements → runs pytest
   (which includes the tie-out no-regression guard) → snapshot checks where relevant →
   code review pass → merge. No task merges red. (This is the `docs/superpowers/`
   convention already used by every shipped feature in this repo.)
2. **The baseline is sacred:** `_baseline_wave0.json` and its successors change only in
   explicit, human-approved commits with a stated reason.
3. **Never break the CLI:** the desktop app is a client of the same engine; CLI flags
   and outputs stay stable (existing users = you + future power users).
4. **No silent defaults:** every valuation input flows derive → registry → UNVERIFIED.
5. **Client data hygiene:** engagement and customer files never enter the repo.
6. **Spend ceiling:** if any month's tooling+API spend approaches $50, stop and re-plan
   rather than drift.

## Budget map (pre-revenue)

| Item | Cost |
|---|---|
| GitHub (repo, Actions CI, releases, Pages) | $0 (free tier) |
| LLM for development (DeepSeek primary; ground-truth passes via `claude` CLI subscription fallback) | ~$5–25/mo depending on phase |
| Dodo Payments | $0 fixed; % per sale |
| Code-signing cert | already owned (Snitch pipeline) — else ~$100/yr, one-time decision 👤 |
| Domain | ~$12/yr 👤 |
| Servers, databases, paid data feeds | **$0 — none exist in this plan** |

## Risk register (top 6)

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| Accuracy regression slips into a client deliverable | Med | Fatal to the trust pitch | Phase 0 CI guard + Phase 2 QA checklist; held-out honesty in ACCURACY.md |
| Bank/insurer extraction much harder than budgeted | Med-High | Schedule stretch | Quality-first pace absorbs it; ship desktop with "industrials + banks; insurers beta" if needed 👤 |
| Rust port drifts from Python reference (subtle number changes) | Med | Fatal to trust pitch | Parity gates per module (R.2–R.6): same tie-out examiner, cell-for-cell Excel diffs; no module "done" without parity |
| rust_xlsxwriter / pdfium feature gaps vs Python libs | Med | Formatting compromises or extra sessions | Gaps surface in R.2/R.5 gates; every compromise is a 👤 decision, never silent |
| Nobody pays (wrong wedge) | Med | Business risk | Phase 2 pre-order probe tests product WTP *before* Phase 3's build cost; outside-network qualifier in Phase 4 gate keeps evidence honest |
| Founder time drained by services | Med | Product stall | Hard cap: 3 engagements pre-launch |
| Legal exposure from model outputs | Low-Med | High | Disclaimers everywhere, ToS review before first sale, no "investment advice" language anywhere |

## Success metrics per phase (one line each)

- **P0:** CI green with tie-out guard wired in + Excel snapshot "answer key" committed.
- **PR:** Rust engine reproduces the committed baseline cell-for-cell (339/350, 7 companies) + passes all snapshot diffs, in CI.
- **P1:** public accuracy table: ≥15 companies, ≥3 sectors, honest held-out number — all on the Rust engine.
- **P2:** ≥2 paid engagements + pre-order responses recorded.
- **P3:** 2 of 3 strangers install→activate→build unaided.
- **P4:** 10 paid seats / 3 team deals, ≥3 from outside the network, ≥half retained past day 60.
- **P5:** every shipped item traces to ≥3 customer requests.

## What was deliberately cut

See the Cut List with revival conditions in the design spec
(`docs/superpowers/specs/2026-07-03-master-plan-design.md`, §5): SOC 2/ISO, Kubernetes
and the 17-piece infra stack, Bloomberg/FactSet/CapIQ/PitchBook/LSEG connectors, the
11-agent library, the RAG pipeline, model-broker/fine-tuning/benchmark suite, real-time
market data, RBAC/SSO/collaboration servers, and the browser-based model editor. Each
has a written revival trigger; none is "never."

## Immediate next actions (execution starts on 👤 "go" — plan approved 2026-07-03, execution deliberately not started)

1. Agent session: Task 0.1.1 — verify the committed baseline still holds on today's code.
2. Agent session: Task 0.2.1 — CI on pull requests, tie-out guard included.
3. Agent session: Task 0.5.1 — Excel snapshot answer key.
4. Agent session: R.1 — tie-out adapter (unblocks the whole port).
5. 👤 Start thinking about Wave 1 company picks (1.1.1) — names you know cold.
