//! Personal subscription providers — on by default.
//!
//! - OpenCode Go — chat-ready through the local OMP auth-gateway
//!   (`http://127.0.0.1:4000/v1`), reusing `opencode-go` credentials from
//!   `~/.omp/agent/agent.db`.
//! - Cursor — chat-ready through the same local OMP auth-gateway.
//!   Both providers keep subscription credentials out of finmodel requests.
//!
//! Opt out with `FINMODEL_DISABLE_SUBSCRIPTION_PROVIDERS=1`. Legacy
//! `FINMODEL_ENABLE_SUBSCRIPTION_PROVIDERS=0/false/off` also disables.

use serde_json::json;

use crate::error::{AppError, AppResult};

pub const OPENCODE_GO_MODEL: &str = "opencode-go/grok-4.5";

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
    let found_go = find_opencode_go_credential().is_some();
    let mut out = vec![json!({
        "id": "opencode-go",
        "name": "OpenCode Go (personal)",
        "base": crate::commands::omp_gateway::GATEWAY_BASE_URL,
        "auth": "omp_auth",
        "chat_ready": found_go,
        "key_found_locally": found_go,
        "note": "Personal-use only. Routes chat through the local OMP auth-gateway using OMP's opencode-go credential.",
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
pub struct FoundCredential {
    pub source: String,
}

/// Find metadata for the OpenCode Go credential owned by OMP's auth broker.
///
/// The local gateway reads `~/.omp/agent/agent.db`; finmodel checks only that
/// an enabled credential row exists and never reads its secret `data` column.
pub fn find_opencode_go_credential() -> Option<FoundCredential> {
    let path = omp_agent_db_path()?;
    if !omp_has_api_key("opencode-go") {
        return None;
    }
    Some(FoundCredential {
        source: format!("omp-db:{}", path.display()),
    })
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

/// Check whether OMP owns an enabled static API-key credential for `provider`.
fn omp_has_api_key(provider: &str) -> bool {
    let Some(path) = omp_agent_db_path() else {
        return false;
    };
    if !path.exists() {
        return false;
    }
    let Ok(conn) =
        rusqlite::Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
    else {
        return false;
    };
    omp_has_api_key_in(&conn, provider)
}

fn omp_has_api_key_in(conn: &rusqlite::Connection, provider: &str) -> bool {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM auth_credentials \
         WHERE provider = ?1 AND disabled_cause IS NULL \
         AND credential_type = 'api_key')",
        [provider],
        |row| row.get::<_, bool>(0),
    )
    .unwrap_or(false)
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

/// Parse Cursor model ids from the local OMP auth-gateway's OpenAI catalog.
/// The gateway is the chat path itself, so Settings never needs to launch the
/// terminal-oriented `omp models` CLI just to populate a dropdown.
pub fn parse_cursor_gateway_models_json(text: &str) -> Result<Vec<String>, String> {
    let v: serde_json::Value =
        serde_json::from_str(text).map_err(|e| format!("gateway model json decode: {e}"))?;
    let data = v
        .get("data")
        .and_then(|models| models.as_array())
        .ok_or_else(|| "gateway model json missing data array".to_string())?;
    let ids: Vec<String> = data
        .iter()
        .filter(|model| model.get("owned_by").and_then(|owner| owner.as_str()) == Some("cursor"))
        .filter_map(|model| model.get("id").and_then(|id| id.as_str()))
        .filter_map(|id| id.strip_prefix("cursor/"))
        .map(str::to_string)
        .collect();
    if ids.is_empty() {
        return Err("gateway model catalog contains no Cursor models".into());
    }
    Ok(ids)
}

/// Read Cursor's live models from the local OMP auth-gateway. This never starts
/// a process: opening Settings must not launch a terminal as a side effect.
pub fn probe_cursor_models_via_omp() -> Result<(usize, Vec<String>), String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| format!("gateway client: {e}"))?;
    let response = client
        .get(format!(
            "{}/models",
            crate::commands::omp_gateway::GATEWAY_BASE_URL
        ))
        .bearer_auth(crate::commands::omp_gateway::GATEWAY_DUMMY_BEARER)
        .send()
        .map_err(|e| format!("gateway /models: {e}"))?;
    if !response.status().is_success() {
        return Err(format!("gateway /models HTTP {}", response.status()));
    }
    let ids = parse_cursor_gateway_models_json(
        &response
            .text()
            .map_err(|e| format!("gateway model body: {e}"))?,
    )?;
    Ok((ids.len(), ids))
}

/// Parse `omp models cursor --json` into (count, complete model ids).
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
    Ok((ids.len(), ids))
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
    let found = find_opencode_go_credential();
    let opencode_ready = found.is_some();
    let opencode_reason = if !subscription_providers_enabled() {
        format!("Disabled via {DISABLE_ENV}=1.")
    } else if let Some(f) = &found {
        format!(
            "OpenCode Go credential available through OMP ({}). Select OpenCode Go to use the local gateway.",
            f.source
        )
    } else {
        "No OpenCode Go credential found in OMP. Click Connect OpenCode Go and finish the terminal login."
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

/// Import an OpenCode Go credential and point finmodel at OMP's local gateway.
/// Gated; no-op surface when disabled.
#[tauri::command(rename_all = "snake_case")]
pub fn import_opencode_go_key(app: tauri::AppHandle) -> AppResult<String> {
    if !subscription_providers_enabled() {
        return Err(AppError::Config(format!(
            "Subscription providers are disabled ({DISABLE_ENV}=1)."
        )));
    }
    let found = find_opencode_go_credential().ok_or_else(|| {
        AppError::Config(
            "No OpenCode Go credential found in OMP. Click Connect OpenCode Go and finish the terminal login, then Import again."
                .into(),
        )
    })?;
    crate::commands::omp_gateway::ensure_cursor_gateway().map_err(AppError::Engine)?;
    let mut s = crate::commands::settings::read_settings(&app);
    s.base_url = crate::commands::omp_gateway::GATEWAY_BASE_URL.to_string();
    s.model = OPENCODE_GO_MODEL.to_string();
    // Provider changed — drop stale capability cache.
    s.model_capability = None;
    crate::commands::settings::write_settings(&app, &s)?;
    Ok(json!({
        "ok": true,
        "base_url": crate::commands::omp_gateway::GATEWAY_BASE_URL,
        "model": OPENCODE_GO_MODEL,
        "source": found.source,
        "has_key": false,
        "credential_owner": "omp",
    })
    .to_string())
}

/// Connect OpenCode Go: reuse OMP's credential when present; otherwise open an
/// interactive OMP login console that saves the pasted key into agent.db.
#[tauri::command(rename_all = "snake_case")]
pub fn connect_opencode_go(app: tauri::AppHandle) -> AppResult<String> {
    if !subscription_providers_enabled() {
        return Err(AppError::Config(format!(
            "Subscription providers are disabled ({DISABLE_ENV}=1)."
        )));
    }
    if let Some(found) = find_opencode_go_credential() {
        crate::commands::omp_gateway::ensure_cursor_gateway().map_err(AppError::Engine)?;
        let mut s = crate::commands::settings::read_settings(&app);
        s.base_url = crate::commands::omp_gateway::GATEWAY_BASE_URL.to_string();
        s.model = OPENCODE_GO_MODEL.to_string();
        s.model_capability = None;
        crate::commands::settings::write_settings(&app, &s)?;
        return Ok(json!({
            "ok": true,
            "chat_ready": true,
            "needs_auth": false,
            "base_url": crate::commands::omp_gateway::GATEWAY_BASE_URL,
            "model": OPENCODE_GO_MODEL,
            "source": found.source,
            "has_key": false,
            "credential_owner": "omp",
            "guidance": "OpenCode Go connected through the local OMP auth-gateway.",
        })
        .to_string());
    }

    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_url(OPENCODE_AUTH_URL, None::<&str>)
        .map_err(|e| AppError::Engine(format!("failed to open OpenCode Go login page: {e}")))?;
    crate::commands::omp_gateway::start_opencode_go_login().map_err(AppError::Engine)?;
    Ok(json!({
        "ok": true,
        "chat_ready": false,
        "needs_auth": true,
        "waiting": true,
        "auth_url": OPENCODE_AUTH_URL,
        "base_url": crate::commands::omp_gateway::GATEWAY_BASE_URL,
        "model": OPENCODE_GO_MODEL,
        "guidance": "OpenCode Go login opened in a terminal. Sign in on opencode.ai, paste the key into that OMP terminal, and Settings will connect automatically. Finmodel never receives the credential.",
    })
    .to_string())
}

/// Migrate an already-selected OpenCode Go model from the obsolete direct
/// endpoint to OMP's gateway. Never copies or overwrites credentials.
pub fn maybe_auto_import_opencode_go(app: &tauri::AppHandle) -> Option<String> {
    if !subscription_providers_enabled() {
        return None;
    }
    let found = find_opencode_go_credential()?;
    let mut s = crate::commands::settings::read_settings(app);
    if !s.model.trim().starts_with("opencode-go/") {
        return None;
    }
    s.base_url = crate::commands::omp_gateway::GATEWAY_BASE_URL.to_string();
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
    let (count, ids) = probe_cursor_models_via_omp().map_err(AppError::Engine)?;
    let sample = ids.iter().take(12).cloned().collect::<Vec<_>>();
    let models = ids
        .iter()
        .map(|id| {
            let qualified = crate::commands::omp_gateway::qualify_cursor_model(id);
            json!({ "id": qualified, "name": id })
        })
        .collect::<Vec<_>>();
    Ok(json!({
        "ok": true,
        "count": count,
        "models": models,
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
    fn opencode_readiness_checks_metadata_without_secret_data() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE auth_credentials (
                provider TEXT NOT NULL,
                credential_type TEXT NOT NULL,
                disabled_cause TEXT
            );
            INSERT INTO auth_credentials VALUES ('opencode-go', 'api_key', NULL);",
        )
        .unwrap();
        assert!(omp_has_api_key_in(&conn, "opencode-go"));
        assert!(!omp_has_api_key_in(&conn, "cursor"));
    }

    #[test]
    fn parses_only_cursor_models_from_gateway_catalog() {
        let text = r#"{"data":[
            {"id":"cursor/claude-4.6-sonnet-medium","owned_by":"cursor"},
            {"id":"cursor/cursor-grok-4.5-medium","owned_by":"cursor"},
            {"id":"openrouter/anthropic/claude","owned_by":"openrouter"}
        ]}"#;
        assert_eq!(
            parse_cursor_gateway_models_json(text).unwrap(),
            vec![
                "claude-4.6-sonnet-medium".to_string(),
                "cursor-grok-4.5-medium".to_string(),
            ]
        );
    }

    #[test]
    fn parse_omp_cursor_models_returns_complete_catalog() {
        let text = r#"{"models":[
            {"id":"claude-4-sonnet"},{"id":"gpt-5.2"},{"name":"composer-1"},
            {"id":"m-4"},{"id":"m-5"},{"id":"m-6"},{"id":"m-7"},
            {"id":"m-8"},{"id":"m-9"},{"id":"m-10"},{"id":"m-11"},
            {"id":"m-12"},{"id":"cursor-grok-4.5-medium"}
        ]}"#;
        let (n, models) = parse_omp_cursor_models_json(text).unwrap();
        assert_eq!(n, 13);
        assert_eq!(models.len(), 13);
        assert_eq!(models[0], "claude-4-sonnet");
        assert_eq!(models[12], "cursor-grok-4.5-medium");
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
