//! Chat: conversation store + the chat engine (OpenRouter tool-calling loop with
//! SSE streaming, plus a deterministic no-key fallback router).
//!
//! Conversations persist to `app_config_dir()/conversations/<id>.json`. The chat
//! engine drives the same blocking internals the old form commands used
//! (`model.rs`, `benchmark.rs`, `fm_research`, `fm_fetch`) — never shelling
//! through the IPC command wrappers — and streams assistant tokens + tool status
//! to the UI over Tauri events (`chat_delta`, `chat_tool`, `chat_done`).

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};
use tauri::{Emitter, Manager};

use crate::agent::executors::{SessionContext, ToolBackend};
use crate::commands::mcp::McpManager;
use crate::commands::settings::read_settings;
use crate::error::{AppError, AppResult};

/// Cap retained provider/error text before UI/persistence (roadmap: 8 KiB).
const MAX_ERROR_CHARS: usize = 8 * 1024;

/// Exact analyst system prompt for the chat brain.
const SYSTEM_PROMPT: &str = "You are finmodel's analyst assistant inside a desktop app. You build 3-statement + DCF Excel models from SEC EDGAR (with optional trading-comps peers, a scenario case, and a PowerPoint summary deck), benchmark peers, read the actual text of 10-K/10-Q filings, analyze local annual-report PDFs, research deals, read news and web pages. Use tools when the user asks for data or artifacts; never fabricate financial numbers — every number must come from a tool result. For qualitative filing content (risk factors, MD&A, business description) use read_filing, never web_search. For a specific reported financial figure (revenue/sales, net income, gross profit, operating income, EPS) for a US company, call get_financials — it returns the exact number from SEC XBRL; do NOT read narrative filing items or say the figure is undisclosed when get_financials can fetch it. 'Sales for year N' means reported revenue for fiscal year N. Answer the number directly and concisely; do not punt to building a model unless the user asks. Use build_model for a full model or foreign filers, research for qualitative/current context. When a request needs more than one step or tool, you MUST begin your reply with a one-line plan on its own line, that starts with Plan: — for example, Plan: pull Tesla and Ford financials, then compare. Then carry it out end to end — call the tools you need and give the answer — without stopping to ask whether to continue; pause only for a required approval or a genuine either/or choice. Be concise. Format with markdown. When a tool returns a card, refer to it instead of repeating its table.";

/// Convert unix seconds to an ISO-8601 UTC timestamp (civil date via Hinnant's
/// algorithm). Lexicographically sortable == chronological.
fn iso_utc(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (h, m, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { y + 1 } else { y };
    format!("{year:04}-{month:02}-{d:02}T{h:02}:{m:02}:{s:02}Z")
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn iso_now() -> String {
    iso_utc(now_secs())
}

/// Conversation title from the first user message (48 chars + ellipsis).
fn title_from(msg: &str) -> String {
    let t = msg.trim();
    let chars: Vec<char> = t.chars().collect();
    if chars.len() <= 48 {
        t.to_string()
    } else {
        format!("{}…", chars[..48].iter().collect::<String>())
    }
}

// ---------------------------------------------------------------------------
// Storage
// ---------------------------------------------------------------------------

fn conv_dir(app: &tauri::AppHandle) -> AppResult<PathBuf> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| AppError::Config(format!("no config dir: {e}")))?
        .join("conversations");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// The app-generated conversation ID format is `<ms>-<4 hex>`. Enforcing it here
/// inherently rejects path separators, traversal (`..`), absolute paths, and
/// reserved device names, so the resolver only ever joins a leaf filename
/// beneath the conversations directory.
fn validate_conv_id(id: &str) -> AppResult<()> {
    let (ms, hex) = id.split_once('-').ok_or_else(bad_conv_id)?;
    let ms_ok = !ms.is_empty() && ms.len() <= 20 && ms.bytes().all(|b| b.is_ascii_digit());
    let hex_ok = hex.len() == 4 && hex.bytes().all(|b| b.is_ascii_hexdigit());
    if ms_ok && hex_ok {
        Ok(())
    } else {
        Err(bad_conv_id())
    }
}

fn bad_conv_id() -> AppError {
    AppError::Config("invalid conversation id".into())
}

fn conv_path(app: &tauri::AppHandle, id: &str) -> AppResult<PathBuf> {
    validate_conv_id(id)?;
    Ok(conv_dir(app)?.join(format!("{id}.json")))
}

/// `[{ id, title, updated, preview }]` from the SQLite store, newest first.
#[tauri::command(rename_all = "snake_case")]
pub async fn list_conversations(app: tauri::AppHandle) -> AppResult<String> {
    let store = app
        .try_state::<crate::store::AppStore>()
        .ok_or_else(|| AppError::Config("store unavailable".into()))?;
    let handle = store.handle.clone();
    let ws = store.default_workspace_id.clone();
    let rows = handle
        .call(move |db| db.list_conversations(&ws))
        .await
        .map_err(|e| AppError::Engine(e.to_string()))?;
    let items: Vec<Value> = rows
        .into_iter()
        .map(|(id, title, updated, preview, project_id)| {
            json!({
                "id": id,
                "title": if title.is_empty() { "New conversation".to_string() } else { title },
                "updated": updated,
                "preview": preview,
                "project_id": project_id,
            })
        })
        .collect();
    Ok(serde_json::to_string(&items)?)
}

/// Legacy-shaped conversation ({id,title,messages:[{role,content|card,ts}]})
/// rebuilt from the SQLite branch path so the existing renderer is unchanged.
#[tauri::command(rename_all = "snake_case")]
pub async fn load_conversation(app: tauri::AppHandle, id: String) -> AppResult<String> {
    let store = app
        .try_state::<crate::store::AppStore>()
        .ok_or_else(|| AppError::Config("store unavailable".into()))?;
    let handle = store.handle.clone();
    let out = handle
        .call(move |db| -> Result<Value, String> {
            let title = db
                .conversation_title(&id)
                .map_err(|e| e.to_string())?
                .ok_or_else(|| "conversation not found".to_string())?;
            let branch = db.branch_path(&id).map_err(|e| e.to_string())?;
            let mut messages: Vec<Value> = Vec::new();
            for m in &branch {
                let parts = db.message_parts(&m.id).map_err(|e| e.to_string())?;
                for part in &parts {
                    match part.kind.as_str() {
                        "text" => {
                            let text = serde_json::from_str::<Value>(&part.payload_json)
                                .ok()
                                .and_then(|v| v.get("text").and_then(|t| t.as_str()).map(String::from))
                                .unwrap_or_default();
                            if !text.trim().is_empty() {
                                messages.push(json!({ "role": m.role, "content": text, "ts": m.created_at }));
                            }
                        }
                        "result" => {
                            let card = serde_json::from_str::<Value>(&part.payload_json)
                                .ok()
                                .and_then(|v| v.get("card").cloned())
                                .unwrap_or(Value::Null);
                            if !card.is_null() {
                                messages.push(json!({ "role": "assistant", "card": card, "ts": m.created_at }));
                            }
                        }
                        _ => {}
                    }
                }
            }
            Ok(json!({ "id": id, "title": title, "messages": messages }))
        })
        .await
        .map_err(AppError::Engine)?;
    Ok(serde_json::to_string(&out)?)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn delete_conversation(app: tauri::AppHandle, id: String) -> AppResult<String> {
    let store = app
        .try_state::<crate::store::AppStore>()
        .ok_or_else(|| AppError::Config("store unavailable".into()))?;
    let handle = store.handle.clone();
    let cid = id.clone();
    handle
        .call(move |db| db.delete_conversation(&cid))
        .await
        .map_err(|e| AppError::Engine(e.to_string()))?;
    // Also remove any legacy JSON so startup import can't resurrect it.
    if let Ok(path) = conv_path(&app, &id) {
        let _ = std::fs::remove_file(&path);
    }
    Ok(json!({ "ok": true }).to_string())
}

#[tauri::command(rename_all = "snake_case")]
pub async fn rename_conversation(
    app: tauri::AppHandle,
    id: String,
    title: String,
) -> AppResult<String> {
    let store = app
        .try_state::<crate::store::AppStore>()
        .ok_or_else(|| AppError::Config("store unavailable".into()))?;
    let handle = store.handle.clone();
    let new_title = title_from(&title);
    let cid = id.clone();
    let t2 = new_title.clone();
    handle
        .call(move |db| db.rename_conversation(&cid, &t2, &crate::store::now_iso()))
        .await
        .map_err(|e| AppError::Engine(e.to_string()))?;
    Ok(json!({ "id": id, "title": new_title }).to_string())
}

// ---------------------------------------------------------------------------
// Chat engine
// ---------------------------------------------------------------------------

/// One accumulated tool call from the streamed `delta.tool_calls` fragments.
#[derive(Clone, Debug, Default, PartialEq)]
struct ToolCall {
    id: String,
    name: String,
    arguments: String,
}

/// Outcome of a single streaming completion request.
enum StreamOutcome {
    Ok {
        content: String,
        tool_calls: Vec<ToolCall>,
        meta: TurnMeta,
    },
    /// Stop was requested mid-stream. Distinct from Ok so the turn terminates
    /// visibly rather than treating partial content as a successful completion.
    Cancelled { content: String },
    /// Pre-stream 400/404 — the model likely rejects `tools`.
    ToolsUnsupported,
    /// Pre-stream failure (auth, network, other status). Redacted category only.
    Failed(String),
}

/// Provider-reported terminal metadata captured from the stream (Phase 1.4).
/// Counts/IDs only — never prompts, keys, paths, or provider bodies.
#[derive(Clone, Debug, Default, PartialEq)]
struct TurnMeta {
    finish_reason: Option<String>,
    native_finish_reason: Option<String>,
    model: Option<String>,
    provider: Option<String>,
    usage: Option<Value>,
    /// Count of SSE payloads that failed JSON parse (never the raw payload).
    sse_parse_errors: u32,
}

/// Redact a provider/transport error to a short category + status. Never retains
/// the provider body, keys, paths, or source text (cap at [`MAX_ERROR_CHARS`]).
fn redact_provider_error(status: Option<u16>, body: &str) -> String {
    let lower = body.to_lowercase();
    let category = if let Some(code) = status {
        if code == 401 || code == 403 {
            "auth"
        } else if code == 429 {
            "rate_limit"
        } else if code >= 500 {
            "provider_5xx"
        } else if lower.contains("content") && lower.contains("filter") {
            "content_filter"
        } else if lower.contains("tool") || lower.contains("unsupported") {
            "unsupported_parameter"
        } else {
            "provider_error"
        }
    } else if lower.contains("timed out") || lower.contains("timeout") {
        "timeout"
    } else if lower.contains("dns") || lower.contains("connect") {
        "network"
    } else {
        "transport"
    };
    let msg = match status {
        Some(code) => format!("OpenRouter request failed ({category}, HTTP {code})"),
        None => format!("OpenRouter request failed ({category})"),
    };
    if msg.chars().count() > MAX_ERROR_CHARS {
        msg.chars().take(MAX_ERROR_CHARS).collect()
    } else {
        msg
    }
}

/// Build the OpenRouter chat request body (pure — unit-tested). When `tools` is
/// non-empty, sets `tool_choice: "auto"` and `parallel_tool_calls` from
/// `parallel` — true for tool-capable models so independent calls (e.g. per-
/// company financials) fan out in one turn and run concurrently.
pub(crate) fn build_chat_request(
    model: &str,
    msgs: &[Value],
    tools: &[Value],
    stream: bool,
    parallel: bool,
) -> Value {
    let mut req = json!({
        "model": model,
        "messages": msgs,
        "temperature": 0,
        "stream": stream,
    });
    if !tools.is_empty() {
        req["tools"] = json!(tools);
        req["tool_choice"] = json!("auto");
        req["parallel_tool_calls"] = json!(parallel);
    }
    req
}

/// Apply one SSE `data:` payload to the running accumulators. Returns the
/// content chunk (for live emission), if any. Malformed JSON increments
/// `meta.sse_parse_errors` and returns `None` (never stores the raw payload).
fn apply_delta(
    content: &mut String,
    calls: &mut Vec<ToolCall>,
    meta: &mut TurnMeta,
    payload: &str,
) -> Option<String> {
    let v: Value = match serde_json::from_str(payload) {
        Ok(v) => v,
        Err(_) => {
            meta.sse_parse_errors = meta.sse_parse_errors.saturating_add(1);
            return None;
        }
    };
    // Top-level model / provider ids (OpenRouter may send either on any chunk).
    if meta.model.is_none() {
        if let Some(m) = v.get("model").and_then(|m| m.as_str()) {
            if !m.is_empty() {
                meta.model = Some(m.to_string());
            }
        }
    }
    if meta.provider.is_none() {
        // OpenRouter: `provider` string, or nested `provider.name`.
        if let Some(p) = v.get("provider").and_then(|p| {
            p.as_str().map(|s| s.to_string()).or_else(|| {
                p.get("name")
                    .and_then(|n| n.as_str())
                    .map(|s| s.to_string())
            })
        }) {
            if !p.is_empty() {
                meta.provider = Some(p);
            }
        }
    }
    let delta = &v["choices"][0]["delta"];
    let mut chunk = None;
    if let Some(c) = delta.get("content").and_then(|c| c.as_str()) {
        if !c.is_empty() {
            content.push_str(c);
            chunk = Some(c.to_string());
        }
    }
    if let Some(tcs) = delta.get("tool_calls").and_then(|t| t.as_array()) {
        for tc in tcs {
            let idx = tc.get("index").and_then(|i| i.as_u64()).unwrap_or(0) as usize;
            while calls.len() <= idx {
                calls.push(ToolCall::default());
            }
            let slot = &mut calls[idx];
            if let Some(id) = tc.get("id").and_then(|i| i.as_str()) {
                if !id.is_empty() {
                    slot.id = id.to_string();
                }
            }
            if let Some(f) = tc.get("function") {
                if let Some(n) = f.get("name").and_then(|n| n.as_str()) {
                    if !n.is_empty() {
                        slot.name.push_str(n);
                    }
                }
                if let Some(a) = f.get("arguments").and_then(|a| a.as_str()) {
                    slot.arguments.push_str(a);
                }
            }
        }
    }
    // Terminal metadata: finish_reason / native_finish_reason (actionable:
    // length/content_filter) and provider-reported token usage (counts only).
    let choice0 = &v["choices"][0];
    if let Some(fr) = choice0.get("finish_reason").and_then(|f| f.as_str()) {
        if !fr.is_empty() {
            meta.finish_reason = Some(fr.to_string());
        }
    }
    if let Some(nfr) = choice0.get("native_finish_reason").and_then(|f| f.as_str()) {
        if !nfr.is_empty() {
            meta.native_finish_reason = Some(nfr.to_string());
        }
    }
    if v.get("usage").map(|u| u.is_object()).unwrap_or(false) {
        // Keep only numeric token fields — never any content-bearing keys.
        let u = &v["usage"];
        meta.usage = Some(json!({
            "prompt_tokens": u.get("prompt_tokens").and_then(|x| x.as_u64()).unwrap_or(0),
            "completion_tokens": u.get("completion_tokens").and_then(|x| x.as_u64()).unwrap_or(0),
            "total_tokens": u.get("total_tokens").and_then(|x| x.as_u64()).unwrap_or(0),
        }));
    }
    chunk
}

/// Accumulate full content + tool calls from a list of SSE `data:` payloads
/// (pure — unit-tested). Stops at `[DONE]`.
#[cfg(test)]
fn sse_accumulate(events: &[&str]) -> (String, Vec<ToolCall>, TurnMeta) {
    let mut content = String::new();
    let mut calls: Vec<ToolCall> = Vec::new();
    let mut meta = TurnMeta::default();
    for ev in events {
        if ev.trim() == "[DONE]" {
            break;
        }
        apply_delta(&mut content, &mut calls, &mut meta, ev);
    }
    (content, calls, meta)
}

/// Drain complete SSE lines from a raw byte buffer. Incomplete trailing bytes
/// (including mid-codepoint UTF-8) stay in `buf` until the next chunk arrives.
fn sse_take_lines(buf: &mut Vec<u8>) -> Vec<String> {
    let mut lines = Vec::new();
    while let Some(nl) = buf.iter().position(|&b| b == b'\n') {
        let mut line_bytes = buf.drain(..=nl).collect::<Vec<u8>>();
        if line_bytes.last() == Some(&b'\n') {
            line_bytes.pop();
        }
        if line_bytes.last() == Some(&b'\r') {
            line_bytes.pop();
        }
        let line = match String::from_utf8(line_bytes) {
            Ok(s) => s,
            Err(e) => String::from_utf8_lossy(e.as_bytes()).into_owned(),
        };
        lines.push(line);
    }
    lines
}

/// Connect timeout for OpenRouter (Phase 3.1).
const CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
/// No-progress timeout while reading the SSE body (Phase 3.1).
const NO_PROGRESS_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);

/// Application-lived async HTTP client (cloneable; connection pool shared).
pub(crate) fn shared_http_client() -> Result<&'static reqwest::Client, String> {
    use std::sync::OnceLock;
    static CLIENT: OnceLock<Result<reqwest::Client, String>> = OnceLock::new();
    match CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .pool_idle_timeout(std::time::Duration::from_secs(90))
            .build()
            .map_err(|e| e.to_string())
    }) {
        Ok(c) => Ok(c),
        Err(e) => Err(e.clone()),
    }
}

