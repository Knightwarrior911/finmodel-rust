//! Personal subscription providers — on by default.
//!
//! - OpenCode Go (`https://opencode.ai/zen/go/v1`) — selectable chat provider;
//!   key from env / OpenCode `auth.json` / OMP `agent.db` (auto-imported on
//!   first launch when the keyring is empty).
//! - Cursor — chat-ready via local OMP auth-gateway (`http://127.0.0.1:4000/v1`).
//!   Reuses OAuth from `~/.omp/agent/agent.db` (never raw api2.cursor.sh).
//!
//! Opt out with `FINMODEL_DISABLE_SUBSCRIPTION_PROVIDERS=1`. Legacy
//! `FINMODEL_ENABLE_SUBSCRIPTION_PROVIDERS=0/false/off` also disables.

use serde_json::json;

use crate::error::{AppError, AppResult};

/// OpenCode Go OpenAI-compatible root (chat = `{base}/chat/completions`).
pub const OPENCODE_GO_BASE_URL: &str = "https://opencode.ai/zen/go/v1";

/// OpenCode console page where the user copies an API key (OMP-compatible flow).
pub const OPENCODE_AUTH_URL: &str = "https://opencode.ai/auth";

const DISABLE_ENV: &str = "FINMODEL_DISABLE_SUBSCRIPTION_PROVIDERS";
const LEGACY_ENABLE_ENV: &str = "FINMODEL_ENABLE_SUBSCRIPTION_PROVIDERS";

