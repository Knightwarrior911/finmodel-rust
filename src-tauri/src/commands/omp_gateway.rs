//! Local OMP auth-broker + auth-gateway for Cursor chat.
//!
//! Cursor AgentService is not OpenAI-compatible. OMP's auth-gateway translates
//! OpenAI `/v1/chat/completions` into Cursor's protobuf Run stream, using the
//! OAuth already stored in `~/.omp/agent/agent.db`.
//!
//! finmodel keeps its existing OpenAI-compatible chat path and points
//! `base_url` at this loopback gateway when the user picks Cursor.

use std::net::TcpStream;
use std::process::{Command, Stdio};
use std::time::Duration;

use serde_json::json;

use crate::error::{AppError, AppResult};

pub const BROKER_URL: &str = "http://127.0.0.1:8765";
pub const GATEWAY_BASE_URL: &str = "http://127.0.0.1:4000/v1";
pub const GATEWAY_HOST: &str = "127.0.0.1";
pub const GATEWAY_PORT: u16 = 4000;
pub const BROKER_PORT: u16 = 8765;

/// Bearer used with `omp auth-gateway serve --no-auth` (gateway ignores it).
pub const GATEWAY_DUMMY_BEARER: &str = "omp-local";

pub fn is_cursor_gateway_base(base: &str) -> bool {
    let b = base.trim().trim_end_matches('/');
    b == GATEWAY_BASE_URL || b == "http://127.0.0.1:4000/v1" || b == "http://localhost:4000/v1"
}

pub fn qualify_cursor_model(model: &str) -> String {
    let m = model.trim();
    if m.is_empty() {
        return "cursor/claude-4.6-sonnet-medium".to_string();
    }
    if m.starts_with("cursor/") {
        m.to_string()
    } else {
        format!("cursor/{m}")
    }
}

/// Keep a persisted Cursor selection on a model the current OMP catalog accepts.
/// The catalog contains bare IDs; outgoing requests always use the `cursor/` selector.
pub fn resolve_cursor_model(saved: &str, available: &[String]) -> String {
    let requested = qualify_cursor_model(saved);
    if available
        .iter()
        .any(|id| qualify_cursor_model(id) == requested)
    {
        return requested;
    }
    let requested_id = requested.trim_start_matches("cursor/");
    let requested_family = requested_id.trim_start_matches("cursor-");
    if requested_family.starts_with("grok-4.5") {
        let mut family_matches = available.iter().filter(|id| {
            id.trim_start_matches("cursor/")
                .trim_start_matches("cursor-")
                .starts_with("grok-4.5")
        });
        if let Some(medium) = family_matches.clone().find(|id| id.ends_with("-medium")) {
            return qualify_cursor_model(medium);
        }
        if let Some(candidate) = family_matches.next() {
            return qualify_cursor_model(candidate);
        }
    }
    for preferred in ["claude-4.6-sonnet-medium", "claude-4-sonnet"] {
        if available.iter().any(|id| id == preferred) {
            return qualify_cursor_model(preferred);
        }
    }
    available
        .first()
        .map(|id| qualify_cursor_model(id))
        .unwrap_or_else(|| requested)
}

fn port_open(port: u16) -> bool {
    TcpStream::connect_timeout(
        &std::net::SocketAddr::from(([127, 0, 0, 1], port)),
        Duration::from_millis(250),
    )
    .is_ok()
}

/// Configure an OMP child command so CLI probes never flash a console on Windows.
pub(crate) fn configure_hidden_command(cmd: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x00000200;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW);
    }
}

fn omp_bin() -> Result<String, String> {
    which_omp().ok_or_else(|| "omp not found on PATH (install Oh My Pi / omp)".into())
}

fn which_omp() -> Option<String> {
    if let Ok(p) = std::env::var("OMP_BIN") {
        let p = p.trim().to_string();
        if !p.is_empty() && std::path::Path::new(&p).exists() {
            return Some(p);
        }
    }
    if let Some(home) = std::env::var_os("USERPROFILE") {
        let candidate = std::path::PathBuf::from(home)
            .join(".bun")
            .join("bin")
            .join("omp.exe");
        if candidate.exists() {
            return Some(candidate.to_string_lossy().into_owned());
        }
    }
    let mut cmd = Command::new("omp");
    cmd.arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    configure_hidden_command(&mut cmd);
    cmd.status().ok().map(|_| "omp".to_string())
}

