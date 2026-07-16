//! Minimal, blocking MCP (Model Context Protocol) client over stdio — no async
//! runtime. Spawns the MCP server as a child process, speaks newline-delimited
//! JSON-RPC 2.0 (`initialize` + `notifications/initialized`, then `tools/list`
//! and `tools/call`). One JSON message per line. `Drop` kills the child.
//!
//! Phase 3.2: least-privilege spawn (scrubbed env), 2 MiB frame/result caps,
//! concurrent capped stderr drain, and a dedicated stdout reader thread so
//! request loops can poll cancel/deadline every 100 ms via `recv_timeout`.

use std::io::{BufReader, Read, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// Default per-call timeout — browser-backed MCP tools (Roam) are slow.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(120);
/// Maximum JSON-RPC frame / result size (Phase 3.2).
pub const MAX_FRAME_BYTES: usize = 2 * 1024 * 1024;
/// Cap captured stderr so a chatty child cannot flood memory.
pub const MAX_STDERR_BYTES: usize = 8 * 1024;
/// Max tools advertised by a single server.
pub const MAX_TOOLS: usize = 64;
/// Max nesting depth for any JSON value we accept (schemas, args, results).
pub const MAX_JSON_DEPTH: usize = 16;
/// Max object keys / array length at any single level.
pub const MAX_COLLECTION: usize = 256;
/// Max string length in schema/arg/result values.
pub const MAX_STRING_CHARS: usize = 8 * 1024;
/// Max tool name / description length.
pub const MAX_NAME_CHARS: usize = 128;
/// How often the request loop re-checks deadline / cancel.
const POLL_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Debug, thiserror::Error)]
pub enum McpError {
    #[error("spawn failed: {0}")]
    Spawn(String),
    #[error("io error: {0}")]
    Io(String),
    #[error("protocol error: {0}")]
    Protocol(String),
    #[error("server error {code}: {message}")]
    Server { code: i64, message: String },
    #[error("timed out after {0:?}")]
    Timeout(Duration),
    #[error("cancelled")]
    Cancelled,
    #[error("frame exceeds {0} byte limit")]
    FrameTooLarge(usize),
    #[error("command must be an absolute existing executable path")]
    BadCommand,
    #[error("client invalidated")]
    Invalidated,
    #[error("schema or payload exceeds validator limits: {0}")]
    SchemaTooComplex(String),
}

/// A tool advertised by the server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolInfo {
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// The tool's JSON-Schema input contract (`inputSchema`).
    #[serde(default, rename = "inputSchema")]
    pub input_schema: Value,
}

/// Spawn options for least-privilege child setup.
#[derive(Debug, Clone)]
pub struct McpSpawnOpts {
    /// Absolute path to the executable.
    pub command: String,
    /// Fixed argument array (never shell-interpolated).
    pub args: Vec<String>,
    /// Per-call default timeout.
    pub timeout: Duration,
    /// When true, clear the child environment and set only a minimal allowlist.
    pub scrub_env: bool,
    /// Extra env vars to pass after scrub (e.g. explicit per-server API key).
    pub extra_env: Vec<(String, String)>,
}

impl Default for McpSpawnOpts {
    fn default() -> Self {
        Self {
            command: String::new(),
            args: Vec::new(),
            timeout: DEFAULT_TIMEOUT,
            scrub_env: true,
            extra_env: Vec::new(),
        }
    }
}

enum FrameMsg {
    Line(String),
    IoErr(String),
    Eof,
    FrameTooLarge,
}

/// Shared kill handle — the manager can kill the child while a request waits.
#[derive(Clone)]
pub struct KillHandle {
    child: Arc<Mutex<Option<Child>>>,
    dead: Arc<AtomicBool>,
}

impl KillHandle {
    pub fn kill(&self) {
        if self.dead.swap(true, Ordering::SeqCst) {
            return;
        }
        if let Ok(mut slot) = self.child.lock() {
            if let Some(mut c) = slot.take() {
                let _ = c.kill();
                let _ = c.wait();
            }
        }
    }

    pub fn is_dead(&self) -> bool {
        self.dead.load(Ordering::SeqCst)
    }
}

