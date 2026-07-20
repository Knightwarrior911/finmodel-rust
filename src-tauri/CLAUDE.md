## Session (2026-07-20) v0.9.22 — attachments backend, vision routing, spend guards
- **commands/attachments.rs** (new): stage_attachment (base64 over IPC → app-data
  staging dir, classify by extension, ArtifactKind::UserImage/UserFile), read-only
  has_image_attachments pre-probe, build_attachment_context (text extraction blocks:
  PPTX via fm-pptx inspect, XLSX via fm-excel calamine, DOCX zip-XML, plain text
  12k cap; images → data URLs ≤5MB ×4; PDFs re-registered to the live conversation).
- **Vision auto-routing** (agent/model_router.rs route_for_vision + launch_run in
  commands/agent.rs): cheapest catalog model with vision+tools, ≥32k ctx
  (MIN_ROUTE_CONTEXT), never :free variants, parseable completion price ≤
  settings.route_price_cap_usd (default $5/M out, 0=off). Per-TURN cfg override,
  nothing persisted; agent_send returns model_note {using,using_id,usual}.
  NoneAffordable fails the send BEFORE provider calls or attachment consumption.
  Unknown current model (custom base) is never switched away from.
- **CostGuard** (agent/driver.rs): conversation_budget_usd (0=off) from settings;
  charge_round on every stream accept (accept_stream, manual retry arm, strong
  finisher); prefers OpenRouter billed usage.cost — stream_completion_for_agent
  injects usage:{include:true} for OpenRouter only; apply_delta keeps numeric cost;
  fallback tokens × catalog snapshot (cached_openrouter_catalog, 5-min TTL,
  settings.rs); total-only ⇒ total × out-rate (overestimates). set_run_usage after
  each round; conversation_spend_usd sums; finish_run COALESCEs usage_json.
  request_model refuses to start a round over budget.
- **refine_prompt** command (settings.rs, registered in mod.rs): configured model,
  600-token cap, returns {text}. grounding.rs write_global (read-modify-write
  config.json, removes legacy 'personalization' alias); load/save_settings expose
  global_instructions from THAT file (no duplicate Settings field).
- Settings: auto_route_vision (default true), route_price_cap_usd,
  conversation_budget_usd; money fields error on junk, never silently default.
- fm-extract llm.rs: OpenRouterArchitecture + vision() (input_modalities, modality
  fallback), prompt/completion_per_mtok (unparseable → None).
- Tests: 344 lib green; live_vision_red_png_mini ships ignored (openrouter.ai TLS
  was reset from this machine all session — pre-existing live tests failed too).

