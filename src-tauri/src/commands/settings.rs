//! Settings + OpenRouter model listing commands.
//!
//! Settings (API key, chosen model) persist to a JSON file in the app config dir.
//! The raw key is never sent to the frontend — only a `has_key` boolean.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::Manager;

use crate::error::{AppError, AppResult};

fn default_model() -> String {
    "anthropic/claude-sonnet-4".to_string()
}

/// Default provider base URL (OpenAI-compatible root). The chat endpoint is
/// `{base}/chat/completions`; the catalog is `{base}/models`. OpenRouter is the
/// default; users may point at any OpenAI-compatible provider with their own key
/// (OpenAI, xAI/Grok, DeepSeek, Groq, Mistral, Together, Fireworks, Cerebras,
/// Moonshot, Gemini/Anthropic OpenAI-compat, …).
fn default_base_url() -> String {
    "https://openrouter.ai/api/v1".to_string()
}

/// Effective provider base URL: the configured `base_url`, or the OpenRouter
/// default when unset/blank (a `Settings::default()` leaves it empty).
pub fn provider_base(s: &Settings) -> String {
    let b = s.base_url.trim().trim_end_matches('/');
    if b.is_empty() {
        default_base_url()
    } else {
        b.to_string()
    }
}

/// The chat-completions endpoint for the configured provider.
pub fn chat_completions_url(s: &Settings) -> String {
    format!("{}/chat/completions", provider_base(s))
}

/// True when the configured provider is OpenRouter (its `/models` catalog has
/// richer capability metadata than the plain OpenAI-compatible shape).
pub fn is_openrouter(s: &Settings) -> bool {
    provider_base(s).contains("openrouter.ai")
}

/// True when Settings point at the local OMP auth gateway.
pub fn is_omp_gateway(s: &Settings) -> bool {
    crate::commands::omp_gateway::is_cursor_gateway_base(&provider_base(s))
}

/// True when the OMP gateway is serving a Cursor model.
pub fn is_cursor_gateway(s: &Settings) -> bool {
    is_omp_gateway(s) && !s.model.trim().starts_with("opencode-go/")
}

fn is_opencode_gateway(s: &Settings) -> bool {
    is_omp_gateway(s) && s.model.trim().starts_with("opencode-go/")
}

/// Catalog-derived capability for OMP subscription models without a live probe.
/// `opencode-go/<id>` → native_tools=true, strict_json=false.
/// `cursor/<id>` → native_tools=false, strict_json=false.
/// Anything else → None.
pub fn omp_subscription_capability(model: &str) -> Option<ModelCapability> {
    let model = model.trim();
    if model.starts_with("opencode-go/") {
        Some(ModelCapability {
            model_id: model.to_string(),
            native_tools: true,
            strict_json: false,
            tested_at: chrono_like_now(),
        })
    } else if model.starts_with("cursor/") {
        Some(ModelCapability {
            model_id: model.to_string(),
            native_tools: false,
            strict_json: false,
            tested_at: chrono_like_now(),
        })
    } else {
        None
    }
}

/// If settings point at the OMP gateway, derive capability from the effective
/// model using catalog knowledge rather than live probes. Leaves capability
/// unchanged for non-OMP providers.
pub fn apply_omp_capability(settings: &mut Settings) {
    if is_omp_gateway(settings) {
        let model = effective_model(settings);
        if let Some(cap) = omp_subscription_capability(&model) {
            settings.model_capability = Some(cap);
        }
    }
}

/// API key used for outbound OpenAI-compatible calls.
/// The local OMP gateway bearer remains owned by OMP and never overwrites the
/// stored OpenRouter/OpenCode key.
fn effective_api_key_from(s: &Settings, gateway_bearer: Option<String>) -> String {
    if is_omp_gateway(s) {
        return gateway_bearer.unwrap_or_default();
    }
    s.openrouter_api_key.trim().to_string()
}

pub fn effective_api_key(s: &Settings) -> String {
    let gateway_bearer = is_omp_gateway(s)
        .then(|| crate::commands::omp_gateway::gateway_bearer().ok())
        .flatten();
    effective_api_key_from(s, gateway_bearer)
}

/// Whether chat/list_models can proceed for the configured provider.
pub fn has_effective_credentials(s: &Settings) -> bool {
    if is_opencode_gateway(s) {
        return crate::commands::subscription::find_opencode_go_credential().is_some();
    }
    if is_cursor_gateway(s) {
        let cur = crate::commands::subscription::cursor_omp_status();
        return cur.reusable();
    }
    !s.openrouter_api_key.trim().is_empty()
}

/// Model id for outbound calls. Fully-qualified gateway selectors keep their
/// provider prefix; legacy bare Cursor ids receive `cursor/`.
pub fn effective_model(s: &Settings) -> String {
    if is_omp_gateway(s) && !s.model.trim().contains('/') {
        crate::commands::omp_gateway::qualify_cursor_model(&s.model)
    } else {
        s.model.clone()
    }
}

pub(crate) fn update_selected_model(settings: &mut Settings, model: &str) {
    let model = model.trim();
    if settings.model != model {
        settings.model = model.to_string();
        settings.model_capability = None;
    }
    // On OMP gateway, reseed from catalog knowledge so tools_ok works
    // without requiring an explicit Test model click.
    apply_omp_capability(settings);
}

/// Ensure usable subscription credentials and the local OMP gateway are ready.
pub fn ensure_provider_ready(s: &Settings) -> Result<(), String> {
    if is_cursor_gateway(s) {
        let cursor = crate::commands::subscription::cursor_omp_status();
        if !cursor.reusable() {
            return Err(if cursor.present {
                "Cursor login expired without a refresh token. Connect Cursor again in Settings."
                    .into()
            } else {
                "Cursor login missing. Connect Cursor in Settings.".into()
            });
        }
    } else if is_opencode_gateway(s)
        && crate::commands::subscription::find_opencode_go_credential().is_none()
    {
        return Err("OpenCode Go credentials missing. Connect OpenCode Go in Settings.".into());
    }
    if is_omp_gateway(s) {
        crate::commands::omp_gateway::ensure_cursor_gateway()?;
    }
    Ok(())
}

/// A recently generated output file (4.2).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecentEntry {
    pub path: String,
    pub label: String,
    pub when: String,
}