/// A connected MCP server (child process + framed stdio).
pub struct McpClient {
    kill: KillHandle,
    stdin: ChildStdin,
    frames: Receiver<FrameMsg>,
    next_id: AtomicI64,
    timeout: Duration,
    /// Last capped stderr snippet (filled by the stderr drain thread).
    stderr: Arc<Mutex<String>>,
}

impl McpClient {
    /// Spawn with least-privilege options, handshake, return ready client.
    pub fn connect_with(opts: McpSpawnOpts) -> Result<Self, McpError> {
        let cmd_path = Path::new(opts.command.trim());
        if !cmd_path.is_absolute() || !cmd_path.is_file() {
            return Err(McpError::BadCommand);
        }
        let mut command = Command::new(cmd_path);
        command
            .args(&opts.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if opts.scrub_env {
            command.env_clear();
            // Minimal allowlist — no API keys unless explicitly in extra_env.
            for key in [
                "PATH",
                "SystemRoot",
                "WINDIR",
                "TEMP",
                "TMP",
                "HOME",
                "USERPROFILE",
                "PATHEXT",
                "COMSPEC",
            ] {
                if let Ok(v) = std::env::var(key) {
                    command.env(key, v);
                }
            }
        }
        for (k, v) in &opts.extra_env {
            command.env(k, v);
        }
        let mut child = command
            .spawn()
            .map_err(|e| McpError::Spawn(format!("{}: {e}", opts.command)))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| McpError::Spawn("no stdin".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| McpError::Spawn("no stdout".into()))?;
        let stderr = child.stderr.take();

        let kill = KillHandle {
            child: Arc::new(Mutex::new(Some(child))),
            dead: Arc::new(AtomicBool::new(false)),
        };

        // Dedicated stdout reader: produces complete lines (or errors) on a
        // *bounded* channel so a chatty server applies backpressure via the pipe.
        let (tx, rx) = mpsc::sync_channel::<FrameMsg>(16);
        std::thread::Builder::new()
            .name("mcp-stdout".into())
            .spawn(move || stdout_reader(stdout, tx))
            .map_err(|e| McpError::Spawn(format!("stdout reader: {e}")))?;

        // Concurrent capped stderr drain so a chatty child never blocks on a full pipe.
        let stderr_buf = Arc::new(Mutex::new(String::new()));
        if let Some(err) = stderr {
            let sink = Arc::clone(&stderr_buf);
            let _ = std::thread::Builder::new()
                .name("mcp-stderr".into())
                .spawn(move || drain_stderr(err, sink));
        }

        let mut c = McpClient {
            kill,
            stdin,
            frames: rx,
            next_id: AtomicI64::new(1),
            timeout: opts.timeout,
            stderr: stderr_buf,
        };
        c.handshake()?;
        Ok(c)
    }

    /// Backward-compatible connect (still requires absolute executable).
    pub fn connect(command: &str, args: &[String]) -> Result<Self, McpError> {
        Self::connect_with(McpSpawnOpts {
            command: command.to_string(),
            args: args.to_vec(),
            ..Default::default()
        })
    }

    /// Shared kill handle (cloneable) so a manager can terminate mid-request.
    pub fn kill_handle(&self) -> KillHandle {
        self.kill.clone()
    }

    /// Override the per-call timeout.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Last capped stderr text.
    pub fn last_stderr(&self) -> String {
        self.stderr.lock().map(|s| s.clone()).unwrap_or_default()
    }

    fn handshake(&mut self) -> Result<(), McpError> {
        let params = json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": { "name": "finmodel", "version": env!("CARGO_PKG_VERSION") },
        });
        let _ = self.request("initialize", params)?;
        self.notify("notifications/initialized", json!({}))?;
        Ok(())
    }

    /// List the server's tools. Requires object `inputSchema` per tool and
    /// enforces collection/depth/string caps. On violation the child is killed
    /// and the client invalidated (adversarial server protection).
    pub fn list_tools(&mut self) -> Result<Vec<ToolInfo>, McpError> {
        let resp = self.request("tools/list", json!({}))?;
        match validate_tools_list(&resp) {
            Ok(tools) => Ok(tools),
            Err(e) => {
                self.kill.kill();
                Err(e)
            }
        }
    }

    /// Call a tool by name with JSON arguments; returns the raw `result`.
    /// Arguments and the result are depth/collection capped.
    pub fn call_tool(&mut self, name: &str, args: Value) -> Result<Value, McpError> {
        if name.is_empty() || name.chars().count() > MAX_NAME_CHARS {
            return Err(McpError::SchemaTooComplex("tool name length".into()));
        }
        validate_json_bounds(&args, 0)?;
        let result = self.request("tools/call", json!({ "name": name, "arguments": args }))?;
        if let Err(e) = validate_json_bounds(&result, 0) {
            self.kill.kill();
            return Err(e);
        }
        Ok(result)
    }

    /// Request with explicit deadline + optional cancel flag. Polls every 100 ms;
    /// on cancel/timeout the child is killed and the client is invalidated.
    pub fn request_cancellable(
        &mut self,
        method: &str,
        params: Value,
        deadline: Instant,
        cancel: &dyn Fn() -> bool,
    ) -> Result<Value, McpError> {
        if self.kill.is_dead() {
            return Err(McpError::Invalidated);
        }
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let msg = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        self.write_line(&msg)?;
        loop {
            if cancel() {
                self.kill.kill();
                return Err(McpError::Cancelled);
            }
            if Instant::now() >= deadline {
                self.kill.kill();
                return Err(McpError::Timeout(self.timeout));
            }
            let wait = deadline
                .saturating_duration_since(Instant::now())
                .min(POLL_INTERVAL);
            match self.frames.recv_timeout(wait) {
                Ok(FrameMsg::Line(line)) => {
                    let v: Value = match serde_json::from_str(line.trim()) {
                        Ok(v) => v,
                        Err(e) => {
                            return Err(McpError::Protocol(format!("{e}: {line}")));
                        }
                    };
                    // Skip notifications / unrelated ids (server may interleave).
                    if v.get("id").and_then(Value::as_i64) != Some(id) {
                        continue;
                    }
                    if let Some(err) = v.get("error") {
                        let code = err.get("code").and_then(Value::as_i64).unwrap_or(0);
                        let message = err
                            .get("message")
                            .and_then(Value::as_str)
                            .unwrap_or("unknown")
                            .to_string();
                        return Err(McpError::Server { code, message });
                    }
                    let result = v.get("result").cloned().unwrap_or(Value::Null);
                    let encoded = serde_json::to_vec(&result).unwrap_or_default();
                    if encoded.len() > MAX_FRAME_BYTES {
                        return Err(McpError::FrameTooLarge(MAX_FRAME_BYTES));
                    }
                    return Ok(result);
                }
                Ok(FrameMsg::FrameTooLarge) => {
                    self.kill.kill();
                    return Err(McpError::FrameTooLarge(MAX_FRAME_BYTES));
                }
                Ok(FrameMsg::IoErr(e)) => {
                    self.kill.kill();
                    return Err(McpError::Io(e));
                }
                Ok(FrameMsg::Eof) => {
                    self.kill.kill();
                    if cancel() {
                        return Err(McpError::Cancelled);
                    }
                    if Instant::now() >= deadline {
                        return Err(McpError::Timeout(self.timeout));
                    }
                    return Err(McpError::Protocol("server closed the stream".into()));
                }
                Err(RecvTimeoutError::Timeout) => {
                    // Re-check cancel/deadline at top of loop.
                    continue;
                }
                Err(RecvTimeoutError::Disconnected) => {
                    self.kill.kill();
                    return Err(McpError::Protocol("stdout reader exited".into()));
                }
            }
        }
    }

    fn request(&mut self, method: &str, params: Value) -> Result<Value, McpError> {
        let deadline = Instant::now() + self.timeout;
        self.request_cancellable(method, params, deadline, &|| false)
    }

    fn notify(&mut self, method: &str, params: Value) -> Result<(), McpError> {
        let msg = json!({ "jsonrpc": "2.0", "method": method, "params": params });
        self.write_line(&msg)
    }

    fn write_line(&mut self, v: &Value) -> Result<(), McpError> {
        let line = serde_json::to_string(v).map_err(|e| McpError::Protocol(e.to_string()))?;
        if line.len() > MAX_FRAME_BYTES {
            return Err(McpError::FrameTooLarge(MAX_FRAME_BYTES));
        }
        self.stdin
            .write_all(line.as_bytes())
            .and_then(|_| self.stdin.write_all(b"\n"))
            .and_then(|_| self.stdin.flush())
            .map_err(|e| McpError::Io(e.to_string()))
    }

    /// Force-kill the child (used on timeout/cancel/settings reset).
    pub fn kill(&mut self) {
        self.kill.kill();
    }
}