async fn openrouter_stream_async(
    app: &tauri::AppHandle,
    conv_id: &str,
    run_id: &str,
    cfg: &fm_extract::LlmConfig,
    req: &Value,
    cancel: &tokio_util::sync::CancellationToken,
    timeout: std::time::Duration,
) -> StreamOutcome {
    use futures_util::StreamExt;

    if cancel.is_cancelled() {
        return StreamOutcome::Cancelled {
            content: String::new(),
        };
    }
    if timeout.is_zero() {
        return StreamOutcome::Failed("chat deadline elapsed".into());
    }

    let client = match shared_http_client() {
        Ok(c) => c,
        Err(e) => return StreamOutcome::Failed(redact_provider_error(None, &e)),
    };

    // Provider endpoint from settings (OpenRouter default; any OpenAI-compatible
    // base works with the user's own key).
    let chat_url = crate::commands::settings::chat_completions_url(&read_settings(app));
    // Race connect against cancel + overall remaining budget.
    let send_fut = client
        .post(&chat_url)
        .header("Authorization", format!("Bearer {}", cfg.api_key))
        .header("Content-Type", "application/json")
        .header("HTTP-Referer", "https://github.com/finmodel")
        .header("X-Title", "finmodel")
        .timeout(timeout)
        .json(req)
        .send();

    let resp = tokio::select! {
        _ = cancel.cancelled() => {
            return StreamOutcome::Cancelled { content: String::new() };
        }
        res = send_fut => match res {
            Ok(r) => r,
            Err(e) => {
                if e.is_timeout() {
                    return StreamOutcome::Failed(redact_provider_error(None, "connect/request timeout"));
                }
                return StreamOutcome::Failed(redact_provider_error(None, &e.to_string()));
            }
        }
    };

    let status = resp.status();
    if !status.is_success() {
        let code = status.as_u16();
        let body = resp.text().await.unwrap_or_default();
        if (code == 400 || code == 404)
            && (body.to_lowercase().contains("tool")
                || body.to_lowercase().contains("unsupported")
                || body.to_lowercase().contains("require_parameters"))
        {
            return StreamOutcome::ToolsUnsupported;
        }
        return StreamOutcome::Failed(redact_provider_error(Some(code), &body));
    }

    let mut content = String::new();
    let mut calls: Vec<ToolCall> = Vec::new();
    let mut meta = TurnMeta::default();
    // Byte buffer so multibyte UTF-8 split across chunks is never lossy-decoded mid-codepoint.
    let mut buf: Vec<u8> = Vec::new();
    let mut stream = resp.bytes_stream();

    loop {
        // Race next chunk against cancel and no-progress timeout.
        let next = tokio::select! {
            _ = cancel.cancelled() => {
                return StreamOutcome::Cancelled { content };
            }
            chunk = tokio::time::timeout(NO_PROGRESS_TIMEOUT, stream.next()) => chunk,
        };
        match next {
            // no-progress elapsed
            Err(_) => {
                return StreamOutcome::Failed(redact_provider_error(None, "no progress timeout"));
            }
            Ok(None) => break, // EOF
            Ok(Some(Err(e))) => {
                return StreamOutcome::Failed(redact_provider_error(None, &e.to_string()));
            }
            Ok(Some(Ok(bytes))) => {
                buf.extend_from_slice(&bytes);
                for line in sse_take_lines(&mut buf) {
                    let payload = match line.strip_prefix("data:") {
                        Some(p) => p.trim(),
                        None => continue,
                    };
                    if payload == "[DONE]" {
                        calls.retain(|c| !c.name.is_empty());
                        if let Some(usage) = &meta.usage {
                            emit_chat(
                                app,
                                "chat_usage",
                                conv_id,
                                run_id,
                                json!({
                                    "usage": usage,
                                    "model": meta.model,
                                    "provider": meta.provider,
                                    "finish_reason": meta.finish_reason,
                                    "native_finish_reason": meta.native_finish_reason,
                                    "sse_parse_errors": meta.sse_parse_errors,
                                }),
                            );
                        }
                        return StreamOutcome::Ok {
                            content,
                            tool_calls: calls,
                            meta,
                        };
                    }
                    if payload.is_empty() {
                        continue;
                    }
                    if let Some(chunk) = apply_delta(&mut content, &mut calls, &mut meta, payload) {
                        emit_agent_ephemeral(
                            app,
                            conv_id,
                            run_id,
                            fm_agent::types::EventKind::AssistantTextDelta,
                            json!({ "text": chunk }),
                        );
                    }
                }
            }
        }
    }

    calls.retain(|c| !c.name.is_empty());
    if let Some(usage) = &meta.usage {
        emit_chat(
            app,
            "chat_usage",
            conv_id,
            run_id,
            json!({
                "usage": usage,
                "model": meta.model,
                "provider": meta.provider,
                "finish_reason": meta.finish_reason,
                "native_finish_reason": meta.native_finish_reason,
                "sse_parse_errors": meta.sse_parse_errors,
            }),
        );
    }
    StreamOutcome::Ok {
        content,
        tool_calls: calls,
        meta,
    }
}