## Session (2026-07-19) v0.9.2–9.9 — Multi-year spread, credit metrics, blocked-source fallback
- **get_financials** became the analyst spread: up to 6 fiscal years, balance sheet
  (cash/assets/LT debt/equity), cash flow (CFO/capex), shares, interest/D&A/short-term
  debt. Three bases: annual (default), quarterly (last 8 quarters, Q4 derived),
  ltm (trailing 12 months via fm_extract::ltm). Growth, margins, EBITDA, leverage,
  FCF, interest coverage pre-computed deterministically — model never does arithmetic.
  Recency-preferring tag selection (discontinued tags don't shadow current).
- **Budget grace**: rounds/tokens exhaustion earns one wrap-up synthesis pass before
  terminating as budget-limited. 8-to-10 round interactive budget.
- **Blocked-source doctrine**: system prompt tells model to fall back immediately
  (research, SEC filings, news) without asking permission. Tool result carries the
  fallback playbook when a page is blocked.
- **Lifecycle fix**: tool_use_skill now calls record_skill_use (async, best-effort).
- **Skill seeding**: seed_builtin_skills at startup, marker file, never overwrites.
  See agent/skills.rs BUILTIN_SKILLS const.
- Chat tests: 297 lib (3 live EDGAR ignored); READ_PAGE schema updated.

# src-tauri — finmodel desktop backend (Tauri 2)

Standalone Cargo workspace (detached from `finmodel-core`; path-deps into the
engine crates). `frontendDist: ../ui`, `withGlobalTauri: true`, no frontend build
step. Every `#[tauri::command]` returns a JSON **string**; errors are
`error::AppError` (`{kind,message}`), alias `AppResult<T>`.

## Command modules (`src/commands/`, registered in `mod.rs::handler()`)
- `model` — `build_model`, `prepare_model`, `finalize_model`, `analyze_pdf`,
  `list_recent`, `open_path`, `open_url`. Blocking **cores** reused by chat tools (all `pub(crate)`):
  `build_model_blocking`, `prepare_model_core`, `finalize_model_core`,
  `obtain_extraction`, `render_build`. `emit_progress(app,stage,detail)` emits
  `build_progress` — copy this pattern for events. `SessionCache` (managed) backs
  prepare→finalize; both cores get it via `app.state::<SessionCache>()`.
- `benchmark` — `benchmark_peers` cmd + `pub(crate) benchmark_blocking(app,tickers,BenchOpts)`.
- `analysis` — post-build analyst actions (Phase 6.5), each returning a JSON
  string: `ev_bridge` (`fm_value::ev_bridge`), `ifrs_bridge` (`fm_ifrs::auto_convert`),
  `tie_out` (`fm_tieout::score_from_json`). UI-invoked only (Analyst-tools panel);
  deliberately NOT in the chat tool list / intent router.
- `news` — `get_news` → `fm_fetch::fetch_headlines`.
- `search` — `web_search`, `read_page` (returns `{title,text,status}`), `test_mcp`;
  `pub(crate) mcp_from_settings(app) -> Option<McpClient>` (Roam MCP first, HTTP fallback).
- `settings` — `Settings{openrouter_api_key,model,edgar_contact,out_dir,mcp_command,
  mcp_args,recent}` at `app_config_dir()/settings.json`; `read_settings`/`write_settings` pub.
- `update` — updater cmds (desktop-only plugin in `lib.rs`).
- `chat` — the chat engine (below).

## Chat engine (`src/commands/chat.rs`)
- **Store:** `app_config_dir()/conversations/<id>.json`, `id = "{unix_ms}-{rand u16:04x}"`.
  `Conversation{id,title,created,updated,messages:Vec<ChatMsg>}`,
  `ChatMsg{role,content,card:Option<Value>,ts}`. Cards persist as assistant messages;
  raw LLM tool payloads are NOT persisted. `iso_utc(secs)` gives sortable ISO stamps
  (no date-lib dep). Commands: `list/load/delete/rename_conversation`.
- **Turn:** `chat_send(app,conversation_id?,message)` → `spawn_blocking` →
  `chat_send_blocking`. `ChatGate` (managed) = `busy`+`cancel` AtomicBools;
  `chat_cancel` sets cancel. Returns `{conversation_id,messages:[appended]}`.
- **LLM loop** (`run_llm_turn`): `build_chat_request` (pure) → `openrouter_stream`
  (blocking reqwest SSE; `apply_delta`/`sse_accumulate` reassemble content +
  `delta.tool_calls[]` fragments by `index`). Emits `chat_delta` per chunk. Up to
  `MAX_TOOL_ROUNDS=8`. **10 tools** dispatch through `run_tool` → shared cores (never
  the command wrappers). Each: emit `chat_tool start` → run → emit `done`+card.
- **Weak-model safety net:** on `ToolsUnsupported` (model 400/404s on `tools`) OR an
  Ok turn where round 0 returns no tool_call for an EXPLICIT data request, drop the
  (possibly fabricated) draft (`chat_reset`) and run `run_routed_tool` — the
  deterministic `route_fallback` router. NEVER let the model free-form finance data.
- **No-key path:** `run_fallback_turn` → `run_routed_tool`.
- `strip_control_tokens` removes model pseudo-tokens (`<|eom|>` etc.) before persist.
- System prompt + tool schemas are exact literals in the file — keep in sync with tools.

## Conventions / gotchas
- Chat tools call `pub(crate)` cores directly (do NOT `invoke` command wrappers).
- Keep ALL existing commands registered (CLI/tests/back-compat) even if the UI stops
  calling some directly.
- `df -h /c` before any build; a signed release build needs >6G free.
- Tests: `cargo test` (unit tests are pure fns: `build_chat_request`, `sse_accumulate`,
  `route_fallback`, `strip_control_tokens`, `iso_utc`, conversation round-trip).

## Build + sign a release (see `../docs/RELEASE_CHECKLIST.md` §6)
Set `CI` explicitly to `true`/`false` (sandbox `CI=1` breaks tauri-cli `--ci`).
Env `TAURI_SIGNING_PRIVATE_KEY=<contents of C:\Users\vinit\.tauri\finmodel.key>`,
`TAURI_SIGNING_PRIVATE_KEY_PASSWORD=""` → `cargo tauri build --bundles nsis`.
Bump `version` in `tauri.conf.json` + `Cargo.toml`. Publish setup.exe + latest.json
to the PUBLIC `finmodel-releases` repo; new version MUST be > installed.