impl Drop for McpClient {
    fn drop(&mut self) {
        self.kill.kill();
    }
}

/// Dedicated stdout reader: emits complete lines, enforces frame size, never
/// holds the request path blocked without a channel timeout.
fn stdout_reader(stdout: impl Read + Send + 'static, tx: SyncSender<FrameMsg>) {
    let mut reader = BufReader::new(stdout);
    loop {
        let mut buf: Vec<u8> = Vec::new();
        // Read until newline with a size cap.
        let mut byte = [0u8; 1];
        let mut got = false;
        loop {
            match reader.read(&mut byte) {
                Ok(0) => {
                    if !got && buf.is_empty() {
                        let _ = tx.send(FrameMsg::Eof);
                        return;
                    }
                    // Partial line at EOF — treat as frame if non-empty.
                    break;
                }
                Ok(_) => {
                    got = true;
                    if byte[0] == b'\n' {
                        break;
                    }
                    if byte[0] == b'\r' {
                        continue;
                    }
                    if buf.len() >= MAX_FRAME_BYTES {
                        let _ = tx.send(FrameMsg::FrameTooLarge);
                        return;
                    }
                    buf.push(byte[0]);
                }
                Err(e) => {
                    let _ = tx.send(FrameMsg::IoErr(e.to_string()));
                    return;
                }
            }
        }
        if buf.is_empty() {
            // Blank keep-alive — skip.
            continue;
        }
        let line = String::from_utf8_lossy(&buf).into_owned();
        if tx.send(FrameMsg::Line(line)).is_err() {
            return; // receiver dropped
        }
    }
}

