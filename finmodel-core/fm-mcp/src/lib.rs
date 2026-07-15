//! Minimal, blocking MCP (Model Context Protocol) client over stdio — no async
//! runtime. Spawns the MCP server as a child process, speaks newline-delimited
//! JSON-RPC 2.0 (`initialize` + `notifications/initialized`, then `tools/list`
//! and `tools/call`). One JSON message per line. `Drop` kills the child.
//!
//! In-house (~300 lines) rather than the `rmcp` SDK, which drags tokio into a
//! blocking, disk-constrained workspace. The public API is small enough to swap
//! for `rmcp` behind the same surface later if the protocol fights back.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// Default per-call timeout — browser-backed MCP tools (Roam) are slow.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(120);

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

/// A connected MCP server (child process + framed stdio).
pub struct McpClient {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: AtomicI64,
    timeout: Duration,
}

impl McpClient {
    /// Spawn `command args...`, perform the MCP handshake, and return a ready
    /// client. Returns `Err` on spawn or handshake failure.
    pub fn connect(command: &str, args: &[String]) -> Result<Self, McpError> {
        let mut child = Command::new(command)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| McpError::Spawn(format!("{command}: {e}")))?;
        let stdin = child.stdin.take().ok_or_else(|| McpError::Spawn("no stdin".into()))?;
        let stdout = child.stdout.take().ok_or_else(|| McpError::Spawn("no stdout".into()))?;
        let mut c = McpClient {
            child,
            stdin,
            stdout: BufReader::new(stdout),
            next_id: AtomicI64::new(1),
            timeout: DEFAULT_TIMEOUT,
        };
        c.handshake()?;
        Ok(c)
    }

    /// Override the per-call timeout.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    fn handshake(&mut self) -> Result<(), McpError> {
        let params = json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": { "name": "finmodel", "version": env!("CARGO_PKG_VERSION") },
        });
        let _ = self.request("initialize", params)?;
        // Notifications carry no id and expect no response.
        self.notify("notifications/initialized", json!({}))?;
        Ok(())
    }

    /// List the server's tools.
    pub fn list_tools(&mut self) -> Result<Vec<ToolInfo>, McpError> {
        let resp = self.request("tools/list", json!({}))?;
        let arr = resp
            .get("tools")
            .and_then(Value::as_array)
            .ok_or_else(|| McpError::Protocol("tools/list: missing tools array".into()))?;
        Ok(arr
            .iter()
            .filter_map(|t| serde_json::from_value(t.clone()).ok())
            .collect())
    }

    /// Call a tool by name with JSON arguments; returns the raw `result`.
    pub fn call_tool(&mut self, name: &str, args: Value) -> Result<Value, McpError> {
        self.request("tools/call", json!({ "name": name, "arguments": args }))
    }

    /// Send a JSON-RPC request and block for its matching response.
    fn request(&mut self, method: &str, params: Value) -> Result<Value, McpError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let msg = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        self.write_line(&msg)?;
        let deadline = Instant::now() + self.timeout;
        loop {
            let v = self.read_line(deadline)?;
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
            return Ok(v.get("result").cloned().unwrap_or(Value::Null));
        }
    }

    fn notify(&mut self, method: &str, params: Value) -> Result<(), McpError> {
        let msg = json!({ "jsonrpc": "2.0", "method": method, "params": params });
        self.write_line(&msg)
    }

    fn write_line(&mut self, v: &Value) -> Result<(), McpError> {
        let line = serde_json::to_string(v).map_err(|e| McpError::Protocol(e.to_string()))?;
        self.stdin
            .write_all(line.as_bytes())
            .and_then(|_| self.stdin.write_all(b"\n"))
            .and_then(|_| self.stdin.flush())
            .map_err(|e| McpError::Io(e.to_string()))
    }

    /// Read one JSON line, honoring the deadline. (Coarse: relies on the child
    /// producing lines; a genuinely hung child trips the deadline via a guard
    /// thread would need more machinery — this checks the clock between lines.)
    fn read_line(&mut self, deadline: Instant) -> Result<Value, McpError> {
        if Instant::now() >= deadline {
            return Err(McpError::Timeout(self.timeout));
        }
        let mut line = String::new();
        let n = self
            .stdout
            .read_line(&mut line)
            .map_err(|e| McpError::Io(e.to_string()))?;
        if n == 0 {
            return Err(McpError::Protocol("server closed the stream".into()));
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            // Blank keep-alive line; try again (bounded by deadline).
            return self.read_line(deadline);
        }
        serde_json::from_str(trimmed).map_err(|e| McpError::Protocol(format!("{e}: {trimmed}")))
    }
}

impl Drop for McpClient {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
