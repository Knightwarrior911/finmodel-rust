# Finmodel Master Plan — Design Spec

**Date:** 2026-07-03
**Status:** Draft for founder review
**Replaces:** `docs/FINMODEL_PRODUCTION_PROMPT.md` Parts 5–6 (kept as historical reference; the Part 5 wish-list is explicitly superseded by this spec's Cut List, and Part 6's orchestration instructions by the working method in §7 and MASTER_PLAN's cross-cutting rules)

---

## 1. What this document decides

The old production prompt was an engineering wish-list with no customer, no price, and no
sequencing. This spec fixes the strategy so the detailed plan (`docs/MASTER_PLAN.md`) has
something real to aim at.

**Decisions locked here:**

| Question | Decision |
|---|---|
| Goal | Paid product (a business, not a portfolio piece) |
| First buyer | Boutique finance firms and consultants (valuation shops, small IB/M&A advisory, fractional CFOs, independent equity analysts) |
| Product shape | **Desktop app, pure Tauri 2 / Rust** — the Python pipeline is ported to a Rust engine (`finmodel-core`) under a strict parity gate (must reproduce the committed accuracy baseline 256/256 before anything builds on it); Python remains dev-only as reference implementation + tie-out instrument. Data stays on the user's machine. (👤 amended 2026-07-03: founder wants the PDF Panda / Snitch feel — no bundled Python. See MASTER_PLAN "Rust amendment" + Phase R.) |
| Revenue bridge | Founder sells model-building **services** to boutiques while the product matures (founder works in finance — credibility and warm access exist) |
| Deferred | Hosted SaaS / web UI — revisit only after ≥10 paying desktop seats ask for it |
| Budget ceiling | < $50/month pre-revenue. No servers, no paid data feeds. BYO LLM key (customer pays own pennies-per-model API cost) |
| Payments | Dodo Payments merchant-of-record (founder is in India — no Stripe; Dodo account + integration patterns already exist from the Snitch project) |
| Pace | Quality first, no deadline. Accuracy work precedes selling. |

## 2. The wedge (why anyone picks this over Rogo)

Rogo sells cloud AI to large institutions ($160M raised, enterprise sales, data goes to
their cloud). A boutique with 3–20 people is not Rogo's customer and cannot easily pass
its own clients' confidential data to a third-party cloud anyway.

Finmodel's pitch to that boutique, in selling order (lead with time saved — that is the
buying motive; privacy is the closer, not the opener):

1. **Hours of modeling work compressed to minutes.** Ticker or filing PDF in,
   client-ready 3-statement model + DCF + deck out. Capacity is the boutique's real
   constraint.
2. **Every number defends itself.** Click any figure in the Excel output and it opens the
   source filing at the right page. Every assumption is tier-tagged (FILING / MARKET /
   DERIVED / ASSUMPTION / UNVERIFIED). Rogo publishes no accuracy numbers; finmodel ships
   with a public, reproducible accuracy harness.
3. **The output is a real, formula-driven Excel model** — the artifact boutique analysts
   actually deliver to clients — not a chat answer. Excel *is* the editor; we do not
   rebuild Excel in a browser.
4. **Non-US (IFRS PDF) filings included** — with marketing claims held strictly to what
   the accuracy table has validated (European IFRS + US EDGAR at launch; other markets
   added as tie-out waves prove them).
5. **Your data never leaves your laptop.** No IT approval, no vendor security review,
   no client-confidentiality problem — the closer for compliance-conscious firms.

## 3. Non-negotiables (carried over — these were the good part of the old doc)

