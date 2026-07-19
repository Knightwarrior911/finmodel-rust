---
name: finmodel
description: Agentic financial-analyst desktop app — warm-neutral canvas, one indigo accent, evidence-forward.
colors:
  paper-white: "#fdfdfc"
  warm-chrome: "#f6f5f4"
  linen-element: "#efedea"
  raised-white: "#ffffff"
  night-ink: "#21222a"
  slate-muted: "#6e6c78"
  faint-gray: "#9a98a1"
  hairline: "#e7e6e3"
  ledger-indigo: "#4338ca"
  ledger-indigo-strong: "#3730a3"
  indigo-wash: "#e7e7fb"
  ok-green: "#16a34a"
  warn-amber: "#d97706"
  err-red: "#dc2626"
typography:
  body:
    fontFamily: "IBM Plex Sans, Segoe UI, Inter, system-ui, sans-serif"
    fontSize: "14px"
    fontWeight: 400
    lineHeight: 1.35
  title:
    fontFamily: "IBM Plex Sans, Segoe UI, Inter, system-ui, sans-serif"
    fontSize: "16px"
    fontWeight: 600
  label:
    fontFamily: "IBM Plex Sans, Segoe UI, Inter, system-ui, sans-serif"
    fontSize: "13px"
    fontWeight: 500
  data:
    fontFamily: "IBM Plex Mono, Cascadia Code, Consolas, ui-monospace, monospace"
    fontSize: "13px"
    fontWeight: 400
rounded:
  sm: "6px"
  md: "8px"
  lg: "12px"
spacing:
  xs: "4px"
  sm: "8px"
  md: "16px"
  lg: "20px"
components:
  button-ghost:
    textColor: "{colors.night-ink}"
    rounded: "{rounded.sm}"
  button-primary:
    backgroundColor: "{colors.ledger-indigo}"
    textColor: "#ffffff"
    rounded: "{rounded.sm}"
  modal-card:
    backgroundColor: "{colors.raised-white}"
    rounded: "{rounded.lg}"
---

# Design System: finmodel

## 1. Overview

**Creative North Star: "The Patient Analyst"**

finmodel's interface is a senior colleague who shows their work: calm warm-neutral surfaces, one deep indigo voice for action and selection, hairline structure instead of boxes, and numbers set in mono with tabular figures. Warmth comes from clarity and guidance — helpful hints, teaching empty states, inline editing where you already are — not from decoration.

The system explicitly rejects cramped modal-heavy UI (editing squeezed into narrow dialogs), generic SaaS dashboard tropes (card grids, hero metrics, gradient accents), and consumer fintech gloss (playful gradients, confetti). It is friendly by being legible, guiding, and unhurried.

**Key Characteristics:**
- Warm-neutral canvas (`#fdfdfc` light / `#1b1c22` dark), tinted — never pure black or white
- One accent: ledger indigo, for primary actions, selection, and citations only
- Hairline borders (`#e7e6e3`) carry structure; shadows are reserved for true overlays
- IBM Plex Sans for UI, IBM Plex Mono + tabular-nums for anything numeric
- Soft and tactile components: rounded, generous touch targets, quiet hover fills

## 2. Colors

A warm paper-and-ink neutral ramp with a single indigo accent; semantic colors appear only as state.

