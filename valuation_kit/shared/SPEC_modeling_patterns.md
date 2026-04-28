\# SPEC\_modeling\_patterns.md - Modeling Patterns Reference

\## This file defines the \*\*structural patterns\*\* that govern how assumptions, scenarios, and cross-tab linkages are organized in the workbook.

\## 1. Assumptions Tab Structure

\### 1.1 Layout



\*\*Row 9\*\*: Case Toggle  \[D9: blue input – 1=Base, 2=Upside, 3=Downside]

\*\*Row 10\*\*: Active Case  \[D10: =CHOOSE(D9, "Base", "Upside", "Downside")]

\*\*Row 12\*\*: ACTIVE CASE block header  \[Sand-04 fill]

\*\*Row 13\*\*: Year headers

\*\*Rows 14-N\*\*: ACTIVE CASE drivers – formulas use CHOOSE to pull from scenarios

\*\*Row N+2\*\*: BASE CASE block header

\*\*Rows N+3\*\*: Year headers

\*\*Rows N+4 to M\*\*: BASE CASE drivers (hardcoded blue inputs)

\*\*Row M+2\*\*: UPSIDE CASE block header

\*\*Row P+2\*\*: DOWNSIDE CASE block header



\### 1.2 What Lives on Assumptions

The Assumptions tab contains:



\*\*Toggle cell and Active Case label\*\* (rows 9-10)

\*\*Active Case block\*\* – formulas pulling from the appropriate scenario (this is what statement tabs read)

\*\*Three scenario blocks\*\* (Base / Upside / Downside) – hardcoded blue inputs

\*\*Forward-looking drivers only\*\* – growth rates, margins, ratios, scenario-specific values

\*\*No historical data\*\*. Historicals live on the statement tabs.

\*\*No calculated outputs.\*\* Revenue, COGS, EBITDA – these belong on statement tabs.



\### 1.3 What does NOT live on Assumptions



Historical line items (those live on IS/BS/CF)

Computed dollar amounts (those are on statement tabs)

Scenario-driven calculations (those happen on statement tabs reading from the Active block)





\## 2. Active Case Block Pattern

The Active Case block is the bridge between scenario blocks and the statement tabs. It uses CHOOSE:

\*\*Active row formula:\*\*

excel=CHOOSE($D$9, base\_block\_cell, upside\_block\_cell, downside\_block\_cell)

 

Example for revenue growth in 2026E (column G): Active!G14 = =CHOOSE($D$9, G46, G75, G104)

Where:



\* G46 = Base case 2026E revenue growth (hardcoded blue input)



\* G75 = Upside case 2026E revenue growth (hardcoded blue input)



\* G104 = Downside case 2026E revenue growth (hardcoded blue input)



Rules



\* The toggle cell ($D$9) is absolute-referenced so the same formula works across all driver rows.



\* Each scenario block has the same row structure as the Active block.



\* Active Case block sits above the scenario blocks. The user sees the live assumptions first.



\* Scenario blocks contain only projection-period drivers. Do not repeat historical data in scenario blocks.



3\. Restate Placement (the two-hop)

Every projection driver follows this pattern on the statement tab:



\* Row N: Line item  \[black formula using local cells only]



\* Row N+1: Driver (% or growth or rate)  \[green pure link to Assumptions Active]



Example – Revenue projection on IS



\* Row 14: Automotive sales  \[hardcoded D,E,F; proj G..K =F14\*(1+G15)]



\* Row 15: Growth %  \[hist =E14/D14-1 etc.; proj: =Assumptions!G14]



Example – COGS as % of Revenue



\* Row 28: Automotive COGS  \[hist: hardcoded; proj: =-G28\_revenue\*G29]



\* Row 29: % of Revenue  \[hist: calculated; proj: =Assumptions!G19]



Why this matters



\* The reader sees the assumption directly under the line item. No need to switch tabs.



\* The validator can mechanically check that the calculation is a black formula referencing only local cells.



\* Changing a driver on Assumptions flows automatically through every line item via the local restate.



Restate placement rules



\* The driver row sits immediately under the line item it drives (not in a separate driver block).



\* The driver row is italicized to visually distinguish it from the line item label.



\* The driver label is indented one level deeper than the line item label.



\* Historical cells in the driver row calculate the implied ratio (e.g., COGS hist / Revenue hist).



\* Projection cells in the driver row are pure green links to the Active block on Assumptions.



