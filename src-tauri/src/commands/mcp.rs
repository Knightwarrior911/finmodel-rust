//! Tauri-managed MCP child process (Phase 3.2).
//!
//! One long-lived, least-privilege child per configured absolute executable.
//! Settings changes kill the old child **without waiting** for an in-flight
//! request (kill handle is outside the request mutex). MCP stays serial: the
//! client mutex is held for each request.

use std::path::Path;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde_json::Value;

use fm_mcp::{KillHandle, McpClient, McpError, McpSpawnOpts, ToolInfo};

use crate::commands::settings::read_settings;

#[derive(Clone, Debug, PartialEq, Eq)]
struct McpKey {
    command: String,
    args: Vec<String>,
}

struct Live {
    key: McpKey,
    client: McpClient,
    tools: Vec<ToolInfo>,
}

/// Managed state: at most one live MCP child.
///
/// `kill` is separate from `live` so [`reset`] can terminate a mid-request
/// child without waiting for the request lock.
#[derive(Default)]
pub struct McpManager {
    kill: Mutex<Option<KillHandle>>,
    live: Mutex<Option<Live>>,
}

impl McpManager {
    /// Kill any live child and clear the slot (settings reset / shutdown).
    /// Does **not** wait for an in-flight request — kills via the shared handle.
    pub fn reset(&self) {
        if let Some(k) = self.kill.lock().take() {
            k.kill();
        }
        *self.live.lock() = None;
    }

    /// Whether a live child is currently configured and connected.
    pub fn is_live(&self) -> bool {
        let live = self.live.lock();
        live.as_ref()
            .map(|l| !l.client.kill_handle().is_dead())
            .unwrap_or(false)
    }

    /// Ensure a live client matching current settings. Returns `Ok(false)` when
    /// MCP is not configured; `Err` when the absolute-path gate fails.
    pub fn ensure(&self, app: &tauri::AppHandle) -> Result<bool, String> {
        let s = read_settings(app);
        let cmd = s.mcp_command.trim().to_string();
        if cmd.is_empty() {
            self.reset();
            return Ok(false);
        }
        if !Path::new(&cmd).is_absolute() || !Path::new(&cmd).is_file() {
            self.reset();
            return Err("MCP command must be an absolute path to an existing executable".into());
        }
        let key = McpKey {
            command: cmd,
            args: s.mcp_args.clone(),
        };

        {
            let live = self.live.lock();
            if let Some(l) = live.as_ref() {
                if l.key == key && !l.client.kill_handle().is_dead() {
                    return Ok(true);
                }
            }
        }

        // (Re)spawn under both locks.
        self.reset();
        let mut client = McpClient::connect_with(McpSpawnOpts {
            command: key.command.clone(),
            args: key.args.clone(),
            scrub_env: true,
            ..Default::default()
        })
        .map_err(|e| e.to_string())?;
        let kill = client.kill_handle();
        let tools = client.list_tools().map_err(|e| e.to_string())?;
        *self.kill.lock() = Some(kill);
        *self.live.lock() = Some(Live { key, client, tools });
        Ok(true)
    }

    /// Advertised tools for the live child (empty if none).
    pub fn tools(&self) -> Vec<ToolInfo> {
        self.live
            .lock()
            .as_ref()
            .map(|l| l.tools.clone())
            .unwrap_or_default()
    }

    /// Call a tool on the live child. Serial. On timeout/cancel the child is
    /// killed and the slot invalidated so the next call re-spawns.
    pub fn call_tool(
        &self,
        name: &str,
        args: Value,
        deadline: Instant,
        cancel: &dyn Fn() -> bool,
    ) -> Result<Value, McpError> {
        let mut guard = self.live.lock();
        let live = guard.as_mut().ok_or(McpError::Invalidated)?;
        if live.client.kill_handle().is_dead() {
            *guard = None;
            return Err(McpError::Invalidated);
        }
        let result = live.client.request_cancellable(
            "tools/call",
            serde_json::json!({ "name": name, "arguments": args }),
            deadline,
            cancel,
        );
        match result {
            Ok(v) => Ok(v),
            Err(e) => {
                if matches!(
                    e,
                    McpError::Timeout(_)
                        | McpError::Cancelled
                        | McpError::Invalidated
                        | McpError::Io(_)
                        | McpError::Protocol(_)
                        | McpError::FrameTooLarge(_)
                ) {
                    // Invalidate without holding live while taking kill (avoid AB/BA).
                    *guard = None;
                    drop(guard);
                    if let Some(k) = self.kill.lock().take() {
                        k.kill();
                    }
                }
                Err(e)
            }
        }
    }

    /// Run a closure against the live client (serial). Prefer [`call_tool`] when
    /// a deadline/cancel is available — this path uses the client's default
    /// timeout only. Used by research adapters that take `&mut McpClient`.
    pub fn with_client<R>(
        &self,
        app: &tauri::AppHandle,
        f: impl FnOnce(&mut McpClient) -> R,
    ) -> Option<R> {
        match self.ensure(app) {
            Ok(true) => {}
            _ => return None,
        }
        let mut guard = self.live.lock();
        let live = guard.as_mut()?;
        if live.client.kill_handle().is_dead() {
            *guard = None;
            return None;
        }
        Some(f(&mut live.client))
    }
}

/// Validate MCP settings: absolute existing executable; empty clears.
pub fn validate_mcp_command(command: &str) -> Result<(), String> {
    let c = command.trim();
    if c.is_empty() {
        return Ok(());
    }
    let p = Path::new(c);
    if !p.is_absolute() {
        return Err("MCP command must be an absolute path".into());
    }
    if !p.is_file() {
        return Err("MCP command path does not exist or is not a file".into());
    }
    Ok(())
}

/// Default tool-call budget used when a caller does not supply a deadline.
pub fn default_deadline() -> Instant {
    Instant::now() + Duration::from_secs(120)
}
