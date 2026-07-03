# FINMODEL: Production-Ready Financial AI Platform

## Mission

Transform finmodel from a CLI-based virtual financial analyst (single-ticker, 3-statement model, Excel + PPTX output) into a full-service, production-grade financial AI platform comparable to Rogo.ai -- capable of end-to-end investment banking workflows, multi-company analysis, institutional-grade outputs, team collaboration, enterprise security, and extensible agent-based architecture.

Target: a platform that competes with / differentiates from Rogo.ai for individual analysts, boutique firms, and open-source-first finance teams.

---

## PART 1: EXISTING PROJECT STATE

### Repository
- **Location:** `C:\Users\vinit\Documents\financial_model` (clone of `github.com/Knightwarrior911/finmodel.git`)
- **Branch:** `master`
- **Tech Stack:** Python 3.11, openpyxl, python-pptx, pytest, Streamlit (basic app), pdfplumber, Anthropic SDK + DeepSeek
- **Tests:** 131 pytest tests; 6 skipped
- **Extraction accuracy:** 100% tie-out on 5-company European industrial basket (256/256 cells)
- **Sectors supported:** industrial, bank, insurer (schemas defined)
- **LLM providers:** DeepSeek (cheapest), Anthropic SDK, or claude CLI fallback (no API key)
- **License:** Proprietary (no license file; copyrighted filings gitignored)

### What It Does Today

1. **Extraction** -- Pulls IS/BS/CFS from US SEC EDGAR XBRL (no API key needed) or non-US annual-report PDFs via LLM + pdfplumber.
2. **Reconciliation** -- Merges statement data with footnote detail, cross-checks internal consistency.
3. **Engine** -- Projects forward, builds linked 3-statement model.
4. **Excel Writer** -- Formula-driven Excel workbook (Rogo-standard layout, colour-coded inputs/formulas/cross-refs).
5. **Valuation** -- 3-scenario DCF, WACC build, trading comps, peer margin/trajectory comparisons, enterprise-value bridge, IFRS adjustment bridges.
6. **PowerPoint** -- Programmatic deck generation and editing (~40 tools: text/image edits, slide management, theme recolor, shape manipulation, tables, footnotes, vision-inspect + render-reflect).
7. **Research** -- Autonomous research pipeline: SEC EDGAR, web + headed-browser for non-US filings, market data, news, M&A deal synthesis.
8. **Source Auditability** -- Every number in the Excel output links back to the source filing page via `file:///...#page=N` hyperlinks.
9. **Assumption Ledger** -- 5-tier trust tracking (FILING / MARKET / DERIVED / ASSUMPTION / UNVERIFIED) on every valuation number with red catch-all in Excel.
10. **Valuation-Sanity Gate** -- Structural invariants enforced on every DCF/WACC output (WACC > g, beta >= 0, weights sum to 1, EV > 0, EV bridge identity, decreasing discount factors, no NaN/Inf).
11. **Sources Appendix** -- Markdown provenance appendix appended to orchestrator answers.
12. **Dynamic Revenue Segments** -- IS reproduces company's actual XBRL disclosure (Products/Services rows for AAPL).
13. **Orchestrator** -- Natural-language entry point (`--ask`) that plans and chains ~40 registered tools.
14. **Tie-Out Harness** -- Independent accuracy measurement instrument; immutable ground truth.

### Project Structure
```
src/                      # Core: extraction, reconciliation, engine, writer, valuation, orchestrator
  cli.py                  # CLI entry point
  extractor.py            # PDF + XBRL extraction (sector-aware)
  fetcher.py              # SEC EDGAR fetcher
  reconciler.py           # Cross-check engine
  engine.py               # Projection engine
  writer.py               # Excel writer
  dcf.py / wacc.py / peers.py / public_comps.py  # Valuation modules
  assumptions.py          # Forward assumption builder
  source_ledger.py        # Trust-tier accumulator
  derivations.py          # Derive-first valuation inputs
  assumption_registry.py  # Declared assumption home
  audit_link.py           # file#page hyperlink builder
  audit_pipeline.py       # Excel audit pass (links + tier colors)
  sources_report.py       # Provenance appendix generator
  valuation_invariants.py # Structural invariant checker
  audit_pptx.py           # PPTX speaker notes annotation
  orchestrator.py         # NL tool orchestrator
  is_builder.py           # Dynamic IS structure builder
  research/               # Autonomous research modules
tieout/                   # Accuracy measurement instrument
  config.py               # Sector schemas, basket
  groundtruth.py          # Ground-truth builder
  run_tieout.py           # Accuracy gate
docs/                     # Design notes, plans, specs
  IS_DYNAMIC_PLAN.md      # Dynamic IS line item plan (Phases 1-4)
  SPEC_powerpoint_editing.md
  superpowers/plans/      # Implementation plans (6 shipped)
  superpowers/specs/      # Design specs (5 shipped)
tests/                    # Pytest suite
config/                   # Sector/assumption configuration
valuation_kit/            # Valuation methodology reference
```