fn env_truthy(v: &str) -> bool {
    matches!(
        v.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

fn env_falsy(v: &str) -> bool {
    matches!(
        v.trim().to_ascii_lowercase().as_str(),
        "0" | "false" | "no" | "off"
    )
}

/// True when personal subscription providers appear in Settings.
/// On by default — no launch-time env required. Opt out with
/// `FINMODEL_DISABLE_SUBSCRIPTION_PROVIDERS=1` (or legacy enable=0/false/off).
pub fn subscription_providers_enabled() -> bool {
    if let Ok(v) = std::env::var(DISABLE_ENV) {
        if env_truthy(&v) {
            return false;
        }
    }
    if let Ok(v) = std::env::var(LEGACY_ENABLE_ENV) {
        if env_falsy(&v) {
            return false;
        }
    }
    true
}

/// Chat-ready provider catalog (OpenCode Go + Cursor via local OMP gateway).
pub fn gated_providers() -> Vec<serde_json::Value> {
    if !subscription_providers_enabled() {
        return Vec::new();
    }
    let found_go = find_opencode_go_key().is_some();
    let mut out = vec![json!({
        "id": "opencode-go",
        "name": "OpenCode Go (personal)",
        "base": OPENCODE_GO_BASE_URL,
        "auth": "api_key",
        // Selectable once a key is findable or already in the keyring.
        // Connect OpenCode Go guides paste/import when missing.
        "chat_ready": found_go || crate::commands::secrets::get_api_key().is_some(),
        "key_found_locally": found_go,
        "note": "Personal-use only. Connect opens opencode.ai/auth when no key is found; otherwise Import reuses env/auth.json/OMP.",
    })];
    let cur = cursor_omp_status();
    out.push(json!({
        "id": "cursor",
        "name": "Cursor (via OMP gateway)",
        "base": crate::commands::omp_gateway::GATEWAY_BASE_URL,
        "auth": "omp_oauth",
        "chat_ready": cur.present && !cur.expired,
        "oauth_present": cur.present,
        "oauth_expired": cur.expired,
        "note": "Routes OpenAI-compatible chat through local omp auth-gateway. Connect Cursor reuses ~/.omp/agent/agent.db or runs omp login. Does not overwrite your API key.",
    }));
    out
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundKey {
    pub key: String,
    pub source: String,
}

/// Pull an OpenCode API key from env, OpenCode auth.json, or OMP agent.db.
/// Does not touch the OS credential store — callers decide whether to save.
pub fn find_opencode_go_key() -> Option<FoundKey> {
    if let Ok(v) = std::env::var("OPENCODE_API_KEY") {
        let v = v.trim().to_string();
        if !v.is_empty() {
            return Some(FoundKey {
                key: v,
                source: "env:OPENCODE_API_KEY".into(),
            });
        }
    }
    for path in opencode_auth_paths() {
        if !path.exists() {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        if let Some(key) = parse_opencode_auth_key(&text) {
            return Some(FoundKey {
                key,
                source: format!("file:{}", path.display()),
            });
        }
    }
    if let Some(key) = read_omp_api_key("opencode-go") {
        return Some(FoundKey {
            key,
            source: format!("omp-db:{}", omp_agent_db_path()?.display()),
        });
    }
    None
}

/// Candidate OpenCode CLI auth.json locations (Windows + XDG-style).
pub fn opencode_auth_paths() -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    if let Some(home) = dirs_home() {
        out.push(
            home.join(".local")
                .join("share")
                .join("opencode")
                .join("auth.json"),
        );
        out.push(home.join(".config").join("opencode").join("auth.json"));
        out.push(home.join(".opencode").join("auth.json"));
        out.push(
            home.join("AppData")
                .join("Roaming")
                .join("opencode")
                .join("auth.json"),
        );
        out.push(
            home.join("AppData")
                .join("Local")
                .join("opencode")
                .join("auth.json"),
        );
    }
    out
}

fn dirs_home() -> Option<std::path::PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(std::path::PathBuf::from)
}

fn omp_agent_db_path() -> Option<std::path::PathBuf> {
    let home = dirs_home()?;
    Some(home.join(".omp").join("agent").join("agent.db"))
}

/// Read a static API key for `provider` from OMP's SQLite auth vault.
pub fn read_omp_api_key(provider: &str) -> Option<String> {
    let path = omp_agent_db_path()?;
    if !path.exists() {
        return None;
    }
    let conn =
        rusqlite::Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .ok()?;
    let data: String = conn
        .query_row(
            "SELECT data FROM auth_credentials \
             WHERE provider = ?1 AND disabled_cause IS NULL \
             AND credential_type = 'api_key' \
             ORDER BY id DESC LIMIT 1",
            [provider],
            |row| row.get(0),
        )
        .ok()?;
    let v: serde_json::Value = serde_json::from_str(&data).ok()?;
    api_key_from_auth_entry(&v)
}

/// Cursor OAuth snapshot from OMP agent.db (never returns token material).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CursorOmpStatus {
    pub present: bool,
    pub expired: bool,
    pub expires_ms: Option<i64>,
    pub source: String,
}

pub fn cursor_omp_status() -> CursorOmpStatus {
    let Some(path) = omp_agent_db_path() else {
        return CursorOmpStatus {
            present: false,
            expired: false,
            expires_ms: None,
            source: "no-home".into(),
        };
    };
    if !path.exists() {
        return CursorOmpStatus {
            present: false,
            expired: false,
            expires_ms: None,
            source: format!("missing:{}", path.display()),
        };
    }
    let Ok(conn) =
        rusqlite::Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
    else {
        return CursorOmpStatus {
            present: false,
            expired: false,
            expires_ms: None,
            source: format!("unreadable:{}", path.display()),
        };
    };
    let Ok(data) = conn.query_row(
        "SELECT data FROM auth_credentials \
         WHERE provider = 'cursor' AND disabled_cause IS NULL \
         AND credential_type = 'oauth' \
         ORDER BY id DESC LIMIT 1",
        [],
        |row| row.get::<_, String>(0),
    ) else {
        return CursorOmpStatus {
            present: false,
            expired: false,
            expires_ms: None,
            source: format!("file:{}", path.display()),
        };
    };
    let v: serde_json::Value = serde_json::from_str(&data).unwrap_or(json!({}));
    let has_access = v
        .get("access")
        .and_then(|x| x.as_str())
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false);
    let expires_ms = v.get("expires").and_then(|x| x.as_i64());
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let expired = match expires_ms {
        Some(e) => now >= e,
        None => false,
    };
    CursorOmpStatus {
        present: has_access,
        expired,
        expires_ms,
        source: format!("omp-db:{}", path.display()),
    }
}

/// Extract an API key from OpenCode's `auth.json` map.
///
/// Prefers provider ids `opencode-go`, then `opencode-zen`, then `opencode`.
/// Accepts `{ "type": "api", "key": "..." }` (and a few `apiKey`/`token` aliases).
/// OAuth entries without a static key are ignored.
pub fn parse_opencode_auth_key(text: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(text).ok()?;
    let obj = v.as_object()?;
    for id in ["opencode-go", "opencode-zen", "opencode"] {
        if let Some(entry) = obj.get(id) {
            if let Some(k) = api_key_from_auth_entry(entry) {
                return Some(k);
            }
        }
    }
    // Last resort: any entry that looks like a static API key provider.
    for (_id, entry) in obj {
        if let Some(k) = api_key_from_auth_entry(entry) {
            return Some(k);
        }
    }
    None
}

