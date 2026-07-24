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

/// Subscription provider catalog. Stored credential metadata is diagnostic;
/// chat_ready becomes true only after the running gateway exposes live models.
pub fn gated_providers() -> Vec<serde_json::Value> {
    if !subscription_providers_enabled() {
        return Vec::new();
    }
    let gateway_valid = crate::commands::omp_gateway::validate_running_omp_services().is_ok();
    let found_go = find_opencode_go_credential().is_some();
    let go_ready = found_go && provider_live_ready("opencode-go", gateway_valid);
    let cur = cursor_omp_status();
    let cursor_ready = cur.reusable() && provider_live_ready("cursor", gateway_valid);
    vec![
        json!({
            "id": "opencode-go",
            "name": "OpenCode Go (personal)",
            "base": crate::commands::omp_gateway::GATEWAY_BASE_URL,
            "auth": "omp_auth",
            "chat_ready": go_ready,
            "credential_present": found_go,
            "key_found_locally": found_go,
            "note": "Personal-use only. A stored OMP login is not marked ready until the live gateway catalog succeeds.",
        }),
        json!({
            "id": "cursor",
            "name": "Cursor (via OMP gateway)",
            "base": crate::commands::omp_gateway::GATEWAY_BASE_URL,
            "auth": "omp_oauth",
            "chat_ready": cursor_ready,
            "credential_present": cur.present,
            "oauth_present": cur.present,
            "oauth_expired": cur.expired,
            "refresh_present": cur.refresh_present,
            "note": "A stored Cursor login can be refreshed by OMP; readiness requires a successful live Cursor catalog.",
        }),
    ]
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
    pub refresh_present: bool,
    pub source: String,
}

impl CursorOmpStatus {
    /// An expired access token is usable only when OMP can refresh it.
    pub fn reusable(&self) -> bool {
        self.present && (!self.expired || self.refresh_present)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CursorCredentialState {
    present: bool,
    expired: bool,
    expires_ms: Option<i64>,
    refresh_present: bool,
}

/// Project only credential metadata through SQLite's JSON functions. Token text
/// never crosses the database boundary into finmodel.
fn cursor_credential_state_in(
    conn: &rusqlite::Connection,
    now_ms: i64,
) -> Option<CursorCredentialState> {
    conn.query_row(
        "SELECT \
            COALESCE(length(trim(json_extract(data, '$.access'))), 0) > 0, \
            COALESCE(length(trim(json_extract(data, '$.refresh'))), 0) > 0, \
            CAST(json_extract(data, '$.expires') AS INTEGER) \
         FROM auth_credentials \
         WHERE provider = 'cursor' AND disabled_cause IS NULL \
         AND credential_type = 'oauth' \
         ORDER BY id DESC LIMIT 1",
        [],
        |row| {
            let access_present = row.get::<_, bool>(0)?;
            let refresh_present = row.get::<_, bool>(1)?;
            let expires_ms = row.get::<_, Option<i64>>(2)?;
            Ok(CursorCredentialState {
                present: access_present || refresh_present,
                expired: expires_ms.map(|expires| now_ms >= expires).unwrap_or(false),
                expires_ms,
                refresh_present,
            })
        },
    )
    .ok()
}

pub fn cursor_omp_status() -> CursorOmpStatus {
    let Some(path) = omp_agent_db_path() else {
        return CursorOmpStatus {
            present: false,
            expired: false,
            expires_ms: None,
            refresh_present: false,
            source: "no-home".into(),
        };
    };
    if !path.exists() {
        return CursorOmpStatus {
            present: false,
            expired: false,
            expires_ms: None,
            refresh_present: false,
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
            refresh_present: false,
            source: format!("unreadable:{}", path.display()),
        };
    };
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0);
    let Some(state) = cursor_credential_state_in(&conn, now_ms) else {
        return CursorOmpStatus {
            present: false,
            expired: false,
            expires_ms: None,
            refresh_present: false,
            source: format!("file:{}", path.display()),
        };
    };
    CursorOmpStatus {
        present: state.present,
        expired: state.expired,
        expires_ms: state.expires_ms,
        refresh_present: state.refresh_present,
        source: format!("omp-db:{}", path.display()),
    }
}

/// Parse one provider's complete model selectors from the OMP gateway catalog.
pub fn parse_provider_gateway_models_json(
    text: &str,
    provider: &str,
) -> Result<Vec<String>, String> {
    let value: serde_json::Value =
        serde_json::from_str(text).map_err(|e| format!("gateway model json decode: {e}"))?;
    let data = value
        .get("data")
        .and_then(|models| models.as_array())
        .ok_or_else(|| "gateway model json missing data array".to_string())?;
    let prefix = format!("{provider}/");
    let ids = data
        .iter()
        .filter(|model| model.get("owned_by").and_then(|owner| owner.as_str()) == Some(provider))
        .filter_map(|model| model.get("id").and_then(|id| id.as_str()))
        .filter(|id| id.starts_with(&prefix))
        .map(str::to_string)
        .collect::<Vec<_>>();
    if ids.is_empty() {
        return Err(format!(
            "gateway model catalog contains no {provider} models"
        ));
    }
    Ok(ids)
}

/// Parse bare Cursor model ids for the existing Cursor selector UI.
pub fn parse_cursor_gateway_models_json(text: &str) -> Result<Vec<String>, String> {
    Ok(parse_provider_gateway_models_json(text, "cursor")?
        .into_iter()
        .filter_map(|id| id.strip_prefix("cursor/").map(str::to_string))
        .collect())
}

pub fn resolve_opencode_go_model(saved: &str, available: &[String]) -> String {
    let saved = saved.trim();
    if available.iter().any(|model| model == saved) {
        return saved.to_string();
    }
    if available.iter().any(|model| model == OPENCODE_GO_MODEL) {
        return OPENCODE_GO_MODEL.to_string();
    }
    available
        .first()
        .cloned()
        .unwrap_or_else(|| OPENCODE_GO_MODEL.to_string())
}

fn read_gateway_catalog() -> Result<String, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| format!("gateway client: {e}"))?;
    let response = client
        .get(format!(
            "{}/models",
            crate::commands::omp_gateway::GATEWAY_BASE_URL
        ))
        .bearer_auth(crate::commands::omp_gateway::gateway_bearer()?)
        .send()
        .map_err(|e| format!("gateway /models: {e}"))?;
    if !response.status().is_success() {
        return Err(format!("gateway /models HTTP {}", response.status()));
    }
    response
        .text()
        .map_err(|e| format!("gateway model body: {e}"))
}

