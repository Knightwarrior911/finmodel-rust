---
name: company-profile
description: When the user wants a company one-pager, profile, overview, or banker-style tearsheet.
---
1. Business description: `read_filing` item 1 for what the company actually does — segments, products, geographic mix, customers. Summarize in 3-5 lines, no boilerplate.
2. Financial snapshot: `get_financials` for 3 years (add `basis: ltm` when current run-rate matters); for segment mix, `read_filing` item 8 segment note of revenue, operating income, net income, diluted EPS; compute growth and margins. `get_quote` for current price and market context.
3. Valuation snapshot: current P/E and EV-based multiples versus 2-3 closest peers (`benchmark_peers` when the user wants the full comparison, otherwise `get_quote` + `get_financials` on peers).
4. Recent developments: `get_news` for the last quarter's headlines; keep only items that change the thesis (deals, guidance, management, regulation).
5. Key risks: `read_filing` item 1A, distilled to the 3 risks that are specific to this company (skip generic macro/cyber boilerplate).
6. Assemble the one-pager in fixed order: Business — Financials table — Valuation — Recent developments — Risks — with every figure cited to its source. Keep it to one screen.
