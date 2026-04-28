# SPEC_methodology.md - Public Comps Methodology

This file defines the **methodology** for public trading comparable analysis.

---

## 1. Tab Structure

| Tab              | Content |
|------------------|---------|
| **Cover**        | Target overview, peer set rationale, summary output (implied valuation) |
| **Peer Set**     | Peer list with company name, ticker, country, market cap, EV; tier groupings |
| **Market Data**  | Per-peer: share price, 52w high/low, shares, market cap, debt, cash, EV |
| **Operating Stats** | Per-peer: LTM and forward revenue, EBITDA, EBIT, NI, EPS; margins; growth rates |
| **Multiples**    | Per-peer multiples grid; summary statistics row at the bottom |
| **Summary Stats**| Median / mean / range by multiple, with implied target valuation |
| **Trading History** (optional) | NTM EV/EBITDA over 1Y / 3Y / 5Y for each peer |

---

## 2. Peer Set Selection

### 2.1 Selection Criteria

A defensible peer set is the difference between credible comps and noise.

- **Same sector and sub-industry** (e.g., "athletic apparel" or "specialty retail")
- **Similar size**: 0.3× to 3× the target’s revenue or market cap
- **Similar geography**: ideally same primary market; flag global vs. regional split
- **Similar business model**: subscription vs. one-time, B2B vs. B2C, asset-heavy vs. asset-light
- **Similar growth profile**: high-growth peers shouldn’t anchor a mature business’s comps
- **Listed on similar exchange / liquid trading**: avoid micro-caps and OTC names
- **Public for ≥ 1 year**: pre-IPO and just-IPO’d names have unstable trading

### 2.2 Documenting the Peer Set

On the Cover tab, list each peer with:
- Why included (1-line rationale)
- Why any obvious peer was excluded (1-line rationale)

This document is the legal and analytical defense of the comp.

### 2.3 Tier Groupings

Often useful to split peers into:
- **Tier 1**: Closest 5-8 comparables (most weight in summary stats)
- **Tier 2**: Broader sector (5-8 more, sanity check)
- **Aspirational / target peers**: companies the target wants to be compared to (premium positioning)

Show summary stats separately for each tier.

---

## 3. Market Data Block

**Per peer:**
- Ticker
- Country
- Currency
- Share Price (as of date)
- 52-week High
- 52-week Low
- % off 52-week High  `[= 1 – Price / High; useful context]`
- Shares Outstanding (diluted)  [million]
- Market Cap  `[= Price × Diluted Shares]`
- (+) Total Debt  [from most recent BS]
- (-) Cash & ST Investments
- (+) Preferred (if applicable)
- (+) Noncontrolling Interest
- **Enterprise Value**  [bold]

**Notes:**
- Use as-of date consistently across all peers.
- Diluted shares: include in-the-money options, RSUs, convertibles, warrants (treasury method).
- For NCI, use fair value if disclosed; otherwise book.
- Operating lease liabilities: be consistent — either include in debt OR exclude. Don’t mix.

---

## 4. Operating Stats Block

**Per peer:**
- Reporting Currency
- FY end (calendar month)
- Calendarized to common year-end if needed (see Section 6)

**LTM:**
- Revenue, EBITDA, EBIT, Net Income, Diluted EPS

**NTM / Forward:**
- NTM Revenue (consensus), NTM EBITDA (consensus), NTM EPS (consensus)
- FY+1 Revenue, EBITDA, EPS (consensus)
- FY+2 Revenue, EBITDA, EPS (consensus)

**Margins (LTM):** Gross %, EBITDA %, EBIT %, Net %  
**Growth rates:** LTM revenue growth, NTM revenue growth, 2-year CAGR

**Notes:**
- Use one consensus source for all peers (FactSet, Bloomberg, S&P CapIQ). Don’t mix.
- For non-calendar fiscal years, show calendarized estimates.
- Show "NM" for negative EBITDA, EPS, or P/E.

---

## 5. Multiples Calculation

### 5.1 Standard Multiples Grid

**Per peer:**
- EV / LTM Revenue
- EV / NTM Revenue
- EV / FY+1 Revenue
- EV / FY+2 Revenue
- EV / LTM EBITDA
- EV / NTM EBITDA
- EV / FY+1 EBITDA
- EV / FY+2 EBITDA
- P / E LTM
- P / E NTM
- P / E FY+1
- P / E FY+2
- EV / EBIT (less common)
- PEG ratio (P/E / Growth) — sometimes useful for high-growth peers

