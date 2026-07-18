# GOAL — Finmodel: the agentic virtual financial analyst

## North star
Turn **finmodel** (local-first Windows Rust/Tauri desktop app) from a "basic app that
runs one tool and replies" into an **AI-era reimagined, agentic virtual financial
analyst** — where using it *feels like talking to a capable analyst*, not filling a
form. Target the conversational agency of **openclaw** and **Hermes Agent** and the
harness rigor of **Oh My Pi** and **Grok Build**, applied to finance. It is **not** a
coding assistant — it is a virtual investment-banking analyst.

## Definition of done — "feels like talking to an agent"
1. **Plans out loud** — shows a short live plan for non-trivial asks and streams progress per step.
2. **Follows through, multi-step, autonomously** — chains tools toward the goal
   (research → extract → model → verify → synthesize); stops only for a real
   decision/approval, never a lazy "want me to continue?".
3. **Rich live tool activity** — animated per-tool status ("Reading TSLA 10-K…",
   "Peers 3/5…"), correlated results, structured result cards.
4. **Just answers** — precise, sourced figures; no hedging ("2025 Tesla sales" →
   "$94.83B, FY2025 10-K").
5. **Remembers** — preferences, deal/company context, corrections; workspace-scoped,
   recalled with provenance.
6. **Parallelizes** — independent work (peer sets, multi-company screens) fans out via subagents.
7. **Any model, any provider** — user brings their own key (OpenAI-compatible:
   OpenRouter / OpenAI / xAI-Grok / Anthropic / Gemini / DeepSeek / Groq / Mistral / …).
   No subscription-OAuth (ToS/account-ban risk for a sold product).
8. **Never invents numbers** — finmodel's Rust engines (SEC EDGAR XBRL, model builder,
   comps, DCF, PPTX) are the calculation/artifact backend; the LLM only plans, selects
   tools, and synthesizes.

## Reference repos — study the *agentic* patterns; reimplement in Rust/vanilla-JS (no upstream code, no Bun runtime; respect licenses)
- **https://github.com/can1357/oh-my-pi** (MIT) — event-driven agent loop, tool-call
  correlation, layered cancellation, tail-buffered streaming, animated tool-activity
  blocks, token-efficient compact tool summaries, first-class subagents, provider catalog.
- **https://github.com/xai-org/grok-build** (Apache-2.0) — single-owner conversation
  actor, durable turn/run events, dangling-tool repair, typed tool registry, crash-safe replay.
- **https://github.com/openclaw/openclaw** — flexible tool-calling harness + the
  conversational "talking to an agent" experience to match.
- **https://github.com/NousResearch/hermes-agent** — agentic conversational feel and autonomy to match.
- **https://github.com/anthropics/financial-services** (Apache-2.0) — finance workflow
  taxonomy + least-privilege orchestrator/leaf-worker pattern.
- **https://github.com/nexu-io/open-design** (Apache-2.0) — UI/interaction polish
  reference; adapt tastefully, keep the restrained editorial-finance aesthetic (not a
  design-tool look).

## Constraints
- Local-first Windows desktop; no Node/Bun sidecar, no hosted memory/vector server.
  SQLite (FTS5) is the store.
- Own-key providers only (OpenAI-compatible); no provider OAuth/subscription login.
- Every material number traces to a tool result / primary source, with visible provenance.
- Ship signed auto-updating releases; verify each change live in the running app before
  claiming done.

## Already shipped (v0.6.0 → v0.7.2 — foundation)
Unified agent loop; SQLite persistence; parallel tool execution; token-efficient tool
summaries; manual memory (save + recall); `get_financials` (exact SEC XBRL income
statement); filing-reader fix; live auto-scroll; multi-provider bring-your-own-key; UI polish.

## Next milestones (toward the north star)
1. **Visible plan + streamed step-by-step progress** in chat — the biggest lever on the
   openclaw/Hermes feel.
2. **Multi-step follow-through** — remove "want me to…?" punts; complete the analysis.
3. **Rich live tool-activity cards** wired into the live transcript (OMP activity surface).
4. **Subagent fan-out** surfaced in the UI (parallel peer/company work + task tray).
5. **Automatic memory** (once the precision gate is met) + a memory-management drawer.
