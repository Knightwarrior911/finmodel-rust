---
name: finmodel
description: Agentic financial-analyst desktop app — two-face warm palette (cream editorial light / near-black terminal dark), one accent per theme, evidence-forward.
colors:
  cream-canvas: "#f7f7f4"
  cream-chrome: "#f1f0eb"
  cream-element: "#e6e5e0"
  raised-white: "#ffffff"
  warm-ink: "#26251e"
  warm-muted: "#5a5852"
  warm-faint: "#807d72"
  hairline: "#e6e5e0"
  action-orange: "#f54e00"
  action-orange-ink: "#b83c00"
  orange-wash: "#fde5da"
  dark-canvas: "#201d1d"
  dark-element: "#302c2c"
  dark-ink: "#fdfcfc"
  signal-blue: "#339cff"
  blue-wash: "#1e2a3c"
  ok-green: "#1f8a65"
  warn-amber: "#d97706"
  err-red: "#cf2d56"
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
    backgroundColor: "{colors.action-orange}"
    textColor: "#ffffff"
    rounded: "{rounded.sm}"
  modal-card:
    backgroundColor: "{colors.raised-white}"
    rounded: "{rounded.lg}"
---

# Design System: finmodel

## 1. Overview

**Creative North Star: "The Patient Analyst"**

finmodel's interface is a senior colleague who shows their work: calm warm surfaces, ONE accent voice per theme for action and selection, hairline structure instead of boxes, and numbers set in mono with tabular figures. Light mode is a cream editorial page (Cursor-inspired) with a single orange voltage; dark mode is a warm near-black terminal (OpenCode-inspired) with a single signal blue. Warmth comes from clarity and guidance — helpful hints, teaching empty states, inline editing where you already are — not from decoration.

The system explicitly rejects cramped modal-heavy UI (editing squeezed into narrow dialogs), generic SaaS dashboard tropes (card grids, hero metrics, gradient accents), and consumer fintech gloss (playful gradients, confetti). It is friendly by being legible, guiding, and unhurried.

**Key Characteristics:**
- Warm-tinted canvas (`#f7f7f4` cream light / `#201d1d` near-black dark) — never pure black or white
- One accent per theme — action orange (light) / signal blue (dark) — for primary actions, selection, and citations only; text uses the AA-readable ink variant (#b83c00 on cream)
- Hairline borders (`#e6e5e0` light / `#3a3636` dark) carry structure; shadows are reserved for true overlays
- IBM Plex Sans for UI, IBM Plex Mono + tabular-nums for anything numeric
- Soft and tactile components: rounded, generous touch targets, quiet hover fills

## 2. Colors

A warm paper-and-ink ramp with a single accent per theme; semantic colors appear only as state.

### Primary
- **Action Orange** (#f54e00 fills, #b83c00 as text — light) / **Signal Blue** (#339cff — dark): primary actions, active selection, citation markers, focus. Never decoration; its rarity is its authority. Small orange TEXT always uses the ink variant — the raw fill orange fails AA on cream.

### Neutral
- **Cream Canvas** (#f7f7f4): the page. **Cream Chrome** (#f1f0eb): sidebars and toolbars. **Cream Element** (#e6e5e0): hover fills and inset fields. **Raised White** (#ffffff): overlays and cards. Dark: #201d1d canvas, #282424 chrome/raised, #302c2c element.
- **Warm Ink** (#26251e / #fdfcfc dark): primary text. **Muted** (#5a5852 / #9a9898): secondary. **Faint** (#807d72 / #6e6e73): tertiary/hints. **Hairline** (#e6e5e0 / #3a3636): all structural lines.
- Semantic: **OK** #1f8a65/#30d158, **Warn** #d97706/#ff9f0a, **Error** #cf2d56/#ff453a (light/dark) — always paired with a glyph or word, never color alone.

### Named Rules
**The One Voice Rule.** Each theme has exactly one voice of action (orange in light, blue in dark). If two accents compete on a surface, one is wrong.
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
- **Don't** introduce a second accent or use the accent decoratively.
