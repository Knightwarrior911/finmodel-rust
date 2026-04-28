# SPEC_methodology.md - 3-Statement Model Methodology

This file defines the **structural methodology** for any 3-statement model. It is **sector-agnostic by default**; sector-specific adaptations are in **Section 7**.

---

## 1. Schedule Order

Build in this order. The downstream item depends on the upstream output.

| Schedule              | Produces / Feeds                                      |
|-----------------------|-------------------------------------------------------|
| **PP&E schedule**     | D&A and CapEx                                         |
| **Working Capital**   | AR, Inventory, AP, Prepaid, Accrued, Deferred Revenue |
| **Debt schedule**     | Interest expense, current/LT split, ending debt       |
| **Retained Earnings** | Ending RE                                             |
| **Income Statement**  | Uses D&A from PP&E, interest from debt                |
| **Balance Sheet**     | Uses outputs from all schedules; cash is the plug     |
| **Cash Flow**         | Uses NI from IS, D&A & CapEx from PP&E, WC deltas from BS, debt flows from debt schedule, ending cash links to BS |

**Key note**: Build **IS first** because BS depends on RE (which depends on IS NI). Build BS line items **after** schedules are done. Build **CF last** because it integrates all of them.

---

## 2. Historical Data Requirements

- Minimum **3 years** of historical IS and CF.
- Minimum **3 years** of historical BS (typical 10-K shows 2 years; pull the third year from the prior year’s filing).
- Normalize non-recurring items before projecting (restructuring, impairment, gain/loss on asset sales, one-time legal settlements).
- Verify every subtotal matches the filing.
- Verify **IS Net Income = CF starting Net Income** for every historical year.
- Verify **CF Ending Cash = BS Cash** for every historical year (small variance acceptable for restricted cash; document it).

---

## 3. Income Statement Structure

### 3.1 Universal Sections (apply to most companies)

**REVENUE**  
[Segment lines reproducing company’s actual disclosure]  
**Total Revenue**  [bold, $ format]  
**Revenue Growth %**  [italic memo]

**COST OF REVENUES**  
[Segment lines if disclosed; else single line]  
**Total Cost of Revenues**  
**Gross Profit**  [bold, $ format]  
**Gross Margin %**  [italic memo]

**OPERATING EXPENSES**  
- Research and development (if applicable)  
- Selling, general and administrative  
- Other (restructuring, impairment, etc.)  
**Total Operating Expenses**

**Operating Income (EBIT)**  [bold, $ format]  
**EBIT Margin %**  [italic memo]

**(+) Depreciation & Amortization**  [from PP&E schedule, GREEN link]  
**EBITDA**  [bold, $ format = EBIT + D&A]  
**EBITDA Margin %**  [italic memo]

**OTHER INCOME / EXPENSE**  
- Interest income  
- Interest expense  [from debt schedule, GREEN link]  
- Other income (expense), net

**Income before income taxes**  
**Provision for income taxes**  [=-MAX(0, EBT × tax rate)]  
**Effective Tax Rate %**  [italic memo]  
**Net Income (consolidated)**  [bold, $ format]  
**Less: Net income to NCI** (if applicable)  
**Net Income to Common Stockholders**  [bold, double bottom border]  
**Net Margin %**  [italic memo]

**PER SHARE DATA**  
- Basic shares outstanding  
- Diluted shares outstanding  
- Basic EPS  
- Diluted EPS

### 3.2 Universal IS Rules

- Show all operating expenses.
- D&A is sourced from the PP&E schedule (single source of truth). If the company reports D&A on the IS face, link the IS line to the PP&E schedule. If D&A is embedded in COGS / OpEx, pull it from the cash flow statement add-back.
- EBITDA must be shown as an explicit labeled row (EBIT + D&A), never implied.
- Interest expense is sourced from the debt schedule (single source of truth).
- Taxes are never negative. Use `=-MAX(0, EBT × tax_rate)`. If NOL carryforwards apply, deduct utilized NOLs from EBT before applying the rate.
- Each driver lives directly below the line item it drives. Drivers are pure green links to the Active block on Assumptions.
- Projection formulas: `Revenue = prior × (1 + local_growth_cell)`, `COGS = -Revenue × local_cogs_pct_cell`, etc.

---

## 4. Balance Sheet Structure

### 4.1 Universal Sections

**ASSETS**