/// Last-probed OpenRouter model capabilities (Phase 1.3). Cached only after a
/// successful catalog fetch or explicit Test model; missing/unknown → app
/// routing + plain JSON (never assume native tools / strict schema).
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct ModelCapability {
    pub model_id: String,
    pub native_tools: bool,
    pub strict_json: bool,
    /// ISO-8601 probe timestamp.
    #[serde(default)]
    pub tested_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Settings {
    /// Legacy plaintext field — **never written** after Phase 1.6. Read only for
    /// one-way migration into the OS credential store, then scrubbed.
    #[serde(default, skip_serializing)]
    pub openrouter_api_key: String,
    #[serde(default = "default_model")]
    pub model: String,
    /// Provider base URL (OpenAI-compatible root). Chat = `{base}/chat/completions`.
    #[serde(default = "default_base_url")]
    pub base_url: String,
    /// EDGAR contact (email) for the SEC User-Agent (2.1 / 3.6).
    #[serde(default)]
    pub edgar_contact: String,
    /// Default output folder for generated workbooks (3.2 / 3.6).
    #[serde(default)]
    pub out_dir: String,
    /// Optional stronger model for research synthesis + budget wrap-ups
    /// (the memo-grade writing). Blank → the main `model` handles everything.
    /// (Full fast-orchestrator/strong-finisher tiering for ordinary chat
    /// turns needs a stream-handoff redesign — this covers the two seams
    /// where a whole call is already dedicated to synthesis.)
    #[serde(default)]
    pub synthesis_model: String,
    /// Optional second-look reviewer model (the advisor role): reads each
    /// drafted answer against the run's tool evidence and surfaces short
    /// notes for material problems. Blank → advisor off (no extra calls).
    #[serde(default)]
    pub advisor_model: String,
    /// Web-research MCP server command + args (Phase 8.2).
    #[serde(default)]
    pub mcp_command: String,
    #[serde(default)]
    pub mcp_args: Vec<String>,
    /// Recent generated files, most-recent-first (4.2).
    #[serde(default)]
    pub recent: Vec<RecentEntry>,
    /// Capability cache for the currently selected model (Phase 1.3).
    #[serde(default)]
    pub model_capability: Option<ModelCapability>,
    /// Explicit worker/verifier/fallback role profiles (Task 1.5). Absent → every
    /// role uses the orchestrator (the flat `model`/`base_url` above).
    #[serde(default)]
    pub model_profiles: Option<crate::agent::model_router::ModelProfiles>,
    /// When a message carries images and the chosen model can't see, quietly
    /// use the cheapest capable model for that one message (default on).
    #[serde(default = "default_true")]
    pub auto_route_vision: bool,
    /// Auto-routing price ceiling in USD per 1M OUTPUT tokens. Models priced
    /// above this are never auto-selected. ≤ 0 disables auto-routing.
    #[serde(default = "default_route_price_cap")]
    pub route_price_cap_usd: f64,
    /// Per-conversation spend ceiling in USD (approximate, checked between
    /// steps). 0 = no limit. Guards runaway loops from surprise bills.
    #[serde(default)]
    pub conversation_budget_usd: f64,
    /// EDINET (Japan disclosure) API key — free registration; enables
    /// structured Japanese filings. Plaintext like other non-billing config.
    #[serde(default)]
    pub edinet_api_key: String,
}

fn default_true() -> bool {
    true
}

fn default_route_price_cap() -> f64 {
    5.0
}

fn settings_path(app: &tauri::AppHandle) -> AppResult<PathBuf> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| AppError::Config(format!("no config dir: {e}")))?;
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join("settings.json"))
}

pub fn read_settings(app: &tauri::AppHandle) -> Settings {
    let mut s = match settings_path(app) {
        Ok(p) if p.exists() => std::fs::read_to_string(&p)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default(),
        _ => Settings {
            model: default_model(),
            ..Default::default()
        },
    };
    // One-way migration: lift any legacy plaintext into the OS store, then
    // scrub settings.json so the key is never rewritten to disk. On migration
    // failure, keep the in-memory legacy value so the user is not locked out;
    // disk is left untouched until a successful store write.
    let legacy = s.openrouter_api_key.clone();
    if !legacy.trim().is_empty() {
        if crate::commands::secrets::migrate_legacy_key(&legacy) {
            s.openrouter_api_key.clear();
            let _ = write_settings(app, &s);
            s.openrouter_api_key = crate::commands::secrets::get_api_key().unwrap_or_default();
        }
        // else: keep `legacy` in memory; do not overwrite with empty keystore.
    } else {
        s.openrouter_api_key = crate::commands::secrets::get_api_key().unwrap_or_default();
    }
    s
}

/// Persist a full [`Settings`] to disk (used by the recent-files updater).
/// The API key field is `skip_serializing` so it is never written.
pub fn write_settings(app: &tauri::AppHandle, s: &Settings) -> AppResult<()> {
    let p = settings_path(app)?;
    // Ensure we never accidentally serialize a key even if the skip attribute
    // is later removed: clone and blank the field before write.
    let mut disk = s.clone();
    disk.openrouter_api_key.clear();
    std::fs::write(&p, serde_json::to_string_pretty(&disk)?)
        .map_err(|e| AppError::Io(e.to_string()))?;
    Ok(())
}

/// Build the settings view object exposed to the UI (Task 1.5 adds
/// `model_profiles`) — never the raw key. Pure over `Settings` so the shape is
/// unit-testable without an `AppHandle`.
pub fn settings_view_json(s: &Settings, version: &str) -> serde_json::Value {
    serde_json::json!({
        "has_key": has_effective_credentials(s),
        "cursor_gateway": is_cursor_gateway(s),
        "model": effective_model(s),
        "base_url": provider_base(s),
        "subscription_providers_enabled": crate::commands::subscription::subscription_providers_enabled(),
        "edgar_contact": s.edgar_contact,
        "out_dir": s.out_dir,
        "mcp_command": s.mcp_command,
        "synthesis_model": s.synthesis_model,
        "advisor_model": s.advisor_model,
        "mcp_args": s.mcp_args,
        "version": version,
        "model_capability": s.model_capability,
        "model_profiles": s.model_profiles,
        "auto_route_vision": s.auto_route_vision,
        "route_price_cap_usd": s.route_price_cap_usd,
        "conversation_budget_usd": s.conversation_budget_usd,
        "has_edinet_key": !s.edinet_api_key.trim().is_empty(),
    })
}

/// Return `{ has_key, model, …, global_instructions }` — never the raw key.
/// `global_instructions` reads the grounding file (`config.json`), the same
/// layer the agent chains into every system prompt — one source of truth.
#[tauri::command(rename_all = "snake_case")]
pub fn load_settings(app: tauri::AppHandle) -> AppResult<String> {
    let s = read_settings(&app);
    let mut v = settings_view_json(&s, &app.package_info().version.to_string());
    let global = app
        .path()
        .app_config_dir()
        .ok()
        .and_then(|dir| crate::agent::grounding::read_global(&dir))
        .unwrap_or_default();
    v["global_instructions"] = serde_json::json!(global);
    Ok(v.to_string())
}

