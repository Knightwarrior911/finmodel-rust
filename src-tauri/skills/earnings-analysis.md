---
name: earnings-analysis
description: When the user asks about a company's earnings, latest results, "how did X do", or a results deep-dive.
---
1. Call `get_financials` for the ticker — exact reported revenue, gross profit, operating income, net income, diluted EPS from EDGAR XBRL. These are the anchor numbers; do not substitute press-release or news figures for them.
2. Compute YoY growth and margins (gross, operating, net) for the last 3 fiscal years. Build a one-line margin bridge: which line item drove the operating margin change.
3. Call `read_filing` for MD&A (item 7) to get management's stated drivers — pricing vs. volume, cost inflation, one-offs. Attribute each big move to a stated cause, or mark it unexplained.
4. Call `get_news` for the ticker to capture market reaction and any guidance commentary; use `research` if the user asks about consensus expectations (beat/miss needs a cited consensus figure).
5. Separate signal from noise: recurring operating performance vs. one-time items (impairments, gains on sale, tax effects). Restate an adjusted view only if you can cite what you adjusted.
6. Report: results table (3yr), margin bridge, 3 drivers with citations, and what to watch next quarter.
