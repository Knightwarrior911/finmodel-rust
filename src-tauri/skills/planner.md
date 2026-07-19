---
name: planner
description: When the user gives a broad or multi-part request (a full analysis, a pitch, "look into X") that needs decomposition before any tool is called.
---
1. Restate the deliverable in one sentence and get the scope boundaries explicit: which company/companies, which period, what output format (answer, table, workbook, profile).
2. Decompose into 3-7 concrete steps, each mapped to a specific tool or skill from the catalog (e.g. financials → `get_financials`, valuation → `dcf-valuation` skill, peers → `comparable-companies` skill). A step with no tool behind it is an assumption — label it.
3. Order the steps by data dependency: anchor numbers first (`get_financials`), context second (`read_filing`, `research`), synthesis last. Never plan a synthesis step before its inputs exist.
4. For each step note the acceptance check: what output proves the step succeeded (a cited figure, a workbook artifact, a peer table).
5. Show the plan to the user in <10 lines ONLY if the request was ambiguous or expensive (multi-model builds); otherwise state the plan in 2 lines and execute it immediately.
6. While executing, keep the plan current: mark steps done, and if a step fails (no data, foreign filer, ambiguous entity) adapt the plan and say what changed — do not silently drop a step.