### Primary
- **Ledger Indigo** (#4338ca, dark: #8b85f1): primary actions, active selection, citation markers, focus. Never decoration; its rarity is its authority.

### Neutral
- **Paper White** (#fdfdfc): the canvas. **Warm Chrome** (#f6f5f4): sidebars and toolbars — the second neutral layer. **Linen Element** (#efedea): hover fills and inset fields. **Raised White** (#ffffff): overlays and cards.
- **Night Ink** (#21222a): primary text. **Slate Muted** (#6e6c78): secondary text. **Faint Gray** (#9a98a1): tertiary/hints. **Hairline** (#e7e6e3): all structural lines.
- Semantic: **OK** #16a34a, **Warn** #d97706, **Error** #dc2626 — always paired with a glyph or word, never color alone.

### Named Rules
**The One Voice Rule.** Indigo is the only voice of action. If two accents compete on a surface, one is wrong.
**The Tinted Neutral Rule.** No `#000`, no untinted grays; every neutral leans warm.

## 3. Typography

**Body Font:** IBM Plex Sans (with Segoe UI, Inter, system-ui)
**Label/Mono Font:** IBM Plex Mono (with Cascadia Code, Consolas)

**Character:** A workmanlike grotesque with just enough personality; Plex Mono makes every figure feel sourced from a ledger.

### Hierarchy
- **Title** (600, 16px): modal and section headings.
- **Body** (400, 14px, 1.35): default UI text; prose runs to ~70ch max.
- **Label** (500, 13px): field labels, buttons.
- **Data** (400 mono, 13px, `font-variant-numeric: tabular-nums`): every number, ticker, code span, and citation ref.

### Named Rules
**The Sacred Number Rule.** Numbers are always mono + tabular. A proportional figure in a table is a bug.

## 4. Elevation

Hybrid, weighted to flatness: hairline borders carry structure at rest; shadows exist only to mean "this floats above the page." Depth at rest is conveyed by the neutral ramp (chrome vs. canvas vs. raised), not by shadow.

### Shadow Vocabulary
- **Whisper** (`0 1px 2px rgba(33,34,42,0.05)`): inputs, subtle lift.
- **Pop** (`0 4px 14px rgba(33,34,42,0.10)`): menus, popovers.
- **Modal** (`0 12px 32px rgba(33,34,42,0.16)`): dialogs only.

### Named Rules
**The Overlay-Only Rule.** If it doesn't float, it doesn't cast.

## 5. Components

Soft and tactile: rounded corners, generous padding, quiet hover fills — friendly to the hand without becoming toy-like.

### Buttons
- **Shape:** rounded-sm (6px)
- **Primary:** Ledger Indigo fill, white text; used at most once per view.
- **Ghost:** transparent, ink text, hairline on hover; the workhorse for row actions.
- **Hover / Focus:** Linen Element fill on hover; 2px indigo `:focus-visible` ring.

### Rows (lists of memories, skills, filings)
- **Style:** full-width, hairline bottom divider, no card wrapper; leading text truncates, trailing ghost actions.
- **State:** hover fill Linen Element; badges as tinted pills with text.

### Cards / Containers
- **Corner Style:** 8px (12px for modals)
- **Background:** Raised White on canvas; hairline border
- **Shadow Strategy:** none at rest (see Elevation)
- **Internal Padding:** 16–20px; editing surfaces get width before chrome

### Inputs / Fields
- **Style:** hairline stroke, canvas background, 6px radius; mono for code-like content (SKILL.md, formulas)
- **Focus:** indigo border shift + soft ring
- **Error:** err-red border + text message below, never color alone

### Navigation
- Sidebar on Warm Chrome (272px), quiet rows, indigo-tinted active state; keyboard shortcuts surfaced in a legend.

### Signature Component: Evidence chips
Citation refs (`[1]`, ticker pills, tool chips) set in mono, indigo-strong on indigo-wash; they make provenance a visible texture of the app.

## 6. Do's and Don'ts

### Do:
- **Do** give editing surfaces room: dialogs that host editors widen (min 760px on desktop) rather than squeeze.
- **Do** disclose progressively: expand inline (details, inline editors) before reaching for another layer.
- **Do** set every number in Plex Mono with tabular-nums and a stated period.
- **Do** pair every status color with a glyph or word.
- **Do** honor `prefers-reduced-motion` and keep transitions 150–250ms ease-out.

### Don't:
- **Don't** build cramped modal-heavy UI — no editing in narrow dialogs, no modal spawned from a modal.
- **Don't** ship generic SaaS dashboard tropes: card grids, hero metrics, gradient accents.
- **Don't** add consumer fintech gloss: gradients, confetti, gamified anything.
- **Don't** use side-stripe borders, gradient text, or glassmorphism.
- **Don't** introduce a second accent or use indigo decoratively.