fn drain_stderr(mut err: impl Read, sink: Arc<Mutex<String>>) {
    let mut total = 0usize;
    let mut chunk = [0u8; 512];
    loop {
        match err.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                if total >= MAX_STDERR_BYTES {
                    continue; // drain but discard past cap
                }
                let take = n.min(MAX_STDERR_BYTES - total);
                if let Ok(mut s) = sink.lock() {
                    s.push_str(&String::from_utf8_lossy(&chunk[..take]));
                    total = s.len();
                    if total >= MAX_STDERR_BYTES {
                        s.push('…');
                    }
                }
            }
            Err(_) => break,
        }
    }
}

/// Validate a `tools/list` result: count, names, schemas.
pub fn validate_tools_list(resp: &Value) -> Result<Vec<ToolInfo>, McpError> {
    let arr = resp
        .get("tools")
        .and_then(Value::as_array)
        .ok_or_else(|| McpError::Protocol("tools/list: missing tools array".into()))?;
    if arr.len() > MAX_TOOLS {
        return Err(McpError::SchemaTooComplex(format!(
            "tools count {} > {MAX_TOOLS}",
            arr.len()
        )));
    }
    let mut out = Vec::with_capacity(arr.len());
    for t in arr {
        let info: ToolInfo = serde_json::from_value(t.clone())
            .map_err(|e| McpError::Protocol(format!("tool decode: {e}")))?;
        if info.name.is_empty() || info.name.chars().count() > MAX_NAME_CHARS {
            return Err(McpError::SchemaTooComplex("tool name".into()));
        }
        if info.description.chars().count() > MAX_STRING_CHARS {
            return Err(McpError::SchemaTooComplex("tool description".into()));
        }
        if !info.input_schema.is_object() {
            return Err(McpError::SchemaTooComplex(format!(
                "tool {} missing object inputSchema",
                info.name
            )));
        }
        validate_json_bounds(&info.input_schema, 0)?;
        out.push(info);
    }
    Ok(out)
}

