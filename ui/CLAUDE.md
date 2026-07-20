## Session (2026-07-20 theme) v0.9.24 — two-face palette
- style.css tokens only (no layout churn): light = Cursor cream editorial
  (canvas #f7f7f4, chrome #f1f0eb, element/hairline #e6e5e0, raised #fff,
  ink #26251e/#5a5852/#807d72, accent #f54e00 FILLS-ONLY, accent-strong
  #d04200, --accent-ink #b83c00 for TEXT — raw orange is ~3.3:1 on cream,
  fails AA); dark = OpenCode near-black terminal (canvas #201d1d, chrome/
  raised #282424, element #302c2c, hairline #3a3636, ink #fdfcfc/#9a9898/
  #6e6e73, accent #339cff, strong #66b5ff, on-accent #10131a, dark HIG
  semantics 30d158/ff9f0a/ff453a).
- NEW TOKEN --accent-ink: every color: var(--accent) text role swapped to
  it (26 sites); border/outline/background/spinner stay --accent. Sources:
  getdesign.md/design-md/{cursor,opencode.ai}/DESIGN.md (palettes only —
  their all-mono/72px display identities deliberately NOT adopted).
- DESIGN.md contract rewritten: one accent per theme, One Voice Rule kept.

## Session (2026-07-20 later) v0.9.23 — previews, power prompt, settings pickers, copy sweep
- composer.mjs: image chips carry data-URL thumbnails (blob: URLs are
  CSP-blocked — img-src 'self' data:); 40px .attach-thumb. Multi-image
  paste: per-paste screenshotSeq suffix (same-second collisions), plural
  hint. refineDraft(mode) — #refineBtn opens #refineMenu (Quick tidy /
  Power prompt → refine_prompt mode param); Escape/click-away close;
  hint-action undo restores the draft.
- settings.mjs: modelCatalogList/providerBaseList datalists (populated from
  list_models when has_key + static PROVIDERS) attached to synthesisModel,
  worker/verifier model + provider-base inputs; edinetKey field (blank
  keeps saved). Capability copy humanized ('can use tools ✓, reliable
  tables ✓'); model options '· uses tools · sees images'.
- index.html: refineMenu markup; SEC email → 'Contact email for SEC
  downloads (optional)' + honest hint; 'baseline files — coming soon'
  removed; roles placeholders 'saved key name (advanced — usually blank)'.
- style.css: .refine-menu/.refine-opt, .field-check flex fix (the
  .field-row input{flex:1} rule stretched checkboxes), reduced-motion.
- Tests: 194 jsdom (power-prompt mode, data-URL thumb, two-screenshot
  paste).

## Session (2026-07-20) v0.9.22 — composer multimodal, prompt polish, spending settings
- **composer.mjs** (new module): model picker popover on the pill (filterModels
  multi-term, arrow/Enter/Escape, list_models 5-min cache), attachment chips
  (paperclip, OS drag-drop via pdf_drop_ready generalization, Ctrl+V screenshot
  paste, long-paste→text attachment, bare-URL hint), stageFile → stage_attachment
  (arrayBuffer→base64 — FileReader rejects Node/jsdom Blobs), attachmentPayload()
  consumed by chat.mjs sendViaAgent. refineDraft: sparkle #refineBtn → refine_prompt,
  rewrite lands in the box, hint-action button restores the original (undo). Use
  ta.ownerDocument.defaultView.Event for dispatched events (jsdom rejects Node's).
- **chat.mjs**: renderModelNote — one quiet .model-note line when agent_send returns
  model_note (vision auto-route); .msg-copy overlap fixed (absolute → in-flow footer
  under the message, style.css).
- **settings.mjs**: Spending section (autoRouteVision checkbox, routePriceCap,
  conversationBudget — blank keeps saved value, junk blocked client-side too) +
  Personal touch (globalInstructions textarea → grounding config.json via
  save_settings). Consumer relabels: SEC filings contact email, Web browsing helper,
  Writing model. 'sees images' badges in both model lists (m.vision).
- style.css: .settings-section-title, .field-money, .field-check, .hint-action,
  .model-note, #refineBtn.refining pulse — all reduced-motion covered.
- Tests: 191 jsdom (composer.test.mjs: picker, chips, paste, drop, polish+undo,
  vision badge).

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