fn api_key_from_auth_entry(entry: &serde_json::Value) -> Option<String> {
    let obj = entry.as_object()?;
    let ty = obj
        .get("type")
        .and_then(|t| t.as_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    // Prefer explicit api entries; still accept a bare key field.
    // OMP agent.db api_key rows often omit `type` and only store `{key}`.
    if !ty.is_empty() && ty != "api" && ty != "api_key" {
        return None;
    }
    for field in ["key", "apiKey", "api_key", "token"] {
        if let Some(k) = obj.get(field).and_then(|x| x.as_str()) {
            let k = k.trim();
            if !k.is_empty() {
                return Some(k.to_string());
            }
        }
    }
    None
}

/// Live Cursor model probe via the installed `omp` CLI (reuses agent.db login).
/// Returns redacted summary only — never tokens.
pub fn probe_cursor_models_via_omp() -> Result<(usize, Vec<String>), String> {
    let output = std::process::Command::new("omp")
        .args(["models", "cursor", "--json"])
        .output()
        .map_err(|e| format!("omp not runnable: {e}"))?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "omp models cursor failed (status {:?}): {}",
            output.status.code(),
            err.chars().take(240).collect::<String>()
        ));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    parse_omp_cursor_models_json(&text)
}

/// Parse `omp models cursor --json` into (count, sample ids).
pub fn parse_omp_cursor_models_json(text: &str) -> Result<(usize, Vec<String>), String> {
    let v: serde_json::Value =
        serde_json::from_str(text).map_err(|e| format!("omp json decode: {e}"))?;
    let arr = v
        .get("models")
        .and_then(|m| m.as_array())
        .or_else(|| v.as_array())
        .ok_or_else(|| "omp json missing models array".to_string())?;
    let ids: Vec<String> = arr
        .iter()
        .filter_map(|m| {
            m.get("id")
                .or_else(|| m.get("name"))
                .or_else(|| m.get("model"))
                .and_then(|x| x.as_str())
                .map(|s| s.to_string())
        })
        .collect();
    let sample = ids.iter().take(12).cloned().collect();
    Ok((ids.len(), sample))
}

/// `{ enabled, providers, cursor }` — never secrets.
#[tauri::command(rename_all = "snake_case")]
pub fn subscription_providers_status() -> AppResult<String> {
    let cur = cursor_omp_status();
    let chat_ready = cur.present && !cur.expired;
    let cursor_reason = if !subscription_providers_enabled() {
        format!("Disabled via {DISABLE_ENV}=1.")
    } else if !cur.present {
        "No Cursor OAuth yet. Click Connect Cursor — omp opens the browser login (no pre-setup)."
            .into()
    } else if cur.expired {
        "Cursor OAuth expired. Click Connect Cursor to re-login via omp.".into()
    } else {
        "Cursor logged in via OMP. Select Cursor or click Use Cursor to chat.".into()
    };
    let found = find_opencode_go_key();
    let opencode_ready = found.is_some() || crate::commands::secrets::get_api_key().is_some();
    let opencode_reason = if !subscription_providers_enabled() {
        format!("Disabled via {DISABLE_ENV}=1.")
    } else if let Some(ref f) = found {
        format!(
            "OpenCode Go key available locally ({}). Import or Connect to use it.",
            f.source
        )
    } else if crate::commands::secrets::get_api_key().is_some() {
        "A key is already saved in Settings. Select OpenCode Go to use it against zen/go.".into()
    } else {
        "No OpenCode Go key found. Click Connect OpenCode Go to open opencode.ai/auth, then paste the key in Settings."
            .into()
    };
    Ok(json!({
        "enabled": subscription_providers_enabled(),
        "providers": gated_providers(),
        "env": DISABLE_ENV,
        "enabled_by_default": true,
        "cursor": {
            "available": chat_ready,
            "present": cur.present,
            "expired": cur.expired,
            "expires_ms": cur.expires_ms,
            "source": cur.source,
            "chat_ready": chat_ready,
            "reason": cursor_reason,
        },
        "opencode": {
            "chat_ready": opencode_ready,
            "key_found_locally": found.is_some(),
            "source": found.as_ref().map(|f| f.source.clone()),
            "auth_url": OPENCODE_AUTH_URL,
            "reason": opencode_reason,
        },
    })
    .to_string())
}