/// Convert a legacy chat stream outcome into the agent StreamAccumulator
/// seam used by crate::agent::driver::model_out_from_stream.
fn legacy_to_accumulator(
    content: String,
    tool_calls: Vec<ToolCall>,
    meta: TurnMeta,
) -> crate::agent::provider::StreamAccumulator {
    use crate::agent::provider::{AccToolCall, StreamAccumulator, TurnMeta as AccMeta};
    StreamAccumulator {
        content,
        calls: tool_calls
            .into_iter()
            .map(|c| AccToolCall {
                id: c.id,
                name: c.name,
                arguments: c.arguments,
            })
            .collect(),
        meta: AccMeta {
            finish_reason: meta.finish_reason,
            native_finish_reason: meta.native_finish_reason,
            model: meta.model,
            provider: meta.provider,
            usage: meta.usage,
            parse_errors: meta.sse_parse_errors,
        },
    }
}

/// Agent-loop streaming entry: same OpenRouter SSE path as chat_send, but
/// returns a StreamAccumulator for the unified driver. Failures surface as
/// Err with a redacted category string (never provider bodies/keys).
pub(crate) async fn stream_completion_for_agent(
    app: &tauri::AppHandle,
    conv_id: &str,
    run_id: &str,
    cfg: &fm_extract::LlmConfig,
    req: &Value,
    cancel: &tokio_util::sync::CancellationToken,
    timeout: std::time::Duration,
) -> Result<crate::agent::provider::StreamAccumulator, String> {
    match openrouter_stream_async(app, conv_id, run_id, cfg, req, cancel, timeout).await {
        StreamOutcome::Ok {
            content,
            tool_calls,
            meta,
        } => Ok(legacy_to_accumulator(content, tool_calls, meta)),
        // Cancelled is user-initiated, so its partial content is kept intentionally.
        // Mid-stream failures arrive as Failed below and surface as Err.
        StreamOutcome::Cancelled { content } => Ok(legacy_to_accumulator(
            content,
            Vec::new(),
            TurnMeta::default(),
        )),
        StreamOutcome::ToolsUnsupported => Err("tools_unsupported".into()),
        StreamOutcome::Failed(e) => Err(e),
    }
}

/// Seed messages for a fresh agent turn (system policy + current user text).
pub(crate) fn seed_agent_messages(user_text: &str) -> Vec<Value> {
    let today = &iso_now()[..10];
    let system = format!(
        "{SYSTEM_PROMPT}\n\nToday's date is {today} (UTC). You do not have reliable knowledge of events after your training cutoff, so for anything current, recent, \"latest\", or time-bound, rely on tool results rather than your own memory.\n\nNever compute material percentages, growth rates, ratios, or multiples in prose — call a deterministic tool (get_financials, benchmark_peers, build_model) and cite its source. The structured tool result, not your own arithmetic, is the authority for every material number."
    );
    vec![
        json!({ "role": "system", "content": system }),
        json!({ "role": "user", "content": user_text }),
    ]
}

/// Truncate a string to `max` chars for the LLM tool result.
fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let head: String = s.chars().take(max).collect();
        format!("{head}…")
    }
}

/// Execute a tool. Returns `(llm_result_text, card)`. Errors bubble as `Err`.
/// `user_msg` is the ORIGINAL user text — the `research` tool normalizes against
/// it rather than the model's rewritten `question` argument.
/// `conversation_id` is a trusted app value used only for artifact ownership.
pub(crate) fn run_tool(
    app: &tauri::AppHandle,
    name: &str,
    args: &Value,
    user_msg: &str,
    conversation_id: &str,
) -> Result<(String, Value), String> {
    match name {
        "build_model" => tool_build_model(app, args),
        "benchmark_peers" => tool_benchmark(app, args),
        "web_search" => tool_web_search(app, args),
        "read_page" => tool_read_page(app, args),
        "get_news" => tool_get_news(args),
        "research_deal" => tool_research_deal(app, args),
        "get_quote" => tool_get_quote(args),
        "get_financials" => tool_get_financials(args),
        "list_filings" => tool_list_filings(args),
        "read_filing" => tool_read_filing(app, args),
        "analyze_pdf" => tool_analyze_pdf(app, args, conversation_id),
        "research" => tool_research(app, args, user_msg),
        "use_skill" => tool_use_skill(app, args),
        other => Err(format!("unknown tool: {other}")),
    }
}

/// Load a named skill's full instructions from the user's skill library so the
/// model can follow them (progressive disclosure — the catalog only carries
/// names + descriptions). The body is returned as the tool summary; there is no
/// result card (display is null) since the payload is instructions, not data.
fn tool_use_skill(app: &tauri::AppHandle, args: &Value) -> Result<(String, Value), String> {
    use tauri::Manager;
    let name = args
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();
    if name.is_empty() {
        return Err("use_skill requires a `name`".into());
    }
    let dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
    let skill = crate::agent::skills::get_skill(&dir, name)
        .ok_or_else(|| format!("skill `{name}` not found"))?;
    // Record the use (Task 7.3) so actively-used skills never age out. A skill
    // with no lifecycle row yet (hand-dropped or seeded file) is registered
    // first. Best-effort and async: a store failure never blocks the skill.
    if let Some(store) = app.try_state::<crate::store::AppStore>() {
        let handle = store.handle.clone();
        let n = skill.name.clone();
        tauri::async_runtime::spawn(async move {
            let now = crate::store::now_iso();
            let _ = handle
                .call(move |db| -> crate::store::StoreResult<()> {
                    if !db.record_skill_use(&n, &now)? {
                        db.upsert_skill(&n, 1, &now)?;
                        db.record_skill_use(&n, &now)?;
                    }
                    Ok(())
                })
                .await;
        });
    }
    let summary = format!(
        "Skill `{}` — {}\n\n{}",
        skill.name, skill.description, skill.body
    );
    Ok((summary, Value::Null))
}

/// Production [`ToolBackend`] that dispatches into the existing chat tool cores.
pub struct ChatToolBackend<'a> {
    pub app: &'a tauri::AppHandle,
}

impl ToolBackend for ChatToolBackend<'_> {
    fn invoke(
        &self,
        name: &str,
        args: &Value,
        ctx: &SessionContext,
    ) -> Result<(String, Value), String> {
        run_tool(self.app, name, args, &ctx.user_msg, &ctx.conversation_id)
    }
}

/// The single `research` tool for an **allowed multi-action plan** only
/// (ordinary native schemas do NOT include it — research is application-invoked).
/// Normalize the model's hints (mode/tickers/depth) against the ORIGINAL user
/// text, fill deal parties from that text, run the pipeline, return summary +
/// card. Cap depth at Quick so the driver wall-clock deadline is 30s.
fn tool_research(
    app: &tauri::AppHandle,
    args: &Value,
    user_msg: &str,
) -> Result<(String, Value), String> {
    use crate::commands::research::{run_research, HttpBackend, OpenRouterSynthesizer};
    use fm_research::machine::{Action, ResearchBudgets, ResearchMachine};
    use fm_research::research::{ResearchDepth, ResearchMode, ResearchOutput, ResearchToolArgs};

    // The registry/schema/fallback all use `query` (ecosystem standard), but
    // ResearchToolArgs is deny_unknown_fields with `question`. Translate the
    // key at this one boundary; into_request overwrites it with the original
    // user text anyway, so this only satisfies the strict parser.
    let mut argv = args.clone();
    if let Some(obj) = argv.as_object_mut() {
        if let Some(q) = obj.remove("query") {
            obj.entry("question").or_insert(q);
        }
    }
    let ta: ResearchToolArgs =
        serde_json::from_value(argv).map_err(|e| format!("invalid arguments: {e}"))?;
    let mut request = ta.into_request(user_msg);
    // Application fills deal parties from the original user text.
    if request.mode == ResearchMode::Deal {
        let (t, a) = fm_research::agent::parse_ma_query(user_msg, "");
        if !t.trim().is_empty() {
            request.target = Some(t);
        }
        if !a.trim().is_empty() {
            request.acquirer = Some(a);
        }
    }
    // Tool-call path has no chat Stop wiring; Quick + driver timeout keep the
    // stage awaits under a 30s wall-clock bound.
    request.depth = ResearchDepth::Quick;
    request.validate().map_err(|e| e.to_string())?;

    let settings = read_settings(app);
    let api_key = settings.openrouter_api_key.trim().to_string();
    if api_key.is_empty() {
        return Err("research requires an OpenRouter API key".into());
    }
    let model = settings.model.trim().to_string();
    let strict_json = settings
        .model_capability
        .as_ref()
        .map(|c| c.model_id == model && c.strict_json)
        .unwrap_or(false);
    let budgets = ResearchBudgets::from_depth(request.depth);
    let machine = ResearchMachine::new(request.clone(), budgets, iso_now());
    let backend = HttpBackend {
        max_sources: budgets.max_sources,
        per_query_results: 6,
        mode: request.mode,
        tickers: request.tickers.clone(),
        filing_forms: request.filing_forms.clone(),
        question: request.question.clone(),
        target: request.target.clone().unwrap_or_default(),
        acquirer: request.acquirer.clone().unwrap_or_default(),
    };
    let synth = OpenRouterSynthesizer {
        api_key,
        model,
        strict_json,
        request: request.clone(),
    };
    // Tool-call path has no chat Stop wiring. Boundedness comes from the
    // driver's per-stage wall-clock timeout on request.depth (Quick = 30s).
    let cancel = tokio_util::sync::CancellationToken::new();
    match tauri::async_runtime::block_on(run_research(
        machine,
        request,
        &backend,
        &synth,
        &cancel,
        &|_| {},
    )) {
        Action::Done(ResearchOutput::Answer(a)) => {
            let card = json!({ "type": "research_answer", "answer": serde_json::to_value(&a).unwrap_or(Value::Null) });
            let mut text = a.summary.text.clone();
            for sec in &a.sections {
                text.push_str(&format!("\n\n{}: ", sec.heading));
                for p in &sec.paragraphs {
                    text.push_str(&p.text);
                    text.push(' ');
                }
            }
            Ok((truncate(&text, 4000), card))
        }
        Action::Done(ResearchOutput::Digest(d)) => {
            let card = json!({ "type": "research_digest", "digest": serde_json::to_value(&d).unwrap_or(Value::Null) });
            Ok((
                format!("Source digest — no synthesis. {}", d.limitations.join(" ")),
                card,
            ))
        }
        Action::Error { code } => Err(format!("research error: {code}")),
        Action::Cancelled => Err("research cancelled".into()),
        _ => Err("research did not reach a terminal".into()),
    }
}

fn tool_build_model(app: &tauri::AppHandle, args: &Value) -> Result<(String, Value), String> {
    let ticker = args["ticker"].as_str().unwrap_or("").trim().to_string();
    if ticker.is_empty() {
        return Err("build_model requires a ticker".into());
    }
    let skip_review = args["skip_review"].as_bool().unwrap_or(false);
    let mut opts = fm_build::BuildOptions::default();
    if let Some(y) = args["years"].as_u64() {
        if (1..=10).contains(&y) {
            opts.proj_years = y as usize;
        }
    }
    opts.deck = true;
    if let Some(peers) = args["peers"].as_array() {
        opts.peers = peers
            .iter()
            .filter_map(|t| t.as_str())
            .map(|t| t.trim().to_uppercase())
            .filter(|t| !t.is_empty())
            .collect();
    }
    opts.active_case = match args["case"].as_str() {
        Some("upside") => 2,
        Some("downside") => 3,
        _ => 1,
    };

    if skip_review {
        let summary = crate::commands::model::build_model_blocking(app, &ticker, opts)
            .map_err(|e| e.to_string())?;
        let v: Value = serde_json::from_str(&summary).map_err(|e| e.to_string())?;
        let val = &v["valuation"];
        let card = json!({
            "type": "model",
            "ticker": v["ticker"],
            "currency": v["currency"],
            "xlsx_path": v["xlsx_path"],
            "pptx_path": v["pptx_path"],
            "comps": v["comps"],
            "case": v["case"],
            "valuation": val,
        });
        let text = format!(
            "Built {} model ({}). Implied price {}, upside {}%, WACC {}, EV {}. Excel: {}",
            ticker,
            v["currency"].as_str().unwrap_or(""),
            fmt_opt(&val["price_per_share"]),
            fmt_opt(&val["upside_pct"]),
            fmt_opt(&val["wacc"]),
            fmt_opt(&val["ev"]),
            v["xlsx_path"].as_str().unwrap_or(""),
        );
        Ok((text, card))
    } else {
        let prep = crate::commands::model::prepare_model_core(app, &ticker, opts)
            .map_err(|e| e.to_string())?;
        let mut card: Value = serde_json::from_str(&prep).map_err(|e| e.to_string())?;
        card["type"] = json!("assumptions");
        Ok((
            "Presented an editable assumptions grid to the user; they will finalize it manually."
                .into(),
            card,
        ))
    }
}

