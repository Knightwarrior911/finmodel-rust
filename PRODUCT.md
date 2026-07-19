# Product

## Register

product

## Users

Finance professionals — investment-banking analysts, equity researchers, and modelers — running real valuation work (DCF, comps, LBO screens, earnings reviews) on a Windows desktop. They are in a task: building or checking a model, chasing a number, assembling a deliverable. They know Excel and EDGAR; they do not want to fight a UI while thinking about WACC.

## Product Purpose

finmodel is an agentic financial-analyst desktop app (Tauri). The analyst chats, calls tools (EDGAR financials, filings, research, model builds), follows reusable skills, and produces evidence-backed models and answers. Success: the user trusts a number enough to put it in front of a client, because the app showed where it came from.

## Brand Personality

Friendly, guiding, approachable. A patient senior colleague who shows their work — never a black box, never a bureaucrat. Warmth comes from clarity and guidance, not decoration.

## Anti-references

- **Cramped modal-heavy UI.** Narrow dialogs, features bolted on without workflow thought, editing squeezed into 520px.
- **Generic SaaS dashboard.** Card grids, hero metrics, gradient accents.
- **Consumer fintech gloss.** Robinhood-style playful gradients, confetti, gamification.

## Design Principles

1. **Show your work.** Every number traceable; evidence sits beside the conversation, not behind it.
2. **Guide, don't gate.** Prefer inline and progressive disclosure over modals-on-modals; teach through empty states and hints.
3. **Numbers are sacred.** Tabular numerals, mono for figures and code, exact values, stated periods.
4. **Room to read.** Editing and reading surfaces get the width they need; density is chosen, cramping never is.
5. **One vocabulary.** The same row, button, and editor patterns everywhere; a feature isn't done until it uses them.

## Accessibility & Inclusion

Keyboard-reachable everything (documented shortcut legend); `:focus-visible` styling; `prefers-reduced-motion` honored globally; status conveyed as glyph + text, never color alone; light and dark themes with `color-scheme` set.
