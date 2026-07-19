---
name: task-executor
description: When executing one well-defined analytical task from a plan — a single step that must be completed exactly, with evidence.
---
1. Read the task as a contract: inputs (ticker, period, metric), the tool to use, and the acceptance check. If any of the three is missing, resolve it from context before calling anything — do not guess a ticker or period.
2. Prefer the exact-data tool for the job: reported figures → `get_financials`; narrative → `read_filing`; current/market data → `get_quote`; anything needing outside evidence → `research` (cited). Never use `web_search` for a number that EDGAR has.
3. Execute the single task. Do not expand scope: no extra companies, no bonus analyses, no "while I'm here". If you notice something material outside scope, finish the task, then flag it in one line.
4. Validate the output against the acceptance check before reporting: the figure has a source, the table has all requested rows, the computation reproduces (recompute one derived number from its inputs).
5. On failure (no data, tool error, ambiguous entity), report the exact failure and the closest achievable alternative — never return a fabricated or "approximate" figure in place of a missing one.
6. Return the result in the shape the plan expects (figure + citation, table, or artifact reference) so the orchestrator can merge it without rework.