**Current Assets**  
- Cash and cash equivalents  [PLUG: links from CF Ending Cash]  
- Short-term investments  [held flat or driver]  
- Accounts receivable, net  [from WC: DSO × Rev / 365]  
- Inventory  [from WC: DIO × COGS / 365]  
- Prepaid expenses and other current assets  [from WC: % × Rev]  
**Total Current Assets**

**Non-Current Assets**  
- Property, plant and equipment, net  [from PP&E schedule]  
- Operating lease right-of-use assets  [held flat or schedule]  
- Goodwill  [held flat unless modeling M&A]  
- Intangible assets, net  [from amortization if material]  
- Deferred tax assets  [held flat or schedule]  
- Other non-current assets  [held flat]  
**Total Assets**  [bold, $ format]

**LIABILITIES**

**Current Liabilities**  
- Accounts payable  [from WC: DPO × COGS / 365]  
- Accrued liabilities  [from WC: % × Rev]  
- Deferred revenue (current)  [from WC: % × Rev]  
- Current portion of debt  [from debt schedule]  
**Total Current Liabilities**

**Non-Current Liabilities**  
- Long-term debt  [from debt schedule × (1 - % current)]  
- Deferred revenue (LT)  [from WC]  
- Deferred tax liability  [held flat]  
- Other long-term liabilities  [held flat]  
**Total Liabilities**  [bold, $ format]

**EQUITY**  
- Redeemable noncontrolling interests  (if applicable)  
- Common stock  [held flat]  
- Additional paid-in capital  [= prior + SBC + stock issuance]  
- Treasury stock  [held flat unless modeling buybacks]  
- AOCI  [held flat]  
- Retained earnings  [from RE roll-forward]  
- Noncontrolling interests  [grow by NCI share of NI – distributions]  
**Total Equity**  [bold]

**Total Liabilities and Equity**  [bold, double bottom border]

**BS Check (TA - TL - TE)**  [italic, MUST = 0]

### 4.2 Universal BS Rules

- The BS itself contains **only links and subtotals**. No calculations on the face of the BS.
- Every BS line item links to its source schedule (WC, PP&E, debt, RE).
- Cash is the **only true plug** — it comes from CF Ending Cash.
- Goodwill is held flat unless an acquisition is being modeled.
- The **BS check row must equal zero**. If it doesn’t, root-cause it.

---

## 5. Cash Flow Statement Structure (Indirect Method)

### 5.1 Universal Sections

**CASH FLOW FROM OPERATIONS**  
**Net Income (consolidated)**  [from IS — full consolidated NI, not NI to common]

**Non-Cash Add-Backs:**  
- (+) Depreciation & Amortization  [from PP&E schedule]  
- (+) Stock-based compensation  [Rev × SBC % via local restate]  
- (+) Amortization of debt issuance costs  (if applicable, from debt schedule)  
- (+) Deferred taxes  (if applicable)  
- (+) Other non-cash adjustments  [historical hardcodes; 0 forward]

**Working Capital Changes:**  
- Δ Accounts receivable  [proj: -(end - begin) of BS AR]  
- Δ Inventory  [proj: -(end - begin) of BS Inv]  
- Δ Prepaid  [proj: -(end - begin)]  
- Δ Accrued and other liabilities  [proj: (end - begin) of AP + Accrued]  
- Δ Deferred revenue  [proj: (end - begin) curr + LT]

**Net Cash from Operating Activities**  [bold, $ format, SUM]

**CASH FLOW FROM INVESTING**  
- CapEx  [-PP&E schedule CapEx, negative]  
- Asset disposal proceeds  (if applicable, separate line, positive)  
- Acquisitions  [negative if modeled]  
- Purchases / maturities of investments  [historical; 0 forward unless modeled]  
**Net Cash from Investing Activities**  [bold, $ format, SUM]

**CASH FLOW FROM FINANCING**  
- (+) Debt borrowings  [from debt schedule]  
- (-) Debt repayments  [from debt schedule]  
- (+) Stock issuance proceeds  [Assumptions driver]  
- (-) Dividends paid  [Assumptions driver]  
- (-) Share buybacks  [if applicable]  
- Other financing activities  [historical; 0 forward unless modeled]  
**Net Cash from Financing Activities**  [bold, $ format, SUM]

**Effect of FX on cash**  [historical; 0 forward unless modeled]

**Net Change in Cash**  [bold, sum of CFO + CFI + CFF + FX]  
**Beginning Cash**  [year 1 hardcoded; else = prior period BS Cash]  
**Ending Cash**  [bold, double bottom border, = Begin + Δ; LINKS TO BS CASH]  
**Check: Ending Cash – BS Cash**  [italic, MUST = 0 in projection]