### 5.2 Sector-specific Multiples

| Sector                  | Common Multiples |
|-------------------------|------------------|
| SaaS / Subscription     | EV / NTM ARR, EV / NTM Revenue, Rule of 40 |
| Banks                   | P / TBV, P / E, ROE × P/B |
| REITs                   | P / FFO, P / AFFO, NAV multiple |
| Insurance               | P / BV, Combined Ratio comp |
| Energy / Commodities    | EV / EBITDax, EV / Production |
| Industrials             | EV / EBITDA, P / E, ROIC comp |

### 5.3 Calculation Rules
- Multiple = EV / Metric (or Price / EPS).
- For EBITDA-based multiples: match lease treatment with EV definition.
- Use diluted EPS for P/E.
- LTM as of most recent reported quarter.
- NTM consensus = weighted quarterly estimates if mid-year.

### 5.4 NM (Not Meaningful) Treatment
Mark as NM if:
- Denominator is negative (negative EBITDA, negative EPS).
- Denominator too small (P/E > 100x usually NM).
- Multiple is so far from the peer set’s range it distorts the median.

NM cells are excluded from summary statistics.

---

## 6. Calendarization

If a peer’s fiscal year ends mid-year, calendarize estimates to the common year-end (typically Dec 31):

**Calendarized CY2025 =** (months of FY2025 in CY2025 / 12) × FY2025 estimate  
+ (months of FY2026 in CY2025 / 12) × FY2026 estimate

Apply consistently to revenue, EBITDA, and EPS estimates.

---

## 7. Summary Statistics

For each multiple, compute across the peer set (excluding NMs):

- Min, 25th percentile, **Median** (bold), Mean, 75th percentile, Max
- StdDev (optional)
- Count (non-NM)

**Display rules:**
- Median is the primary statistic.
- Mean is a sanity check.
- 25th–75th percentile = inter-quartile range ("typical" valuation band).
- Highlight the median row.

---

## 8. Implied Valuation for Target

For each multiple where the target has the relevant metric:

| Multiple              | Peer Stat     | Target Metric       | Implied EV | Implied Equity | Implied $/Share |
|-----------------------|---------------|---------------------|------------|----------------|-----------------|
| EV / LTM Revenue      | [stat]        | [target rev]        | [calc]     | [calc]         | [calc]          |

**Equity bridge for each implied EV:**
- Implied EV
- (-) Net Debt (target)
- (-) Preferred (target)
- (-) NCI (target)
- **Implied Equity Value**
- ÷ Diluted Shares Outstanding (target)
- **Implied Per-Share Price**

**Range and base case:**
- Low end: 25th percentile multiple × low end of target metric
- High end: 75th percentile multiple × high end of target metric
- Base case: median multiple × base case target metric

Display as a "football field" range (horizontal bar).

---

## 9. Trading History (Optional)

Show NTM EV/EBITDA over time for each peer (1Y / 3Y / 5Y).  
Include peer set median and target (if public).  
This shows whether the peer set is trading near peaks, troughs, or in line with history.

---

## 10. Sector Adaptations

### 10.1 Banks and Financial Institutions
- Use P / TBV and P / E primarily.
- ROE × P/B regression for implied P/B.

### 10.2 REITs
- P / FFO, P / AFFO, NAV multiple primary.

### 10.3 SaaS / High-growth Software
- EV / NTM ARR, EV / NTM Revenue, Rule of 40.
- LTV / CAC and net retention rates as operating benchmarks.

### 10.4 Energy / Commodities
- EV / EBITDax, EV / Production.

### 10.5 Insurance
- P / BV, Combined Ratio.

---

## 11. Common Errors

- Mixed EV definitions across peers.
- Mixed share count basis (basic vs. diluted).
- Stale market data or mixed consensus sources.
- Currency not converted.
- Calendarization missed for off-cycle peers.
- Outliers included in summary stats.
- Peer set chosen to flatter rather than reflect actual comparables.
- Forward multiples used without growth context.
- Not showing range (single-point valuation hides distribution).

---

**End of SPEC_methodology.md (Public Comps)**