fn spawn_detached(bin: &str, args: &[&str]) -> Result<(), String> {
    let mut cmd = Command::new(bin);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    configure_hidden_command(&mut cmd);
    cmd.spawn()
        .map(|_| ())
        .map_err(|e| format!("failed to start `{bin} {}`: {e}", args.join(" ")))
}

/// Spawn OMP's provider-specific login and open the printed browser URL.
/// OMP owns the browser PKCE/API-key flow; finmodel only launches it and
/// watches the existing agent.db for completion.
fn spawn_omp_login_visible(bin: &str, provider: &str) -> Result<(), String> {
    let mut cmd = Command::new(bin);
    cmd.args(["auth-broker", "login", provider])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    configure_hidden_command(&mut cmd);
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("failed to start omp auth-broker login {provider}: {e}"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "omp login did not expose stdout".to_string())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "omp login did not expose stderr".to_string())?;
    let opened = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    watch_login_output(stdout, opened.clone());
    watch_login_output(stderr, opened);
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

/// Open an interactive OMP login console for providers that require pasted
/// input (OpenCode Go opens opencode.ai/auth, then prompts for its API key).
pub fn start_opencode_go_login() -> Result<(), String> {
    let bin = omp_bin()?;
    let mut cmd = Command::new(&bin);
    cmd.args(["auth-broker", "login", "opencode-go"]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NEW_CONSOLE: u32 = 0x00000010;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x00000200;
        cmd.creation_flags(CREATE_NEW_CONSOLE | CREATE_NEW_PROCESS_GROUP);
    }
    #[cfg(not(windows))]
    {
        cmd.stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());
    }
    cmd.spawn()
        .map(|_| ())
        .map_err(|e| format!("failed to open interactive OpenCode Go login: {e}"))
}

fn watch_login_output<R: std::io::Read + Send + 'static>(
    stream: R,
    opened: std::sync::Arc<std::sync::atomic::AtomicBool>,
) {
    std::thread::spawn(move || {
        use std::io::BufRead;
        let reader = std::io::BufReader::new(stream);
        for line in reader.lines().map_while(Result::ok) {
            if opened.load(std::sync::atomic::Ordering::Acquire) {
                continue;
            }
            if let Some(url) = cursor_login_url(&line) {
                if opened
                    .compare_exchange(
                        false,
                        true,
                        std::sync::atomic::Ordering::AcqRel,
                        std::sync::atomic::Ordering::Acquire,
                    )
                    .is_ok()
                {
                    let _ = open_external_url(&url);
                }
            }
        }
    });
}

fn cursor_login_url(line: &str) -> Option<String> {
    line.split_whitespace()
        .map(|token| token.trim_matches(|c: char| matches!(c, '"' | '\'' | ',' | ')' | ']')))
        .map(|token| token.replace("&amp;", "&"))
        .find(|token| token.starts_with("https://cursor.com/loginDeepControl"))
}

fn open_external_url(url: &str) -> Result<(), String> {
    #[cfg(windows)]
    {
        Command::new("rundll32.exe")
            .args(["url.dll,FileProtocolHandler", url])
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("failed to open Cursor login URL: {e}"))
    }
    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .arg(url)
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("failed to open Cursor login URL: {e}"))
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        Command::new("xdg-open")
            .arg(url)
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("failed to open Cursor login URL: {e}"))
    }
}

fn wait_port(port: u16, timeout: Duration) -> bool {
    let start = std::time::Instant::now();
    while start.elapsed() < timeout {
        if port_open(port) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(150));
    }
    port_open(port)
}

