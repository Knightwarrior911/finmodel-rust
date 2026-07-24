//! Local OMP auth-broker + auth-gateway for Cursor chat.
//!
//! Cursor AgentService is not OpenAI-compatible. OMP's auth-gateway translates
//! OpenAI `/v1/chat/completions` into Cursor's protobuf Run stream, using the
//! OAuth already stored in `~/.omp/agent/agent.db`.
//!
//! finmodel keeps its existing OpenAI-compatible chat path and points
//! `base_url` at this loopback gateway when the user picks Cursor.

use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use serde_json::json;

use crate::error::{AppError, AppResult};

pub const BROKER_URL: &str = "http://127.0.0.1:8765";
pub const GATEWAY_BASE_URL: &str = "http://127.0.0.1:4000/v1";
pub const GATEWAY_HOST: &str = "127.0.0.1";
pub const GATEWAY_PORT: u16 = 4000;
pub const BROKER_PORT: u16 = 8765;
const SUPPORTED_OMP_MAJOR: u64 = 17;

#[derive(Debug, Clone, PartialEq, Eq)]
struct GatewayHealth {
    version: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GatewayAuthState {
    Authenticated,
}

fn parse_gateway_health(text: &str) -> Result<GatewayHealth, String> {
    let value: serde_json::Value = serde_json::from_str(text)
        .map_err(|_| "OMP gateway health was not valid JSON".to_string())?;
    if value.get("ok").and_then(|v| v.as_bool()) != Some(true) {
        return Err("OMP gateway health did not report ok".into());
    }
    let version = value
        .get("version")
        .and_then(|v| v.as_str())
        .filter(|v| !v.trim().is_empty())
        .ok_or_else(|| "OMP gateway health omitted its version".to_string())?;
    let major = version
        .split('.')
        .next()
        .and_then(|part| part.parse::<u64>().ok())
        .ok_or_else(|| format!("unsupported OMP gateway version {version}"))?;
    if major != SUPPORTED_OMP_MAJOR {
        return Err(format!(
            "unsupported OMP gateway version {version}; finmodel requires OMP {SUPPORTED_OMP_MAJOR}.x"
        ));
    }
    Ok(GatewayHealth {
        version: version.to_string(),
    })
}

fn classify_gateway_auth(
    unauthenticated_status: u16,
    authenticated_status: u16,
) -> Result<GatewayAuthState, String> {
    if (200..300).contains(&unauthenticated_status) {
        return Err(
            "OMP gateway authentication disabled; stop that gateway before reconnecting".into(),
        );
    }
    if !(200..300).contains(&authenticated_status) {
        return Err("OMP gateway rejected its backend bearer token".into());
    }
    Ok(GatewayAuthState::Authenticated)
}

fn auth_gateway_token_path(home: &std::path::Path) -> std::path::PathBuf {
    home.join(".omp").join("auth-gateway.token")
}

pub fn gateway_bearer() -> Result<String, String> {
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(std::path::PathBuf::from)
        .ok_or_else(|| "home directory unavailable for OMP gateway token".to_string())?;
    std::fs::read_to_string(auth_gateway_token_path(&home))
        .map_err(|e| format!("cannot read OMP auth-gateway token: {e}"))
        .map(|token| token.trim().to_string())
        .and_then(|token| {
            if token.is_empty() {
                Err("OMP auth-gateway token file is empty".into())
            } else {
                Ok(token)
            }
        })
}

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

pub(crate) fn port_open(port: u16) -> bool {
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

pub(crate) fn omp_bin() -> Result<String, String> {
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

#[derive(Default)]
struct OwnedOmpProcesses {
    broker: Option<Child>,
    gateway: Option<Child>,
}

impl OwnedOmpProcesses {
    fn reap_finished(&mut self) {
        reap_finished_child(&mut self.gateway);
        reap_finished_child(&mut self.broker);
    }

    fn shutdown(&mut self) {
        stop_owned_child(&mut self.gateway);
        stop_owned_child(&mut self.broker);
    }
}

fn reap_finished_child(slot: &mut Option<Child>) {
    let finished = slot
        .as_mut()
        .and_then(|child| child.try_wait().ok())
        .flatten()
        .is_some();
    if finished {
        *slot = None;
    }
}

fn stop_owned_child(slot: &mut Option<Child>) {
    let Some(mut child) = slot.take() else {
        return;
    };
    let pid = child.id();
    // omp spawns a serving descendant — Child::id() is the wrapper, not the
    // listener. Kill the process tree rooted at the wrapper PID.
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("taskkill")
            .args(["/T", "/F", "/PID", &pid.to_string()])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
    #[cfg(not(windows))]
    {
        let _ = child.kill();
    }
    // Reap without blocking forever — the tree kill should have ended it.
    let _ = child.try_wait();
}

fn owned_omp_processes() -> &'static Mutex<OwnedOmpProcesses> {
    static PROCESSES: OnceLock<Mutex<OwnedOmpProcesses>> = OnceLock::new();
    PROCESSES.get_or_init(|| Mutex::new(OwnedOmpProcesses::default()))
}

pub fn shutdown_owned_processes() {
    if let Ok(mut processes) = owned_omp_processes().lock() {
        processes.shutdown();
    }
}

fn spawn_owned(bin: &str, args: &[&str]) -> Result<Child, String> {
    let mut cmd = Command::new(bin);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    configure_hidden_command(&mut cmd);
    cmd.spawn()
        .map_err(|e| format!("failed to start `{bin} {}`: {e}", args.join(" ")))
}

fn gateway_serve_args() -> [&'static str; 3] {
    ["auth-gateway", "serve", "--bind=127.0.0.1:4000"]
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

pub(crate) fn broker_token() -> Option<String> {
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"))?;
    let path = std::path::PathBuf::from(home)
        .join(".omp")
        .join("auth-broker.token");
    std::fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn validate_broker(client: &reqwest::blocking::Client) -> Result<(), String> {
    let response = client
        .get(format!("{BROKER_URL}/v1/healthz"))
        .send()
        .map_err(|e| format!("OMP auth-broker health check failed: {e}"))?;
    let status = response.status();
    let body = response
        .text()
        .map_err(|e| format!("OMP auth-broker health response failed: {e}"))?;
    let ok = serde_json::from_str::<serde_json::Value>(&body)
        .ok()
        .and_then(|value| value.get("ok").and_then(|v| v.as_bool()))
        == Some(true);
    if !status.is_success() || !ok {
        return Err(format!(
            "unexpected service on OMP auth-broker port {BROKER_PORT}"
        ));
    }
    Ok(())
}

fn validate_gateway(client: &reqwest::blocking::Client) -> Result<GatewayHealth, String> {
    let health_response = client
        .get(format!("http://{GATEWAY_HOST}:{GATEWAY_PORT}/healthz"))
        .send()
        .map_err(|e| format!("OMP auth-gateway health check failed: {e}"))?;
    if !health_response.status().is_success() {
        return Err(format!(
            "OMP auth-gateway health HTTP {}",
            health_response.status()
        ));
    }
    let health = parse_gateway_health(
        &health_response
            .text()
            .map_err(|e| format!("OMP auth-gateway health response failed: {e}"))?,
    )?;

    let models_url = format!("{GATEWAY_BASE_URL}/models");
    let unauthenticated_status = client
        .get(&models_url)
        .send()
        .map_err(|e| format!("OMP auth-gateway unauthenticated probe failed: {e}"))?
        .status()
        .as_u16();
    let token = gateway_bearer()?;
    let authenticated_response = client
        .get(&models_url)
        .bearer_auth(token)
        .send()
        .map_err(|e| format!("OMP auth-gateway authenticated probe failed: {e}"))?;
    let authenticated_status = authenticated_response.status().as_u16();
    classify_gateway_auth(unauthenticated_status, authenticated_status)?;
    let models: serde_json::Value = authenticated_response
        .json()
        .map_err(|_| "OMP auth-gateway /models returned invalid JSON".to_string())?;
    if models
        .get("data")
        .and_then(|data| data.as_array())
        .is_none()
    {
        return Err("OMP auth-gateway /models omitted its data array".into());
    }
    Ok(health)
}

/// Validate already-running OMP services without spawning or adopting them.
/// Settings uses this passive check before advertising provider readiness.
pub(crate) fn validate_running_omp_services() -> Result<(), String> {
    if !port_open(BROKER_PORT) || !port_open(GATEWAY_PORT) {
        return Err("OMP broker/gateway is not running".into());
    }
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .map_err(|error| error.to_string())?;
    validate_broker(&client)?;
    validate_gateway(&client)?;
    Ok(())
}

/// Ensure compatible, authenticated local OMP broker and gateway services are ready.
pub fn ensure_cursor_gateway() -> Result<(), String> {
    let bin = omp_bin()?;
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .map_err(|e| e.to_string())?;
    let mut processes = owned_omp_processes()
        .lock()
        .map_err(|_| "OMP process manager lock poisoned".to_string())?;
    processes.reap_finished();

    if port_open(BROKER_PORT) {
        validate_broker(&client)?;
    } else {
        processes.broker = Some(spawn_owned(&bin, &["auth-broker", "serve"])?);
        if !wait_port(BROKER_PORT, Duration::from_secs(8)) {
            stop_owned_child(&mut processes.broker);
            return Err("omp auth-broker did not open :8765".into());
        }
        if let Err(error) = validate_broker(&client) {
            stop_owned_child(&mut processes.broker);
            return Err(error);
        }
    }

    if port_open(GATEWAY_PORT) {
        validate_gateway(&client)?;
        return Ok(());
    }

    let broker_token =
        broker_token().ok_or_else(|| "OMP auth-broker token was not created".to_string())?;
    let mut command = Command::new(&bin);
    command
        .args(gateway_serve_args())
        .env("OMP_AUTH_BROKER_URL", BROKER_URL)
        .env("OMP_AUTH_BROKER_TOKEN", broker_token)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    configure_hidden_command(&mut command);
    processes.gateway = Some(
        command
            .spawn()
            .map_err(|e| format!("failed to start authenticated OMP auth-gateway: {e}"))?,
    );
    if !wait_port(GATEWAY_PORT, Duration::from_secs(10)) {
        stop_owned_child(&mut processes.gateway);
        return Err("omp auth-gateway did not open :4000".into());
    }
    if let Err(error) = validate_gateway(&client) {
        stop_owned_child(&mut processes.gateway);
        return Err(error);
    }
    Ok(())
}

pub fn gateway_status_json() -> serde_json::Value {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(2))
        .build();
    let broker_ready = client
        .as_ref()
        .ok()
        .map(|client| validate_broker(client).is_ok())
        .unwrap_or(false);
    let gateway_health = client
        .as_ref()
        .ok()
        .and_then(|client| validate_gateway(client).ok());
    json!({
        "broker_up": broker_ready,
        "gateway_up": gateway_health.is_some(),
        "broker_url": BROKER_URL,
        "gateway_base": GATEWAY_BASE_URL,
        "chat_ready": broker_ready && gateway_health.is_some(),
        "gateway_version": gateway_health.map(|health| health.version),
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
    if !status.reusable() {
        return Ok(None);
    }
    let (_, available) = crate::commands::subscription::probe_cursor_models_via_omp()?;
    let resolved = resolve_cursor_model(&settings.model, &available);
    if resolved == settings.model {
        return Ok(None);
    }
    settings.model = resolved.clone();
    crate::commands::settings::apply_omp_capability(&mut settings);
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
    let resolved = resolve_cursor_model(&s.model, &available);
    crate::commands::settings::update_selected_model(&mut s, &resolved);
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
    if !cur.reusable() {
        return Err(AppError::Config(
            "Cursor OAuth expired without a refresh token. Click Connect Cursor to log in again."
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
    if cur.reusable() {
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
        if cur.reusable() {
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

    #[test]
    fn accepts_only_supported_omp_gateway_health() {
        let health = parse_gateway_health(r#"{"ok":true,"version":"17.1.0"}"#).unwrap();
        assert_eq!(health.version, "17.1.0");
        assert!(parse_gateway_health(r#"{"ok":false,"version":"17.1.0"}"#).is_err());
        assert!(parse_gateway_health(r#"{"ok":true,"version":"16.9.0"}"#).is_err());
        assert!(parse_gateway_health("not-json").is_err());
    }

    #[test]
    fn rejects_an_unauthenticated_gateway_listener() {
        assert_eq!(
            classify_gateway_auth(401, 200).unwrap(),
            GatewayAuthState::Authenticated
        );
        assert!(classify_gateway_auth(200, 200)
            .unwrap_err()
            .contains("authentication disabled"));
        assert!(classify_gateway_auth(401, 401)
            .unwrap_err()
            .contains("bearer token"));
    }

    #[test]
    fn gateway_token_path_stays_under_omp_home() {
        let home = std::path::Path::new("C:/test-home");
        assert_eq!(
            auth_gateway_token_path(home),
            home.join(".omp").join("auth-gateway.token")
        );
    }

    #[test]
    fn gateway_serve_command_requires_authentication() {
        let args = gateway_serve_args();
        assert_eq!(args[0..2], ["auth-gateway", "serve"]);
        assert!(!args.iter().any(|arg| *arg == "--no-auth"));
        assert!(args.iter().any(|arg| *arg == "--bind=127.0.0.1:4000"));
    }
    /// Capture the PID listening on a TCP port (Windows netstat -ano).
    fn pid_on_port(port: u16) -> Option<u32> {
        let output = std::process::Command::new("netstat")
            .args(["-ano", "-p", "TCP"])
            .output()
            .ok()?;
        let text = String::from_utf8_lossy(&output.stdout);
        for line in text.lines() {
            if line.contains(&format!(":{port}")) && line.contains("LISTENING") {
                return line.split_whitespace().last()?.parse().ok();
            }
        }
        None
    }

    /// Kill a process by PID (Windows taskkill /F /PID).
    fn kill_pid(pid: u32) {
        let _ = std::process::Command::new("taskkill")
            .args(["/F", "/PID", &pid.to_string()])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }

    /// RAII guard that captures initial port state and restores it on drop.
    /// Even if tests panic, Drop restarts whatever was running before.
    struct PortStateGuard {
        initial_broker_pid: Option<u32>,
        initial_gateway_pid: Option<u32>,
    }

    impl PortStateGuard {
        /// Capture current state. Call BEFORE any test mutations.
        fn capture() -> Self {
            Self {
                initial_broker_pid: pid_on_port(BROKER_PORT),
                initial_gateway_pid: pid_on_port(GATEWAY_PORT),
            }
        }
    }

    impl Drop for PortStateGuard {
        fn drop(&mut self) {
            // Kill anything currently on the ports that we didn't start
            if let Some(pid) = pid_on_port(BROKER_PORT) {
                if self.initial_broker_pid != Some(pid) {
                    kill_pid(pid);
                }
            }
            if let Some(pid) = pid_on_port(GATEWAY_PORT) {
                if self.initial_gateway_pid != Some(pid) {
                    kill_pid(pid);
                }
            }
            std::thread::sleep(Duration::from_secs(1));
            // Restart whatever was running before if it's now dead
            let bin = omp_bin().unwrap_or_default();
            if self.initial_broker_pid.is_some() && !port_open(BROKER_PORT) {
                let _ = std::process::Command::new(&bin)
                    .args(["auth-broker", "serve"])
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn();
            }
            if self.initial_gateway_pid.is_some() && !port_open(GATEWAY_PORT) {
                if let Some(tok) = broker_token() {
                    let _ = std::process::Command::new(&bin)
                        .args(["auth-gateway", "serve", "--bind=127.0.0.1:4000"])
                        .env("OMP_AUTH_BROKER_URL", BROKER_URL)
                        .env("OMP_AUTH_BROKER_TOKEN", tok)
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .spawn();
                }
            }
            std::thread::sleep(Duration::from_secs(3));
        }
    }

    #[test]
    #[ignore = "OMP_LIFECYCLE_TEST=1 required; uses fixed ports 8765/4000"]
    fn lifecycle_spawned_children_die_on_shutdown() {
        if std::env::var("OMP_LIFECYCLE_TEST").unwrap_or_default() != "1" {
            return;
        }
        let _guard = PortStateGuard::capture();
        // Kill whatever is on the ports now so we start clean
        if let Some(pid) = pid_on_port(BROKER_PORT) { kill_pid(pid); }
        if let Some(pid) = pid_on_port(GATEWAY_PORT) { kill_pid(pid); }
        std::thread::sleep(Duration::from_secs(2));
        assert!(!port_open(BROKER_PORT), "broker port free after kill");
        assert!(!port_open(GATEWAY_PORT), "gateway port free after kill");

        // App spawns via ensure_cursor_gateway
        let result = ensure_cursor_gateway();
        assert!(result.is_ok(), "ensure failed: {:?}", result);
        assert!(port_open(BROKER_PORT), "broker up after ensure");
        assert!(port_open(GATEWAY_PORT), "gateway up after ensure");

        let broker_pid = pid_on_port(BROKER_PORT);
        let gateway_pid = pid_on_port(GATEWAY_PORT);
        eprintln!("app-owned PIDs: broker={broker_pid:?}, gateway={gateway_pid:?}");

        // Shutdown kills only app-owned
        shutdown_owned_processes();
        std::thread::sleep(Duration::from_secs(2));

        assert!(!port_open(BROKER_PORT), "broker dead after shutdown");
        assert!(!port_open(GATEWAY_PORT), "gateway dead after shutdown");
        // guard.drop() restores initial state
    }

    #[test]
    #[ignore = "OMP_LIFECYCLE_TEST=1 required; uses fixed ports 8765/4000"]
    fn lifecycle_preexisting_survives_shutdown() {
        if std::env::var("OMP_LIFECYCLE_TEST").unwrap_or_default() != "1" {
            return;
        }
        let _guard = PortStateGuard::capture();
        // Kill whatever is on the ports now
        if let Some(pid) = pid_on_port(BROKER_PORT) { kill_pid(pid); }
        if let Some(pid) = pid_on_port(GATEWAY_PORT) { kill_pid(pid); }
        std::thread::sleep(Duration::from_secs(2));

        // Start broker+gateway EXTERNALLY (not through ensure_cursor_gateway)
        let bin = omp_bin().expect("omp on PATH");
        let mut ext_broker = std::process::Command::new(&bin)
            .args(["auth-broker", "serve"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("spawn external broker");
        std::thread::sleep(Duration::from_secs(3));
        let ext_broker_pid = ext_broker.id();
        assert!(port_open(BROKER_PORT), "external broker up");

        let tok = broker_token().expect("broker token");
        let mut ext_gateway = std::process::Command::new(&bin)
            .args(["auth-gateway", "serve", "--bind=127.0.0.1:4000"])
            .env("OMP_AUTH_BROKER_URL", BROKER_URL)
            .env("OMP_AUTH_BROKER_TOKEN", &tok)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("spawn external gateway");
        std::thread::sleep(Duration::from_secs(4));
        let ext_gateway_pid = ext_gateway.id();
        assert!(port_open(GATEWAY_PORT), "external gateway up");
        eprintln!("external PIDs: broker={ext_broker_pid}, gateway={ext_gateway_pid}");

        // ensure_cursor_gateway detects existing, does NOT spawn new
        let result = ensure_cursor_gateway();
        assert!(result.is_ok(), "ensure on preexisting: {:?}", result);

        // shutdown_owned_processes must NOT kill external processes
        shutdown_owned_processes();
        std::thread::sleep(Duration::from_secs(1));

        assert!(port_open(BROKER_PORT), "external broker survives");
        assert!(port_open(GATEWAY_PORT), "external gateway survives");

        // Verify PIDs still alive
        let check_pid = |pid: u32| -> bool {
            std::process::Command::new("tasklist")
                .args(["/FI", &format!("PID eq {pid}")])
                .output()
                .map(|o| String::from_utf8_lossy(&o.stdout).contains(&pid.to_string()))
                .unwrap_or(false)
        };
        assert!(check_pid(ext_broker_pid), "broker PID survives");
        assert!(check_pid(ext_gateway_pid), "gateway PID survives");

        // Cleanup external processes, guard.drop() restores initial state
        let _ = ext_broker.kill();
        let _ = ext_gateway.kill();
        let _ = ext_broker.wait();
        let _ = ext_gateway.wait();
    }
}