/// Read one provider's live models without starting local services.
pub fn probe_provider_models_via_omp(provider: &str) -> Result<Vec<String>, String> {
    parse_provider_gateway_models_json(&read_gateway_catalog()?, provider)
}

/// Read Cursor's live models from the local OMP auth-gateway. This never starts
/// a process: opening Settings must not launch a terminal as a side effect.
pub fn probe_cursor_models_via_omp() -> Result<(usize, Vec<String>), String> {
    let ids = probe_provider_models_via_omp("cursor")?
        .into_iter()
        .filter_map(|id| id.strip_prefix("cursor/").map(str::to_string))
        .collect::<Vec<_>>();
    Ok((ids.len(), ids))
}

fn provider_live_ready(provider: &str, gateway_valid: bool) -> bool {
    gateway_valid && probe_provider_models_via_omp(provider).is_ok()
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
    let enabled = subscription_providers_enabled();
    let gateway_valid =
        enabled && crate::commands::omp_gateway::validate_running_omp_services().is_ok();
    let cur = cursor_omp_status();
    let cursor_ready = enabled && cur.reusable() && provider_live_ready("cursor", gateway_valid);
    let cursor_reason = if !enabled {
        format!("Disabled via {DISABLE_ENV}=1.")
    } else if !cur.present {
        "No Cursor login stored in OMP. Click Connect Cursor to start browser login.".into()
    } else if cur.expired && !cur.refresh_present {
        "Cursor login expired and cannot be refreshed. Connect Cursor again to replace it.".into()
    } else if cursor_ready {
        "Cursor live catalog verified through the authenticated OMP gateway.".into()
    } else if cur.expired && cur.refresh_present {
        "Cursor access token expired, but OMP has a refresh token. Use the existing login to refresh and verify it.".into()
    } else {
        "Cursor login is stored in OMP, but live gateway readiness has not been verified.".into()
    };

    let found = find_opencode_go_credential();
    let opencode_ready =
        enabled && found.is_some() && provider_live_ready("opencode-go", gateway_valid);
    let opencode_reason = if !enabled {
        format!("Disabled via {DISABLE_ENV}=1.")
    } else if found.is_none() {
        "No OpenCode Go login stored in OMP. Click Connect OpenCode Go to authenticate.".into()
    } else if opencode_ready {
        "OpenCode Go live catalog verified through the authenticated OMP gateway.".into()
    } else {
        "OpenCode Go login is stored in OMP, but live gateway readiness has not been verified."
            .into()
    };

    Ok(json!({
        "enabled": enabled,
        "providers": gated_providers(),
        "env": DISABLE_ENV,
        "enabled_by_default": true,
        "cursor": {
            "available": cursor_ready,
            "present": cur.present,
            "expired": cur.expired,
            "expires_ms": cur.expires_ms,
            "refresh_present": cur.refresh_present,
            "source": cur.source,
            "chat_ready": cursor_ready,
            "reason": cursor_reason,
        },
        "opencode": {
            "chat_ready": opencode_ready,
            "credential_present": found.is_some(),
            "key_found_locally": found.is_some(),
            "source": found.as_ref().map(|credential| credential.source.clone()),
            "auth_url": OPENCODE_AUTH_URL,
            "reason": opencode_reason,
        },
    })
    .to_string())
}