/// Save settings. A blank `api_key` keeps the existing one (so the frontend can
/// send blank to change only the model). A blank `model` keeps the existing one.
/// Non-empty `api_key` is written **only** to the OS credential store.
#[tauri::command(rename_all = "snake_case")]
pub fn save_settings(
    app: tauri::AppHandle,
    api_key: String,
    model: String,
    base_url: Option<String>,
    edgar_contact: Option<String>,
    out_dir: Option<String>,
    mcp_command: Option<String>,
    mcp_args: Option<Vec<String>>,
    synthesis_model: Option<String>,
    advisor_model: Option<String>,
    model_profiles: Option<crate::agent::model_router::ModelProfiles>,
    auto_route_vision: Option<bool>,
    route_price_cap_usd: Option<f64>,
    conversation_budget_usd: Option<f64>,
    global_instructions: Option<String>,
    edinet_api_key: Option<String>,
) -> AppResult<String> {
    let mut s = read_settings(&app);
    if !api_key.trim().is_empty() {
        crate::commands::secrets::set_api_key(api_key.trim()).map_err(AppError::Config)?;
        // Memory copy for this process; never persisted. A changed account must
        // never inherit capabilities probed with the prior credential.
        s.openrouter_api_key = api_key.trim().to_string();
        s.model_capability = None;
    }
    if !model.trim().is_empty() {
        update_selected_model(&mut s, &model);
    }
    if let Some(b) = base_url {
        let b = b.trim().to_string();
        if b != s.base_url {
            // Provider changed — the cached capability is for the old provider.
            s.model_capability = None;
        }
        s.base_url = b;
    }
    // After any provider change, reseed OMP capability if the base now
    // points at the OMP gateway.
    apply_omp_capability(&mut s);
    // These are set-if-present (blank string clears; absent keeps existing).
    if let Some(c) = edgar_contact {
        s.edgar_contact = c.trim().to_string();
    }
    if let Some(d) = out_dir {
        s.out_dir = d.trim().to_string();
    }
    if let Some(c) = mcp_command {
        crate::commands::mcp::validate_mcp_command(&c).map_err(AppError::Config)?;
        s.mcp_command = c.trim().to_string();
    }
    if let Some(a) = mcp_args {
        s.mcp_args = a;
    }
    if let Some(m) = synthesis_model {
        s.synthesis_model = m.trim().to_string();
    }
    if let Some(m) = advisor_model {
        s.advisor_model = m.trim().to_string();
    }
    // Explicit role profiles (Task 1.5). Present → set (an empty object clears the
    // roles back to orchestrator-only); absent → keep existing.
    if let Some(mp) = model_profiles {
        s.model_profiles = Some(mp);
    }
    if let Some(v) = auto_route_vision {
        s.auto_route_vision = v;
    }
    if let Some(cap) = route_price_cap_usd {
        // Money fields never guess: invalid input is an error, not a silent
        // default in either direction. 0 is the explicit "off" value.
        if !cap.is_finite() || cap < 0.0 {
            return Err(AppError::Config(
                "That price limit doesn't look like a number. Enter a dollar amount like 5, or 0 to turn automatic switching off.".into(),
            ));
        }
        s.route_price_cap_usd = cap;
    }
    if let Some(b) = conversation_budget_usd {
        if !b.is_finite() || b < 0.0 {
            return Err(AppError::Config(
                "That budget doesn't look like a number. Enter a dollar amount like 2.50, or 0 for no limit.".into(),
            ));
        }
        s.conversation_budget_usd = b;
    }
    if let Some(k) = edinet_api_key {
        // Blank clears; whitespace trimmed. Not a billing credential — kept
        // with the rest of the plain config.
        s.edinet_api_key = k.trim().to_string();
    }
    if let Some(g) = global_instructions {
        // One source of truth: the grounding file the agent already chains
        // into every system prompt. Bounded so a pasted novel can't crowd
        // out the base prompt. Blank clears the layer.
        let text: String = g.trim().chars().take(4_000).collect();
        let dir = app
            .path()
            .app_config_dir()
            .map_err(|e| AppError::Config(format!("no config dir: {e}")))?;
        crate::agent::grounding::write_global(&dir, &text)
            .map_err(|e| AppError::Io(e.to_string()))?;
    }
    write_settings(&app, &s)?;
    // Settings change kills any live MCP child (Phase 3.2).
    if let Some(mgr) = app.try_state::<crate::commands::mcp::McpManager>() {
        mgr.reset();
    }
    Ok(serde_json::json!({ "ok": true }).to_string())
}

/// Clear the saved API key (back to offline demo mode), keeping the model.
/// Deletes the OS credential entry; the only path back to demo mode.
#[tauri::command(rename_all = "snake_case")]
pub fn clear_api_key(app: tauri::AppHandle) -> AppResult<String> {
    crate::commands::secrets::delete_api_key().map_err(AppError::Config)?;
    let mut s = read_settings(&app);
    s.openrouter_api_key = String::new();
    s.model_capability = None;
    write_settings(&app, &s)?;
    Ok(serde_json::json!({ "ok": true }).to_string())
}

