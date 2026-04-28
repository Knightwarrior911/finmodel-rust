\# Build Public Comps - Master Build Instructions



\*\*Name:\*\* `build-public-comps`  

\*\*Description:\*\*  

Build a public trading comparables analysis in Excel. Output a single `.xlsx` with Cover, Peer Set, Market Data, Operating Stats, Multiples, and Summary Stats tabs. Produces grouped peer multiples (EV/Revenue, EV/EBITDA, P/E forward and trailing), operating benchmarks, and an implied valuation range for the target.



\---



\## 0. Mission



Identify a peer set, pull market and operating data for each peer, calculate trading multiples, and apply the peer-set summary statistics (median, mean, high, low) to the target’s metrics to produce an implied valuation range.



Public comps are \*\*reference-class valuation\*\*: what the public market is paying for similar businesses today. The output is sensitive to peer-set selection and to the metric being benchmarked. Both decisions must be defensible.



\---



\## 1. Inputs Required from User



| Input                        | Default if not specified |

|------------------------------|--------------------------|

| Target company name and ticker | \*(must be provided)\* |

| Peer set                     | Auto-suggest 8-15 peers from same industry, similar size, similar geography. User confirms |

| As-of date                   | Today |

| Currency                     | Target’s reporting currency; convert peers if needed |

| Calendarization              | To a common year-end (typically Dec 31) |

| Multiples to show            | EV/Revenue, EV/EBITDA, P/E (LTM, NTM, FY+1, FY+2 where relevant) |

| Output path                  | `/workspace/outputs/{TICKER}\_Public\_Comps.xlsx` |



\---



\## 2. Specs to Load



| # | File | What it governs |

|---|------|-----------------|

| 1 | `public\_comps/SPEC\_methodology.md` | Peer selection, market data, operating stats, multiples calc, summary stats |

| 2 | `shared/SPEC\_spreadsheet\_engineering.md` | Font color rules, two-hop pattern, validator |

| 3 | `shared/SPEC\_excel\_formatting.md` | Tab layout, columns, number formats |

| 4 | `shared/SPEC\_modeling\_patterns.md` | Cross-tab linking |



\---



\## 3. Workflow



\### Step 1 – Resolve company and select peers

\- Get `rogoCompanyId` and confirm sector/industry.

\- Auto-suggest and present 8-15 peers. User confirms final set.

\- Document rationale for inclusion/exclusion on Peer Set tab.



\### Step 2 – Pull market data

For each peer:

\- Current share price

\- 52-week high / low / % off high

\- Shares outstanding (basic and diluted)

\- Market cap (price × diluted shares)

\- Total debt (most recent BS)

\- Cash and equivalents

\- Preferred (if applicable)

\- NCI, Preferred

\- \*\*Enterprise Value\*\* = Market Cap + Net Debt + NCI + Preferred



\### Step 3 – Pull operating stats

For each peer:

\- LTM Revenue, EBITDA, EBIT, Net Income, EPS

\- Forward (FY+1, FY+2) consensus estimates: Revenue, EBITDA, EPS

\- Margins: Gross %, EBITDA %, EBIT %, Net %

\- Growth rates: LTM revenue growth, NTM revenue growth, 2-year CAGR



\### Step 4 – Calculate multiples

For each peer:

\- EV / LTM Revenue

\- EV / NTM Revenue

\- EV / FY+1 Revenue

\- EV / FY+2 Revenue

\- EV / LTM EBITDA

\- EV / NTM EBITDA

\- EV / FY+1 EBITDA

\- P/E LTM

\- P/E NTM

\- P/E FY+1

\- P/E FY+2



Calendarize off-cycle fiscal years to a common period.



\### Step 5 – Summary statistics

For each multiple, compute across the peer set:

\- Min, 25th percentile, Median, Mean, 75th percentile, Max

\- Standard deviation (optional)

\- Filter outliers if median diverges meaningfully from mean



\### Step 6 – Tier the comp set (optional)

\- "Tier 1" peers (closest comparables) – show separately

\- "Tier 2" peers (broader sector) – show separately



\### Step 7 – Apply to target

\- For each multiple type, multiply target’s metric by the median (or mean) to produce an implied EV.

\- Bridge implied EV to implied equity value to implied per-share price.

\- Show range: low = 25th percentile, high = 75th percentile (or min/max for wider band).



\### Step 8 – Validate

\- All peers have market data and operating stats.

\- All multiples calculated consistently (same EV definition, same time period).

\- Currency conversion applied consistently.

\- Outliers reviewed and either explained or excluded.



\---



\## 4. Must-Follow Rules



In addition to `shared/SPEC\_spreadsheet\_engineering.md`:



\- Peer set selection must be documented with rationale.

\- Use \*\*same definition of EV\*\* across all peers (e.g., always include or always exclude operating lease liabilities; always include or always exclude pension underfunding).

\- Use \*\*diluted shares\*\*, not basic, for market cap.

\- Calendarize off-cycle peers to the common period (CY).

\- Currency convert at as-of-date FX rate, applied consistently.

\- Forward estimates from the same data source (e.g., FactSet consensus, Bloomberg consensus) – don’t mix.

\- Outliers (e.g., negative EBITDA, P/E > 100x, EV/Revenue > 50x) flagged and either excluded with rationale or shown but excluded from summary stats (use NM – "not meaningful").

\- Show a "current trading vs. 52w high" column for each peer – gives context on whether the peer set is trading near peaks or troughs.



\---



\## 5. Final Acceptance



| Check | Required |

|-------|----------|

| Peer set documented (8-15 peers with rationale) | yes |

| All peers have market data and operating stats | yes (or NM with reason) |

| EV definition consistent across peers | yes |

| Multiples calculated for LTM and forward periods | yes |

| Summary stats (min, 25th, median, mean, 75th, max) shown | yes |

| Implied valuation range computed for target | yes |

| Currency / calendarization applied consistently | yes |

| All hardcoded inputs commented with citation | yes |



\*\*Deliver only when all checks above are satisfied.\*\*



\---



\*\*End of build-public-comps Master Build Instructions\*\*