/// Import an OpenCode Go API key into finmodel's OS keyring and point
/// `base_url` at the Go endpoint. Gated; no-op surface when disabled.
#[tauri::command(rename_all = "snake_case")]
pub fn import_opencode_go_key(app: tauri::AppHandle) -> AppResult<String> {
    if !subscription_providers_enabled() {
        return Err(AppError::Config(format!(
            "Subscription providers are disabled ({DISABLE_ENV}=1)."
        )));
    }
    let found = find_opencode_go_key().ok_or_else(|| {
        AppError::Config(
            "No OpenCode API key found. Click Connect OpenCode Go to open              opencode.ai/auth, paste the key in Settings, or save it to              OPENCODE_API_KEY / OpenCode auth.json / OMP agent.db then Import."
                .into(),
        )
    })?;
    crate::commands::secrets::set_api_key(&found.key).map_err(AppError::Config)?;
    let mut s = crate::commands::settings::read_settings(&app);
    s.openrouter_api_key = found.key;
    s.base_url = OPENCODE_GO_BASE_URL.to_string();
    // Provider changed — drop stale capability cache.
    s.model_capability = None;
    crate::commands::settings::write_settings(&app, &s)?;
    Ok(json!({
        "ok": true,
        "base_url": OPENCODE_GO_BASE_URL,
        "source": found.source,
        "has_key": true,
    })
    .to_string())
}

/// Connect OpenCode Go: reuse a locally discoverable key when present; otherwise
/// open https://opencode.ai/auth and return paste guidance (no DIY OAuth).
#[tauri::command(rename_all = "snake_case")]
pub fn connect_opencode_go(app: tauri::AppHandle) -> AppResult<String> {
    if !subscription_providers_enabled() {
        return Err(AppError::Config(format!(
            "Subscription providers are disabled ({DISABLE_ENV}=1)."
        )));
    }
    if let Some(found) = find_opencode_go_key() {
        crate::commands::secrets::set_api_key(&found.key).map_err(AppError::Config)?;
        let mut s = crate::commands::settings::read_settings(&app);
        s.openrouter_api_key = found.key;
        s.base_url = OPENCODE_GO_BASE_URL.to_string();
        s.model_capability = None;
        crate::commands::settings::write_settings(&app, &s)?;
        return Ok(json!({
            "ok": true,
            "chat_ready": true,
            "needs_auth": false,
            "base_url": OPENCODE_GO_BASE_URL,
            "source": found.source,
            "has_key": true,
            "guidance": "OpenCode Go key imported and provider selected.",
        })
        .to_string());
    }

    use tauri_plugin_opener::OpenerExt;
    let _ = app.opener().open_url(OPENCODE_AUTH_URL, None::<&str>);
    // Point provider at Go so Save after paste is one step; no key written yet.
    let mut s = crate::commands::settings::read_settings(&app);
    s.base_url = OPENCODE_GO_BASE_URL.to_string();
    s.model_capability = None;
    let _ = crate::commands::settings::write_settings(&app, &s);
    Ok(json!({
        "ok": true,
        "chat_ready": false,
        "needs_auth": true,
        "auth_url": OPENCODE_AUTH_URL,
        "base_url": OPENCODE_GO_BASE_URL,
        "guidance": "Log in at opencode.ai/auth, copy your API key, paste it into Settings → API key, then Save. Or save the key to OpenCode/OMP and click Import OpenCode Go key.",
    })
    .to_string())
}

/// If the keyring has no API key yet, import OpenCode Go from local sources
/// and point `base_url` at Go. Never overwrites an existing key. Returns a
/// short source label when import happened.
pub fn maybe_auto_import_opencode_go(app: &tauri::AppHandle) -> Option<String> {
    if !subscription_providers_enabled() {
        return None;
    }
    if crate::commands::secrets::get_api_key().is_some() {
        return None;
    }
    let found = find_opencode_go_key()?;
    if crate::commands::secrets::set_api_key(&found.key).is_err() {
        return None;
    }
    let mut s = crate::commands::settings::read_settings(app);
    s.openrouter_api_key = found.key;
    s.base_url = OPENCODE_GO_BASE_URL.to_string();
    s.model_capability = None;
    if crate::commands::settings::write_settings(app, &s).is_err() {
        return None;
    }
    Some(found.source)
}