/// Fetch the live OpenRouter model catalog using the saved key.
/// Returns a JSON array of `{ id, name, context_length, pricing, supported_parameters,
/// native_tools, strict_json }` for UI badges. Catalog fetch is NOT a capability
/// probe — only [`test_model`] writes `model_capability` (Phase 1.3).
#[tauri::command(rename_all = "snake_case")]
pub async fn list_models(app: tauri::AppHandle, provider_id: Option<String>) -> AppResult<String> {
    // Network fetch — run off the IPC thread.
    tauri::async_runtime::spawn_blocking(move || {
        let mut s = read_settings(&app);
        if let Some(provider) = provider_id.as_deref() {
            if provider == "cursor" {
                if !crate::commands::subscription::subscription_providers_enabled() {
                    return Err(AppError::Config("Subscription providers are disabled.".into()));
                }
                let cur = crate::commands::subscription::cursor_omp_status();
                if !cur.present {
                    return Err(AppError::Config("No Cursor login in ~/.omp/agent/agent.db. Click Connect Cursor to log in via omp.".into()));
                }
                if !cur.reusable() {
                    return Err(AppError::Config("Cursor login expired without a refresh token. Click Connect Cursor to log in again.".into()));
                }
                ensure_provider_ready(&s).map_err(AppError::Engine)?;
                let (_, ids) = crate::commands::subscription::probe_cursor_models_via_omp()
                    .map_err(AppError::Engine)?;
                let resolved = crate::commands::omp_gateway::resolve_cursor_model(&s.model, &ids);
                if resolved != s.model {
                    update_selected_model(&mut s, &resolved);
                    write_settings(&app, &s)?;
                }
                let models = ids
                    .iter()
                    .map(|id| {
                        let qualified = crate::commands::omp_gateway::qualify_cursor_model(id);
                        serde_json::json!({ "id": qualified, "name": id })
                    })
                    .collect::<Vec<_>>();
                return serde_json::to_string(&models)
                    .map_err(|e| AppError::Engine(e.to_string()));
            }
            match provider {
                "openrouter" => s.base_url = "https://openrouter.ai/api/v1".into(),
                "opencode-go" => {
                    s.base_url = crate::commands::omp_gateway::GATEWAY_BASE_URL.into();
                    if !s.model.starts_with("opencode-go/") {
                        s.model = crate::commands::subscription::OPENCODE_GO_MODEL.into();
                    }
                }
                _ => {}
            }
        }
        if let Err(e) = ensure_provider_ready(&s) {
            return Err(AppError::Engine(e));
        }
        if !has_effective_credentials(&s) {
            return Err(AppError::Config(if is_opencode_gateway(&s) {
                "OpenCode Go credentials missing in OMP agent.db — connect OpenCode Go in Settings."
                    .into()
            } else if is_cursor_gateway(&s) {
                "Cursor OAuth missing/expired in OMP agent.db — connect Cursor in Settings.".into()
            } else {
                "No API key set. Add one in Settings first.".into()
            }));
        }
        let key = effective_api_key(&s);
        // OpenRouter's catalog carries capability badges; other OpenAI-compatible
        // providers expose a plainer `{base}/models` (ids only).
        if is_openrouter(&s) {
            let models = cached_openrouter_catalog(&key)
                .map_err(|e| AppError::Engine(format!("OpenRouter model fetch failed: {e}")))?;
            let enriched: Vec<serde_json::Value> = models
                .iter()
                .map(|m| {
                    serde_json::json!({
                        "id": m.id,
                        "name": m.name,
                        "context_length": m.context_length,
                        "pricing": m.pricing,
                        "supported_parameters": m.supported_parameters,
                        "native_tools": m.native_tools(),
                        "strict_json": m.strict_json(),
                        "vision": m.vision(),
                    })
                })
                .collect();
            return serde_json::to_string(&enriched).map_err(|e| AppError::Engine(e.to_string()));
        }
        // Generic OpenAI-compatible catalog: GET {base}/models -> {data:[{id}]}.
        let url = format!("{}/models", provider_base(&s));
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(20))
            .build()
            .map_err(|e| AppError::Engine(e.to_string()))?;
        let resp = client
            .get(&url)
            .bearer_auth(&key)
            .send()
            .map_err(|e| AppError::Engine(format!("model fetch transport error: {e}")))?;
        if !resp.status().is_success() {
            // Provider has no usable catalog — UI falls back to manual model entry.
            return Ok("[]".to_string());
        }
        let body: serde_json::Value = resp
            .json()
            .map_err(|_| AppError::Engine("model catalog decode error".into()))?;
        let ids = body
            .get("data")
            .and_then(|d| d.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|m| m.get("id").and_then(|i| i.as_str()))
                    .filter(|id| {
                        provider_id.as_deref() != Some("opencode-go")
                            || id.starts_with("opencode-go/")
                    })
                    .map(|id| serde_json::json!({ "id": id, "name": id }))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        serde_json::to_string(&ids).map_err(|e| AppError::Engine(e.to_string()))
    })
    .await
    .map_err(|e| AppError::Engine(format!("model fetch task failed: {e}")))?
}

/// Explicit Test-model capability probe (Phase 1.3).
///
/// Runs **bounded live requests** against OpenRouter for the selected model:
/// 1. Catalog lookup for advertised parameters (cheap filter).
/// 2. If advertised tools → one non-streaming forced-`ping` tool-call probe;
///    success requires an actual `ping` entry in `message.tool_calls` (no
///    `provider.require_parameters` — with `tool_choice` it matches no
///    endpoint and 404s, misclassifying capable models). ⇒ `native_tools`.
/// 3. If advertised structured outputs → one non-streaming json_schema probe
///    with `provider.require_parameters:true` (validated combo; live research
///    sends the same pair). Success ⇒ `strict_json=true`.
///
/// Only successful probe responses are cached. A failed/missing probe leaves
/// that capability false (or clears the cache entirely if the model is
/// unknown / auth fails) so callers fall back to app routing + plain JSON.
#[tauri::command(rename_all = "snake_case")]
pub async fn test_model(app: tauri::AppHandle, model_id: Option<String>) -> AppResult<String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut s = read_settings(&app);
        if let Err(e) = ensure_provider_ready(&s) {
            return Err(AppError::Engine(e));
        }
        if !has_effective_credentials(&s) {
            return Err(AppError::Config(if is_cursor_gateway(&s) {
                "Cursor OAuth missing/expired in ~/.omp/agent/agent.db.".into()
            } else {
                "No OpenRouter API key set. Add one in Settings first.".into()
            }));
        }
        let key = effective_api_key(&s);
        let mut wanted = model_id
            .as_deref()
            .map(str::trim)
            .filter(|m| !m.is_empty())
            .map(|m| m.to_string())
            .unwrap_or_else(|| effective_model(&s));
        if is_cursor_gateway(&s) {
            wanted = crate::commands::omp_gateway::qualify_cursor_model(&wanted);
        }

        // Any failed probe for this model must invalidate a prior cache so
        // run_llm_turn falls back to app routing + plain JSON.
        let clear_matching_cache = |s: &mut Settings| {
            if s.model_capability
                .as_ref()
                .map(|c| c.model_id == wanted)
                .unwrap_or(false)
            {
                s.model_capability = None;
                let _ = write_settings(&app, s);
            }
        };

        // OMP subscription models: derive capability from the authenticated
        // gateway catalog without a forced non-streaming tool probe.
        if is_omp_gateway(&s) {
            let provider = if is_cursor_gateway(&s) {
                "cursor"
            } else {
                "opencode-go"
            };
            let available = crate::commands::subscription::probe_provider_models_via_omp(provider)
                .map_err(|e| {
                    clear_matching_cache(&mut s);
                    AppError::Engine(format!("OMP {provider} model probe failed: {e}"))
                })?;
            // wanted is already qualified (cursor/xxx or opencode-go/xxx).
            // available entries include the provider/ prefix.
            if !available.iter().any(|m| m == &wanted) {
                clear_matching_cache(&mut s);
                return Err(AppError::Config(format!(
                    "Model `{wanted}` not found in the OMP {provider} catalog."
                )));
            }
            if let Some(cap) = omp_subscription_capability(&wanted) {
                s.model = wanted.clone();
                s.model_capability = Some(cap.clone());
                write_settings(&app, &s)?;
                return serde_json::to_string(&cap)
                    .map_err(|e| AppError::Engine(e.to_string()));
            }
        }

        let chat_url = chat_completions_url(&s);
        let openrouter = is_openrouter(&s);

        // OpenRouter gates probes on its catalog's advertised params. Other
        // OpenAI-compatible providers have no such catalog — probe directly.
        let (model_id, native_tools, strict_json) = if openrouter {
            let models = match fm_extract::list_openrouter_models(&key) {
                Ok(m) => m,
                Err(e) => {
                    clear_matching_cache(&mut s);
                    return Err(AppError::Engine(format!(
                        "OpenRouter model probe failed: {e}"
                    )));
                }
            };
            let Some(m) = models.iter().find(|m| m.id == wanted) else {
                clear_matching_cache(&mut s);
                return Err(AppError::Config(format!(
                    "Model `{wanted}` not found in the OpenRouter catalog."
                )));
            };
            let nt = if m.native_tools() {
                match probe_tools(&key, &m.id, &chat_url) {
                    Ok(ok) => ok,
                    Err(e) => {
                        clear_matching_cache(&mut s);
                        return Err(AppError::Engine(e));
                    }
                }
            } else {
                false
            };
            let sj = if m.strict_json() {
                match probe_strict_json(&key, &m.id, &chat_url, true) {
                    Ok(ok) => ok,
                    Err(e) => {
                        clear_matching_cache(&mut s);
                        return Err(AppError::Engine(e));
                    }
                }
            } else {
                false
            };
            (m.id.clone(), nt, sj)
        } else {
            // Generic provider: probe the requested model directly. Transport/
            // auth failure clears the cache + errors; unsupported params → false.
            let nt = match probe_tools(&key, &wanted, &chat_url) {
                Ok(ok) => ok,
                Err(e) => {
                    clear_matching_cache(&mut s);
                    return Err(AppError::Engine(e));
                }
            };
            let sj = probe_strict_json(&key, &wanted, &chat_url, false).unwrap_or(false);
            (wanted.clone(), nt, sj)
        };

        let cap = ModelCapability {
            model_id: model_id.clone(),
            native_tools,
            strict_json,
            tested_at: chrono_like_now(),
        };
        s.model = model_id;
        s.model_capability = Some(cap.clone());
        write_settings(&app, &s)?;
        serde_json::to_string(&cap).map_err(|e| AppError::Engine(e.to_string()))
    })
    .await
    .map_err(|e| AppError::Engine(format!("model probe task failed: {e}")))?
}