4\. Single-Case Models (no toggle)

If the user explicitly opts out of the Base/Upside/Downside toggle, the Assumptions tab contains only one block – the Active block – with hardcoded blue inputs (no CHOOSE). All other rules still apply.

5\. Multi-Segment Line Items

If a line item has multiple segments or components (e.g., revenue by product line, COGS broken into materials + labor + hosting):



\* Show each component as its own sub-line item with its own driver directly below.



\* Do not stack multiple drivers under a single aggregated line.



\* Bolded total row sums the components.



Example:

text

REVENUE

  Automotive sales          \[own driver: growth %]

  Energy generation         \[own driver: growth %]

  Services and other        \[own driver: growth %]

Total Revenue             \[bold, sum]

textEach segment gets its own driver. The total revenue line is a sum.

6\. Driver Nesting Depth

Keep driver detail on the statement to one level of indentation.

If a driver requires a sub-buildup (e.g., users = beginning users + new users – churned users), put that sub-driver buildup on a supporting tab and reference the output back into the statement.

The statement should show the key assumption driving each line item, not the full backup.

7\. Cross-Tab Linking

When a model has both financial statements (IS / BS / CF) and an analysis tab (LBO, DCF, comps):



\* The analysis tab links to the statement tabs. It does not hardcode values that exist elsewhere.



\* Interest expense, D\&A, CapEx, NWC changes, Net Income, and debt balances on an analysis tab must be green cross-sheet references to the source.



\* If a value appears on multiple tabs, it is a formula on every tab except one (the source of truth).



8\. Active Case Display on Statement Tabs

Every tab driven by the scenario toggle must display the active case name near the top so the user always knows which case is running:



\* Row 9 of every statement tab: C9: "Active Case:"  \[bold ink label] D9: =Assumptions!$D$10  \[green cross-sheet – displays "Base" / "Upside" / "Downside"]



This is a pure link, no math. It tells the reader at a glance which scenario is active.

9\. Toggles and Switches

9.1 Case toggle



\* Single cell on Assumptions (e.g., Assumptions!D9).



\* Values: 1 = Base, 2 = Upside, 3 = Downside.



\* Hardcoded blue input.



\* Comment: Case toggle: 1 = Base, 2 = Upside, 3 = Downside.



9.2 Circularity switch



\* Single cell on the IS tab (e.g., IS!D10).



\* Values: 0 = off, 1 = on.



\* When off (0): interest expense uses beginning balance × rate.



\* When on (1): interest expense uses average balance × rate.



\* Always start with 0 during build. Flip to 1 only after all three statements balance.



\* Excel users must enable iterative calculation when circ is on.



9.3 Other toggles



\* Place near the top of the relevant section.



\* Always make the toggle obvious (clear label, blue input cell).



10\. Sources and Uses, Pro Forma Capital Structure

These belong on a Transaction Summary or LBO tab. Never on the Income Statement.

The IS contains only the P\&L and its assumptions.

Format S\&U with sources on the left, uses on the right, both in one row per item, with % of Total and Multiple of EBITDA columns inline.

11\. Visual Separation of Assumptions vs. Line Items

Assumption rows must be visually distinguishable from line item rows:



\* Assumption rows: italic, lighter visual weight, indented further than line items.



\* Line item rows: regular weight (bold for subtotals).



\* Cross-sheet green color on assumption rows usually provides sufficient distinction.



Without visual separation, a reader cannot quickly distinguish "this is the COGS dollar amount" from "this is the COGS % of Revenue assumption".

12\. Common Anti-Patterns



\* Scenario CHOOSE formulas on statement tabs. Always belongs on Assumptions.



\* Restate cells that don’t feed any formula. Every restate must be consumed by at least one calculation.



\* Direct Assumptions reference inside arithmetic. Use the two-hop.



\* Same value calculated independently on two tabs. Pick one source of truth.



\* Drivers stacked in a separate block far from the line items. Drivers go directly under the line item.



\* Calculated outputs on the Assumptions tab. Outputs belong on statement tabs.



\* Historical data in scenario blocks. Scenario blocks are projection-only.



\* Hardcoded growth rate inside a formula. Put it in its own cell.



\* Active block placed below scenario blocks. Active goes on top.



\* No circ switch on a model that has interest-on-debt circularity. Always include the switch.



End of SPEC\_modeling\_patterns.md