fn fmt_opt(v: &Value) -> String {
    match v.as_f64() {
        Some(n) => format!("{n:.2}"),
        None => "n/a".to_string(),
    }
}

fn tool_benchmark(app: &tauri::AppHandle, args: &Value) -> Result<(String, Value), String> {
    let tickers: Vec<String> = match args["tickers"].as_array() {
        Some(arr) => arr
            .iter()
            .filter_map(|t| t.as_str())
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
            .collect(),
        None => args["tickers"]
            .as_str()
            .unwrap_or("")
            .split(',')
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
            .collect(),
    };
    if tickers.len() < 2 {
        return Err("benchmark_peers needs at least two tickers".into());
    }
    let opts = crate::commands::benchmark::BenchOpts {
        period: args["period"].as_str().unwrap_or("annual").to_string(),
        multiples: args["multiples"].as_bool().unwrap_or(false),
        usd: args["usd"].as_bool().unwrap_or(false),
        title: None,
        out_path: None,
        deck: true,
    };
    let summary = crate::commands::benchmark::benchmark_blocking(app, &tickers.join(","), opts)
        .map_err(|e| e.to_string())?;
    let v: Value = serde_json::from_str(&summary).map_err(|e| e.to_string())?;

    let headers = json!([
        { "key": "ticker", "label": "Ticker" },
        { "key": "fiscal_year", "label": "FY" },
        { "key": "revenue_m", "label": "Revenue (m)" },
        { "key": "ebitda_m", "label": "EBITDA (m)" },
        { "key": "net_income_m", "label": "Net income (m)" },
        { "key": "ebitda_margin", "label": "EBITDA margin" },
        { "key": "net_margin", "label": "Net margin" },
        { "key": "roe", "label": "ROE" },
        { "key": "net_debt_to_ebitda", "label": "ND/EBITDA" }
    ]);
    let card = json!({
        "type": "benchmark",
        "title": v["title"],
        "headers": headers,
        "rows": v["rows"],
        "failed": v["failed"],
        "xlsx_path": v["xlsx_path"],
        "csv_path": v["csv_path"],
        "pptx_path": v["pptx_path"],
    });
    let count = v["count"].as_u64().unwrap_or(0);
    let requested = v["requested"].as_u64().unwrap_or(0);
    let mut text = format!("Benchmarked {count} of {requested} tickers.\n");
    if let Some(rows) = v["rows"].as_array() {
        for r in rows {
            text.push_str(&format!(
                "- {} FY{}: revenue {}m, EBITDA margin {}, net margin {}\n",
                r["ticker"].as_str().unwrap_or("?"),
                r["fiscal_year"].as_str().unwrap_or("?"),
                fmt_opt(&r["revenue_m"]),
                fmt_opt(&r["ebitda_margin"]),
                fmt_opt(&r["net_margin"]),
            ));
        }
    }
    Ok((truncate(&text, 1600), card))
}

fn tool_web_search(app: &tauri::AppHandle, args: &Value) -> Result<(String, Value), String> {
    let q = args["query"].as_str().unwrap_or("").trim().to_string();
    if q.is_empty() {
        return Err("web_search requires a query".into());
    }
    let mgr = app.state::<McpManager>();
    let hits = if mgr.ensure(app).unwrap_or(false) {
        match mgr.with_client(app, |c| fm_research::web::web_search(&q, Some(c))) {
            Some(Ok(h)) => h,
            Some(Err(_)) | None => {
                fm_research::web::web_search(&q, None).map_err(|e| e.to_string())?
            }
        }
    } else {
        fm_research::web::web_search(&q, None).map_err(|e| e.to_string())?
    };

    let mut text = format!("Search results for \"{q}\":\n");
    for h in hits.iter().take(8) {
        text.push_str(&format!("- {} — {}\n  {}\n", h.title, h.url, h.snippet));
    }
    let card = json!({ "type": "search", "query": q, "hits": hits });
    Ok((truncate(&text, 2500), card))
}

fn tool_read_page(app: &tauri::AppHandle, args: &Value) -> Result<(String, Value), String> {
    let url = args["url"].as_str().unwrap_or("").trim().to_string();
    if url.is_empty() {
        return Err("read_page requires a url".into());
    }
    let mgr = app.state::<McpManager>();
    let page = if mgr.ensure(app).unwrap_or(false) {
        match mgr.with_client(app, |c| {
            fm_research::web::read_page_full(&url, None, Some(c))
        }) {
            Some(Ok(p)) => p,
            Some(Err(_)) | None => {
                fm_research::web::read_page_full(&url, None, None).map_err(|e| e.to_string())?
            }
        }
    } else {
        fm_research::web::read_page_full(&url, None, None).map_err(|e| e.to_string())?
    };

    let status = serde_json::to_value(page.status).unwrap_or(json!("ok"));
    let card = json!({ "type": "page", "url": url, "title": page.title, "status": status });
    let text = if page.text.trim().is_empty() {
        format!("(page returned no readable text; status: {status})")
    } else {
        truncate(&page.text, 8000)
    };
    Ok((text, card))
}

fn tool_get_news(args: &Value) -> Result<(String, Value), String> {
    let q = args["query"].as_str().unwrap_or("").trim().to_string();
    if q.is_empty() {
        return Err("get_news requires a query".into());
    }
    let limit = args["limit"].as_u64().unwrap_or(5).clamp(1, 20) as usize;
    let items = fm_fetch::fetch_headlines(&q, limit).map_err(|e| e.to_string())?;
    let mut text = format!("Headlines for \"{q}\":\n");
    for h in &items {
        text.push_str(&format!("- {} ({})\n", h.title, h.source));
    }
    let card = json!({ "type": "news", "query": q, "items": items });
    Ok((truncate(&text, 1500), card))
}

fn tool_research_deal(app: &tauri::AppHandle, args: &Value) -> Result<(String, Value), String> {
    let q = args["query"].as_str().unwrap_or("").trim().to_string();
    if q.is_empty() {
        return Err("research_deal requires a query".into());
    }
    let mgr = app.state::<McpManager>();
    let deal = if mgr.ensure(app).unwrap_or(false) {
        mgr.with_client(app, |c| fm_research::agent::run_deal_research(&q, Some(c)))
            .unwrap_or_else(|| fm_research::agent::run_deal_research(&q, None))
    } else {
        fm_research::agent::run_deal_research(&q, None)
    };
    let card = json!({
        "type": "deal",
        "target": deal.target,
        "acquirer": deal.acquirer,
        "summary": deal.summary,
        "sources_read": deal.sources_read,
        "sufficient": deal.sufficient,
    });
    let text = format!(
        "Deal research{}{} (sufficient: {}). Sources read: {}. Summary: {}",
        if deal.target.is_empty() {
            String::new()
        } else {
            format!(" target={}", deal.target)
        },
        if deal.acquirer.is_empty() {
            String::new()
        } else {
            format!(" acquirer={}", deal.acquirer)
        },
        deal.sufficient,
        deal.sources_read.len(),
        truncate(
            &serde_json::to_string(&deal.summary).unwrap_or_default(),
            1500
        ),
    );
    Ok((truncate(&text, 2000), card))
}

fn tool_get_quote(args: &Value) -> Result<(String, Value), String> {
    let ticker = args["ticker"].as_str().unwrap_or("").trim().to_string();
    if ticker.is_empty() {
        return Err("get_quote requires a ticker".into());
    }
    let q = fm_fetch::fetch_quote(&ticker).map_err(|e| e.to_string())?;
    let text = format!(
        "{} {:.2} {} (52w {:?}-{:?})",
        q.ticker, q.price, q.currency, q.week52_low, q.week52_high
    );
    let card = json!({
        "type": "quote",
        "ticker": q.ticker,
        "price": q.price,
        "currency": q.currency,
        "week52_high": q.week52_high,
        "week52_low": q.week52_low,
        "as_of_epoch": q.as_of_epoch,
    });
    Ok((text, card))
}

/// Fetch exact reported financials for a ticker straight from SEC EDGAR XBRL
/// company facts — the deterministic, citable source. Returns a multi-year
/// annual spread (income statement, balance sheet, cash flow, share counts)
/// plus derived analyst metrics (growth, margins, FCF, net cash) computed HERE
/// from the reported inputs, so the model never does the arithmetic.
fn tool_get_financials(args: &Value) -> Result<(String, Value), String> {
    let ticker = args["ticker"].as_str().unwrap_or("").trim().to_uppercase();
    if ticker.is_empty() {
        return Err("get_financials requires a ticker".into());
    }
    let want_year: Option<i64> = args["year"]
        .as_i64()
        .or_else(|| args["year"].as_str().and_then(|s| s.trim().parse().ok()));
    let years = args["years"].as_i64().unwrap_or(3).clamp(1, 6) as usize;
    let basis = args["basis"]
        .as_str()
        .unwrap_or("annual")
        .trim()
        .to_lowercase();
    let cik = fm_fetch::cik_from_ticker(&ticker).map_err(|e| e.to_string())?;
    let raw = fm_fetch::edgar::fetch_companyfacts_raw(&cik).map_err(|e| e.to_string())?;
    match basis.as_str() {
        "ltm" => financials_ltm(&ticker, &cik, &raw),
        "quarterly" => financials_quarterly(&ticker, &cik, &raw),
        _ => financials_from_facts(&ticker, &cik, &raw, want_year, years),
    }
}

/// One annual observation: (value, period end YYYY-MM-DD, filed date).
type AnnualFact = (f64, String, String);

