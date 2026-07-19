---
name: comparable-companies
description: When the user wants trading comps, a peer multiples analysis, or "how does X trade versus peers".
---
1. Establish the peer set (5-10 names). If the user did not supply one, propose peers by business mix and size — use `research` to confirm the closest public comparables, and state the selection logic in one line each.
2. Call `benchmark_peers` with the full ticker list (target + peers) to build the comparison workbook (revenue, margins, ROE, leverage).
3. For valuation multiples, pull current prices with `get_quote` and financials with `get_financials` using `basis: ltm` (real comps are LTM-based; EBITDA, total debt, and net debt come pre-computed); spread EV/EBITDA, EV/Revenue, and P/E. Keep the basis consistent across the set.
4. Compute min / 25th / median / 75th / max for each multiple. Flag outliers and say WHY they are outliers (growth premium, distress, pending deal) before excluding anything — never silently drop a peer.
5. Apply the peer median and 25th-75th range to the target's metric to get an implied valuation range vs. the current price.
6. Report as a table: peer set with multiples, summary stats, implied value range, and the target's premium/discount to the median with a one-line explanation of whether it is deserved.