/// Recursive bounds check on any JSON value (depth / collection / string).
pub fn validate_json_bounds(v: &Value, depth: usize) -> Result<(), McpError> {
    if depth > MAX_JSON_DEPTH {
        return Err(McpError::SchemaTooComplex(format!(
            "depth {depth} > {MAX_JSON_DEPTH}"
        )));
    }
    match v {
        Value::Null | Value::Bool(_) | Value::Number(_) => Ok(()),
        Value::String(s) => {
            if s.chars().count() > MAX_STRING_CHARS {
                Err(McpError::SchemaTooComplex("string length".into()))
            } else {
                Ok(())
            }
        }
        Value::Array(a) => {
            if a.len() > MAX_COLLECTION {
                return Err(McpError::SchemaTooComplex(format!(
                    "array len {} > {MAX_COLLECTION}",
                    a.len()
                )));
            }
            for item in a {
                validate_json_bounds(item, depth + 1)?;
            }
            Ok(())
        }
        Value::Object(m) => {
            if m.len() > MAX_COLLECTION {
                return Err(McpError::SchemaTooComplex(format!(
                    "object keys {} > {MAX_COLLECTION}",
                    m.len()
                )));
            }
            for (k, val) in m {
                if k.chars().count() > MAX_NAME_CHARS {
                    return Err(McpError::SchemaTooComplex("object key length".into()));
                }
                validate_json_bounds(val, depth + 1)?;
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_relative_command() {
        let err = match McpClient::connect_with(McpSpawnOpts {
            command: "echo".into(),
            ..Default::default()
        }) {
            Ok(_) => panic!("expected BadCommand"),
            Err(e) => e,
        };
        assert!(matches!(err, McpError::BadCommand));
    }

    #[test]
    fn rejects_missing_absolute() {
        let err = match McpClient::connect_with(McpSpawnOpts {
            command: if cfg!(windows) {
                r"C:\this\does\not\exist-finmodel-mcp.exe".into()
            } else {
                "/tmp/this-does-not-exist-finmodel-mcp".into()
            },
            ..Default::default()
        }) {
            Ok(_) => panic!("expected BadCommand"),
            Err(e) => e,
        };
        assert!(matches!(err, McpError::BadCommand));
    }
    #[test]
    fn rejects_deeply_nested_schema() {
        let mut v = json!({"type": "object"});
        for _ in 0..MAX_JSON_DEPTH + 2 {
            v = json!({"properties": {"x": v}});
        }
        let err = validate_json_bounds(&v, 0).unwrap_err();
        assert!(matches!(err, McpError::SchemaTooComplex(_)));
    }

    #[test]
    fn rejects_oversized_collection() {
        let arr: Vec<Value> = (0..MAX_COLLECTION + 1).map(|i| json!(i)).collect();
        let err = validate_json_bounds(&Value::Array(arr), 0).unwrap_err();
        assert!(matches!(err, McpError::SchemaTooComplex(_)));
    }

    #[test]
    fn rejects_tool_list_without_schema() {
        let resp = json!({ "tools": [{ "name": "x", "description": "d" }] });
        let err = validate_tools_list(&resp).unwrap_err();
        assert!(matches!(err, McpError::SchemaTooComplex(_)));
    }

    #[test]
    fn accepts_bounded_tool_list() {
        let resp = json!({ "tools": [
            { "name": "web_search", "description": "s", "inputSchema": { "type": "object" } }
        ]});
        let tools = validate_tools_list(&resp).unwrap();
        assert_eq!(tools.len(), 1);
    }

    #[test]
    fn rejects_too_many_tools() {
        let tools: Vec<Value> = (0..MAX_TOOLS + 1)
            .map(|i| {
                json!({
                    "name": format!("t{i}"),
                    "description": "d",
                    "inputSchema": { "type": "object" }
                })
            })
            .collect();
        let err = validate_tools_list(&json!({ "tools": tools })).unwrap_err();
        assert!(matches!(err, McpError::SchemaTooComplex(_)));
    }
}