fn wire_opencode_go_settings(app: &tauri::AppHandle) -> AppResult<String> {
    crate::commands::omp_gateway::ensure_cursor_gateway().map_err(AppError::Engine)?;
    let available = probe_provider_models_via_omp("opencode-go").map_err(AppError::Engine)?;
    let mut settings = crate::commands::settings::read_settings(app);
    let model = resolve_opencode_go_model(&settings.model, &available);
    settings.base_url = crate::commands::omp_gateway::GATEWAY_BASE_URL.to_string();
    crate::commands::settings::update_selected_model(&mut settings, &model);
    crate::commands::settings::write_settings(app, &settings)?;
    Ok(model)
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
    let model = wire_opencode_go_settings(&app)?;
    Ok(json!({
        "ok": true,
        "base_url": crate::commands::omp_gateway::GATEWAY_BASE_URL,
        "model": model,
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
        let model = wire_opencode_go_settings(&app)?;
        return Ok(json!({
            "ok": true,
            "chat_ready": true,
            "needs_auth": false,
            "base_url": crate::commands::omp_gateway::GATEWAY_BASE_URL,
            "model": model,
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
    crate::commands::settings::apply_omp_capability(&mut s);
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
    if !cur.reusable() {
        return Err(AppError::Config(
            "Cursor OAuth expired without a refresh token. Click Connect Cursor to log in again."
                .into(),
        ));
    }
    crate::commands::omp_gateway::ensure_cursor_gateway().map_err(AppError::Engine)?;
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
    fn parses_only_opencode_models_from_gateway_catalog() {
        let text = r#"{"data":[
            {"id":"opencode-go/deepseek-v4-pro","owned_by":"opencode-go"},
            {"id":"opencode-go/glm-5","owned_by":"opencode-go"},
            {"id":"cursor/claude-4.6-sonnet-medium","owned_by":"cursor"}
        ]}"#;
        assert_eq!(
            parse_provider_gateway_models_json(text, "opencode-go").unwrap(),
            vec![
                "opencode-go/deepseek-v4-pro".to_string(),
                "opencode-go/glm-5".to_string(),
            ]
        );
    }

    #[test]
    fn cursor_readiness_projects_metadata_without_copying_tokens() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            r#"CREATE TABLE auth_credentials (
                id INTEGER PRIMARY KEY,
                provider TEXT NOT NULL,
                credential_type TEXT NOT NULL,
                disabled_cause TEXT,
                data TEXT NOT NULL
            );
            INSERT INTO auth_credentials (provider, credential_type, disabled_cause, data)
            VALUES ('cursor', 'oauth', NULL,
                    '{"access":"expired-access","refresh":"refresh-token","expires":1000}');"#,
        )
        .unwrap();
        let state = cursor_credential_state_in(&conn, 2000).unwrap();
        assert!(state.present);
        assert!(state.expired);
        assert!(state.refresh_present);
        assert_eq!(state.expires_ms, Some(1000));
    }

    #[test]
    fn provider_readiness_requires_validated_gateway() {
        assert!(!provider_live_ready("cursor", false));
        assert!(!provider_live_ready("opencode-go", false));
    }

    #[test]
    fn cursor_login_is_reusable_only_when_live_or_refreshable() {
        let status = |expired, refresh_present| CursorOmpStatus {
            present: true,
            expired,
            expires_ms: None,
            refresh_present,
            source: "test".into(),
        };
        assert!(status(false, false).reusable());
        assert!(status(true, true).reusable());
        assert!(!status(true, false).reusable());
    }

    #[test]
    fn opencode_model_resolution_uses_the_live_catalog() {
        let available = vec![
            "opencode-go/deepseek-v4-pro".to_string(),
            "opencode-go/glm-5".to_string(),
        ];
        assert_eq!(
            resolve_opencode_go_model("opencode-go/grok-4.5", &available),
            "opencode-go/deepseek-v4-pro"
        );
        assert_eq!(
            resolve_opencode_go_model("opencode-go/glm-5", &available),
            "opencode-go/glm-5"
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
