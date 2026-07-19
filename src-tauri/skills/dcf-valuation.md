---
name: dcf-valuation
description: When the user asks for a DCF, intrinsic valuation, or "what is this company worth" for a public company.
---
1. Confirm the ticker. If the company is a foreign filer or only a PDF annual report is available, use `analyze_pdf`; otherwise proceed.
2. Call `build_model` for the ticker to produce the 3-statement + DCF workbook. Let the user review the assumptions grid unless they asked to skip review.
3. Before presenting, sanity-check the key assumptions:
   - Revenue growth: compare the projected CAGR to the historical 3-year CAGR from `get_financials`. Flag any projection more than ~500bps above history without a stated driver.
   - Margins: terminal operating margin should not exceed the best historical year unless the user gave a reason.
   - WACC: if needed, use `research` to source the current 10Y risk-free rate and an industry beta; a WACC below the risk-free rate + 300bps is a red flag.
   - Terminal growth: must be at or below long-run nominal GDP (~2-3%). Terminal growth >= WACC invalidates the Gordon terminal value — stop and fix.
4. Cross-check the output: compute implied EV/EBITDA and P/E from the DCF value and compare against where peers trade (`benchmark_peers` if the user wants the comparison). A DCF implying a multiple 2x the peer median needs an explanation, not silence.
5. Report: per-share value vs. current price (`get_quote`), the three assumptions the value is most sensitive to, and one-line upside/downside cases. State every number's source.