### Already Shipped (via PRs merged to master)
- PR #1: Universal source auditability (click number -> filing page)
- PR #2: Assumption ledger (5-tier tracking, kill silent defaults)
- PR #3: Total auditability Tier 2 (formula lineage + sources appendix)
- PR #4: Valuation-sanity gate (structural invariants)
- PR #5: Ledger follow-ups (PPTX sources notes + DCF preferred/investments)
- PR #6: Dynamic IS Phase 1 -- Revenue Segments [DONE]

---

## PART 2: COMPETITIVE ANALYSIS -- ROGO.AI BENCHMARK

Rogo.ai is the current market leader: $160M Series D (Kleiner Perkins), 35,000+ bankers/investors, 300+ institutions, 50,000+ daily queries. Key capabilities to match or beat:

### Rogo's Capabilities (must-address)

| Capability | Finmodel Status | Gap |
|---|---|---|
| **Pre-built workflow prompts** (Earnings Comp, Company Profile, Meeting Prep, Precedent Transactions, etc.) | No prompt library; `--ask` orchestrator is generic | **Build prompt library** |
| **Multi-company/basket analysis** | Single-ticker only; no basket analysis | **Build basket analysis** |
| **Data source integrations** (SEC, LSEG, FactSet, Capital IQ, PitchBook, Preqin, Daloopa, Intralinks, Dow Jones, Quartr, transcripts) | SEC + web only | **Add data integrations** |
| **AI Table Interface** (interactive sort/filter/edit) | No UI beyond Streamlit prototype | **Build web UI** |
| **Custom-trained finance LLMs** | Uses generic DeepSeek/Anthropic | **Fine-tune or prompt-optimize** |
| **Agent Library** (pre-built AI agents for workflows) | Single orchestrator | **Build multi-agent system** |
| **Model Broker** (multi-LLM switching) | Hardcoded provider priority | **Build model routing** |
| **Governance & permissions** (RBAC, audit trails) | None | **Build auth + governance** |
| **Enterprise deployment** (SOC 2, ISO 27001, single-tenant) | Local CLI only | **Build cloud + enterprise** |
| **Team collaboration** | Single-user CLI | **Build collaboration** |
| **Credit analysis module** ("Credit Center") | No credit capability | **Optional follow-on** |
| **Internal document query** (firm's proprietary data) | No | **Build RAG pipeline** |
| **Real-time market data** | No | **Add market data feeds** |

### Finmodel's Unique Advantages (differentiate)
1. **Open-source architecture** (vs Rogo's black-box SaaS) -- users can audit, extend, self-host
2. **Tie-out harness** -- independently verified 100% extraction accuracy (Rogo doesn't publish accuracy metrics)
3. **Assumption ledger** -- every number tiered with provenance, red catch-all for blind spots
4. **Valuation-sanity gate** -- structural invariants prevent silent math errors
5. **Non-US PDF extraction** -- works without EDGAR coverage (Rogo's non-US capability unknown)
6. **Formula-driven Excel** -- full formula lineage, not hardcoded values
7. **PowerPoint programmatic editing** -- ~40 tools for deck generation and editing
8. **Local/offline capable** -- works without internet for non-US filings

---

## PART 3: ROADMAP -- COMPLETED PHASES

All items with [DONE] are already shipped in `master`.

- Dynamic IS Phase 1 -- Revenue Segments [DONE]
- Source auditability (click number -> filing page) [DONE]
- Assumption ledger 5-tier tracking [DONE]
- Total auditability Tier 2 (formula lineage + sources appendix) [DONE]
- Valuation-sanity gate [DONE]
- Ledger follow-ups (PPTX notes + DCF improvements) [DONE]

---

## PART 4: ROADMAP -- PLANNED BUT NOT STARTED

These have formal plans in `docs/` but no implementation:

### 4.1 Dynamic IS Phases 2-4
- **Phase 2: Dynamic OpEx Rows** -- Replace hardcoded has_rd/has_sga with all-XBRL-opex pull. Show every operating expense the company actually reports (restructuring, stock-based comp, impairment, etc.). Robust fallback when XBRL is thin.
- **Phase 3: Complete Sector Templates** -- Bank (NIM, loan loss provisions, efficiency ratio), Insurance (combined ratio, loss ratio, premiums earned), REIT (FFO/AFFO, occupancy rate, cap rate), SaaS (ARR/NRR/GRR, internal-use software cap).
- **Phase 4: Actual Filing Labels** -- Use XBRL concept labels from company filings instead of hardcoded generic labels.

### 4.2 Tie-Out Waves 1-3
- **Wave 1** -- Add 5-6 diverse industrials; run extractor improvement loop
- **Wave 2** -- Add banks; per-sector report columns; sector-aware reconciler
- **Wave 3** -- Add insurers + cold held-out generalization test set; final gate

### 4.3 Source Audit Links for Remaining Outputs
- Comps sheet, DCF sheet, IFRS bridge, PPTX decks all need the `file:///...#page=N` hyperlink treatment

### 4.4 Fetcher Interest Derivation
- Wire the fetcher's debt-interest / cash-yield heuristics (`debt * 3.5%`, `cash * 2%`) through the derivation cascade so their tier appears in the ledger

### 4.5 Extraction Schema Expansion
- Add `preferred_stock`, `short_term_investments` to the extraction prompt (currently deferred -- needs supervised tie-out re-run)

### 4.6 Live-Basket Valuation Pass
- Run the full DCF/WACC/comps pipeline across a basket of names; report aggregate sanity metrics

---

## PART 5: NEW CAPABILITIES FOR PRODUCTION READINESS

### 5.1 Web Application (UI)

Build a full-featured web UI replacing the basic Streamlit prototype:

- **Dashboard** -- Project overview, recently analyzed tickers, accuracy metrics, ledger health
- **Ticker Analyzer** -- Input ticker/filing, select sectors, configure assumptions, run full pipeline
- **Interactive Table** (Rogo-style) -- Sort/filter/edit the financial model in-browser; inline editing of assumptions with instant recalculation
- **Basket Analysis** -- Compare multiple companies side-by-side (peer comps, sector aggregates, percentile rankings)
- **Model Explorer** -- Browse the Excel model online with clickable source links, tier colors, and lineage tooltips
- **Deck Builder** -- Visual PPTX builder: drag-and-drop slides, choose templates, auto-generate from model
- **Research Console** -- Query SEC EDGAR, view filings, extract sections, search by keyword
- **Prompt Library** (like Rogo's) -- Pre-built workflow templates: "Run comps on MSFT", "Build a merger model for X acquiring Y", "Prepare meeting prep memo for Z", "Analyze earnings release"
- **Admin Console** -- User management, API keys, audit logs, data source configuration
- **Real-time Collaboration** -- Shared projects, comments, version history, approval workflows

**Tech recommendation:** FastAPI backend + React/Next.js frontend (or Streamlit if faster MVP needed). Deployable via Docker.

### 5.2 Data Source Integrations

Build a pluggable data source system. Pre-built connectors for:

- **SEC EDGAR** (existing -- enhance for speed, bulk download)
- **Non-US filings** (existing -- add more country registries)
- **yfinance / Yahoo Finance** (existing partially -- make robust)
- **Bloomberg Terminal** (API if available)
- **FactSet** (REST API)
- **Capital IQ** (API)
- **PitchBook** (API)
- **LSEG / Refinitiv** (API)
- **Dow Jones / News APIs**
- **Earnings transcripts** (Seeking Alpha, Motley Fool, or direct)
- **Private company data** (Crunchbase, PitchBook private markets)
- **Macroeconomic data** (FRED, World Bank, IMF)
- **Real-time market data** (websocket feeds for live pricing)

Each connector implements a standard interface: `fetch(ticker, start_date, end_date) -> dict`. Caching layer (Redis or disk-based) with configurable TTL.

### 5.3 Agent Library & Prompt Library

Create a library of pre-built agents/prompts for common finance workflows:

**Agents:**
- `CompanyProfileAgent` -- Build a full company profile (business description, financials, comps, recent events, SWOT)
- `EarningsCompAgent` -- Compare a company's earnings to its peers (beat/miss analysis, margin trends, guidance)
- `PrecedentTransactionsAgent` -- Extract and analyze precedent M&A transactions from filings
- `MeetingPrepAgent` -- Generate a meeting prep memo with background, financials, questions, and risks
- `DealScreeningAgent` -- Screen a universe of companies against user-defined criteria
- `IndustryMapAgent` -- Build an industry landscape map with market positions and competitive dynamics
- `CreditAnalysisAgent` -- Analyze credit metrics, debt capacity, covenant headroom, rating agency methodology
- `MergerModelAgent` -- Build a merger model (accretion/dilution, financing structure, synergies)
- `LBOAgent` -- Build a leveraged buyout model (returns analysis, debt capacity, exit scenarios)
- `ESGAnalysisAgent` -- Extract and analyze ESG disclosures, ratings, and trends
- `PortfolioReviewAgent` -- Review a portfolio: performance attribution, risk metrics, concentration analysis

**Prompt Library** (Curated prompts with validated outputs):
- "Build a 3-statement model for {ticker} including DCF and comps"
- "Run a precedent transaction analysis for {industry/sector}"
- "Prepare a board meeting packet for {company} with financial summary, comps, and key risks"
- "Analyze {company}'s latest earnings release and compare to consensus"
- "Build a sum-of-the-parts valuation for {conglomerate}"
- "Generate a fairness opinion analysis for {target} being acquired by {acquirer}"
- "Create a pitch book for {sell-side company}"
- "Evaluate {company} as an acquisition target: strategic fit, synergies, financing, returns"

### 5.4 Multi-Company / Basket Analysis

- **Peer Group Analysis** -- Auto-detect peers by sector/SIC, run comps across entire peer group, percentile rankings
- **Basket Valuation** -- Run DCF/WACC across entire basket, report distribution of outputs, flag outliers
- **Sector Dashboard** -- Aggregate financials for a sector (sum of revenues, margins trends, valuation multiples)
- **Screening Engine** -- Screen companies by financial criteria (revenue growth > X%, debt/EBITDA < Y, etc.)
- **Index Construction** -- Build and track custom indices from screened companies

### 5.5 Model Broker / Multi-LLM Architecture

- **Router** -- Route each task (extraction, analysis, writing, research) to the optimal model
  - Extraction (high accuracy, cheap): DeepSeek / Claude Haiku
  - Analysis / Reasoning (complex): Claude Opus / Sonnet, GPT-5
  - Research / Web: cheapest capable model
  - Writing / Presentations: best at prose/structure
- **Fallback chains** -- If primary model fails/rate-limited, cascade to next
- **Cost tracking** -- Budget per task, per user, per project
- **Caching** -- Cache LLM responses for identical inputs (hash of prompt + context)
- **Custom fine-tuning** -- Support fine-tuned models for extraction (labeled training data from tie-out harness)
- **Benchmarking** -- Run finmodel's own "Big Finance Bench" (like Rogo's) to compare models on financial accuracy

### 5.6 RAG Pipeline for Internal Documents

- **Document Ingestion** -- Index firm's proprietary documents (previous models, memos, research, deal files)
- **Chunking & Embedding** -- Finance-aware chunking (preserve tables, financial figures, footnotes)
- **Hybrid Search** -- Vector similarity + keyword (BM25) + structured metadata filters
- **Citation** -- Every retrieved fact links back to source document, page, paragraph
- **Security** -- Document-level access controls, data isolation per team/client
- **Supported formats** -- PDF, DOCX, XLSX, PPTX, email archives, plain text

### 5.7 Web Research Pipeline (for non-US, private companies, M&A)

- **Multi-source search** -- Google, Bing, company websites, news, regulatory filings
- **Headless browser** -- Auto-login to paywalled sources, handle Cloudflare/bot detection
- **PDF discovery** -- Find and download annual reports from company IR pages
- **News synthesis** -- Aggregate recent news, classify by sentiment/impact, extract key facts
- **Deal sourcing** -- Monitor for M&A announcements, rumours, regulatory filings

### 5.8 Production Infrastructure

- **Async Architecture** -- Background task queue (Celery / Redis Queue) for long-running models (extraction can take 2-5 min per ticker)
- **Caching Layer** -- Redis/memcached for LLM responses, market data, computed models; LRU eviction
- **Database** -- PostgreSQL for users, projects, audit logs, model cache; TimescaleDB for time-series market data
- **Object Storage** -- S3/MinIO for filings, generated Excel/PPTX, screenshots
- **API Layer** -- REST + WebSocket API for programmatic access; auto-generated OpenAPI docs
- **Authentication** -- OAuth 2.0 / SSO (Google, Microsoft, Okta), API keys for programmatic access
- **Authorization** -- Role-based (admin, analyst, viewer), project-level permissions
- **Rate Limiting** -- Per-user, per-endpoint, per-LLM-provider rate limits
- **Monitoring** -- Prometheus + Grafana for metrics (model runtime, accuracy, cost, error rates, API latency)
- **Logging** -- Structured JSON logging (ELK stack or similar); audit trail for every model run
- **Alerting** -- PagerDuty/Slack alerts for model failures, accuracy regressions, cost anomalies
- **CI/CD** -- GitHub Actions: pytest on PR, tie-out guard, integration tests, deployment
- **Docker** -- Containerized deployment with docker-compose (dev) and Kubernetes (prod)
- **Backup & Recovery** -- Automated DB snapshots, S3 versioning for filings/models

### 5.9 Enterprise Features

- **Single-tenant deployment** -- Customer's own VPC/cloud
- **SOC 2 Type II** -- Evidence collection, access controls, encryption at rest + in transit
- **Audit Trails** -- Every analyst action logged: who ran what model on which ticker, what assumptions they changed, what outputs were generated
- **Data Residency** -- Regional deployment options (US, EU, APAC)
- **Encryption** -- AES-256 at rest, TLS 1.3 in transit, customer-managed KMS
- **SSO / SAML** -- Enterprise identity provider integration
- **Compliance Reporting** -- Auto-generated compliance reports for regulated use
- **Tenant Isolation** -- Strict data separation between clients/teams

### 5.10 Testing & Quality

- **Accuracy Regression** -- Tie-out harness runs on every CI build; 100% accuracy enforced
- **Model Output Verification** -- Every generated Excel model checked by the valuation-sanity gate
- **Integration Tests** -- End-to-end pipeline for 10+ tickers across all sectors
- **Performance Benchmarks** -- Extract-to-Excel time (target < 30s for US tickers, < 2min for non-US PDFs)
- **Load Testing** -- Simulate 100 concurrent users, measure p50/p95/p99 latency
- **Chaos Engineering** -- Test resilience to LLM API failures, network timeouts, corrupted filings
- **Prompt Regression** -- Capture common extraction prompts and verify output structure hasn't changed
- **Golden Dataset** -- 20-company ground-truth dataset (filing numbers independently verified) for accuracy measurement

### 5.11 Documentation & Onboarding

- **User Docs** -- Getting started, tutorial videos, workflow guides, FAQ
- **Developer Docs** -- API reference, architecture overview, contributing guide, local dev setup
- **CLI Reference** -- Auto-generated man pages for every tool/subcommand
- **Prompt Catalog** -- Gallery of pre-built prompts with expected outputs
- **Model Cards** -- For each supported LLM: accuracy benchmarks, cost, latency, known limitations
- **Architecture Decision Records** -- Written ADRs for every major design decision
- **CHANGELOG** -- Structured changelog with migration guides

### 5.12 Security Hardening

- **Dependency scanning** -- Dependabot / Snyk for Python dependencies
- **SAST/DAST** -- Static analysis (Bandit, Semgrep) + dynamic scanning
- **Secrets management** -- Vault or cloud KMS for API keys; never in .env in production
- **Prompt injection protection** -- Input sanitization for `--ask` orchestrator
- **File upload validation** -- PDF/DOCX uploads scanned for malware
- **Rate limiting** -- API rate limits to prevent abuse
- **CORS/CSRF protection** -- For web app
- **Penetration testing** -- Annual third-party pen test
- **Bug bounty** -- Public vulnerability disclosure program

### 5.13 LLM Provider Management

- **DeepSeek** (primary extraction -- cheapest)
- **Anthropic (Claude)** -- Analysis, reasoning, writing
- **OpenAI (GPT)** -- Fallback, specific workflows
- **OpenRouter** -- Unified API for 200+ models
- **Local models** -- Support for local LLMs (Ollama, vLLM) for offline/air-gapped deployments
- **Cost optimization** -- Smart routing based on task complexity, urgency, cost budget
- **Response validation** -- Validate LLM JSON output structure before passing downstream

---

## PART 6: INSTRUCTIONS FOR THE AI MODEL

You are the **finmodel Master Planner and Orchestrator**. Your job is threefold:

### Step 1: Produce a Master Plan

Given the complete context above (existing project state, Rogo competitive analysis, completed roadmap, planned roadmap, and new production-readiness items), produce a comprehensive, phased implementation plan. The plan should:

- **Define phases** -- Logical groupings of work (e.g., Phase A: Foundation, Phase B: Web UI, Phase C: Multi-Company, Phase D: Enterprise, Phase E: Agents). Each phase should be independently shippable and valuable.
- **Prioritize** -- Mark each item as P0 (must-have for MVP), P1 (important), P2 (nice-to-have). Provide rationale.
- **Estimate effort** -- Small (days), Medium (1-2 weeks), Large (2-4 weeks), Epic (1-3 months).
- **Identify dependencies** -- What must be built before what. Identify parallelizable work.
- **Architecture decisions** -- For each major item, propose concrete architecture: tech choices, data flow, module structure.
- **Risk assessment** -- Identify the riskiest items (accuracy regressions, LLM reliability, API dependencies) and mitigation strategies.
- **Success criteria** -- For each phase, define clear, measurable success criteria.

### Step 2: Act as Orchestrator

Once the master plan exists, act as orchestrator:
- **Spawn sub-agents** -- For each task in the current phase, spawn a sub-agent with:
  1. The specific task description
  2. All relevant context files (existing code paths, plan docs)
  3. TDD instructions (write test first, then implement, then verify)
  4. Acceptance criteria
- **Monitor progress** -- Review sub-agent outputs for correctness, consistency, and adherence to architecture
- **Manage dependencies** -- Ensure tasks complete in the right order; resolve conflicts
- **Quality gate** -- Every sub-agent's output must pass:
  1. pytest suite (no regressions)
  2. Tie-out harness (accuracy unchanged)
  3. Static analysis (no new linting errors)
  4. Integration verification (run end-to-end on test ticker)

### Step 3: Output Format

**The primary deliverable is the Master Plan** -- a structured, actionable document. Sub-agent execution logs are secondary but useful for the user to review later.

The master plan should be written in `docs/MASTER_PLAN.md` with cross-references to:
- `docs/IS_DYNAMIC_PLAN.md` -- Phases 2-4 details
- `docs/superpowers/plans/` -- Existing plans for shipped items (reference architecture patterns)
- `docs/superpowers/specs/` -- Design specs (reference for conventions, data structures)

### Constraints & Conventions

1. **Backward compatibility** -- Never break existing CLI flags, module signatures, or output formats. New features add, never remove.
2. **Test-driven** -- Every new module gets a test file first. Every bug fix gets a regression test first.
3. **Accuracy is sacred** -- The tie-out harness must never regress. Extraction accuracy is the #1 metric.
4. **No silent defaults** -- The assumption ledger pattern (derive -> registry -> UNVERIFIED) is mandatory for every valuation input.
5. **Everything auditable** -- All outputs must carry provenance back to source documents.
6. **Modular design** -- Each new capability is a standalone module with a clear interface; the system is built for extension.
7. **Incremental value** -- Every phase must deliver standalone value; no phase depends on a future phase to be useful.
8. **Python 3.11+** -- Stick to the existing tech stack unless a clear, justified reason exists to diverge.

---

## APPENDIX: EXISTING DOCUMENTS TO REFERENCE

- `C:\Users\vinit\Documents\financial_model\README.md` -- Full project overview, architecture diagram, quick start
- `C:\Users\vinit\Documents\financial_model\docs\IS_DYNAMIC_PLAN.md` -- Phases 1-4 with detailed approach for each
- `C:\Users\vinit\Documents\financial_model\docs\superpowers\plans\2026-05-31-assumption-ledger.md` -- Reference architecture for ledger pattern
- `C:\Users\vinit\Documents\financial_model\docs\superpowers\plans\2026-05-31-valuation-sanity-gate.md` -- Reference architecture for invariant pattern
- `C:\Users\vinit\Documents\financial_model\docs\superpowers\specs\` -- All design specs for architecture patterns
- `C:\Users\vinit\Documents\financial_model\src\source_ledger.py` -- Trust-tier accumulator pattern
- `C:\Users\vinit\Documents\financial_model\src\assumption_registry.py` -- Declared assumption pattern
- `C:\Users\vinit\Documents\financial_model\src\derivations.py` -- Derive-first cascade pattern
- `C:\Users\vinit\Documents\financial_model\src\valuation_invariants.py` -- Structural invariant pattern
- `C:\Users\vinit\Documents\financial_model\src\audit_link.py` -- Source hyperlink pattern
