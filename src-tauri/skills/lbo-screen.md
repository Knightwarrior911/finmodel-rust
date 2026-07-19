---
name: lbo-screen
description: When the user asks if a company is an LBO candidate, wants a quick leveraged buyout screen, or sponsor-returns math.
---
1. Pull the target's 3-year spread with `get_financials` (revenue, operating income → proxy EBITDA, operating cash flow, capex, FCF, LT debt, net cash — growth and FCF pre-computed) and market pricing with `get_quote`. Use `read_filing` (MD&A / risk factors) to check cash-flow stability and existing debt commentary.
2. Screen the candidate qualitatively first: stable cash flows, low existing leverage, modest capex needs, no structural decline. Say plainly if it fails the screen and why.
3. Entry assumptions (state each): entry EV = current EV + control premium (default 25-30%); leverage = 4.5-6.0x EBITDA depending on stability (source current LBO debt market tone via `research` if the user wants precision); rest is sponsor equity.
4. Project a simple 5-year case: revenue growth at historical rate, flat-to-modest margin improvement, EBITDA less capex/taxes/interest → free cash flow sweeps down debt each year. Keep the model to ~6 lines/year and show it.
5. Exit at entry multiple (base case). Sponsor equity value = exit EV − remaining net debt. Compute MOIC and IRR. Run the 3x3 sensitivity: exit multiple ±1.0x by leverage ±1.0x.
6. Report: sources & uses, debt paydown schedule, IRR/MOIC grid, and the 2-3 assumptions the return most depends on. A base-case IRR below ~15% means the price, not the model, is the problem — say so.