/// All annual (10-K, fp=FY) observations for the FIRST tag with data, one per
/// period end, latest filing winning (restatement-correct), newest first.
/// Single-tag series keep one accounting definition across years.
fn annual_series(
    map: &serde_json::Map<String, Value>,
    tags: &[&str],
    unit: &str,
) -> Vec<AnnualFact> {
    // Among candidates, the tag with the MOST RECENT data wins — a discontinued
    // tag (e.g. TSLA's DepreciationAndAmortization, last used FY2017) must not
    // shadow the currently-tagged alternative (Depreciation). Mirrors
    // fm-extract's facts_for policy.
    let mut best: Option<(String, Vec<AnnualFact>)> = None;
    for &tag in tags {
        let Some(vals) = map.get(tag).and_then(|e| e["units"][unit].as_array()) else {
            continue;
        };
        let mut by_end: std::collections::BTreeMap<String, (f64, String)> =
            std::collections::BTreeMap::new();
        for v in vals {
            let (Some(val), Some(end)) = (v["val"].as_f64(), v["end"].as_str()) else {
                continue;
            };
            if v["fp"].as_str() != Some("FY")
                || !v["form"].as_str().map_or(false, |f| f.contains("10-K"))
            {
                continue;
            }
            let filed = v["filed"].as_str().unwrap_or("").to_string();
            match by_end.get(end) {
                Some((_, f)) if *f >= filed => {}
                _ => {
                    by_end.insert(end.to_string(), (val, filed));
                }
            }
        }
        if !by_end.is_empty() {
            let series: Vec<AnnualFact> = by_end
                .into_iter()
                .rev()
                .map(|(end, (val, filed))| (val, end, filed))
                .collect();
            let newest = series[0].1.clone();
            if best.as_ref().map_or(true, |(b, _)| newest > *b) {
                best = Some((newest, series));
            }
        }
    }
    best.map(|(_, s)| s).unwrap_or_default()
}

/// Days-scale ordinal for a YYYY-MM-DD string (windowing only, not calendar math).
fn date_ord(d: &str) -> i64 {
    let mut it = d.split('-').filter_map(|p| p.parse::<i64>().ok());
    let (y, m, dd) = (
        it.next().unwrap_or(0),
        it.next().unwrap_or(0),
        it.next().unwrap_or(0),
    );
    y * 372 + m * 31 + dd
}

/// Build the multi-year financials answer from a companyfacts JSON (pure; unit
/// tested against captured EDGAR data, live-smoked via the ignored test below).
fn financials_from_facts(
    ticker: &str,
    cik: &str,
    raw: &Value,
    want_year: Option<i64>,
    years: usize,
) -> Result<(String, Value), String> {
    let entity = raw["entityName"].as_str().unwrap_or(ticker).to_string();
    let us = raw["facts"]["us-gaap"].as_object().ok_or_else(|| {
        format!("{ticker} has no US-GAAP XBRL facts (likely a foreign filer) — try build_model")
    })?;
    let dei = raw["facts"]["dei"].as_object();

    // ---- Reported series ------------------------------------------------
    // (label, series, kind). kind: money | eps | count.
    let is_metrics: &[(&str, &[&str])] = &[
        (
            "Revenue",
            &[
                "RevenueFromContractWithCustomerExcludingAssessedTax",
                "Revenues",
                "SalesRevenueNet",
                "RevenueFromContractWithCustomerIncludingAssessedTax",
            ],
        ),
        (
            "Cost of revenue",
            &["CostOfRevenue", "CostOfGoodsAndServicesSold"],
        ),
        ("Gross profit", &["GrossProfit"]),
        ("Operating income", &["OperatingIncomeLoss"]),
        ("Net income", &["NetIncomeLoss", "ProfitLoss"]),
        (
            "Interest expense",
            &[
                "InterestExpense",
                "InterestAndDebtExpense",
                "InterestExpenseDebt",
                "InterestExpenseBorrowings",
            ],
        ),
        (
            "Depreciation & amortization",
            &[
                "DepreciationAndAmortization",
                "DepreciationDepletionAndAmortization",
                "DepreciationAmortizationAndAccretionNet",
                "Depreciation",
            ],
        ),
    ];
    let bs_metrics: &[(&str, &[&str])] = &[
        (
            "Cash & equivalents",
            &[
                "CashAndCashEquivalentsAtCarryingValue",
                "CashCashEquivalentsRestrictedCashAndRestrictedCashEquivalents",
            ],
        ),
        ("Total assets", &["Assets"]),
        (
            "Long-term debt",
            &["LongTermDebt", "LongTermDebtNoncurrent"],
        ),
        (
            "Short-term debt",
            &[
                "DebtCurrent",
                "LongTermDebtCurrent",
                "ShortTermBorrowings",
                "CommercialPaper",
            ],
        ),
        (
            "Stockholders' equity",
            &[
                "StockholdersEquity",
                "StockholdersEquityIncludingPortionAttributableToNoncontrollingInterest",
            ],
        ),
    ];
    let cf_metrics: &[(&str, &[&str])] = &[
        (
            "Operating cash flow",
            &[
                "NetCashProvidedByUsedInOperatingActivities",
                "NetCashProvidedByUsedInOperatingActivitiesContinuingOperations",
            ],
        ),
        (
            "Capital expenditure",
            &[
                "PaymentsToAcquirePropertyPlantAndEquipment",
                "PaymentsToAcquireProductiveAssets",
            ],
        ),
    ];

    let mut series: std::collections::BTreeMap<&str, Vec<AnnualFact>> =
        std::collections::BTreeMap::new();
    for &(label, tags) in is_metrics.iter().chain(bs_metrics).chain(cf_metrics) {
        series.insert(label, annual_series(us, tags, "USD"));
    }
    series.insert(
        "Diluted EPS",
        annual_series(us, &["EarningsPerShareDiluted"], "USD/shares"),
    );
    series.insert(
        "Diluted shares (weighted average)",
        annual_series(
            us,
            &["WeightedAverageNumberOfDilutedSharesOutstanding"],
            "shares",
        ),
    );

    // ---- Period axis: revenue's fiscal ends (fallback net income) -------
    let axis_src = if !series["Revenue"].is_empty() {
        "Revenue"
    } else {
        "Net income"
    };
    let mut axis: Vec<String> = series[axis_src].iter().map(|(_, e, _)| e.clone()).collect();
    if let Some(y) = want_year {
        axis.retain(|end| {
            end.get(..4)
                .and_then(|p| p.parse::<i64>().ok())
                .map_or(true, |ey| ey <= y)
        });
    }
    axis.truncate(years);
    if axis.is_empty() {
        return Err(format!(
            "No annual XBRL figures found for {ticker}{}. Try list_filings or build_model.",
            want_year.map(|y| format!(" FY{y}")).unwrap_or_default()
        ));
    }
    fn fy_label(end: &str) -> String {
        format!("FY{}", end.get(..4).unwrap_or("?"))
    }
    let filed_latest = series[axis_src]
        .iter()
        .find(|(_, e, _)| *e == axis[0])
        .map(|(_, _, f)| f.clone())
        .unwrap_or_default();

    // ---- Cover-page shares: align each axis end to the cover dated within
    // ~130 days AFTER it (the 10-K cover count is stamped weeks after FY end).
    let cover: Vec<AnnualFact> = dei
        .map(|d| annual_series(d, &["EntityCommonStockSharesOutstanding"], "shares"))
        .unwrap_or_default();
    let cover_for = |end: &str| -> Option<f64> {
        let e0 = date_ord(end);
        cover
            .iter()
            .filter(|(_, ce, _)| {
                let c = date_ord(ce);
                c >= e0 && c - e0 <= 130
            })
            .min_by_key(|(_, ce, _)| date_ord(ce) - e0)
            .map(|(v, _, _)| *v)
    };

    // ---- Formatting ------------------------------------------------------
    let money = |v: f64| -> String {
        let a = v.abs();
        if a >= 1e9 {
            format!("${:.2}B", v / 1e9)
        } else if a >= 1e6 {
            format!("${:.1}M", v / 1e6)
        } else {
            format!("${v:.0}")
        }
    };
    let count = |v: f64| -> String {
        let n = v.round() as i64;
        let mut out = String::new();
        for (i, c) in n.abs().to_string().chars().rev().enumerate() {
            if i > 0 && i % 3 == 0 {
                out.push(',');
            }
            out.push(c);
        }
        if n < 0 {
            out.push('-');
        }
        out.chars().rev().collect()
    };
    let pct = |v: f64| format!("{:+.1}%", v * 100.0);
    let val_at = |label: &str, end: &str| -> Option<f64> {
        series
            .get(label)
            .and_then(|s| s.iter().find(|(_, e, _)| e == end))
            .map(|(v, _, _)| *v)
    };

    // ---- Assemble rows ---------------------------------------------------
    let mut lines: Vec<String> = Vec::new();
    let mut rows: Vec<Value> = Vec::new();
    fn push_row(
        axis: &[String],
        lines: &mut Vec<String>,
        rows: &mut Vec<Value>,
        label: &str,
        kind: &str,
        vals: Vec<Option<String>>,
    ) {
        if vals.iter().all(|v| v.is_none()) {
            return;
        }
        let line = axis
            .iter()
            .zip(&vals)
            .filter_map(|(end, v)| v.as_ref().map(|d| format!("{} {d}", fy_label(end))))
            .collect::<Vec<_>>()
            .join(" · ");
        lines.push(format!("- {label}: {line}"));
        rows.push(json!({
            "label": label,
            "kind": kind,
            "values": vals,
        }));
    }

    for &(label, _) in is_metrics {
        push_row(
            &axis,
            &mut lines,
            &mut rows,
            label,
            "reported",
            axis.iter().map(|e| val_at(label, e).map(money)).collect(),
        );
    }
    push_row(
        &axis,
        &mut lines,
        &mut rows,
        "Diluted EPS",
        "reported",
        axis.iter()
            .map(|e| val_at("Diluted EPS", e).map(|v| format!("${v:.2}")))
            .collect(),
    );
    for &(label, _) in bs_metrics {
        push_row(
            &axis,
            &mut lines,
            &mut rows,
            label,
            "reported",
            axis.iter().map(|e| val_at(label, e).map(money)).collect(),
        );
    }
    for &(label, _) in cf_metrics {
        push_row(
            &axis,
            &mut lines,
            &mut rows,
            label,
            "reported",
            axis.iter().map(|e| val_at(label, e).map(money)).collect(),
        );
    }
    push_row(
        &axis,
        &mut lines,
        &mut rows,
        "Shares outstanding (10-K cover)",
        "reported",
        axis.iter().map(|e| cover_for(e).map(count)).collect(),
    );
    push_row(
        &axis,
        &mut lines,
        &mut rows,
        "Diluted shares (weighted average)",
        "reported",
        axis.iter()
            .map(|e| val_at("Diluted shares (weighted average)", e).map(count))
            .collect(),
    );

    // Derived — computed here so the model never does the arithmetic.
    let n_reported = lines.len();
    push_row(
        &axis,
        &mut lines,
        &mut rows,
        "Revenue growth YoY",
        "derived",
        axis.iter()
            .enumerate()
            .map(|(i, e)| {
                let prev = axis.get(i + 1)?;
                let (cur, prv) = (val_at("Revenue", e)?, val_at("Revenue", prev)?);
                (prv != 0.0).then(|| pct((cur - prv) / prv.abs()))
            })
            .collect(),
    );
    for (dl, num) in [
        ("Gross margin", "Gross profit"),
        ("Operating margin", "Operating income"),
        ("Net margin", "Net income"),
    ] {
        push_row(
            &axis,
            &mut lines,
            &mut rows,
            dl,
            "derived",
            axis.iter()
                .map(|e| {
                    let (n, r) = (val_at(num, e)?, val_at("Revenue", e)?);
                    (r != 0.0).then(|| format!("{:.1}%", n / r * 100.0))
                })
                .collect(),
        );
    }
    push_row(
        &axis,
        &mut lines,
        &mut rows,
        "Free cash flow (CFO − capex)",
        "derived",
        axis.iter()
            .map(|e| {
                Some(money(
                    val_at("Operating cash flow", e)? - val_at("Capital expenditure", e)?,
                ))
            })
            .collect(),
    );
    push_row(
        &axis,
        &mut lines,
        &mut rows,
        "EBITDA (op income + D&A)",
        "derived",
        axis.iter()
            .map(|e| {
                Some(money(
                    val_at("Operating income", e)? + val_at("Depreciation & amortization", e)?,
                ))
            })
            .collect(),
    );
    let total_debt_at = |e: &String| -> Option<f64> {
        match (val_at("Long-term debt", e), val_at("Short-term debt", e)) {
            (Some(l), Some(st)) => Some(l + st),
            (Some(l), None) => Some(l),
            (None, Some(st)) => Some(st),
            (None, None) => None,
        }
    };
    push_row(
        &axis,
        &mut lines,
        &mut rows,
        "Total debt (LT + ST)",
        "derived",
        axis.iter().map(|e| total_debt_at(e).map(money)).collect(),
    );
    push_row(
        &axis,
        &mut lines,
        &mut rows,
        "Interest coverage (op income / interest)",
        "derived",
        axis.iter()
            .map(|e| {
                let (oi, ie) = (
                    val_at("Operating income", e)?,
                    val_at("Interest expense", e)?,
                );
                (ie != 0.0).then(|| format!("{:.1}x", oi / ie))
            })
            .collect(),
    );
    push_row(
        &axis,
        &mut lines,
        &mut rows,
        "Leverage (total debt / EBITDA)",
        "derived",
        axis.iter()
            .map(|e| {
                let ebitda =
                    val_at("Operating income", e)? + val_at("Depreciation & amortization", e)?;
                let td = total_debt_at(e)?;
                (ebitda != 0.0).then(|| format!("{:.1}x", td / ebitda))
            })
            .collect(),
    );
    push_row(
        &axis,
        &mut lines,
        &mut rows,
        "Net cash (debt) (cash − total debt)",
        "derived",
        axis.iter()
            .map(|e| Some(money(val_at("Cash & equivalents", e)? - total_debt_at(e)?)))
            .collect(),
    );

    let periods: Vec<Value> = axis
        .iter()
        .map(|e| json!({ "label": fy_label(e), "end": e }))
        .collect();
    let header = format!(
        "{} ({}) — annual (10-K), {} fiscal year{} (latest filed {}):",
        entity,
        ticker,
        axis.len(),
        if axis.len() == 1 { "" } else { "s" },
        filed_latest
    );
    let (reported, derived) = lines.split_at(n_reported.min(lines.len()));
    let mut text = format!(
        "{header}
Reported (SEC EDGAR XBRL):
{}",
        reported.join(
            "
"
        )
    );
    if !derived.is_empty() {
        text.push_str(&format!(
            "
Derived (computed deterministically from the reported figures — do not recompute):
{}",
            derived.join(
                "
"
            )
        ));
    }
    text.push_str(
        "
Source: SEC EDGAR XBRL company facts (10-K; latest filing wins per period).",
    );
    let card = json!({
        "type": "financials",
        "ticker": ticker,
        "entity": entity,
        "fiscal_year": axis.first().and_then(|e| e.get(..4)).unwrap_or(""),
        "period_end": axis.first().cloned().unwrap_or_default(),
        "filed": filed_latest,
        "currency": "USD",
        "periods": periods,
        "rows": rows,
        "source": format!("https://www.sec.gov/cgi-bin/browse-edgar?action=getcompany&CIK={cik}&type=10-K"),
    });
    Ok((text, card))
}

