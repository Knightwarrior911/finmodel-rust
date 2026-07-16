//! Web-search bridge commands — thin wrappers over `fm_research::web`, using
//! the Tauri-managed [`McpManager`] when configured, else the plain-HTTP
//! fallback. All heavy work runs on the blocking pool.

use crate::commands::mcp::McpManager;
use crate::error::{AppError, AppResult};
use fm_mcp::{McpClient, McpSpawnOpts};
use serde_json::json;
use tauri::Manager;

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
        let mgr = app.state::<McpManager>();
        let (backend, hits) = if mgr.ensure(&app).unwrap_or(false) {
            match mgr.with_client(&app, |c| fm_research::web::web_search(&q, Some(c))) {
                Some(Ok(h)) => ("roam", h),
                Some(Err(_)) | None => {
                    let h = fm_research::web::web_search(&q, None)
                        .map_err(|e| AppError::Engine(e.to_string()))?;
                    ("basic", h)
                }
            }
        } else {
            let h = fm_research::web::web_search(&q, None)
                .map_err(|e| AppError::Engine(e.to_string()))?;
            ("basic", h)
        };
        Ok(json!({ "backend": backend, "hits": hits }).to_string())
    })
    .await
    .map_err(|e| AppError::Engine(format!("search task failed: {e}")))?
}

/// Read a page as markdown/text + status. Roam `read_markdown` when configured
/// (degrades to the tag-stripped GET on failure), else the basic fetch.
#[tauri::command(rename_all = "snake_case")]
pub async fn read_page(
    app: tauri::AppHandle,
    url: String,
    query: Option<String>,
) -> AppResult<String> {
    let u = url.trim().to_string();
    if u.is_empty() {
        return Err(AppError::Config("Enter a URL.".into()));
    }
    let q = query;
    tauri::async_runtime::spawn_blocking(move || {
        let mgr = app.state::<McpManager>();
        let qref = q.as_deref();
        let page = if mgr.ensure(&app).unwrap_or(false) {
            match mgr.with_client(&app, |c| {
                fm_research::web::read_page_full(&u, qref, Some(c))
            }) {
                Some(Ok(p)) => p,
                Some(Err(_)) | None => fm_research::web::read_page_full(&u, qref, None)
                    .map_err(|e| AppError::Engine(e.to_string()))?,
            }
        } else {
            fm_research::web::read_page_full(&u, qref, None)
                .map_err(|e| AppError::Engine(e.to_string()))?
        };
        Ok(json!({
            "title": page.title,
            "text": page.text,
            "status": serde_json::to_value(page.status).unwrap_or(json!("ok")),
        })
        .to_string())
    })
    .await
    .map_err(|e| AppError::Engine(format!("read_page task failed: {e}")))?
}

/// Settings "Test connection": one-shot connect (does not touch the manager).
#[tauri::command(rename_all = "snake_case")]
pub async fn test_mcp(command: String, args: Option<Vec<String>>) -> AppResult<String> {
    let cmd = command.trim().to_string();
    let args = args.unwrap_or_default();
    crate::commands::mcp::validate_mcp_command(&cmd).map_err(AppError::Config)?;
    if cmd.is_empty() {
        return Err(AppError::Config(
            "Enter an absolute MCP command path.".into(),
        ));
    }
    tauri::async_runtime::spawn_blocking(move || {
        let mut client = McpClient::connect_with(McpSpawnOpts {
            command: cmd,
            args,
            scrub_env: true,
            ..Default::default()
        })
        .map_err(|e| AppError::Engine(e.to_string()))?;
        let tools = client
            .list_tools()
            .map_err(|e| AppError::Engine(e.to_string()))?;
        let names: Vec<_> = tools
            .iter()
            .map(|t| json!({ "name": t.name, "description": t.description }))
            .collect();
        Ok(json!({ "tool_count": names.len(), "tools": names }).to_string())
    })
    .await
    .map_err(|e| AppError::Engine(format!("test_mcp task failed: {e}")))?
}
