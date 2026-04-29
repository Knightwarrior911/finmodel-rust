# Rogo.ai Research & Questions to Ask

*Researched: 2026-04-24*

---

## What Rogo Actually Is

### Architecture Under the Hood

- **Multi-model routing**: GPT-4o, o1, o1-mini originally; now Gemini 2.5 Flash/Pro + Claude Opus 4.7. Routes tasks to cheapest model that can do the job.
- **50M+ financial documents indexed.** Hybrid RAG: token-based + embedding-based search via Google Spanner (their DB).
- **Fine-tuned models** on finance-specific tasks, labeled by ex-bankers. Not just prompt engineering.
- **Hallucination rate**: 34.1% → 3.9% after switching to Gemini 2.5 Flash.
- **Multimodal**: reads visual data in PDFs (charts, tables, not just text).

### Felix Agent — Three Core Categories

| Category | Specific Tasks |
|---|---|
| **Decks** | Shell deck from template, refresh materials, turn comments into edits |
| **Spreadsheets** | Build model from scratch, spread comps, roll forward models, swap comps, fix errors with trace |
| **Reports** | Company profiles, meeting prep, investment memos, diligence questions, market maps, buyer screens |

### Named Workflow Templates (visible on product page)

- Earnings Comp Analysis
- Public Company Profile
- Strip Profile
- Meeting Prep
- Private Company Profile
- Personal Bio
- Financial Sponsor Overview
- News Run
- Secondaries Buyer Overview
- Proofread My Deck

### Data Sources Integrated (all paid/enterprise)