fn tool_list_filings(args: &Value) -> Result<(String, Value), String> {
    let ticker = args["ticker"].as_str().unwrap_or("").trim().to_string();
    if ticker.is_empty() {
        return Err("list_filings requires a ticker".into());
    }
    let cik = fm_fetch::cik_from_ticker(&ticker).map_err(|e| e.to_string())?;
    let form = args["form"].as_str().map(|s| s.trim().to_uppercase());
    let filings = match form.as_deref().filter(|f| !f.is_empty()) {
        Some(f) => fm_fetch::recent_filings(&cik, f, 15).map_err(|e| e.to_string())?,
        None => fm_fetch::search_filings(&cik, fm_fetch::DEFAULT_FORM_TYPES, 15)
            .map_err(|e| e.to_string())?,
    };
    let rows: Vec<Value> = filings
        .iter()
        .map(|f| {
            json!({
                "form_type": f.form_type,
                "filing_date": f.filing_date,
                "fiscal_period_end": f.fiscal_period_end,
                "url": f.url,
            })
        })
        .collect();
    let mut text = format!("Recent filings for {ticker} (CIK {cik}):\n");
    for f in filings.iter().take(15) {
        text.push_str(&format!(
            "- {} filed {} — {}\n",
            f.form_type, f.filing_date, f.url
        ));
    }
    let card = json!({ "type": "filings", "ticker": ticker, "rows": rows });
    Ok((truncate(&text, 1800), card))
}

fn tool_read_filing(app: &tauri::AppHandle, args: &Value) -> Result<(String, Value), String> {
    let ticker = args["ticker"].as_str().unwrap_or("").trim().to_string();
    if ticker.is_empty() {
        return Err("read_filing requires a ticker".into());
    }
    let form = args["form"]
        .as_str()
        .map(|s| s.trim().to_uppercase())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "10-K".into());
    let item = args["item"]
        .as_str()
        .map(|s| s.trim().to_uppercase())
        .filter(|s| !s.is_empty());

    let s = read_settings(app);
    if !s.edgar_contact.trim().is_empty() {
        fm_fetch::edgar::set_edgar_contact(s.edgar_contact.trim().to_string());
    }
    let cik = fm_fetch::cik_from_ticker(&ticker).map_err(|e| e.to_string())?;
    let filings = fm_fetch::recent_filings(&cik, &form, 1).map_err(|e| e.to_string())?;
    let filing = filings
        .into_iter()
        .next()
        .ok_or_else(|| format!("No recent {form} for {ticker}"))?;
    let text = fm_fetch::fetch_filing_doc(&filing.url).map_err(|e| e.to_string())?;
    let items = fm_fetch::split_filing_items(&text);
    let ids: Vec<String> = items.iter().map(|(id, _)| id.clone()).collect();

    let (llm_text, item_val, chars) = if let Some(want) = &item {
        match items.iter().find(|(id, _)| id == want) {
            Some((_, body)) => {
                let clipped = truncate(body, 20_000);
                let n = clipped.chars().count();
                (
                    format!("{form} Item {want} for {ticker}:\n\n{clipped}"),
                    json!(want),
                    n,
                )
            }
            None => (
                format!(
                    "Item {want} not found in {ticker} {form}. Available items: {}",
                    ids.join(", ")
                ),
                Value::Null,
                0,
            ),
        }
    } else {
        let head = truncate(&text, 4_000);
        let n = head.chars().count();
        (
            format!(
                "{form} for {ticker}. Items: {}\n\nExcerpt:\n{head}",
                ids.join(", ")
            ),
            Value::Null,
            n,
        )
    };
    let card = json!({
        "type": "filing_doc",
        "ticker": ticker,
        "form": form,
        "filing_date": filing.filing_date,
        "url": filing.url,
        "item": item_val,
        "items": ids,
        "chars": chars,
    });
    Ok((truncate(&llm_text, 20_500), card))
}

fn tool_analyze_pdf(
    app: &tauri::AppHandle,
    args: &Value,
    conversation_id: &str,
) -> Result<(String, Value), String> {
    use crate::commands::artifacts::ArtifactRegistry;
    let artifact_id = args["artifact_id"]
        .as_str()
        .unwrap_or("")
        .trim()
        .to_string();
    if artifact_id.is_empty() {
        return Err("analyze_pdf requires an artifact_id".into());
    }
    let reg = app
        .try_state::<ArtifactRegistry>()
        .ok_or_else(|| "artifact registry unavailable".to_string())?;
    let (path, kind, reg_label) = reg
        .resolve(&artifact_id, Some(conversation_id))
        .map_err(|e| e.to_string())?;
    if !matches!(kind, crate::commands::artifacts::ArtifactKind::UserPdf) {
        return Err("analyze_pdf requires a UserPdf artifact handle".into());
    }
    if !path.is_file()
        || !path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("pdf"))
            .unwrap_or(false)
    {
        return Err("artifact no longer points at a readable PDF".into());
    }
    let label = args["label"].as_str().unwrap_or("").trim().to_string();
    let label = if label.is_empty() { reg_label } else { label };
    let path_str = path.to_string_lossy().to_string();
    let opts = fm_build::BuildOptions {
        deck: true,
        ..Default::default()
    };
    let summary = crate::commands::model::analyze_pdf_blocking(app, &path_str, &label, opts)
        .map_err(|e| e.to_string())?;
    let v: Value = serde_json::from_str(&summary).map_err(|e| e.to_string())?;
    let val = &v["valuation"];
    let card = json!({
        "type": "model",
        "ticker": v["ticker"],
        "currency": v["currency"],
        "xlsx_path": v["xlsx_path"],
        "pptx_path": v["pptx_path"],
        "valuation": val,
    });
    let text = format!(
        "Built model from PDF {}. Implied price {}, upside {}%. Excel: {}",
        label,
        fmt_opt(&val["price_per_share"]),
        fmt_opt(&val["upside_pct"]),
        v["xlsx_path"].as_str().unwrap_or(""),
    );
    Ok((text, card))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

fn emit(app: &tauri::AppHandle, event: &str, payload: Value) {
    let _ = app.emit(event, payload);
}

/// Emit a chat event with owning conversation_id + run_id (Phase 3.5).
fn emit_chat(app: &tauri::AppHandle, event: &str, conv_id: &str, run_id: &str, mut payload: Value) {
    if let Some(obj) = payload.as_object_mut() {
        obj.insert("conversation_id".into(), json!(conv_id));
        obj.insert("run_id".into(), json!(run_id));
    }
    emit(app, event, payload);
}

