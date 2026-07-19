---
name: orchestrator
description: When a request spans multiple companies, workstreams, or deliverables and the work must be coordinated into one consistent output.
---
1. Split the request into independent workstreams (per company, per analysis type). For each, decide the owning skill: `earnings-analysis`, `comparable-companies`, `dcf-valuation`, `credit-analysis`, etc.
2. Fix the shared conventions BEFORE running any workstream, so results can merge: same fiscal period basis, same currency, same metric definitions (e.g. operating income as the EBITDA proxy everywhere). Write them down once.
3. Run each workstream via `use_skill` for its owning skill, feeding it the shared conventions. Between workstreams, carry over only conclusions and cited figures — not raw dumps.
4. After all workstreams complete, reconcile: the same company's revenue must be identical everywhere it appears; a valuation range from comps must be compared against the DCF, not reported in isolation. Any contradiction between workstreams must be resolved or explicitly flagged — never ship both numbers without comment.
5. Synthesize one deliverable in the user's requested format, ordered by decision relevance (answer first, evidence after).
6. Close with a coverage check against the original request: every asked-for item is either delivered or explicitly listed as not deliverable and why.
