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
    /// EDGAR contact (email) for the SEC User-Agent (2.1 / 3.6).
    #[serde(default)]
    pub edgar_contact: String,
    /// Default output folder for generated workbooks (3.2 / 3.6).
    #[serde(default)]
    pub out_dir: String,
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

/// Return `{ has_key, model, …, model_capability }` — never the raw key.
#[tauri::command(rename_all = "snake_case")]
pub fn load_settings(app: tauri::AppHandle) -> AppResult<String> {
    let s = read_settings(&app);
    Ok(serde_json::json!({
        "has_key": !s.openrouter_api_key.trim().is_empty(),
        "model": s.model,
        "edgar_contact": s.edgar_contact,
        "out_dir": s.out_dir,
        "mcp_command": s.mcp_command,
        "mcp_args": s.mcp_args,
        "version": app.package_info().version.to_string(),
        "model_capability": s.model_capability,
    })
    .to_string())
}

/// Save settings. A blank `api_key` keeps the existing one (so the frontend can
/// send blank to change only the model). A blank `model` keeps the existing one.
/// Non-empty `api_key` is written **only** to the OS credential store.
#[tauri::command(rename_all = "snake_case")]
pub fn save_settings(
    app: tauri::AppHandle,
    api_key: String,
    model: String,
    edgar_contact: Option<String>,
    out_dir: Option<String>,
    mcp_command: Option<String>,
    mcp_args: Option<Vec<String>>,
) -> AppResult<String> {
    let mut s = read_settings(&app);
    if !api_key.trim().is_empty() {
        crate::commands::secrets::set_api_key(api_key.trim()).map_err(AppError::Config)?;
        // Memory copy for this process; never persisted.
        s.openrouter_api_key = api_key.trim().to_string();
    }
    if !model.trim().is_empty() {
        s.model = model.trim().to_string();
    }
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
    write_settings(&app, &s)?;
    Ok(serde_json::json!({ "ok": true }).to_string())
}

/// Fetch the live OpenRouter model catalog using the saved key.
/// Returns a JSON array of `{ id, name, context_length, pricing, supported_parameters,
/// native_tools, strict_json }` for UI badges. Catalog fetch is NOT a capability
/// probe — only [`test_model`] writes `model_capability` (Phase 1.3).
#[tauri::command(rename_all = "snake_case")]
pub async fn list_models(app: tauri::AppHandle) -> AppResult<String> {
    // Network fetch — run off the IPC thread.
    tauri::async_runtime::spawn_blocking(move || {
        let s = read_settings(&app);
        if s.openrouter_api_key.trim().is_empty() {
            return Err(AppError::Config(
                "No OpenRouter API key set. Add one in Settings first.".into(),
            ));
        }
        let models = fm_extract::list_openrouter_models(s.openrouter_api_key.trim())
            .map_err(|e| AppError::Engine(format!("OpenRouter model fetch failed: {e}")))?;
        // Enrich each entry with the boolean badges the UI shows. Do NOT write
        // model_capability here — that is reserved for Test model.
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
                })
            })
            .collect();
        serde_json::to_string(&enriched).map_err(|e| AppError::Engine(e.to_string()))
    })
    .await
    .map_err(|e| AppError::Engine(format!("model fetch task failed: {e}")))?
}

/// Explicit Test-model capability probe (Phase 1.3).
///
/// Runs **bounded live requests** against OpenRouter for the selected model:
/// 1. Catalog lookup for advertised parameters (cheap filter).
/// 2. If advertised tools → one non-streaming tool-call probe with
///    `provider.require_parameters:true`. Success ⇒ `native_tools=true`.
/// 3. If advertised structured outputs → one non-streaming json_schema probe
///    with `provider.require_parameters:true`. Success ⇒ `strict_json=true`.
///
/// Only successful probe responses are cached. A failed/missing probe leaves
/// that capability false (or clears the cache entirely if the model is
/// unknown / auth fails) so callers fall back to app routing + plain JSON.
#[tauri::command(rename_all = "snake_case")]
pub async fn test_model(app: tauri::AppHandle, model_id: Option<String>) -> AppResult<String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut s = read_settings(&app);
        if s.openrouter_api_key.trim().is_empty() {
            return Err(AppError::Config(
                "No OpenRouter API key set. Add one in Settings first.".into(),
            ));
        }
        let key = s.openrouter_api_key.trim().to_string();
        let wanted = model_id
            .as_deref()
            .map(str::trim)
            .filter(|m| !m.is_empty())
            .unwrap_or(s.model.as_str())
            .to_string();

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

        // Live probes only for capabilities the catalog advertises. An
        // unsupported-parameter response (400/404) means the capability is
        // false; auth/rate-limit/transport failures clear the cache and error.
        let native_tools = if m.native_tools() {
            match probe_tools(&key, &m.id) {
                Ok(ok) => ok,
                Err(e) => {
                    clear_matching_cache(&mut s);
                    return Err(AppError::Engine(e));
                }
            }
        } else {
            false
        };
        let strict_json = if m.strict_json() {
            match probe_strict_json(&key, &m.id) {
                Ok(ok) => ok,
                Err(e) => {
                    clear_matching_cache(&mut s);
                    return Err(AppError::Engine(e));
                }
            }
        } else {
            false
        };

        let cap = ModelCapability {
            model_id: m.id.clone(),
            native_tools,
            strict_json,
            tested_at: chrono_like_now(),
        };
        s.model = m.id.clone();
        s.model_capability = Some(cap.clone());
        write_settings(&app, &s)?;
        serde_json::to_string(&cap).map_err(|e| AppError::Engine(e.to_string()))
    })
    .await
    .map_err(|e| AppError::Engine(format!("model probe task failed: {e}")))?
}

const OPENROUTER_CHAT_URL: &str = "https://openrouter.ai/api/v1/chat/completions";

/// Probe native tool-calling by **forcing** the `ping` function and verifying
/// `choices[0].message.tool_calls` contains a `ping` entry. HTTP 2xx alone is
/// not enough; unsupported-parameter responses map to `Ok(false)`.
fn probe_tools(api_key: &str, model: &str) -> Result<bool, String> {
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
        "parallel_tool_calls": false,
        "max_tokens": 64,
        "temperature": 0,
        "stream": false,
        "provider": { "require_parameters": true }
    });
    match post_probe_json(api_key, &body)? {
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
fn probe_strict_json(api_key: &str, model: &str) -> Result<bool, String> {
    let body = serde_json::json!({
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
        "stream": false,
        "provider": { "require_parameters": true }
    });
    match post_probe_json(api_key, &body)? {
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
fn post_probe_json(api_key: &str, body: &serde_json::Value) -> Result<ProbeOutcome, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| format!("probe client: {e}"))?;
    let resp = client
        .post(OPENROUTER_CHAT_URL)
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