/// Emit an ephemeral event on the single `agent_event` channel (Task 2.1). Used
/// for the streaming text delta so the UI consumes ONE channel; ephemeral events
/// carry no sequence and never determine terminal state.
fn emit_agent_ephemeral(
    app: &tauri::AppHandle,
    conv_id: &str,
    run_id: &str,
    kind: fm_agent::types::EventKind,
    payload: Value,
) {
    let mut b = [0u8; 16];
    rand::Rng::fill(&mut rand::thread_rng(), &mut b);
    let env = crate::agent::events::AgentEventEnvelope::ephemeral(
        fm_agent::ids::format_uuid_v4(b),
        conv_id.to_string(),
        run_id.to_string(),
        kind,
        payload,
        crate::store::now_iso(),
    );
    let _ = app.emit(crate::agent::events::CHANNEL, &env);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore] // live: hits SEC EDGAR XBRL
    fn get_financials_tsla_fy2025_revenue_live() {
        let (text, card) = tool_get_financials(&json!({ "ticker": "TSLA", "year": 2025 })).unwrap();
        eprintln!("{text}");
        assert!(text.contains("Revenue"), "must report a revenue line");
        // Spread card: rows carry per-period display values, newest first.
        let rev = card["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["label"] == "Revenue")
            .expect("revenue row")["values"][0]
            .as_str()
            .unwrap()
            .to_string();
        let n: f64 = rev
            .trim_start_matches('$')
            .trim_end_matches('B')
            .parse()
            .expect("revenue display parses");
        assert!(
            (85.0..105.0).contains(&n),
            "TSLA FY2025 revenue ~$94.8B, got {rev}"
        );
        assert_eq!(card["fiscal_year"], json!("2025"));
    }

    #[test]
    fn iso_utc_epoch_and_known_dates() {
        assert_eq!(iso_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso_utc(1_700_000_000), "2023-11-14T22:13:20Z");
    }

    #[test]
    fn build_chat_request_shape_with_and_without_tools() {
        let msgs = vec![json!({ "role": "user", "content": "hi" })];
        let tools = vec![json!({ "type": "function" })];
        let req = build_chat_request("m", &msgs, &tools, true, true);
        assert_eq!(req["model"], json!("m"));
        assert_eq!(req["stream"], json!(true));
        assert_eq!(req["tool_choice"], json!("auto"));
        assert_eq!(req["parallel_tool_calls"], json!(true));
        assert!(req["tools"].is_array());

        let bare = build_chat_request("m", &msgs, &[], false, false);
        assert_eq!(bare["stream"], json!(false));
        assert!(bare.get("tools").is_none());
        assert!(bare.get("tool_choice").is_none());
    }

    #[test]
    fn sse_accumulate_content_deltas() {
        let events = [
            r#"{"choices":[{"delta":{"content":"Hel"}}]}"#,
            r#"{"choices":[{"delta":{"content":"lo"}}]}"#,
            "[DONE]",
            r#"{"choices":[{"delta":{"content":" world"}}]}"#,
        ];
        let (content, calls, _meta) = sse_accumulate(&events);
        assert_eq!(content, "Hello");
        assert!(calls.is_empty());
    }

    #[test]
    fn sse_take_lines_preserves_split_multibyte_utf8() {
        // € is UTF-8 E2 82 AC — split after first byte mid-codepoint.
        let payload = r#"data: {"choices":[{"delta":{"content":"café €"}}]}"#;
        let bytes = format!("{payload}\n").into_bytes();
        assert!(bytes.len() > 10);
        // Find the euro sign's first byte position inside the content.
        let euro_at = bytes
            .windows(3)
            .position(|w| w == [0xE2, 0x82, 0xAC])
            .expect("euro present");
        let mut buf = Vec::new();
        // Feed everything up to mid-codepoint — no complete line yet, and no
        // lossy replacement for the dangling lead byte.
        buf.extend_from_slice(&bytes[..=euro_at]);
        assert!(sse_take_lines(&mut buf).is_empty());
        assert_eq!(buf, bytes[..=euro_at]);
        // Rest of the stream completes the line.
        buf.extend_from_slice(&bytes[euro_at + 1..]);
        let lines = sse_take_lines(&mut buf);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("café €"));
        assert!(buf.is_empty());
    }

    #[test]
    fn sse_accumulate_split_tool_call_fragments() {
        let events = [
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","function":{"name":"build_model","arguments":"{\"tic"}}]}}]}"#,
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"ker\":\"AAPL\"}"}}]}}]}"#,
            "[DONE]",
        ];
        let (_content, calls, _meta) = sse_accumulate(&events);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "call_1");
        assert_eq!(calls[0].name, "build_model");
        assert_eq!(calls[0].arguments, "{\"ticker\":\"AAPL\"}");
        let args: Value = serde_json::from_str(&calls[0].arguments).unwrap();
        assert_eq!(args["ticker"], json!("AAPL"));
    }

    #[test]
    fn sse_accumulate_captures_finish_reason_and_usage() {
        let events = [
            r#"{"model":"anthropic/claude-sonnet-4","provider":"Anthropic","choices":[{"delta":{"content":"Hi"}}]}"#,
            r#"{"choices":[{"delta":{},"finish_reason":"length","native_finish_reason":"max_tokens"}]}"#,
            r#"{"choices":[],"usage":{"prompt_tokens":10,"completion_tokens":5,"total_tokens":15,"ignored_body":"secret"}}"#,
            "not-json-at-all",
            "[DONE]",
        ];
        let (content, _calls, meta) = sse_accumulate(&events);
        assert_eq!(content, "Hi");
        assert_eq!(meta.finish_reason.as_deref(), Some("length"));
        assert_eq!(meta.native_finish_reason.as_deref(), Some("max_tokens"));
        assert_eq!(meta.model.as_deref(), Some("anthropic/claude-sonnet-4"));
        assert_eq!(meta.provider.as_deref(), Some("Anthropic"));
        assert_eq!(meta.usage.as_ref().unwrap()["total_tokens"], json!(15));
        // Non-token usage fields are stripped — never retained.
        assert!(meta.usage.as_ref().unwrap().get("ignored_body").is_none());
        assert_eq!(meta.sse_parse_errors, 1);
    }

    #[test]
    fn redact_provider_error_never_leaks_body() {
        let body = r#"{"error":{"message":"sk-or-v1-SECRET_KEY rate limited"}}"#;
        let msg = redact_provider_error(Some(429), body);
        assert!(msg.contains("rate_limit"), "got: {msg}");
        assert!(msg.contains("HTTP 429"));
        assert!(!msg.contains("SECRET"));
        assert!(!msg.contains("sk-or"));
        let auth = redact_provider_error(Some(401), "invalid api key sk-or-v1-xxx");
        assert!(auth.contains("auth"), "got: {auth}");
        assert!(!auth.contains("sk-or"));
        let net = redact_provider_error(None, "dns resolve failed for openrouter.ai");
        assert!(net.contains("network"), "got: {net}");
    }

    #[test]
    fn title_truncation() {
        assert_eq!(title_from("short"), "short");
        let long: String = "x".repeat(60);
        let t = title_from(&long);
        assert_eq!(t.chars().count(), 49); // 48 + ellipsis
        assert!(t.ends_with('…'));
    }
}

/// LTM basis: trailing-twelve-months flows (FY + latest YTD − prior YTD via
/// [`fm_extract::ltm`], staleness-guarded) + latest balance-sheet instants,
/// with the same deterministic derived metrics as the annual spread.
fn financials_ltm(ticker: &str, cik: &str, raw: &Value) -> Result<(String, Value), String> {
    let entity = raw["entityName"].as_str().unwrap_or(ticker).to_string();
    let d = fm_extract::ltm::extract_ltm(raw, "USD")
        .ok_or_else(|| format!("No LTM-usable XBRL data for {ticker} — try basis=annual"))?;
    let money = |v: f64| -> String {
        let a = v.abs();
        if a >= 1e9 {
            format!("${:.2}B", v / 1e9)
        } else if a >= 1e6 {
            format!("${:.1}M", v / 1e6)
        } else {
            format!("${v:.0}")
        }
    };
    let mut lines: Vec<String> = Vec::new();
    let mut rows: Vec<Value> = Vec::new();
    let mut push = |label: &str, kind: &str, v: Option<String>| {
        if let Some(disp) = v {
            lines.push(format!("- {label}: {disp}"));
            rows.push(json!({ "label": label, "kind": kind, "values": [disp] }));
        }
    };
    push("Revenue", "reported", d.revenue.map(money));
    push("Gross profit", "reported", d.gross_profit.map(money));
    push("EBIT", "reported", d.ebit.map(money));
    push("Depreciation & amortization", "reported", d.da.map(money));
    push("Net income", "reported", d.net_income.map(money));
    push(
        "Interest expense",
        "reported",
        d.interest_expense.map(money),
    );
    push("Operating cash flow", "reported", d.cfo.map(money));
    push("Capital expenditure", "reported", d.capex.map(money));
    push("Cash & equivalents", "reported", d.cash.map(money));
    push(
        "Total debt (LT + ST)",
        "reported",
        d.total_debt().map(money),
    );
    push(
        "Stockholders' equity",
        "reported",
        d.total_equity.map(money),
    );
    // Derived — computed here, not by the model.
    let ebitda = match (d.ebit, d.da) {
        (Some(e), Some(da)) => Some(e + da),
        _ => None,
    };
    push("EBITDA (EBIT + D&A)", "derived", ebitda.map(money));
    push(
        "Free cash flow (CFO − capex)",
        "derived",
        match (d.cfo, d.capex) {
            (Some(c), Some(x)) => Some(money(c - x)),
            _ => None,
        },
    );
    push(
        "Net debt (total debt − cash)",
        "derived",
        match (d.total_debt(), d.cash) {
            (Some(td), Some(c)) => Some(money(td - c)),
            _ => None,
        },
    );
    push(
        "Leverage (total debt / EBITDA)",
        "derived",
        match (d.total_debt(), ebitda) {
            (Some(td), Some(e)) if e != 0.0 => Some(format!("{:.1}x", td / e)),
            _ => None,
        },
    );
    push(
        "Interest coverage (EBIT / interest)",
        "derived",
        match (d.ebit, d.interest_expense) {
            (Some(e), Some(i)) if i != 0.0 => Some(format!("{:.1}x", e / i)),
            _ => None,
        },
    );
    let genuine = if d.is_ltm {
        "stitched FY + latest interim − prior-year interim"
    } else {
        "no usable interim — annual figures as fallback"
    };
    let text = format!(
        "{entity} ({ticker}) — LTM as of {} ({genuine}):\n{}\nDerived rows are computed deterministically from the reported figures — do not recompute.\nSource: SEC EDGAR XBRL company facts (10-K + 10-Q).",
        d.as_of,
        lines.join("\n")
    );
    let card = json!({
        "type": "financials",
        "ticker": ticker,
        "entity": entity,
        "fiscal_year": "LTM",
        "period_end": d.as_of,
        "currency": "USD",
        "periods": [{ "label": format!("LTM {}", d.as_of), "end": d.as_of }],
        "rows": rows,
        "source": format!("https://www.sec.gov/cgi-bin/browse-edgar?action=getcompany&CIK={cik}&type=10-Q"),
    });
    Ok((text, card))
}

/// One duration observation: (value, start, end, filed).
type DurFact = (f64, String, String, String);

/// All duration facts for the tag with the most recent data (current tag beats
/// discontinued), deduped by (start,end) with the latest filing winning.
/// Returns (quarterlies ~3mo, annuals ~12mo), both newest first.
fn duration_facts(
    map: &serde_json::Map<String, Value>,
    tags: &[&str],
    unit: &str,
) -> (Vec<DurFact>, Vec<DurFact>) {
    let mut best: Option<(String, Vec<DurFact>, Vec<DurFact>)> = None;
    for &tag in tags {
        let Some(vals) = map.get(tag).and_then(|e| e["units"][unit].as_array()) else {
            continue;
        };
        let mut by_span: std::collections::BTreeMap<(String, String), (f64, String)> =
            std::collections::BTreeMap::new();
        for v in vals {
            let (Some(val), Some(start), Some(end)) =
                (v["val"].as_f64(), v["start"].as_str(), v["end"].as_str())
            else {
                continue;
            };
            let form = v["form"].as_str().unwrap_or("");
            if !form.contains("10-Q") && !form.contains("10-K") {
                continue;
            }
            let filed = v["filed"].as_str().unwrap_or("").to_string();
            let key = (start.to_string(), end.to_string());
            match by_span.get(&key) {
                Some((_, f)) if *f >= filed => {}
                _ => {
                    by_span.insert(key, (val, filed));
                }
            }
        }
        if by_span.is_empty() {
            continue;
        }
        let (mut q, mut a) = (Vec::new(), Vec::new());
        for ((start, end), (val, filed)) in by_span {
            let dur = date_ord(&end) - date_ord(&start);
            if (70..=110).contains(&dur) {
                q.push((val, start, end, filed));
            } else if (330..=400).contains(&dur) {
                a.push((val, start, end, filed));
            }
        }
        q.sort_by(|x, y| y.2.cmp(&x.2));
        a.sort_by(|x, y| y.2.cmp(&x.2));
        let newest = q
            .first()
            .map(|f| f.2.clone())
            .into_iter()
            .chain(a.first().map(|f| f.2.clone()))
            .max()
            .unwrap_or_default();
        if best.as_ref().map_or(true, |(b, _, _)| newest > *b) {
            best = Some((newest, q, a));
        }
    }
    best.map(|(_, q, a)| (q, a)).unwrap_or_default()
}