/// Probe native tool-calling by **forcing** the `ping` function and verifying
/// `choices[0].message.tool_calls` contains a `ping` entry. HTTP 2xx alone is
/// not enough; unsupported-parameter responses map to `Ok(false)`.
fn probe_tools(api_key: &str, model: &str, chat_url: &str) -> Result<bool, String> {
    let body = serde_json::json!({
        "model": model,
        "messages": [{ "role": "user", "content": "Call the ping tool with ok=true." }],
        "tools": [{
            "type": "function",
            "function": {
                "name": "ping",
                "description": "Capability probe — call with ok=true.",
                "parameters": {
                    "type": "object",
                    "properties": { "ok": { "type": "boolean" } },
                    "required": ["ok"],
                    "additionalProperties": false
                }
            }
        }],
        "tool_choice": { "type": "function", "function": { "name": "ping" } },
        // No `provider.require_parameters` / `parallel_tool_calls`: that combo
        // makes OpenRouter's routing find NO endpoint (404 "no endpoints"),
        // misclassifying genuinely tool-capable models as incapable. The probe's
        // truth test is the response itself — a real forced `ping` tool_call.
        // 512 tokens leaves room for reasoning-model preambles.
        "max_tokens": 512,
        "temperature": 0,
        "stream": false
    });
    match post_probe_json(api_key, chat_url, &body)? {
        ProbeOutcome::Unsupported => Ok(false),
        ProbeOutcome::Body(v) => {
            let calls = v
                .pointer("/choices/0/message/tool_calls")
                .and_then(|c| c.as_array())
                .cloned()
                .unwrap_or_default();
            let ok = calls.iter().any(|c| {
                c.pointer("/function/name")
                    .and_then(|n| n.as_str())
                    .map(|n| n == "ping")
                    .unwrap_or(false)
            });
            Ok(ok)
        }
    }
}

/// Probe strict structured outputs: require a 2xx body whose content parses as
/// JSON with a boolean `ok` field. Unsupported-parameter → `Ok(false)`.
fn probe_strict_json(
    api_key: &str,
    model: &str,
    chat_url: &str,
    openrouter: bool,
) -> Result<bool, String> {
    let mut body = serde_json::json!({
        "model": model,
        "messages": [{ "role": "user", "content": "Reply with {\"ok\":true}." }],
        "response_format": {
            "type": "json_schema",
            "json_schema": {
                "name": "capability_probe",
                "strict": true,
                "schema": {
                    "type": "object",
                    "properties": { "ok": { "type": "boolean" } },
                    "required": ["ok"],
                    "additionalProperties": false
                }
            }
        },
        "max_tokens": 32,
        "temperature": 0,
        "stream": false
    });
    // `provider.require_parameters` is OpenRouter-only; other OpenAI-compatible
    // APIs 400 on the unknown field, which would falsely read as "unsupported".
    if openrouter {
        body["provider"] = serde_json::json!({ "require_parameters": true });
    }
    match post_probe_json(api_key, chat_url, &body)? {
        ProbeOutcome::Unsupported => Ok(false),
        ProbeOutcome::Body(v) => {
            let content = v
                .pointer("/choices/0/message/content")
                .and_then(|c| c.as_str())
                .unwrap_or("")
                .trim();
            // Tolerate optional ```json fences.
            let stripped = content
                .trim_start_matches("```json")
                .trim_start_matches("```")
                .trim_end_matches("```")
                .trim();
            match serde_json::from_str::<serde_json::Value>(stripped) {
                Ok(obj) => Ok(obj.get("ok").and_then(|b| b.as_bool()).is_some()),
                Err(_) => Ok(false),
            }
        }
    }
}

enum ProbeOutcome {
    Body(serde_json::Value),
    Unsupported,
}

