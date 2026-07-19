---
name: ma-accretion-dilution
description: When the user asks whether an acquisition is accretive or dilutive, or wants a merger consequences / deal math screen.
---
1. Identify acquirer and target. Pull both companies' net income and diluted EPS with `get_financials`, and current prices/share counts with `get_quote`. Use `research_deal` for announced terms if a real deal exists; otherwise ask for (or assume and state) an offer premium, default 30%.
2. Set the consideration mix (cash / stock / debt) — from deal terms or user input. State the financing assumptions explicitly: cost of new debt (source current yields via `research`), foregone interest on cash, exchange ratio for stock.
3. Compute pro forma EPS:
   - Combined net income = acquirer NI + target NI + after-tax synergies (only if the user provides or approves a synergy figure — never invent one) − after-tax incremental interest − foregone interest income.
   - Pro forma shares = acquirer shares + new shares issued (stock consideration ÷ acquirer price).
4. Accretion/(dilution) = pro forma EPS ÷ acquirer standalone EPS − 1. Show the math line by line so it can be checked.
5. Sanity checks: 100% stock deal where the acquirer trades at a higher P/E than the deal P/E of the target should be accretive — if your result contradicts that rule, recheck before reporting. Compute the breakeven synergies that make the deal EPS-neutral.
6. Report: sources & uses, pro forma EPS bridge, accretion % in year 1, breakeven synergies, and the caveat that EPS accretion is not value creation.