/// Quarterly basis: the last 8 reported fiscal quarters. Q4 flows (never filed
/// as a discrete period) are derived as FY − (Q1+Q2+Q3) and marked with '*';
/// EPS is never derived that way (quarterly EPS is not additive under
/// share-count changes).
fn financials_quarterly(ticker: &str, cik: &str, raw: &Value) -> Result<(String, Value), String> {
    let entity = raw["entityName"].as_str().unwrap_or(ticker).to_string();
    let us = raw["facts"]["us-gaap"].as_object().ok_or_else(|| {
        format!("{ticker} has no US-GAAP XBRL facts (likely a foreign filer) — try build_model")
    })?;
    let metrics: &[(&str, &[&str], bool)] = &[
        (
            "Revenue",
            &[
                "RevenueFromContractWithCustomerExcludingAssessedTax",
                "Revenues",
                "SalesRevenueNet",
            ],
            true,
        ),
        ("Operating income", &["OperatingIncomeLoss"], true),
        ("Net income", &["NetIncomeLoss", "ProfitLoss"], true),
        ("Diluted EPS", &["EarningsPerShareDiluted"], false),
    ];
    // Per metric: quarter end -> (val, derived?). Q4 derived from FY − ΣQ1..3.
    let mut per: std::collections::BTreeMap<&str, std::collections::BTreeMap<String, (f64, bool)>> =
        std::collections::BTreeMap::new();
    for &(label, tags, derivable) in metrics {
        let unit = if label == "Diluted EPS" {
            "USD/shares"
        } else {
            "USD"
        };
        let (quarters, annuals) = duration_facts(us, tags, unit);
        let mut m: std::collections::BTreeMap<String, (f64, bool)> = quarters
            .iter()
            .map(|(v, _, e, _)| (e.clone(), (*v, false)))
            .collect();
        if derivable {
            for (aval, astart, aend, _) in &annuals {
                if m.contains_key(aend) {
                    continue;
                }
                let inside: Vec<f64> = quarters
                    .iter()
                    .filter(|(_, qs, qe, _)| {
                        qs.as_str() >= astart.as_str() && qe.as_str() < aend.as_str()
                    })
                    .map(|(v, _, _, _)| *v)
                    .collect();
                if inside.len() == 3 {
                    m.insert(aend.clone(), (aval - inside.iter().sum::<f64>(), true));
                }
            }
        }
        per.insert(label, m);
    }
    // Axis: revenue's quarter ends, newest first, last 8.
    let mut axis: Vec<String> = per
        .get("Revenue")
        .map(|m| m.keys().rev().cloned().collect())
        .unwrap_or_default();
    axis.truncate(8);
    if axis.is_empty() {
        return Err(format!(
            "No quarterly XBRL figures found for {ticker}. Try basis=annual."
        ));
    }
    let money = |v: f64| -> String {
        let a = v.abs();
        if a >= 1e9 {
            format!("${:.2}B", v / 1e9)
        } else if a >= 1e6 {
            format!("${:.1}M", v / 1e6)
        } else {
            format!("${v:.0}")
        }
    };
    let mut lines: Vec<String> = Vec::new();
    let mut rows: Vec<Value> = Vec::new();
    for &(label, _, _) in metrics {
        let vals: Vec<Option<String>> = axis
            .iter()
            .map(|e| {
                per[label].get(e).map(|(v, derived)| {
                    let disp = if label == "Diluted EPS" {
                        format!("${v:.2}")
                    } else {
                        money(*v)
                    };
                    if *derived {
                        format!("{disp}*")
                    } else {
                        disp
                    }
                })
            })
            .collect();
        if vals.iter().all(|v| v.is_none()) {
            continue;
        }
        let line = axis
            .iter()
            .zip(&vals)
            .filter_map(|(e, v)| v.as_ref().map(|d| format!("{e} {d}")))
            .collect::<Vec<_>>()
            .join(" · ");
        lines.push(format!("- {label}: {line}"));
        rows.push(json!({ "label": label, "kind": "reported", "values": vals }));
    }
    // Derived net margin per quarter.
    let margin: Vec<Option<String>> = axis
        .iter()
        .map(|e| {
            let (n, _) = per["Net income"].get(e)?;
            let (r, _) = per["Revenue"].get(e)?;
            (*r != 0.0).then(|| format!("{:.1}%", n / r * 100.0))
        })
        .collect();
    if margin.iter().any(|v| v.is_some()) {
        rows.push(json!({ "label": "Net margin", "kind": "derived", "values": margin }));
    }
    let text = format!(
        "{entity} ({ticker}) — quarterly, last {} fiscal quarters (newest first; * = Q4 derived as FY − Q1..Q3):\n{}\nSource: SEC EDGAR XBRL company facts (10-Q + 10-K).",
        axis.len(),
        lines.join("\n")
    );
    let periods: Vec<Value> = axis
        .iter()
        .map(|e| json!({ "label": e, "end": e }))
        .collect();
    let card = json!({
        "type": "financials",
        "ticker": ticker,
        "entity": entity,
        "fiscal_year": "quarterly",
        "period_end": axis.first().cloned().unwrap_or_default(),
        "currency": "USD",
        "periods": periods,
        "rows": rows,
        "source": format!("https://www.sec.gov/cgi-bin/browse-edgar?action=getcompany&CIK={cik}&type=10-Q"),
    });
    Ok((text, card))
}

#[cfg(test)]
mod financials_tests {
    use super::*;

    /// Live EDGAR smoke (network): the TSLA spread must contain the exact
    /// FY2025 cover-page share count and multi-year revenue with derived
    /// margins. Run explicitly: cargo test --lib financials_live -- --ignored
    #[test]
    #[ignore]
    fn financials_live_tsla_spread() {
        let cik = fm_fetch::cik_from_ticker("TSLA").unwrap();
        let raw = fm_fetch::edgar::fetch_companyfacts_raw(&cik).unwrap();
        let (text, card) = financials_from_facts("TSLA", &cik, &raw, None, 3).unwrap();
        // Exact disclosed cover-page count (FY2025 10-K/A).
        assert!(
            text.contains("3,752,431,984"),
            "cover shares missing:\n{text}"
        );
        // Three fiscal years on the axis.
        assert_eq!(card["periods"].as_array().unwrap().len(), 3);
        // Reported + derived sections both present.
        assert!(text.contains("Reported (SEC EDGAR XBRL):"));
        assert!(text.contains("Derived (computed deterministically"));
        assert!(text.contains("Revenue growth YoY"));
        assert!(text.contains("Net margin"));
        assert!(text.contains("Free cash flow"));
        // Rows carry per-period values arrays for the UI table.
        let rows = card["rows"].as_array().unwrap();
        assert!(rows.iter().any(|r| r["kind"] == "derived"));
        assert!(rows
            .iter()
            .all(|r| r["values"].as_array().unwrap().len() == 3));
        // Credit metrics present in the annual spread (Item 3).
        assert!(text.contains("EBITDA"), "EBITDA missing:\n{text}");
        assert!(text.contains("Leverage"), "leverage missing:\n{text}");
    }

    /// Live EDGAR smoke: LTM and quarterly bases produce data for TSLA.
    #[test]
    #[ignore]
    fn financials_live_tsla_ltm_and_quarterly() {
        let cik = fm_fetch::cik_from_ticker("TSLA").unwrap();
        let raw = fm_fetch::edgar::fetch_companyfacts_raw(&cik).unwrap();
        let (text, card) = financials_ltm("TSLA", &cik, &raw).unwrap();
        assert!(text.contains("LTM as of"), "{text}");
        assert!(text.contains("Revenue"), "{text}");
        assert_eq!(card["periods"].as_array().unwrap().len(), 1);
        let (qt, qcard) = financials_quarterly("TSLA", &cik, &raw).unwrap();
        assert!(qcard["periods"].as_array().unwrap().len() >= 6, "{qt}");
        assert!(qt.contains("Revenue"), "{qt}");
    }

    /// Quarterly parser: Q4 derived as FY − (Q1+Q2+Q3), marked with '*'.
    #[test]
    fn quarterly_derives_q4_from_annual() {
        let dur = |s: &str, e: &str, v: f64, form: &str| json!({ "start": s, "end": e, "val": v, "form": form, "filed": "2026-01-30" });
        let raw = json!({ "entityName": "TestCo", "facts": { "us-gaap": {
            "Revenues": { "units": { "USD": [
                dur("2025-01-01", "2025-03-31", 20.0e9, "10-Q"),
                dur("2025-04-01", "2025-06-30", 22.0e9, "10-Q"),
                dur("2025-07-01", "2025-09-30", 24.0e9, "10-Q"),
                dur("2025-01-01", "2025-12-31", 94.0e9, "10-K"),
            ]}}
        }}});
        let (text, card) = financials_quarterly("TC", "1", &raw).unwrap();
        // Q4 = 94 − 66 = 28, derived-marked with '*'.
        assert!(text.contains("2025-12-31 $28.00B*"), "q4 wrong:\n{text}");
        assert_eq!(card["periods"].as_array().unwrap().len(), 4);
    }

    /// Pure-parser test on a synthetic companyfacts document: restatement
    /// (later filed) wins, derived margins compute, missing series omitted.
    #[test]
    fn financials_from_facts_restatement_and_derived() {
        let raw = serde_json::json!({
            "entityName": "TestCo",
            "facts": { "us-gaap": {
                "Revenues": { "units": { "USD": [
                    { "val": 100.0e9, "end": "2024-12-31", "fp": "FY", "form": "10-K", "filed": "2025-01-30" },
                    { "val": 101.0e9, "end": "2024-12-31", "fp": "FY", "form": "10-K/A", "filed": "2025-06-01" },
                    { "val": 80.0e9,  "end": "2023-12-31", "fp": "FY", "form": "10-K", "filed": "2024-01-30" }
                ]}},
                "NetIncomeLoss": { "units": { "USD": [
                    { "val": 10.1e9, "end": "2024-12-31", "fp": "FY", "form": "10-K", "filed": "2025-01-30" },
                    { "val": 8.0e9,  "end": "2023-12-31", "fp": "FY", "form": "10-K", "filed": "2024-01-30" }
                ]}}
            }}
        });
        let (text, card) = financials_from_facts("TC", "1", &raw, None, 3).unwrap();
        // Restated revenue (later filed) wins over the original.
        assert!(
            text.contains("FY2024 $101.00B"),
            "restatement lost:\n{text}"
        );
        // Growth computed from restated figure: (101-80)/80 = 26.25% -> +26.2%
        // (Rust {:+.1} rounds half-to-even).
        assert!(text.contains("+26.2%"), "growth wrong:\n{text}");
        // Net margin 10.1/101 = 10.0%.
        assert!(text.contains("10.0%"), "margin wrong:\n{text}");
        // Absent series (gross profit, cash flow) are omitted, not zeroed.
        assert!(!text.contains("Gross margin"));
        assert!(!text.contains("Free cash flow"));
        assert_eq!(card["periods"].as_array().unwrap().len(), 2);
    }
}
