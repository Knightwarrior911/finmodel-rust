\# SPEC\_spreadsheet\_engineering.md - Spreadsheet Engineering Standards



This file defines the \*\*formula-level rules\*\* that govern every cell in the workbook. These rules are \*\*blocking\*\*; the validator checks them.



\---



\## 1. Font Color Equals Cell Type



Every data cell must follow this scheme. There are no exceptions.



| Cell content                          | Font color | Hex       |

|---------------------------------------|------------|-----------|

| Hardcoded number (no `=`)             | Blue       | `#0000FF` |

| Formula, same-sheet (no `!`)          | Black      | `#000000` |

| Formula, cross-sheet (contains `!`)   | Green      | `#008000` |



\*\*Rules:\*\*

\- If a cell is a number, it must be blue.

\- If a cell is a formula referencing only the current sheet, it must be black.

\- If a cell is a formula referencing another sheet (contains `!`), it must be green.

\- Do not use shades of blue (`#0070C0`, `#0066CC`, etc.). Only `#0000FF`.



This scheme lets a reader instantly distinguish inputs from formulas and from cross-tab links.



\---



\## 2. Formulas, Not Hardcodes



Every calculation must be an Excel formula using cell references. Never compute a value externally and write the result.



\- Subtotals, totals, net values, margins, EBITDA, net income, ending balances — these are always formulas.

\- The only hardcoded numbers allowed are true external inputs: user assumptions or historical actuals from filings.

\- Never hardcode a number inside a formula. Put it in its own cell and reference it.



When writing the model programmatically, always emit formula strings (`=B5\*1.05`) — never compute the value and write it as a number.



\---



\## 3. Two-Hop Pattern



A calculation must never reference the Assumptions tab directly inside arithmetic. The pattern is:



\*\*Step 1:\*\* Local cell on the statement tab is a pure green link to Assumptions  

e.g., `IS!G15 = =Assumptions!G60` (no math)



\*\*Step 2:\*\* Calculation cell uses only local cells (black)  

e.g., `IS!G14 = =F14 \* (1 + G15)` (no `!`)



\### Why

\- The `% of Revenue` or `growth rate` row is the green link.

\- The dollar-amount line item is the black formula.

\- The reader sees the assumption directly under the line item it drives. No need to switch tabs.

\- The validator can mechanically check the rule: any formula containing `Assumptions!` mixed with arithmetic operators (`\*`, `+`, `-`, `/`) is a FAIL.



\### Examples



\*\*WRONG:\*\*

\- `COGS = -H9 \* Assumptions!H34`

\- `Revenue = =F14 \* (1 + Assumptions!G60)`



\*\*RIGHT:\*\*

\- `COGS = -H9 \* H13`  (where H13 = Assumptions!H34)

\- `Revenue = =F14 \* (1 + G15)`  (where G15 = Assumptions!G60)



This pattern applies to every projection driver: revenue growth, COGS %, R\&D %, SG\&A %, CapEx %, D\&A %, working capital ratios, debt rate, tax rate, SBC %, dividend amount, and minimum cash balance.



\---



\## 4. Single Source of Truth



If a value appears on multiple tabs, it is a formula on every tab except one. The \*\*origin\*\* tab is the source of truth.



| Item                        | Origin                  | Referenced by |

|-----------------------------|-------------------------|---------------|

| D\&A                         | PP\&E schedule           | IS D\&A line, CF D\&A add-back |

| CapEx                       | PP\&E schedule           | CF CFI |

| Interest expense            | Debt schedule           | IS interest expense line |

| Net income (consolidated)   | IS                      | CF CFO starting line |

| Net income to common        | IS                      | BS RE roll-forward |

| Working capital balances    | BS RE items             | CF working capital change lines |

| SBC                         | CF tab                  | BS APIC walk |

| Cash                        | CF Ending Cash          | BS Cash |

| Total debt ending           | BS debt schedule        | BS current debt + LT debt |



Never independently calculate the same value on two tabs — they will drift over time as edits are made.



\---



\## 5. Citations



Every hardcoded input cell must have an Excel cell comment citing its source. Comments are added at the same time the value is written, never deferred.



\*\*Format:\*\*
"Description: cite:{citationId}"

text\*\*Examples:\*\*

\- `"Revenue FY2024 ($391.0B): cite:metric1"`

\- `"S\&P Capital IQ retrieved 2026-03-12: cite:src5"`

\- `"Industry growth rate (5.2%): cite:web3"`

\- `"Assumption – management guidance for 5% growth"` (no source = rationale)



\*\*Rules:\*\*

\- When a tool result includes a `citationId`, use the exact returned string.

\- Multiple sources in one comment: separate with `|`.

\- For user assumptions without a data source, use a rationale comment (no `cite:` prefix needed).

\- The validator flags any blue cell missing a comment as a FAIL.



\---



\## 6. Validator



After every build, run the validator. The validator checks:



| Check                  | What it verifies |

|------------------------|------------------|

| Font colors            | Every cell’s font color matches its type (blue/black/green) |

| Two-hop violations     | No formula mixes `Assumptions!` with arithmetic |

| Gridlines              | Every tab has gridlines off |

| Comments on inputs     | Every blue cell has a non-empty comment |

| Formula errors         | No `#REF!`, `#VALUE!`, `#DIV/0!`, `#N/A`, `#NAME?` |

| Root-cause errors      | Errors are categorized by root vs. propagated |

| Label colors           | Row labels use the standard label color, not data colors |

| Cell stats             | Counts blue / black / green cells; flags miscounts |



\*\*Validator output example:\*\*

```json

{

&#x20; "status": "success",

&#x20; "total\_errors": 0,

&#x20; "validation": { "failures": 0, "warnings": 0 }

}

Acceptance threshold:



Status must be "success".

Total errors must be 0.

Validation FAILS must be 0.



If the validator finds any FAIL, fix and re-run. Do not deliver until it passes.



7\. Common Errors to Avoid



Hardcoding a calculated value. A subtotal, ratio, or total must be a formula.

Hardcoding a growth rate, margin, or rate inside a formula. The number must be in its own cell.

Using shades of blue for inputs. Only #0000FF.

Mixing Assumptions! with arithmetic. Use the two-hop.

Calculating D\&A inline on the IS. D\&A originates in the PP\&E schedule.

Calculating interest on ending balance only when circ is on. Use average balance.

Letting Cash go negative. If projected cash is below the minimum, the revolver must draw to cover.

Wrong sign on working capital changes. Asset increase = -CFO; liability increase = +CFO.

Plugging the BS. Never insert a balancing row. Root-cause and fix.

Skipping cell comments. Every blue cell needs one, even cells set to 0.





8\. Workflow Around Validation

The full build cycle:



Build the model

Run validator

If FAILS > 0:

a. Read the failure messages

b. Fix the root cause (do not patch the validator output)

c. Re-run validator

d. Loop until FAILS = 0

Run scenario test (toggle each case)

Run reasonableness check

Deliver



A model is not done when the formulas are written. It is done when the validator passes and the scenario tests pass.



End of SPEC\_spreadsheet\_engineering.md