**FREE CASH FLOW**  
**FCF (CFO – CapEx)**  [bold, $ format]  
**FCF Margin %**  [italic memo]

### 5.2 Universal CF Rules

- CFO starts with consolidated NI, not NI to common.
- D&A and CapEx use the same cell references as the PP&E schedule.
- Working capital changes are computed as period-over-period BS deltas.
- Sign convention: asset increase = negative CFO; liability increase = positive CFO.
- Ending Cash links to BS Cash. This is the final step that balances the model.
- FCF (CFO – CapEx) is shown explicitly as a labeled row.

---

## 6. Supporting Schedules

### 6.1 PP&E Schedule

- Beginning Net PP&E  [= prior period Ending Net PP&E]  
- (+) CapEx  [hist: hardcoded; proj = Rev × CapEx %]  
- CapEx % of Revenue  [driver: link to Active block]  
- (-) Depreciation & Amortization  [proj = -avg net PP&E × D&A %; negative]  
- D&A % of Avg Net PP&E  [driver]  
- (±) Disposals (if applicable)  [hist hardcoded; 0 forward unless modeled]  
- **Ending Net PP&E**  [bold, sum]

### 6.2 Working Capital Schedule

- DSO (days), DIO (days), DPO (days), Prepaid % of Revenue, Deferred Rev % etc.  
- Projection rows are pure green links to Assumptions Active block.  
- Historical rows compute implied ratios from BS / IS line items × 365.

### 6.3 Debt Schedule

- Beginning Total Debt  
- (+) Debt issuances / borrowings  
- (-) Debt repayments  
- **Ending Total Debt**  
- % Current portion (next 12 mo.)  
- Avg Interest Rate %  
- Interest expense (calc)

### 6.4 Retained Earnings Roll-Forward

- Beginning Retained Earnings  
- (+) Net Income to Common  [from IS]  
- (-) Dividends  [Assumptions driver]  
- (+/-) Other adjustments  
- **Ending Retained Earnings**  [bold; LINKS TO BS RE LINE]

---

## 7. Sector Adaptations

The default structure fits ~80% of public companies. The following sectors require modifications:

### 7.1 Banks and Financial Institutions
- Replace COGS / Gross Profit with **Net Interest Income** (Interest Income – Interest Expense).
- Replace OpEx with **Non-Interest Expense**.
- Add **Provision for Loan Losses** as a separate line.
- Replace CFO add-backs with bank-specific items.
- Drivers: NIM, efficiency ratio, loan growth, deposit growth, credit cost.

### 7.2 Insurance Companies
- Replace Revenue with **Net Premiums Earned** + **Net Investment Income**.
- Replace COGS with **Net Losses and LAE Incurred** + **Acquisition Costs**.
- BS includes **Reserves for Losses**, **Unearned Premium Reserve**, **Reinsurance Recoverables**.
- Drivers: combined ratio, loss ratio, expense ratio, investment yield, premium growth.

### 7.3 REITs
- Show **Rental Revenue** + other property-level income.
- Add **FFO** and **AFFO** as supplemental measures.
- Heavy capitalization: model PP&E at gross + accumulated depreciation.

### 7.4 Subscription / SaaS
- Track **ARR**, **Net Revenue Retention (NRR)**, **Gross Revenue Retention (GRR)**.
- Capitalize internal-use software → CapEx outflow + amortization.

### 7.5–7.7 Other Sectors
(Retail, Heavy Industrial, Asset-light services) — detailed drivers and schedule adjustments as needed per company disclosure.

---

## 8. Sanity Checks

Run **all** checks. Each must pass before the model is delivered.

### 8.1 Balancing Checks
- BS Check = $0 every period  
- Cash Tie-out = $0 every projection period  
- NI link, RE roll, PP&E roll, Debt roll = $0 every period

### 8.2 Reasonableness Checks
- No hockey-stick revenue growth without driver  
- Terminal-year EBITDA plausible  
- Margins within historical ±500 bps unless justified  
- Terminal CapEx ≈ D&A  
- Tax rate 15–30% for most US corporates

### 8.3 Edge Case Checks
- 0% revenue growth → no broken formulas  
- Negative revenue growth → no #DIV/0! or tax issues  
- Negative EBITDA → tax line returns 0  
- All 3 scenarios + circ switch on → model still balances

---

**End of SPEC_methodology.md**