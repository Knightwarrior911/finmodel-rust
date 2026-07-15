//! Web-search bridge commands (Phase 8.5) — thin wrappers over
//! `fm_research::web`, building a Roam MCP client from settings when configured,
//! else the plain-HTTP fallback. All heavy work runs on the blocking pool.

use crate::commands::settings::read_settings;
use crate::error::{AppError, AppResult};
use fm_mcp::McpClient;

/// Build an MCP client from settings, if an `mcp_command` is configured and it
/// connects. `None` → the basic (DDG/HTTP) fallback path.
fn mcp_from_settings(app: &tauri::AppHandle) -> Option<McpClient> {
    let s = read_settings(app);
    let cmd = s.mcp_command.trim().to_string();
    if cmd.is_empty() {
        return None;
    }
    McpClient::connect(&cmd, &s.mcp_args).ok()
}

/// Search the web. Roam MCP browser when configured (degrades to basic HTTP
/// search if the MCP call fails), else basic search. Returns
/// `{ "backend": "roam"|"basic", "hits": [ {title,url,snippet} ] }`.
#[tauri::command(rename_all = "snake_case")]
pub async fn web_search(app: tauri::AppHandle, query: String) -> AppResult<String> {
    let q = query.trim().to_string();
    if q.is_empty() {
        return Err(AppError::Config("Enter a search query.".into()));
    }
    tauri::async_runtime::spawn_blocking(move || {
        let mut client = mcp_from_settings(&app);
        let (backend, hits) = match client.as_mut() {
            Some(_) => match fm_research::web::web_search(&q, client.as_mut()) {
                Ok(h) => ("roam", h),
                // MCP path failed — degrade to basic rather than error out.
                Err(_) => (
                    "basic",
                    fm_research::web::web_search(&q, None)
                        .map_err(|e| AppError::Engine(format!("search failed: {e}")))?,
                ),
            },
            None => (
                "basic",
                fm_research::web::web_search(&q, None)
                    .map_err(|e| AppError::Engine(format!("search failed: {e}")))?,
            ),
        };
        serde_json::to_string(&serde_json::json!({ "backend": backend, "hits": hits }))
            .map_err(|e| AppError::Engine(e.to_string()))
    })
    .await
    .map_err(|e| AppError::Engine(format!("search task failed: {e}")))?
}

/// Read a page as markdown/text. Roam `read_markdown` when configured (degrades
/// to a tag-stripped GET on failure), else the basic fetch. Returns
/// `{ "text": "…" }`.
#[tauri::command(rename_all = "snake_case")]
pub async fn read_page(
    app: tauri::AppHandle,
    url: String,
    query: Option<String>,
) -> AppResult<String> {
    let url = url.trim().to_string();
    if url.is_empty() {
        return Err(AppError::Config("No URL to read.".into()));
    }
    tauri::async_runtime::spawn_blocking(move || {
        let mut client = mcp_from_settings(&app);
        let q = query.as_deref();
        let text = match client.as_mut() {
            Some(_) => match fm_research::web::read_page(&url, q, client.as_mut()) {
                Ok(t) => t,
                Err(_) => fm_research::web::read_page(&url, q, None)
                    .map_err(|e| AppError::Engine(format!("read failed: {e}")))?,
            },
            None => fm_research::web::read_page(&url, q, None)
                .map_err(|e| AppError::Engine(format!("read failed: {e}")))?,
        };
        serde_json::to_string(&serde_json::json!({ "text": text }))
            .map_err(|e| AppError::Engine(e.to_string()))
    })
    .await
    .map_err(|e| AppError::Engine(format!("read task failed: {e}")))?
}

/// Settings "Test connection" (8.2): connect to the given MCP command and list
/// its tools. Returns `{ "tool_count": N, "tools": [ {name,description} ] }`.
#[tauri::command(rename_all = "snake_case")]
pub async fn test_mcp(command: String, args: Option<Vec<String>>) -> AppResult<String> {
    let cmd = command.trim().to_string();
    if cmd.is_empty() {
        return Err(AppError::Config("Enter an MCP server command.".into()));
    }
    let args = args.unwrap_or_default();
    tauri::async_runtime::spawn_blocking(move || {
        let mut client = McpClient::connect(&cmd, &args)
            .map_err(|e| AppError::Engine(format!("connect failed: {e}")))?;
        let tools = client
            .list_tools()
            .map_err(|e| AppError::Engine(format!("list_tools failed: {e}")))?;
        serde_json::to_string(&serde_json::json!({
            "tool_count": tools.len(),
            "tools": tools,
        }))
        .map_err(|e| AppError::Engine(e.to_string()))
    })
    .await
    .map_err(|e| AppError::Engine(format!("mcp test task failed: {e}")))?
}