/// POST a probe body. Success returns the JSON body; 400/404 with
/// unsupported-parameter language → Unsupported; other statuses → hard error
/// with a **redacted** category (never the provider body).
fn post_probe_json(
    api_key: &str,
    chat_url: &str,
    body: &serde_json::Value,
) -> Result<ProbeOutcome, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| format!("probe client: {e}"))?;
    let resp = client
        .post(chat_url)
        .bearer_auth(api_key)
        .header("HTTP-Referer", "https://github.com/finmodel")
        .header("X-Title", "finmodel-capability-probe")
        .json(body)
        .send()
        .map_err(|e| format!("probe transport: {e}"))?;
    let status = resp.status();
    let code = status.as_u16();
    let text = resp.text().unwrap_or_default();
    if status.is_success() {
        let v: serde_json::Value = serde_json::from_str(&text)
            .map_err(|_| "probe response was not valid JSON".to_string())?;
        return Ok(ProbeOutcome::Body(v));
    }
    let lower = text.to_lowercase();
    // Unsupported-parameter responses are a clean "capability false".
    if (code == 400 || code == 404)
        && (lower.contains("unsupported")
            || lower.contains("not support")
            || lower.contains("no endpoints")
            || lower.contains("require_parameters")
            || lower.contains("tool")
            || lower.contains("response_format")
            || lower.contains("structured"))
    {
        return Ok(ProbeOutcome::Unsupported);
    }
    // Auth / rate-limit / policy / generic — redacted category only.
    let category = if code == 401 || code == 403 {
        "auth"
    } else if code == 429 {
        "rate_limit"
    } else if code >= 500 {
        "provider_5xx"
    } else {
        "provider_error"
    };
    Err(format!("OpenRouter probe failed ({category}, HTTP {code})"))
}

/// One-shot non-streaming completion through the CONFIGURED provider (honors
/// `Settings.base_url`, not a hardcoded endpoint). Returns the assistant message
/// content. Used by self-evolution (skill drafting).
/// Patch just the model without touching keys or other settings — the
/// composer's model picker writes through here.
#[tauri::command]
pub fn set_model(app: tauri::AppHandle, model: String) -> AppResult<String> {
    let model = model.trim().to_string();
    if model.is_empty() {
        return Err(AppError::Config("model id required".into()));
    }
    let mut s = read_settings(&app);
    update_selected_model(&mut s, &model);
    write_settings(&app, &s)?;
    Ok(serde_json::json!({ "model": model }).to_string())
}

/// Build a chat-completions body, optionally multimodal. Shared by
/// [`complete_once`] and the vision path so the shape is tested once.
pub(crate) fn completion_body(
    model: &str,
    system: &str,
    user: &str,
    images: &[String],
    max_tokens: u32,
) -> serde_json::Value {
    let user_content: serde_json::Value = if images.is_empty() {
        serde_json::json!(user)
    } else {
        let mut parts = vec![serde_json::json!({ "type": "text", "text": user })];
        for u in images {
            parts.push(serde_json::json!({ "type": "image_url", "image_url": { "url": u } }));
        }
        serde_json::json!(parts)
    };
    serde_json::json!({
        "model": model,
        "messages": [
            { "role": "system", "content": system },
            { "role": "user", "content": user_content }
        ],
        "max_tokens": max_tokens,
        "temperature": 0.2,
        "stream": false
    })
}

/// One-shot completion with vision inputs (data URLs). Blocking.
#[allow(dead_code)]
pub(crate) fn complete_once_vision(
    api_key: &str,
    model: &str,
    chat_url: &str,
    system: &str,
    user: &str,
    images: &[String],
    max_tokens: u32,
) -> Result<String, String> {
    let body = completion_body(model, system, user, images, max_tokens);
    match post_probe_json(api_key, chat_url, &body)? {
        ProbeOutcome::Unsupported => Err("provider rejected the request".into()),
        ProbeOutcome::Body(v) => v
            .pointer("/choices/0/message/content")
            .and_then(|c| c.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| "no content in provider response".into()),
    }
}

pub(crate) fn complete_once(
    api_key: &str,
    model: &str,
    chat_url: &str,
    system: &str,
    user: &str,
    max_tokens: u32,
) -> Result<String, String> {
    let body = completion_body(model, system, user, &[], max_tokens);
    match post_probe_json(api_key, chat_url, &body)? {
        ProbeOutcome::Unsupported => Err("provider rejected the request".into()),
        ProbeOutcome::Body(v) => v
            .pointer("/choices/0/message/content")
            .and_then(|c| c.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| "no content in provider response".into()),
    }
}
/// Process-wide OpenRouter catalog cache (5-minute TTL). Shared by the UI's
/// model picker (`list_models`) and vision auto-routing so an image message
/// never costs a second catalog fetch. Blocking — call off the IPC thread.
pub(crate) fn cached_openrouter_catalog(
    api_key: &str,
) -> Result<std::sync::Arc<Vec<fm_extract::OpenRouterModel>>, String> {
    use std::sync::{Arc, Mutex, OnceLock};
    use std::time::{Duration, Instant};
    static CACHE: OnceLock<Mutex<Option<(Instant, Arc<Vec<fm_extract::OpenRouterModel>>)>>> =
        OnceLock::new();
    const TTL: Duration = Duration::from_secs(5 * 60);
    let cell = CACHE.get_or_init(|| Mutex::new(None));
    if let Some((at, models)) = cell.lock().ok().and_then(|g| g.clone()) {
        if at.elapsed() < TTL {
            return Ok(models);
        }
    }
    let fresh = Arc::new(fm_extract::list_openrouter_models(api_key).map_err(|e| e.to_string())?);
    if let Ok(mut g) = cell.lock() {
        *g = Some((Instant::now(), Arc::clone(&fresh)));
    }
    Ok(fresh)
}

/// System prompt for the composer's "quick tidy" mode. The reply must be
/// ONLY the rewritten prompt — no preamble, no quotes, no options.
const REFINE_SYSTEM: &str = "You tidy up a draft question for a financial-analysis assistant. \
Rewrite the draft so it is clear, specific, and complete: keep the user's language, intent, \
tickers, figures, and constraints exactly; fix grammar; spell out vague references; add the \
obvious missing specifics (time period, metric, company) ONLY when the draft clearly implies \
them. Never invent facts, never answer the question, never add commentary. \
Reply with the rewritten prompt text and nothing else.";

