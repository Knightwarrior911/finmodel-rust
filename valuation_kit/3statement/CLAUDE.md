\# Build 3-Statement Model - Master Build Instructions



\*\*Name:\*\* `build-3statement-model`  

\*\*Description:\*\*  

Build a fully integrated, dynamic 3-statement model in Excel for any public company. Output a single `.xlsx` with a Cover tab, Assumptions tab, and IS / BS / CF tabs, three years of \*\*historicals\*\*, five years of projections, a Base / Upside / Downside case toggle, and a balance sheet that ties to zero in every period and every case.



\---



\## 0. Mission



Produce a \*\*client-ready, fully-linked 3-statement model\*\* for the target company.  

\- Every projected line item must trace back to a documented driver.  

\- The balance sheet must balance in every period and every case.  



\*\*This is not a template fill-in.\*\* It is a \*\*primary-source build\*\*:  

\- Historical numbers come from filings.  

\- The line item structure mirrors the company’s actual disclosure.  

\- Projections are driven by an Assumptions tab that the user can edit.



\---



\## 1. Inputs Required from User



\*\*Before starting, collect:\*\*



| Input                          | Default if not specified                  |

|--------------------------------|-------------------------------------------|

| Target company name and ticker | \*(must be provided)\*                      |

| Historical period              | Most recent 3 fiscal years                |

| Projection period              | 5 years                                   |

| Case toggle                    | Yes – Base / Upside / Downside            |

| Revenue build granularity      | Single line if no segments; segment-level if company reports segments |

| Tab structure                  | Cover → Assumptions → IS → BS → CF       |

| Output path                    | `/workspace/outputs/{TICKER}\_3Statement\_Model.xlsx` |



If the user has not specified an input, use defaults. \*\*Do not block the build asking questions\*\* – make reasonable choices and document them on the Cover tab.



\---



\## 2. Specs to Load



These four specs govern the build. \*\*Read them in full before writing any code.\*\* They override default behavior.



| # | File                              | What it governs |

|---|-----------------------------------|-----------------|

| 1 | `SPEC\_methodology.md`             | Schedule order, IS / BS / CF structure, supporting schedules, sector adaptations, sanity checks |

| 2 | `SPEC\_spreadsheet\_engineering.md` | Font color rules, two-hop pattern, single source of truth, validator rules, citations |

| 3 | `SPEC\_excel\_formatting.md`        | Tab layout, columns, number formats, year conventions, colors, print setup. \*\*Firm-specific brand standards override this file if available.\*\* |

| 4 | `SPEC\_modeling\_patterns.md`       | Assumptions tab structure, case toggle architecture, restate placement, CHOOSE pattern, multi-segment handling |



\---



\## 3. Workflow



Execute in this order. Do not skip steps.



\### Step 1 – Resolve the company

\- Get the company’s `rogoCompanyId` via `companyLookup`.

\- Confirm sector and industry (drives line item adaptations – see \*\*SPEC\_methodology Section 7\*\*).



\### Step 2 – Identify filings

\- \*\*US issuers\*\*: Fetch the most recent 10-K. If the BS in the most recent 10-K only shows two years (typical), also fetch the prior year’s 10-K for the third historical BS.

\- \*\*Non-US issuers\*\*: Fetch the most recent 20-F or annual report.

\- \*\*UK private\*\*: Fetch UK Annual Accounts.

\- Extract IS, BS, CF, and supporting notes (debt, PP\&E, intangibles, segments).



\### Step 3 – Pre-projection tie-out

\- Sum all BS line items. Verify \*\*Total Assets = Total Liabilities + Equity\*\* for every historical year.

\- Verify all subtotals (Gross Profit, EBIT, EBITDA, Net Income, CFO, CFI, CFF) match the filing exactly.

\- Verify IS Net Income equals CF starting Net Income.

\- Verify CF Ending Cash equals BS Cash for each historical year (small variance acceptable for restricted cash treatment – document it).

\- \*\*If any check fails, stop. Do not proceed to forward modeling.\*\*