LSEG, FactSet, Capital IQ, PitchBook, Preqin, Dow Jones, Quartr (earnings call transcripts), Third Bridge (expert network), SEC filings, International filings, SharePoint (firm's internal data), CRM, real-time news, Fitch Solutions.

### Spreadsheet Agent (acquired Subset, Sep 2025)

- Reads existing 40-tab models with circular references
- "Why is FCF negative in 2027?" → full driver trace + fix recommendation
- Pulls live data from CapIQ/FactSet/PitchBook directly into cells
- In-cell source citations for every number
- Build models from scratch, roll forward, swap comps, refresh charts

### AI Tables (their proprietary feature)

- Structured analytical tables powered by their own financial reasoning model
- Multi-step logic and comparative analyses in-cell
- Auto-references filings, ownership data, news in real time

### Recent Shipping (Nov 2025)

- In-cell source citations on every Excel cell
- AI Tables upgrade with proprietary reasoning model
- SharePoint as first-class data source
- Rich copy-paste into PPT/Word/email with formatting preserved
- PDF export

### Security

SOC2 Type II, ISO 27001, GDPR, CCPA, EU AI Act compliant. Single-tenant deployment. No training on client data. End-to-end encryption. Full audit trails.

### Business Scale

- 25,000+ users (bankers and investors)
- 50,000+ daily queries
- $75M Series C (2026)
- Clients: Truist Securities, Nomura, top-5 US bulge brackets, mega-cap PE firms

---

## Our Gaps vs Rogo

| Gap | Rogo Has | We Have |
|---|---|---|
| Market data | CapIQ, FactSet, PitchBook, LSEG (paid APIs) | yfinance (free, public markets only) |
| Private company data | PitchBook, Preqin, CapIQ private | Nothing |
| Expert network transcripts | Third Bridge | Nothing |
| International filings | Integrated | Nothing |
| RAG / document search | Hybrid search, 50M docs indexed, Spanner | Nothing (raw requests only) |
| Fine-tuned model | Finance-specific model, custom labeled training data | Claude out-of-box |
| Spreadsheet agent | Live read/write Excel, data-connected | openpyxl/xlsxwriter as file builder only |
| In-cell citations | Every cell cites its source | Nothing |
| Multi-model routing | Routes by task type and cost | Single model |
| Template ingestion | Takes firm PPT/Excel template and populates it | Builds from scratch only |
| Firm internal data | SharePoint, CRM, internal docs connected | Nothing |
| AI Tables | Proprietary structured analytical table interface | Nothing |
| Async agent | Email Felix, get results later | Synchronous only |

---

## Questions to Ask Rogo (Felix) — In Order

Ask these when you have access. Each group builds on the previous one.

### Group 1 — Understand the Output Format

1. *"Build me a DCF model for Apple — send me the Excel file."*
   > See: real downloadable .xlsx with live formulas, or just a table of numbers? Are formulas linked or hardcoded? Do cells show source citations?

2. *"Spread comps for the US software sector, 10 companies, EV/Revenue, EV/EBITDA, P/E, NTM and LTM. Send as Excel."*
   > See: default metrics, data source, whether cells are cited, formatting quality.

3. *"Build me a 3-statement model for Microsoft for the last 3 years."*
   > See: does income statement, balance sheet, and cash flow all tie? Circular references handled?

### Group 2 — Trace the Data

4. *"For the Apple DCF you built — show me exactly which source each assumption came from."*
   > See: auditability depth and transparency.

5. *"Revenue for Apple FY2024 — is that from FactSet, Capital IQ, or the SEC filing? What if they disagree?"*
   > See: source prioritization logic when data vendors conflict.

6. *"Pull private company financials for a company like Stripe. How much data do you have and from which source?"*
   > See: private market data depth — this is where most tools break down.

### Group 3 — Test Complex Model Work

7. *"I'll upload a 40-tab LBO model. Can you roll it forward one year and update the comps?"*
   > See: can it READ and edit an existing model, or only write new ones from scratch?

8. *"FCF in year 3 of that model is negative and shouldn't be. Find the bug and fix it."*
   > See: driver trace capability, circular reference handling, error diagnosis.

9. *"Build an LBO for a company with $500M EBITDA, 10x entry, 65% debt financing. Full debt schedule, PIK toggle, returns at 5x/6x/7x exit."*
   > See: LBO depth and model conventions — BIWS style? Macabacus? Something else?

### Group 4 — Test Deck and Document Work

10. *"Here's our firm's pitch deck template. Build me a company profile for Tesla using our template."*
    > See: does it respect brand/layout/slide design, or produce generic output?

11. *"I'm uploading a 200-page CIM. Give me: exec summary, key risks, list of diligence questions, and potential buyer list."*
    > See: PDF processing quality, multimodal reading of tables and charts inside PDFs.

12. *"Draft an investment memo for buying this company at 12x EBITDA. Include valuation, risks, and return analysis."*
    > See: structure and depth of long-form written output.

### Group 5 — Probe the Limits

13. *"What's something a banker would ask you where you'd give a bad or unreliable answer?"*
    > See: self-awareness of failure modes — a good system knows what it can't do.

14. *"Pull me comps for private SaaS companies — ARR multiples from deals in the last 12 months."*
    > See: private market comps depth — the hardest thing to do well and where most tools fake it.

15. *"Can you access Bloomberg data?"*
    > See: Bloomberg is notoriously hard to integrate — a key gap to confirm.

16. *"If you're not sure a number is right, what do you do — answer anyway or flag it?"*
    > See: confidence calibration and verification behavior vs hallucination risk.

### Group 6 — Understand the Agent vs Chat Boundary

17. *"What's the difference between asking you a question here in chat vs sending you a task as Felix?"*
    > See: where the autonomous agent mode kicks in vs just Q&A.

18. *"Can you run autonomously overnight — like: research 20 companies, build a market map, email me the Excel in the morning?"*
    > See: true async agent capability or still synchronous (you wait for output in real time).

---

## What to Note When Testing

For each question above, record:
- **Output format**: text / table / downloadable file / in-app viewer
- **Data source cited**: named source, or generic "based on available data"
- **Formula quality** (for Excel): live formulas vs hardcoded values
- **Accuracy**: spot-check 3-5 numbers against the primary source yourself
- **Speed**: how long does it take for complex tasks
- **Failure mode**: does it refuse, hallucinate, or give a low-confidence flag

This gives us a scorecard to know exactly what to build.