fn broker_token() -> Option<String> {
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"))?;
    let path = std::path::PathBuf::from(home)
        .join(".omp")
        .join("auth-broker.token");
    std::fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Ensure local OMP broker + gateway are accepting connections.
pub fn ensure_cursor_gateway() -> Result<(), String> {
    let bin = omp_bin()?;
    if !port_open(BROKER_PORT) {
        spawn_detached(&bin, &["auth-broker", "serve"])?;
        if !wait_port(BROKER_PORT, Duration::from_secs(8)) {
            return Err("omp auth-broker did not open :8765".into());
        }
    }
    if !port_open(GATEWAY_PORT) {
        let mut cmd = Command::new(&bin);
        cmd.args([
            "auth-gateway",
            "serve",
            "--no-auth",
            "--bind=127.0.0.1:4000",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
        if let Some(tok) = broker_token() {
            cmd.env("OMP_AUTH_BROKER_URL", BROKER_URL);
            cmd.env("OMP_AUTH_BROKER_TOKEN", tok);
        } else {
            cmd.env("OMP_AUTH_BROKER_URL", BROKER_URL);
        }
        configure_hidden_command(&mut cmd);
        cmd.spawn()
            .map_err(|e| format!("failed to start auth-gateway: {e}"))?;
        if !wait_port(GATEWAY_PORT, Duration::from_secs(10)) {
            return Err("omp auth-gateway did not open :4000".into());
        }
    }
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client
        .get(format!("{GATEWAY_BASE_URL}/models"))
        .bearer_auth(GATEWAY_DUMMY_BEARER)
        .send()
        .map_err(|e| format!("gateway /models: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("gateway /models HTTP {}", resp.status()));
    }
    Ok(())
}

pub fn gateway_status_json() -> serde_json::Value {
    let broker_up = port_open(BROKER_PORT);
    let gateway_up = port_open(GATEWAY_PORT);
    json!({
        "broker_up": broker_up,
        "gateway_up": gateway_up,
        "broker_url": BROKER_URL,
        "gateway_base": GATEWAY_BASE_URL,
        "chat_ready": broker_up && gateway_up,
    })
}

/// Repair an obsolete Cursor model id once the live OMP catalog is available.
/// This is deliberately a startup migration: a stale selection must never turn
/// the user's first chat request into a gateway 404.
pub fn reconcile_cursor_model(app: &tauri::AppHandle) -> Result<Option<String>, String> {
    let mut settings = crate::commands::settings::read_settings(app);
    if !crate::commands::settings::is_cursor_gateway(&settings) {
        return Ok(None);
    }
    let status = crate::commands::subscription::cursor_omp_status();
    if !status.present || status.expired {
        return Ok(None);
    }
    let (_, available) = crate::commands::subscription::probe_cursor_models_via_omp()?;
    let resolved = resolve_cursor_model(&settings.model, &available);
    if resolved == settings.model {
        return Ok(None);
    }
    settings.model = resolved.clone();
    settings.model_capability = None;
    crate::commands::settings::write_settings(app, &settings).map_err(|e| e.to_string())?;
    Ok(Some(resolved))
}

/// Start (if needed) the OMP Cursor gateway and return a redacted status.
#[tauri::command(rename_all = "snake_case")]
pub fn ensure_cursor_omp_gateway() -> AppResult<String> {
    ensure_cursor_gateway().map_err(AppError::Engine)?;
    Ok(gateway_status_json().to_string())
}

fn wire_cursor_settings(app: &tauri::AppHandle, source: &str) -> AppResult<String> {
    ensure_cursor_gateway().map_err(AppError::Engine)?;

    let mut s = crate::commands::settings::read_settings(app);
    s.base_url = GATEWAY_BASE_URL.to_string();
    let (_, available) =
        crate::commands::subscription::probe_cursor_models_via_omp().map_err(AppError::Engine)?;
    s.model = resolve_cursor_model(&s.model, &available);
    s.model_capability = None;
    crate::commands::settings::write_settings(app, &s)?;
    Ok(json!({
        "ok": true,
        "base_url": GATEWAY_BASE_URL,
        "model": s.model,
        "chat_ready": true,
        "needs_auth": false,
        "waiting": false,
        "source": source,
        "note": "Cursor chat routes through local omp auth-gateway (OpenAI-compatible). Your saved API key is unchanged.",
    })
    .to_string())
}

/// Point Settings at the local Cursor gateway without overwriting a real API key.
#[tauri::command(rename_all = "snake_case")]
pub fn use_cursor_omp(app: tauri::AppHandle) -> AppResult<String> {
    if !crate::commands::subscription::subscription_providers_enabled() {
        return Err(AppError::Config(
            "Subscription providers are disabled.".into(),
        ));
    }
    let cur = crate::commands::subscription::cursor_omp_status();
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
    wire_cursor_settings(&app, &cur.source)
}

/// Connect Cursor for chat: reuse OMP agent.db OAuth when present; otherwise
/// launch `omp auth-broker login cursor` and open its browser PKCE URL. Returns
/// `waiting: true` while login is in progress; the UI polls until OAuth appears
/// and then wires the local gateway automatically.
#[tauri::command(rename_all = "snake_case")]
pub fn connect_cursor_omp(app: tauri::AppHandle) -> AppResult<String> {
    if !crate::commands::subscription::subscription_providers_enabled() {
        return Err(AppError::Config(
            "Subscription providers are disabled.".into(),
        ));
    }
    let cur = crate::commands::subscription::cursor_omp_status();
    if cur.present && !cur.expired {
        return wire_cursor_settings(&app, &cur.source);
    }

    let bin = omp_bin().map_err(AppError::Engine)?;
    spawn_omp_login_visible(&bin, "cursor").map_err(AppError::Engine)?;

    // Brief poll so a fast re-login (already signed into cursor.com) wires
    // without a second click. Longer waits stay in the UI poller.
    let deadline = std::time::Instant::now() + Duration::from_secs(12);
    while std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(800));
        let cur = crate::commands::subscription::cursor_omp_status();
        if cur.present && !cur.expired {
            return wire_cursor_settings(&app, &cur.source);
        }
    }

    Ok(json!({
        "ok": true,
        "chat_ready": false,
        "needs_auth": true,
        "waiting": true,
        "guidance": "Cursor login opened in your browser. Finish the browser flow; Settings will keep checking and start the local gateway automatically.",
    })
    .to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_a_stale_cursor_grok_model_with_live_family_variant() {
        let available = vec![
            "claude-4-sonnet".to_string(),
            "claude-4.6-sonnet-medium".to_string(),
            "cursor-grok-4.5-medium".to_string(),
        ];
        assert_eq!(
            resolve_cursor_model("cursor/grok-4.5", &available),
            "cursor/cursor-grok-4.5-medium"
        );
    }

    #[test]
    fn hidden_command_helper_runs_a_child_without_output() {
        let mut cmd = if cfg!(windows) {
            let mut c = Command::new("cmd");
            c.args(["/C", "exit 0"]);
            c
        } else {
            Command::new("true")
        };
        configure_hidden_command(&mut cmd);
        let output = cmd.output().expect("hidden child should start");
        assert!(output.status.success());
        assert!(output.stdout.is_empty());
        assert!(output.stderr.is_empty());
    }

    #[test]
    fn extracts_cursor_login_url() {
        let line = "Login: https://cursor.com/loginDeepControl?challenge=abc&amp;uuid=def";
        assert_eq!(
            cursor_login_url(line).as_deref(),
            Some("https://cursor.com/loginDeepControl?challenge=abc&uuid=def")
        );
        assert_eq!(cursor_login_url("waiting for login"), None);
    }

    #[test]
    fn qualify_prefixes_once() {
        assert_eq!(
            qualify_cursor_model("claude-4.6-sonnet-medium"),
            "cursor/claude-4.6-sonnet-medium"
        );
        assert_eq!(
            qualify_cursor_model("cursor/claude-4.6-sonnet-medium"),
            "cursor/claude-4.6-sonnet-medium"
        );
    }

    #[test]
    fn detects_gateway_base() {
        assert!(is_cursor_gateway_base("http://127.0.0.1:4000/v1"));
        assert!(is_cursor_gateway_base("http://127.0.0.1:4000/v1/"));
        assert!(!is_cursor_gateway_base("https://openrouter.ai/api/v1"));
        assert!(!is_cursor_gateway_base("https://api2.cursor.sh"));
    }
}