\### Step 4 – Build the workbook

Build in this exact order. Do not skip ahead.



1\. Apply tab layout, formatting, and column widths to all tabs (per \*\*SPEC\_excel\_formatting\*\*).

2\. Build the \*\*Cover\*\* tab.

3\. Build the \*\*Assumptions\*\* tab – toggle, Active block, three scenario blocks (per \*\*SPEC\_modeling\_patterns\*\*).

4\. Build the \*\*IS\*\* tab – historicals, then forward formulas. Drivers below each line item (per \*\*SPEC\_methodology Section 3\*\*).

5\. Build supporting schedules below the BS: \*\*Working Capital → PP\&E → Debt → Retained Earnings\*\*. Build schedules first, then BS line items link to them.

6\. Build the \*\*BS\*\* tab line items, linking only – no calculations on the face of the BS.

7\. Build the \*\*CF\*\* tab. Link Ending Cash to BS Cash as the last step – this closes the loop.



\### Step 5 – Validate (per \*\*SPEC\_spreadsheet\_engineering Section 6\*\*)

Run the validator.

\- Status must be \*\*'success'\*\* with \*\*0 FAILS\*\*.

\- BS check row must equal $0 in every period in every case.

\- Cash tie-out must equal $0 in every projection period.



\*\*If any check fails, root-cause and fix. Do not insert plugs.\*\*



\### Step 6 – Test scenarios

Toggle to each of \*\*Base / Upside / Downside\*\*. Verify:

\- BS still balances.

\- No formula errors (`#DIV/0!`, `#VALUE!`, `#REF!`).

\- Outputs are directionally correct (Upside revenue > Base > Downside).

\- Toggle the circ switch on. Verify the model still balances.



\### Step 7 – Document and deliver

\- Update the Cover tab with model date, data source citations, and the case currently active.

\- Write a chat response summarizing key projections, citing every claim.

\- Reference the `.xlsx` as a markdown file link.



\---



\## 4. Must-Follow Rules



These are \*\*blocking\*\*. If any are violated, fix before proceeding.



\- Use Excel formulas. \*\*Never hardcode a calculated value.\*\*

\- Font color equals cell type: \*\*blue\*\* inputs, \*\*black\*\* local formulas, \*\*green\*\* cross-sheet.

\- \*\*Two-hop pattern\*\*: Assumptions → local restate → calculation. Calculations never reference Assumptions directly.

\- Balance sheet must balance every period. \*\*Total Assets = Total Liabilities + Total Equity.\*\*

\- \*\*No plugs.\*\* If the BS doesn’t balance, root-cause it. Do not insert balancing rows.

\- Cell comments on every hardcoded input, with `` `cite:{citationId}` `` referencing the source.

\- Replicate the company’s \*\*actual reported line item structure\*\*. Do not impose a generic template.

\- Build supporting schedules \*\*before\*\* statements (PP\&E → Debt → WC → IS → BS → CF).

\- Verify \*\*historicals tie\*\* before projecting.

\- \*\*No scenario logic\*\* on statement tabs. All CHOOSE / IF-case logic lives \*\*only\*\* on Assumptions.

\- No merged cells. Use “center across selection” if needed.

\- Gridlines \*\*off\*\* on every tab.



\---



\## 5. Final Acceptance



The model is ready to deliver \*\*only when\*\*:



| Check                                 | Required          |

|---------------------------------------|-------------------|

| Validator status                      | success           |

| Validation FAILS                      | 0                 |

| Formula errors                        | 0                 |

| BS check (all periods × all cases)    | $0                |

| Cash tie-out (projection periods × all cases) | $0       |

| NI link (IS NI = CF starting NI)      | $0                |

| RE roll (Begin + NI – Div – End)      | $0                |

| PP\&E roll (Begin + CapEx – D\&A – End) | $0                |

| Debt roll (Begin + Borr – Repay – End)| $0                |

| All hardcoded inputs have cell comments | yes            |

| Toggle test passes for all 3 cases    | yes               |



\*\*Deliver only when all checks above are satisfied.\*\*