/// System prompt for the composer's "power prompt" mode: expands a rough
/// draft into the structured, unambiguous request an LLM executes best —
/// while inventing nothing the user didn't imply.
const POWER_SYSTEM: &str = "You are a prompt engineer rewriting a rough draft into a powerful, \
unambiguous prompt for a financial-analysis assistant that has tools for SEC filings, live \
quotes, peer benchmarks, research, and Excel model building. Rewrite the draft as a complete \
request with, where relevant: the precise goal; the companies/tickers and time periods; the \
specific metrics or comparisons wanted; how to ground the work (filings, live data, cited \
sources); and the desired output shape (table, memo, model, brief answer). Keep every fact, \
ticker, figure, and constraint from the draft exactly; sharpen vague wording; NEVER invent \
companies, numbers, periods, or requirements the draft doesn't imply. Write it as the user \
speaking (\"Build...\", \"Compare...\"). Plain prose or short bullet lines, no headings, no \
meta-commentary. Reply with the rewritten prompt text and nothing else.";

/// One-shot draft polish for the composer (never auto-sends). `mode` is
/// `tidy` (default — light cleanup) or `power` (structured expansion).
/// Tight output caps keep a click at a few hundred output tokens.
#[tauri::command(rename_all = "snake_case")]
pub async fn refine_prompt(
    app: tauri::AppHandle,
    draft: String,
    mode: Option<String>,
) -> AppResult<String> {
    let draft = draft.trim().to_string();
    if draft.is_empty() {
        return Err(AppError::Config(
            "Type a question first, then I can tidy it up.".into(),
        ));
    }
    // Bounded input: a pasted document isn't a prompt to rewrite.
    let draft: String = draft.chars().take(8_000).collect();
    let power = mode.as_deref() == Some("power");
    tauri::async_runtime::spawn_blocking(move || {
        let s = read_settings(&app);
        let key = effective_api_key(&s);
        if key.is_empty() {
            return Err(AppError::Config(
                "Add your API key in Settings first.".into(),
            ));
        }
        let url = format!("{}/chat/completions", provider_base(&s));
        let (system, cap) = if power {
            (POWER_SYSTEM, 900)
        } else {
            (REFINE_SYSTEM, 600)
        };
        let out = complete_once(&key, &s.model, &url, system, &draft, cap)
            .map_err(|e| AppError::Engine(format!("couldn't polish the prompt: {e}")))?;
        let text = out.trim();
        if text.is_empty() {
            return Err(AppError::Engine(
                "the model returned an empty rewrite".into(),
            ));
        }
        Ok(serde_json::json!({ "text": text }).to_string())
    })
    .await
    .map_err(|e| AppError::Engine(format!("refine task failed: {e}")))?
}

/// Minimal UTC ISO-8601 stamp without pulling chrono into the app crate.
fn chrono_like_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = secs / 86_400;
    let rem = secs % 86_400;
    let hours = rem / 3_600;
    let mins = (rem % 3_600) / 60;
    let s = rem % 60;
    let (y, m, d) = days_to_ymd(days as i64);
    format!("{y:04}-{m:02}-{d:02}T{hours:02}:{mins:02}:{s:02}Z")
}

