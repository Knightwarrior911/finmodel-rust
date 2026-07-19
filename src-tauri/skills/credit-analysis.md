---
name: credit-analysis
description: When the user asks about a company's credit quality, leverage, debt capacity, or bond/loan risk.
---
1. Pull `get_financials` for the issuer: revenue, operating income, net income, 3-year trend. Use `read_filing` MD&A and risk factors for debt structure commentary, maturities, covenants, and liquidity (revolver size, cash).
2. Compute the core credit ratios and show the inputs: gross leverage (debt/EBITDA-proxy), interest coverage (operating income/interest expense), and margin stability across the 3 years. If a needed input (interest expense, total debt) is not in the XBRL summary, get it from the filing text via `read_filing` and cite the section — do not estimate silently.
3. Benchmark against 3-5 sector peers with `benchmark_peers` (leverage and ROE columns matter most here). Position the issuer: better/worse than the peer median and why.
4. Check the trajectory, not just the level: is leverage rising into margin pressure (deteriorating) or falling with stable margins (improving)? Use `get_news` and `research` for rating-agency actions and recent refinancing activity.
5. Qualitative overlay: cyclicality, customer concentration, secured vs. unsecured mix, near-term maturity wall.
6. Report: ratio table with peer comparison, trajectory verdict (improving/stable/deteriorating), the single most likely stress path, and what would change the view. Cite every figure.
