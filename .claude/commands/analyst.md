# /analyst — Virtual Financial Analyst (Research Mode)

Invoke the Python research agent for ad-hoc IB research.
This command handles: company profiles, M&A deal search, earnings analysis,
peer comparison, precedent transactions, and general company intelligence.

## Usage

```
/analyst research: <company> <what you need>
/analyst deal: <target> [acquirer]
/analyst profile: <company>
/analyst earnings: <company> <period>
/analyst ifrs: <company> <year> [direction]
```

## How it works

1. Detects query type from natural language
2. For US companies: SEC EDGAR XBRL API (direct, no browser, instant)
3. For non-US companies: browser-use via your Chrome CDP (real browser, no bot detection)
4. Cross-verifies findings from 2+ sources
5. Synthesizes with domain KB rules (EV bridge, IFRS, sector frameworks)

## Critical Rules (from OpenClaw KB)

These are non-negotiable. Apply always:

1. **Pension = NOTES section only (R-015)** — never use balance sheet XBRL tag. Find PBO, plan assets from pension footnote. Formula: max(0, PBO − Plan Assets).
2. **Shares = latest filing weighted average basic (F-001)** — not period-end from annual report.
3. **IFRS 16**: Only ROU Depreciation + Lease Interest. Short-term rent = NOT adjustment item.
4. **EV bridge is a checklist** — only include items that are present, material, disclosed.
5. **All financials from company filings** — never Yahoo Finance/Bloomberg/Refinitiv for financial figures.
6. **Goodwill = NOT subtracted (R-014)** — acquired businesses still operating.
7. **Two-source verification** — every fact must be confirmed by 2+ independent sources.

## Browser Setup

For non-US companies and company IR sites, browser-use connects to your Chrome.
Ensure Chrome is running with remote debugging:

```
chrome --remote-debugging-port=9222
```

Or use browser-use with profile (close Chrome first):
```
browser-use --profile "vinit" --headed
```

## Source Priority

| Company Type | Primary Source | Method |
|---|---|---|
| US-listed (NYSE/NASDAQ) | SEC EDGAR XBRL API | Direct HTTP |
| US-listed ADR | SEC EDGAR (20-F, 6-K) | Direct HTTP |
| UK (LSE) | Companies House + LSE RNS | Browser CDP |
| EU (Euronext) | AMF + company IR | Browser CDP |
| India (BSE/NSE) | BSE/NSE filings + company IR | Browser CDP |

## Execution

When invoked:
1. Run: `python -m src.research.agent "<query>" [--ticker TICKER] [--company NAME]`
2. If SEC API fails → fall back to browser for company IR
3. If browser fails → Google search → alternate URL → try again
4. Cross-verify → apply KB rules → synthesize → deliver