fn days_to_ymd(mut days: i64) -> (i32, u32, u32) {
    // Algorithm from civil_from_days (Howard Hinnant).
    days += 719_468;
    let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
    let doe = (days - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = (yoe as i64) + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y as i32, m as u32, d as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::model_router::{ModelProfile, ModelProfiles};

    fn worker_profile() -> ModelProfile {
        ModelProfile {
            provider_base: "https://api.deepseek.com/v1".into(),
            model: "deepseek-chat".into(),
            context_window: 64_000,
            native_tools: true,
            structured_output: true,
            cost_per_mtok_in: None,
            cost_per_mtok_out: None,
            credential_ref: "deepseek_api_key".into(),
        }
    }

    #[test]
    fn changing_the_selected_model_invalidates_its_capability_cache() {
        let mut settings = Settings {
            model: "old-model".into(),
            model_capability: Some(ModelCapability {
                model_id: "old-model".into(),
                native_tools: true,
                strict_json: true,
                tested_at: "now".into(),
            }),
            ..Default::default()
        };
        update_selected_model(&mut settings, "new-model");
        assert_eq!(settings.model, "new-model");
        assert!(settings.model_capability.is_none());
    }

    #[test]
    fn omp_gateway_preserves_qualified_opencode_model() {
        let s = Settings {
            model: "opencode-go/grok-4.5".into(),
            base_url: crate::commands::omp_gateway::GATEWAY_BASE_URL.into(),
            ..Default::default()
        };
        assert_eq!(effective_model(&s), "opencode-go/grok-4.5");
        assert_eq!(
            effective_api_key_from(&s, Some("gateway-secret".into())),
            "gateway-secret"
        );
    }

    #[test]
    fn omp_subscription_capability_opencode_go_has_native_tools() {
        let cap = omp_subscription_capability("opencode-go/grok-4.5").unwrap();
        assert_eq!(cap.model_id, "opencode-go/grok-4.5");
        assert!(cap.native_tools);
        assert!(!cap.strict_json);
        assert!(!cap.tested_at.is_empty());
    }

    #[test]
    fn omp_subscription_capability_cursor_no_native_tools() {
        let cap = omp_subscription_capability("cursor/claude-sonnet-4").unwrap();
        assert_eq!(cap.model_id, "cursor/claude-sonnet-4");
        assert!(!cap.native_tools);
        assert!(!cap.strict_json);
    }

    #[test]
    fn omp_subscription_capability_unknown_returns_none() {
        assert!(omp_subscription_capability("openai/gpt-4").is_none());
        assert!(omp_subscription_capability("").is_none());
        assert!(omp_subscription_capability("    ").is_none());
    }

    #[test]
    fn apply_omp_capability_opencode_go_on_omp_gateway() {
        let mut s = Settings {
            model: "opencode-go/grok-4.5".into(),
            base_url: crate::commands::omp_gateway::GATEWAY_BASE_URL.into(),
            ..Default::default()
        };
        apply_omp_capability(&mut s);
        let cap = s.model_capability.unwrap();
        assert_eq!(cap.model_id, "opencode-go/grok-4.5");
        assert!(cap.native_tools);
    }

    #[test]
    fn apply_omp_capability_cursor_on_omp_gateway() {
        let mut s = Settings {
            model: "cursor/claude-sonnet-4".into(),
            base_url: crate::commands::omp_gateway::GATEWAY_BASE_URL.into(),
            ..Default::default()
        };
        apply_omp_capability(&mut s);
        let cap = s.model_capability.unwrap();
        assert_eq!(cap.model_id, "cursor/claude-sonnet-4");
        assert!(!cap.native_tools);
    }

    #[test]
    fn apply_omp_capability_non_omp_leaves_capability_unchanged() {
        let mut s = Settings {
            model: "openai/gpt-4".into(),
            base_url: "https://api.openai.com/v1".into(),
            model_capability: Some(ModelCapability {
                model_id: "openai/gpt-4".into(),
                native_tools: true,
                strict_json: true,
                tested_at: "2025-01-01T00:00:00Z".into(),
            }),
            ..Default::default()
        };
        apply_omp_capability(&mut s);
        // Capability must survive unchanged for non-OMP providers.
        let cap = s.model_capability.unwrap();
        assert_eq!(cap.model_id, "openai/gpt-4");
        assert!(cap.native_tools);
        assert!(cap.strict_json);
    }

    #[test]
    fn update_selected_model_same_base_model_on_omp_gateway_reseeds() {
        let mut s = Settings {
            model: "cursor/old-model".into(),
            base_url: crate::commands::omp_gateway::GATEWAY_BASE_URL.into(),
            model_capability: Some(ModelCapability {
                model_id: "cursor/old-model".into(),
                native_tools: false,
                strict_json: false,
                tested_at: "2025-01-01T00:00:00Z".into(),
            }),
            ..Default::default()
        };
        // Switch to another cursor model — capability should be reseeded.
        update_selected_model(&mut s, "cursor/claude-sonnet-4");
        assert_eq!(s.model, "cursor/claude-sonnet-4");
        let cap = s.model_capability.unwrap();
        assert_eq!(cap.model_id, "cursor/claude-sonnet-4");
        assert!(!cap.native_tools);
    }

    #[test]
    fn update_selected_model_switch_to_non_omp_on_omp_gateway_clears() {
        let mut s = Settings {
            model: "cursor/old-model".into(),
            base_url: crate::commands::omp_gateway::GATEWAY_BASE_URL.into(),
            model_capability: Some(ModelCapability {
                model_id: "cursor/old-model".into(),
                native_tools: false,
                strict_json: false,
                tested_at: "2025-01-01T00:00:00Z".into(),
            }),
            ..Default::default()
        };
        // Switch to a non-OMP model while still on the OMP base URL.
        // apply_omp_capability should not match because the model doesn't
        // start with opencode-go/ or cursor/.
        update_selected_model(&mut s, "openai/gpt-4");
        assert_eq!(s.model, "openai/gpt-4");
        assert!(s.model_capability.is_none());
    }

    #[test]
    fn update_selected_model_non_omp_gateway_leaves_capability_none() {
        let mut s = Settings {
            model: "anthropic/claude-sonnet-4".into(),
            base_url: "https://openrouter.ai/api/v1".into(),
            model_capability: Some(ModelCapability {
                model_id: "anthropic/claude-sonnet-4".into(),
                native_tools: true,
                strict_json: true,
                tested_at: "2025-01-01T00:00:00Z".into(),
            }),
            ..Default::default()
        };
        update_selected_model(&mut s, "anthropic/claude-sonnet-4");
        // Same model on non-OMP — capability unchanged.
        let cap = s.model_capability.unwrap();
        assert!(cap.native_tools);

        let mut s2 = Settings {
            model: "anthropic/claude-opus-4".into(),
            base_url: "https://openrouter.ai/api/v1".into(),
            model_capability: Some(ModelCapability {
                model_id: "anthropic/claude-opus-4".into(),
                native_tools: true,
                strict_json: true,
                tested_at: "2025-01-01T00:00:00Z".into(),
            }),
            ..Default::default()
        };
        update_selected_model(&mut s2, "openai/gpt-4");
        // Different model on non-OMP — capability cleared.
        assert_eq!(s2.model, "openai/gpt-4");
        assert!(s2.model_capability.is_none());
    }

    #[test]
    fn settings_view_exposes_model_profiles_never_key() {
        let mut s = Settings {
            model: "gpt-4".into(),
            openrouter_api_key: "secret".into(),
            ..Default::default()
        };
        s.model_profiles = Some(ModelProfiles {
            worker: Some(worker_profile()),
            verifier: None,
            fallbacks: vec![],
        });
        let v = settings_view_json(&s, "9.9.9");
        // The raw key never leaks; only a boolean presence flag.
        assert_eq!(v.get("has_key").and_then(|b| b.as_bool()), Some(true));
        assert!(v.get("openrouter_api_key").is_none());
        // Provider base must round-trip to the UI (OpenCode Go / custom).
        assert!(v.get("base_url").and_then(|b| b.as_str()).is_some());
        assert!(v
            .get("subscription_providers_enabled")
            .and_then(|b| b.as_bool())
            .is_some());
        // The role profiles round-trip out to the UI.
        let worker_model = v
            .get("model_profiles")
            .and_then(|p| p.get("worker"))
            .and_then(|w| w.get("model"))
            .and_then(|m| m.as_str());
        assert_eq!(worker_model, Some("deepseek-chat"));
        assert_eq!(v.get("version").and_then(|x| x.as_str()), Some("9.9.9"));
    }
    /// LIVE (network + configured key): the mini model must SEE — a solid
    /// red 8x8 PNG rides the multimodal content array through the exact
    /// production body builder (`completion_body` → `complete_once_vision`).
    /// Run: cargo test --lib live_vision_red_png_mini -- --ignored --nocapture
    #[test]
    #[ignore]
    fn live_vision_red_png_mini() {
        let Some(key) = crate::commands::secrets::get_api_key() else {
            panic!("no API key in the credential store");
        };
        let png = "iVBORw0KGgoAAAANSUhEUgAAAAgAAAAICAIAAABLbSncAAAAEklEQVR4nGP4z8CAFWEXHbQSACj/P8Fu7N9hAAAAAElFTkSuQmCC";
        let url = format!("data:image/png;base64,{png}");
        let out = complete_once_vision(
            &key,
            "openai/gpt-4.1-mini",
            "https://openrouter.ai/api/v1/chat/completions",
            "You answer in one lowercase word.",
            "What color is this image?",
            &[url],
            10,
        )
        .expect("vision call");
        println!("mini saw: {out}");
        assert!(
            out.to_lowercase().contains("red"),
            "expected 'red', got: {out}"
        );
    }

    #[test]
    fn settings_persist_round_trips_model_profiles() {
        // read_settings/write_settings persist via serde; prove the profiles
        // survive a serialize→deserialize cycle (the on-disk round-trip).
        let mut s = Settings::default();
        s.model_profiles = Some(ModelProfiles {
            worker: Some(worker_profile()),
            verifier: None,
            fallbacks: vec![],
        });
        let json = serde_json::to_string(&s).unwrap();
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(back.model_profiles, s.model_profiles);
        // Absent profiles deserialize to None (orchestrator-only default).
        let bare: Settings = serde_json::from_str("{}").unwrap();
        assert!(bare.model_profiles.is_none());
    }
}
