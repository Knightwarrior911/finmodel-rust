---
name: verification-loop
description: When the user wants verified, trustworthy output — or after any multi-step analysis — to prove the numbers and claims before delivery.
---
Run this loop until clean or twice at most; report honestly if issues remain.
1. EXTRACT: list every factual claim and number in the draft answer, each with its claimed source. A claim with no source goes straight to the fix list.
2. VERIFY ANCHORS: re-fetch the 3 most decision-relevant figures from the primary source — reported financials via `get_financials`, prices via `get_quote`, deal terms via `research_deal`. Compare digit-for-digit against the draft. Never verify a number against the same secondary source that produced it.
3. RECOMPUTE: rerun all derived math (growth, margins, multiples, EPS accretion, IRR) from the verified anchors. Sanity-order checks: terminal growth < WACC, pro forma shares >= acquirer shares in stock deals, leverage ratios use the same EBITDA basis everywhere.
4. CROSS-EXAMINE: for the single most important conclusion, seek disconfirming evidence with `research` (e.g. "the market disagrees because…", a filing disclosure that cuts against it). If disconfirming evidence exists, the conclusion must acknowledge it.
5. FIX: correct every failed item at its root (the wrong input, not the visible symptom) and re-derive everything downstream of the fix.
6. CERTIFY: deliver with a short verification note — which figures were verified against which primary source, what was recomputed, what remains unverified (and why). Unverified claims stay visibly labeled; certainty is never manufactured.