/// Probe Cursor usable models via OMP (GetUsableModels under the hood).
#[tauri::command(rename_all = "snake_case")]
pub fn probe_cursor_models() -> AppResult<String> {
    if !subscription_providers_enabled() {
        return Err(AppError::Config(format!(
            "Subscription providers are disabled ({DISABLE_ENV}=1)."
        )));
    }
    let cur = cursor_omp_status();
    if !cur.present {
        return Err(AppError::Config(
            "No Cursor OAuth in ~/.omp/agent/agent.db. Click Connect Cursor to log in via omp."
                .into(),
        ));
    }
    if cur.expired {
        return Err(AppError::Config(
            "Cursor OAuth expired in OMP agent.db — click Connect Cursor to re-login via omp."
                .into(),
        ));
    }
    let (count, sample) = probe_cursor_models_via_omp().map_err(AppError::Engine)?;
    Ok(json!({
        "ok": true,
        "count": count,
        "sample": sample,
        "source": cur.source,
        "chat_ready": true,
        "note": "Live GetUsableModels via omp succeeded. Chat uses local omp auth-gateway (not raw api2.cursor.sh). Prefer cursor/default or cursor/claude-* over bare composer-1.5.",
    })
    .to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_prefers_opencode_go_api_key() {
        let text = r#"{
            "xai": {"type":"oauth","access":"a","refresh":"r"},
            "opencode": {"type":"api","key":"sk-fallback"},
            "opencode-go": {"type":"api","key":"sk-go"}
        }"#;
        assert_eq!(parse_opencode_auth_key(text).as_deref(), Some("sk-go"));
    }

    #[test]
    fn parse_skips_oauth_only_entries() {
        let text = r#"{"xai":{"type":"oauth","access":"a","refresh":"r"}}"#;
        assert_eq!(parse_opencode_auth_key(text), None);
    }

    #[test]
    fn parse_accepts_opencode_zen_alias() {
        let text = r#"{"opencode-zen":{"type":"api","key":"sk-zen"}}"#;
        assert_eq!(parse_opencode_auth_key(text).as_deref(), Some("sk-zen"));
    }

    #[test]
    fn parse_omp_api_key_row_without_type() {
        // OMP agent.db stores `{ "key": "..." }` without a type field.
        assert_eq!(
            api_key_from_auth_entry(&json!({"key": "sk-from-omp"})).as_deref(),
            Some("sk-from-omp")
        );
    }

    #[test]
    fn parse_omp_cursor_models_json_counts_sample() {
        let text =
            r#"{"models":[{"id":"claude-4-sonnet"},{"id":"gpt-5.2"},{"name":"composer-1"}]}"#;
        let (n, sample) = parse_omp_cursor_models_json(text).unwrap();
        assert_eq!(n, 3);
        assert_eq!(sample[0], "claude-4-sonnet");
        assert!(sample.contains(&"composer-1".to_string()));
    }

    #[test]
    fn gated_providers_include_opencode_go_and_cursor_by_default() {
        // Default is enabled (unless DISABLE env is set in the test process).
        if !subscription_providers_enabled() {
            assert!(gated_providers().is_empty());
            return;
        }
        let providers = gated_providers();
        assert!(providers.len() >= 2);
        assert_eq!(
            providers[0].get("id").and_then(|x| x.as_str()),
            Some("opencode-go")
        );
        assert!(providers[0]
            .get("chat_ready")
            .and_then(|x| x.as_bool())
            .is_some());
        let cursor = providers
            .iter()
            .find(|p| p.get("id").and_then(|x| x.as_str()) == Some("cursor"))
            .expect("cursor provider present");
        assert_eq!(
            cursor.get("base").and_then(|x| x.as_str()),
            Some(crate::commands::omp_gateway::GATEWAY_BASE_URL)
        );
        // chat_ready depends on local OMP agent.db; only assert the field exists.
        assert!(cursor.get("chat_ready").and_then(|x| x.as_bool()).is_some());
    }

    #[test]
    fn subscription_providers_enabled_by_default() {
        // This process usually has neither env set → on.
        if std::env::var(DISABLE_ENV).is_err()
            && std::env::var(LEGACY_ENABLE_ENV)
                .map(|v| !env_falsy(&v))
                .unwrap_or(true)
        {
            assert!(subscription_providers_enabled());
        }
    }
}