1. Backward compatibility — never break existing CLI flags or output formats.
2. Test-driven — new module ⇒ test first; bug fix ⇒ regression test first.
3. Accuracy is sacred — tie-out harness must never regress; it runs as a gate on every change.
4. No silent defaults — derive → registry → UNVERIFIED cascade mandatory for valuation inputs.
5. Everything auditable — all outputs carry provenance to source documents.
6. Modular design — each capability is a standalone module with a clear interface.
7. Every phase ships standalone value.
8. Python 3.11+ engine; Tauri 2 for the shell (founder's proven stack).

## 4. Phase map

Six phases. Each is independently valuable; each ends with a verifiable gate.

- **Phase 0 — Safety Net.** Verify the committed tie-out baseline (it exists: 100% on
  5 of 7 basket companies, commit `57a7b41`) and wire its guard into CI, split the
  3615-line `writer.py` monolith under a byte-identical output snapshot, extend
  `reconciler.py`'s deterministic accounting-identity checks, make the repo
  pip-installable. *Gate: CI green on PR with the tie-out guard included; writer split
  with unchanged output.*
- **Phase 1 — Make the Accuracy Claim Real.** Tie-out Waves 1–3 (fix the 2 skipped
  basket companies + diverse industrials → banks → insurers + held-out set), engine
  *projection* modes for insurance (IS layouts already exist from Dynamic IS Phases 1–4,
  which are implemented — commit `9174435`), re-validate Dynamic IS across the new
  basket, extraction schema gaps, audit links on the remaining outputs. *Gate: published
  accuracy table across ≥15 companies in ≥3 sectors including a never-seen held-out set.*
- **Phase 2 — Dogfood + Services Bridge.** Founder uses finmodel in real finance work;
  sells 2–3 paid model-building engagements to boutiques; deliverable polish (branding,
  deck templates, QA checklist); legal disclaimer baseline; a standing desktop pre-order
  offer to every engagement client (the earliest product willingness-to-pay signal).
  *Gate: ≥2 paid engagements delivered; defect/request list + pre-order responses captured.*
- **Phase 3 — Finmodel Desktop v1.** Tauri 2 app wrapping the engine (PyInstaller
  sidecar — a 1-session spike proves this novel piece before the build; browser-research
  deps excluded, desktop ships US/EDGAR + drag-and-drop PDF), LLM-provider abstraction,
  4 screens (New Model, Progress, Library incl. `.finmodel` export/import bundle for
  analyst→partner review, Settings), BYO API key, Dodo licensing + trial, auto-update,
  Windows first. *Gate: a stranger can install, activate, and build a model with their
  own key, unaided.*
- **Phase 4 — First 10 Boutique Seats.** Design-partner pilots from founder's network,
  trust-content marketing (accuracy methodology, auditability), pricing test
  ($79–149/seat/mo or annual), feedback loop. *Gate: 10 paying seats or 3 boutique
  teams, ≥3 seats from outside the warm network, ≥half active past day 60.*
- **Phase 5 — Deepen the Wedge (evidence-gated).** Only what ≥3 customers ask for:
  top-5 workflow prompt library, basket/peer analysis, merger/LBO modules, macOS build,
  open-core decision, hosted API. Nothing here starts before Phase 4 evidence exists.

## 5. Cut list (from the old doc — with revival conditions)

| Cut item | Why cut | Revisit when |
|---|---|---|
| SOC 2 / ISO 27001, pen tests, bug bounty | $50K–$200K + months; answers enterprise procurement that isn't asking | An enterprise offers a signed LOI conditional on it |
| Kubernetes, Celery, TimescaleDB, ELK, Prometheus, PagerDuty, MinIO | 17 infra pieces for a CLI whose heavy step is one LLM call; desktop app needs none | A hosted product with real load exists |
| Bloomberg / FactSet / CapIQ / PitchBook / LSEG / Preqin connectors | Enterprise contracts a solo founder can't sign; 13 feeds = full-time maintenance | A customer supplies their own licensed credentials and pays for the connector |
| 11 pre-built agents | The 43-tool orchestrator already covers the workflows; each agent is a vertical app | ≥3 customers request the same workflow (build that one) |
| RAG pipeline for internal docs | 2+ months; no user asked | Paying customers ask to query their own document sets |
| Model broker / fine-tuning / Big Finance Bench | Solves scaling problems that need users first | LLM spend or accuracy plateaus demand it |
| Real-time market data, websockets | Valuation models don't need ticks; yfinance suffices | A screening/monitoring feature ships |
| RBAC, SSO, team collaboration, audit-trail governance | Single-seat desktop product has no tenants | Multi-seat team licenses sell |
| Web UI / interactive table editor | Excel is the editor boutiques already trust; browser rebuild is months of frontend | Desktop demand proves out and demo-friction data says web wins |

## 6. Economics sketch

- **Costs pre-revenue:** ~$0 infra (GitHub free CI, no servers), LLM keys for development
  (DeepSeek ≈ $0.10–0.50 per full model run), Dodo takes % of sales only. Under the $50/mo ceiling.
- **Customer cost:** BYO key ⇒ customer pays their own ~pennies per model; no margin risk to founder.
- **Price hypothesis (test in Phase 4):** $99/seat/mo, annual $79/seat/mo-equivalent,
  14-day trial. One 5-seat boutique ≈ $6k ARR.
- **Liability:** outputs carry "not investment advice" disclaimers; ToS at first sale
  (template + review, not bespoke lawyering).

## 7. Working method (how a non-technical founder executes this)

Every phase runs the same loop the repo already uses (`docs/superpowers/`):
brainstorm → design spec → implementation plan → AI agents execute in small, test-gated
tasks → tie-out + pytest + review gates → merge. The founder's jobs are: pick priorities,
answer domain questions (finance judgment), run the manual gates (install the app, read
the output like a client would), and do the human parts of Phases 2 and 4 (selling).

## 8. Open items for founder review

1. Approve/adjust the price hypothesis and trial length (Phase 4 tests it regardless).
2. Confirm services engagements are acceptable use of your time (Phase 2 assumes 2–3).
3. Windows-first confirmed? (macOS deferred to Phase 5 via CI build, per PDF Panda pattern.)
4. Open-core decision intentionally deferred to Phase 5 — veto now if you want it earlier.
