\# Build DCF Model - Master Build Instructions



\*\*Name:\*\* `build-dcf-model`  

\*\*Description:\*\*  

Build a discounted cash flow valuation model in Excel for any public or private company. Output a single `.xlsx` with a Cover, Assumptions, DCF, WACC, and Sensitivities tab; produces an implied enterprise value, equity value, and per-share price target with a sensitivity table for WACC × terminal growth and WACC × exit multiple.



\---



\## 0. Mission



Produce an \*\*intrinsic valuation\*\* of the target company using projected unlevered free cash flows discounted at WACC.  

\- Show both the \*\*perpetuity-growth\*\* and \*\*exit-multiple\*\* terminal value methods.  

\- Bridge enterprise value to equity value to per-share price.  

\- Provide sensitivity tables.  



The DCF is an opinion expressed as math: every assumption (growth, margin, WACC, terminal growth, exit multiple) drives a price. The model must let the user change any one and see the impact propagate.



\---



\## 1. Inputs Required from User



| Input                        | Default if not specified |

|------------------------------|--------------------------|

| Target company name and ticker | \*(must be provided)\* |

| Projection period            | 5 years (10 for high-growth or pre-cash-flow companies) |

| Terminal value method        | Both: perpetuity growth AND exit multiple – show side by side |

| Source of UFCF               | Build inline OR link from an existing 3-statement model |

| WACC method                  | CAPM with comparable company beta median |

| Output path                  | `/workspace/outputs/{TICKER}\_DCF\_Model.xlsx` |



If linking from a 3-statement model, ask the user for the file path. Otherwise build UFCF from scratch on the DCF tab.



\---



\## 2. Specs to Load



| # | File | What it governs |

|---|------|-----------------|

| 1 | `dcf/SPEC\_methodology.md` | UFCF buildup, terminal value, WACC, equity bridge, sensitivities |

| 2 | `shared/SPEC\_spreadsheet\_engineering.md` | Font color rules, two-hop pattern, validator rules, citations |

| 3 | `shared/SPEC\_excel\_formatting.md` | Tab layout, columns, number formats, year conventions, colors |

| 4 | `shared/SPEC\_modeling\_patterns.md` | Assumptions tab, restate placement, cross-tab linking |



\---



\## 3. Workflow



\### Step 1 – Resolve company and gather inputs

\- Get `rogoCompanyId` via `companyLookup`. Confirm sector (sector affects valuation method; banks and REITs use different approaches).

\- Pull the most recent 10-K for historicals.

\- Pull or compute: current share price, share count (basic and diluted), total debt, cash, NCI, preferred, market cap.

\- Identify a peer set (5-8 comparables) for beta and capital structure benchmarking.



\### Step 2 – Build the WACC tab

1\. Pull each peer’s beta (5-year monthly), capital structure (D/E, D/V), tax rate.

2\. Unlever each peer’s beta: `Bu = Be / (1 + (1 - t) × D/E)`

3\. Take the median (or mean) unlevered beta of the peer set.

4\. Re-lever to the target’s capital structure: `Be\_target = Bu\_median × (1 + (1 - t) × (D/E)\_target)`

5\. Compute cost of equity via CAPM: `Ke = Rf + Be × ERP`

6\. Compute after-tax cost of debt: `Kd\_after\_tax = Kd × (1 - t)`

7\. Compute WACC: `WACC = (E/V) × Ke + (D/V) × Kd\_after\_tax`



\### Step 3 – Build the DCF tab

1\. Project revenue, EBIT, taxes, D\&A, CapEx, working capital changes for the explicit forecast period.

2\. Compute UFCF = `EBIT × (1 - t) + D\&A – CapEx – ΔNWC` for each year.

3\. Compute discount factor for each year using mid-year convention: `DF\_t = 1 / (1 + WACC)^(t - 0.5)`.

4\. Compute PV of explicit UFCFs.

5\. Build terminal value via both methods:

&#x20;  - Perpetuity growth: `TV = UFCF\_terminal+1 / (WACC – g)`

&#x20;  - Exit multiple: `TV = EBITDA\_terminal × Exit Multiple`

6\. Discount TV to present.

7\. Sum: PV of UFCFs + PV of TV = \*\*Enterprise Value\*\*.



\### Step 4 – Equity bridge

\- EV – Total Debt – Preferred – NCI + Cash + Investments = Equity Value

\- Equity Value / Diluted Shares Outstanding = Implied Per-Share Price

\- Implied vs. Current Price = Upside / Downside %



\### Step 5 – Sensitivities

\- Build two 2D sensitivity tables: WACC × Terminal Growth, and WACC × Exit Multiple.

\- Output per-share implied price for each combination.

\- Highlight the base case cell.



\### Step 6 – Validate and stress-test

\- Run validator (per `SPEC\_spreadsheet\_engineering`).

\- Verify implied price is within a sensible range of current price (typically ±50%; outliers warrant scrutiny).

\- Verify terminal value as % of EV is reasonable (typically 60-80% for stable businesses).

\- Stress test: WACC at +/-100 bps, terminal g at +/-50 bps. No formula breaks.



\### Step 7 – Document and deliver

\- Cover tab: company, valuation date, share price reference, WACC, terminal g, exit multiple, implied per-share, % upside/downside.

\- Chat response: cite each major assumption (WACC, terminal g, exit multiple) and the implied price.



\---



\## 4. Must-Follow Rules



These are blocking. Apply in addition to the rules in `shared/SPEC\_spreadsheet\_engineering.md`.



\- WACC must be reasonable for the sector (typically 6%-14% for most sectors; flag if outside this range).

\- Terminal growth rate must be ≤ long-run nominal GDP growth (typically 2.0%-3.5%). Never above 4%.

\- The two terminal value methods (perpetuity growth and exit multiple) must produce results within \~25% of each other. If they diverge by more than that, one of them is mis-calibrated.

\- Show implied per-share price in dollars and percentage upside / downside vs. current.

\- Mid-year discounting unless the user specifies year-end.

\- Terminal year UFCF must reflect a steady-state business: CapEx ≈ D\&A, working capital growth in line with revenue.

\- All shares in the equity bridge must be diluted (treasury method) — basic shares understate dilution.



\---



\## 5. Final Acceptance



| Check | Required |

|-------|----------|

| Validator status | success |

| Validation FAILS | 0 |

| Formula errors | 0 |

| WACC in plausible range (6-14%) | yes |

| Terminal g ≤ 3.5% | yes |

| TV methods within \~25% of each other | yes |

| Implied price flows through to per-share with sensitivities | yes |

| Sensitivity tables update when base WACC or growth changes | yes |

| Cell comments on all hardcoded inputs | yes |



\*\*Deliver only when all checks above are satisfied.\*\*



\---



\*\*End of build-dcf-model Master Build Instructions\*\*

