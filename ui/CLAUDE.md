## Session (2026-07-19) v0.9.2–9.9 — Skill editor, settings tabs, visual refinement
- **Settings restructured** (settings.mjs): General / Connections / Memory / Skills
  tabs, roving tablist with arrow keys. Dialog widened to modal-card--wide (780px).
  selectSettingsTab(tab) export, openSettingsWithSkillDraft lands on Skills tab.
- **Skill editor** (settings.mjs loadSkillsList): each row has Edit button that opens
  inline SKILL.md editor via skills_get/skills_save. Renaming via frontmatter name
  deletes the old file. Use counts surfaced as skill-uses (mono/tabular); lifecycle
  state as tinted skill-state pill.
- **Financials card** (cards.mjs renderFinancials): multi-year spread with periods[]
  and rows[].values per-column; derived rows get class="fin-derived". Backward compat
  with old single-value cards.
- **Visual de-cartooning** (style.css): greeting 21px left-aligned, chips become
  6px rectangles, New chat btn gets accent-soft fill at rest, composer 10px radius,
  user bubbles/cards flat at rest (no shadows), all 999px border-radius converted to
  radius-sm. Memory pin badge: 📌 emoji replaced with SVG glyph (settings.mjs + css).
- **Thinking trail** (chat.mjs + style.css): boxed panel → hairline timeline with
  state nodes, measured durations stamped in mono, breathing dot live indicator,
  220ms step entrance, reduced-motion honored. sr-only utility class for a11y.
  animation: think-step-in, think-breathe.
- Tests: 143 jsdom tests (memory.test.mjs pin test updated from emoji assertion to
  SVG+aria-label; cards.test.mjs has multi-year spread + legacy fallback tests).
  
# ui — finmodel desktop frontend (vanilla ES modules, no build step)

Chat-first, claude.ai-style. `index.html` (3-region grid) + `style.css` +
`js/*.mjs` loaded via `<script type="module" src="js/main.mjs">`. Served by Tauri
over its custom protocol (CSP `script-src 'self'`, `font-src 'self' data:`).
NO framework, NO bundler — edit the `.mjs` files directly.

## Module map (`js/`)
- `core.mjs` — `call(name,payload)` (invoke wrapper; every command returns a JSON
  string), `$`, `on(event,handler)` (Tauri event subscribe), `escapeHtml`,
  `renderMarkdown` (sanitized: headings/p/ul/ol/fenced code/GFM tables/http links —
  escape-first, no raw HTML), `stripControlTokens`, `domainOf`, `openExternal`,
  `openPath`, `flashBtn`, `copyToClipboard`, theme fns (`initTheme`, `currentTheme`,
  `setTheme`, `toggleTheme`, `themeChoice`), formatters (`fmt*`, `relTime`).
- `sidebar.mjs` — conversation list, new chat, inline rename, delete, collapse
  (persist `localStorage.sidebar`), theme toggle (sun/moon).
- `chat.mjs` — composer + streaming send flow + message render. Listens
  `chat_delta`/`chat_tool`/`chat_done`/`chat_reset`/`build_progress`. Single-flight
  routes ALL events to the current `activeTurn` (only one turn at a time). Live
  assistant node gets `.streaming` (caret) — removed on `finalizeLive`, which also
  strips control tokens and renders markdown. `chat_reset` clears a fabricated draft.
- `cards.mjs` — `renderCard(card)` by `card.type`: `model`, `benchmark`, `search`
  (row → reader), `page`, `news`, `deal`, `quote`, `filings`, `assumptions`
  (interactive grid → `finalize_model`), `error`. Cards are the ONLY card treatment.
- `reader.mjs` — right slide-in panel; `read_page` result rendered by `status`
  (`ok`→markdown, `blocked`/`thin`→honest prompt, never a dead end). Esc closes.
- `analyst.mjs` — Analyst-tools modal (Phase 6.5): EV / IFRS / tie-out forms →
  `ev_bridge`/`ifrs_bridge`/`tie_out` commands, launched from the model card.
  Each submit is one selected action (never a flat tool list); focus-trapped dialog.
- `settings.mjs`, `update.mjs` — modal + footer updater. `main.mjs` — boot wiring.

## Design language (binding — professional finance, no AI slop)
- Tokens live in `style.css :root` (light) + `[data-theme="dark"]`. Extend tokens,
  NEVER hardcode colors elsewhere. One indigo accent used sparingly; hairline borders
  over shadows; no gradients/glassmorphism/emoji in chrome.
- Fonts: **IBM Plex Sans** (UI) + **IBM Plex Mono** (`--font-num`) bundled in
  `fonts/*.woff2`. ALL figures/tickers/table numerics use `.num` (tabular-nums).
- Assistant prose is **cardless** on `--canvas`; user messages are right-aligned
  `--element` bubbles; tool results are the only cards.
- Type scale 13/14/15/16/20/24; 4px spacing grid; chat column max-width 780px.
- a11y: semantic `nav/main/aside/button`, visible `:focus-visible` ring, `aria-live`
  on the streaming message, Esc closes reader + modal, WCAG AA in both themes.
- Copy = utility not marketing. BANNED strings: "unlock", "experience the",
  "seamless", "supercharge", exclamation marks in chrome.
- `[hidden]{display:none!important}` is enforced globally — needed because class
  rules (e.g. `.modal{display:flex}`) otherwise beat the `[hidden]` attribute.

## Testing
ES modules are blocked over `file://` — serve `ui/` over HTTP and mock
`window.__TAURI__` (incl. `event.listen` capturing handlers so the test can fire
`chat_delta`/`chat_tool`/`chat_done`/`chat_reset`). `node --check js/*.mjs` for syntax.
Grep guard (must be 0 hits): `buildHeading|benchHeading|searchHeading|toolcard|tool-card|demoChips|modeBanner`.
