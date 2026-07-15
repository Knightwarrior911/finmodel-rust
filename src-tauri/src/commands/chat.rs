//! Chat: conversation store + the chat engine (OpenRouter tool-calling loop with
//! SSE streaming, plus a deterministic no-key fallback router).
//!
//! Conversations persist to `app_config_dir()/conversations/<id>.json`. The chat
//! engine drives the same blocking internals the old form commands used
//! (`model.rs`, `benchmark.rs`, `fm_research`, `fm_fetch`) — never shelling
//! through the IPC command wrappers — and streams assistant tokens + tool status
//! to the UI over Tauri events (`chat_delta`, `chat_tool`, `chat_done`).

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{Emitter, Manager};

use crate::commands::search::mcp_from_settings;
use crate::commands::settings::read_settings;
use crate::error::{AppError, AppResult};

const OPENROUTER_CHAT_URL: &str = "https://openrouter.ai/api/v1/chat/completions";
const MAX_TOOL_ROUNDS: usize = 8;

/// Exact analyst system prompt for the chat brain.
const SYSTEM_PROMPT: &str = "You are finmodel's analyst assistant inside a desktop app. You build 3-statement + DCF Excel models from SEC EDGAR (with optional trading-comps peers, a scenario case, and a PowerPoint summary deck), benchmark peers, read the actual text of 10-K/10-Q filings, analyze local annual-report PDFs, research deals, read news and web pages. Use tools when the user asks for data or artifacts; never fabricate financial numbers — every number must come from a tool result. For qualitative filing content (risk factors, MD&A, business description) use read_filing, never web_search. Be concise. Format with markdown. When a tool returns a card, refer to it instead of repeating its table.";

const FALLBACK_HELP: &str = "I couldn't map that to a tool. Try 'build AAPL', 'benchmark AAPL, MSFT', 'news NVDA', 'search …', or add an OpenRouter API key in Settings for full natural-language chat.";

// ---------------------------------------------------------------------------
// Data model
// ---------------------------------------------------------------------------

/// One message in a conversation. Tool results are persisted as assistant
/// messages carrying a `card`; raw LLM tool-call/result payloads are not stored.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChatMsg {
    pub role: String, // "user" | "assistant"
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub card: Option<Value>,
    pub ts: String,
}

/// A stored conversation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Conversation {
    pub id: String,
    pub title: String,
    pub created: String,
    pub updated: String,
    pub messages: Vec<ChatMsg>,
}

/// Single-flight + cancellation gate for the chat engine (managed state).
#[derive(Default)]
pub struct ChatGate {
    busy: AtomicBool,
    cancel: AtomicBool,
}

/// Resets the busy flag on drop so a `?`-early-return never wedges the gate.
struct BusyGuard<'a>(&'a AtomicBool);
impl<'a> Drop for BusyGuard<'a> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

// ---------------------------------------------------------------------------
// Time helpers (ISO-8601 UTC without a date-lib dependency)
// ---------------------------------------------------------------------------

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

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
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

fn conv_path(app: &tauri::AppHandle, id: &str) -> AppResult<PathBuf> {
    Ok(conv_dir(app)?.join(format!("{id}.json")))
}

/// Pure fs write (path resolution split out for testability).
fn write_conversation(path: &std::path::Path, conv: &Conversation) -> AppResult<()> {
    std::fs::write(path, serde_json::to_string_pretty(conv)?).map_err(|e| AppError::Io(e.to_string()))
}

/// Pure fs read (path resolution split out for testability).
fn read_conversation(path: &std::path::Path) -> AppResult<Conversation> {
    let txt = std::fs::read_to_string(path).map_err(|e| AppError::Io(e.to_string()))?;
    serde_json::from_str(&txt).map_err(|e| AppError::Engine(e.to_string()))
}

/// First 80 chars of the last non-empty message — the sidebar preview.
fn preview_of(conv: &Conversation) -> String {
    let last = conv
        .messages
        .iter()
        .rev()
        .find(|m| !m.content.trim().is_empty())
        .map(|m| m.content.as_str())
        .unwrap_or("");
    last.chars().take(80).collect()
}

fn new_conversation() -> Conversation {
    let id = format!("{}-{:04x}", now_ms(), rand::random::<u16>());
    let now = iso_now();
    Conversation {
        id,
        title: String::new(),
        created: now.clone(),
        updated: now,
        messages: Vec::new(),
    }
}

// ---------------------------------------------------------------------------
// Conversation commands
// ---------------------------------------------------------------------------

/// `[{ id, title, updated, preview }]`, sorted by `updated` desc. Corrupt files
/// are skipped (logged to stderr), never fatal.
#[tauri::command(rename_all = "snake_case")]
pub fn list_conversations(app: tauri::AppHandle) -> AppResult<String> {
    let dir = conv_dir(&app)?;
    let mut items: Vec<Value> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            match read_conversation(&path) {
                Ok(conv) => items.push(json!({
                    "id": conv.id,
                    "title": if conv.title.is_empty() { "New conversation".to_string() } else { conv.title.clone() },
                    "updated": conv.updated,
                    "preview": preview_of(&conv),
                })),
                Err(e) => eprintln!("skip corrupt conversation {}: {e}", path.display()),
            }
        }
    }
    items.sort_by(|a, b| {
        b["updated"]
            .as_str()
            .unwrap_or("")
            .cmp(a["updated"].as_str().unwrap_or(""))
    });
    Ok(serde_json::to_string(&items)?)
}

#[tauri::command(rename_all = "snake_case")]
pub fn load_conversation(app: tauri::AppHandle, id: String) -> AppResult<String> {
    let path = conv_path(&app, &id)?;
    if !path.exists() {
        return Err(AppError::Config("conversation not found".into()));
    }
    let conv = read_conversation(&path)?;
    Ok(serde_json::to_string(&conv)?)
}

#[tauri::command(rename_all = "snake_case")]
pub fn delete_conversation(app: tauri::AppHandle, id: String) -> AppResult<String> {
    let path = conv_path(&app, &id)?;
    if path.exists() {
        std::fs::remove_file(&path).map_err(|e| AppError::Io(e.to_string()))?;
    }
    Ok(json!({ "ok": true }).to_string())
}

#[tauri::command(rename_all = "snake_case")]
pub fn rename_conversation(app: tauri::AppHandle, id: String, title: String) -> AppResult<String> {
    let path = conv_path(&app, &id)?;
    if !path.exists() {
        return Err(AppError::Config("conversation not found".into()));
    }
    let mut conv = read_conversation(&path)?;
    conv.title = title_from(&title);
    conv.updated = iso_now();
    write_conversation(&path, &conv)?;
    Ok(serde_json::to_string(&conv)?)
}

// ---------------------------------------------------------------------------
// Chat engine
// ---------------------------------------------------------------------------

/// Send a chat turn. Streams `chat_delta`/`chat_tool`/`chat_done` events and
/// returns `{ conversation_id, messages: [appended this turn] }`.
#[tauri::command(rename_all = "snake_case")]
pub async fn chat_send(
    app: tauri::AppHandle,
    conversation_id: Option<String>,
    message: String,
) -> AppResult<String> {
    tauri::async_runtime::spawn_blocking(move || chat_send_blocking(&app, conversation_id, message))
        .await
        .map_err(|e| AppError::Engine(format!("chat task failed: {e}")))?
}

/// Request cancellation of the running turn (checked between SSE reads + rounds).
#[tauri::command(rename_all = "snake_case")]
pub fn chat_cancel(app: tauri::AppHandle) -> AppResult<String> {
    app.state::<ChatGate>().cancel.store(true, Ordering::SeqCst);
    Ok(json!({ "ok": true }).to_string())
}

fn chat_send_blocking(
    app: &tauri::AppHandle,
    conversation_id: Option<String>,
    message: String,
) -> AppResult<String> {
    let text = message.trim().to_string();
    if text.is_empty() {
        return Err(AppError::Config("Type a message.".into()));
    }

    let gate = app.state::<ChatGate>();
    if gate.busy.swap(true, Ordering::SeqCst) {
        return Err(AppError::Config("A chat turn is already running.".into()));
    }
    gate.cancel.store(false, Ordering::SeqCst);
    let _guard = BusyGuard(&gate.busy);

    // Load or create the conversation.
    let mut conv = match conversation_id {
        Some(id) => match conv_path(app, &id).ok().filter(|p| p.exists()) {
            Some(p) => read_conversation(&p)?,
            None => {
                let mut c = new_conversation();
                c.id = id;
                c
            }
        },
        None => new_conversation(),
    };

    // Append the user message; set the title from the first message.
    if conv.title.is_empty() {
        conv.title = title_from(&text);
    }
    conv.messages.push(ChatMsg {
        role: "user".into(),
        content: text,
        card: None,
        ts: iso_now(),
    });

    let settings = read_settings(app);
    let has_key = !settings.openrouter_api_key.trim().is_empty();

    let appended = if has_key {
        let cfg = fm_extract::LlmConfig {
            api_key: settings.openrouter_api_key.trim().to_string(),
            model: settings.model.trim().to_string(),
        };
        run_llm_turn(app, &mut conv, &cfg, &gate.cancel)
    } else {
        run_fallback_turn(app, &mut conv, &gate.cancel)
    };

    conv.updated = iso_now();
    write_conversation(&conv_path(app, &conv.id)?, &conv)?;
    emit(app, "chat_done", json!({ "conversation_id": conv.id }));

    Ok(json!({ "conversation_id": conv.id, "messages": appended }).to_string())
}

fn emit(app: &tauri::AppHandle, event: &str, payload: Value) {
    let _ = app.emit(event, payload);
}

// ---------------------------------------------------------------------------
// LLM tool-calling loop + SSE streaming
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
    },
    /// Mid-stream network failure — whatever streamed is kept.
    Partial { content: String, error: String },
    /// Pre-stream 400/404 — the model likely rejects `tools`.
    ToolsUnsupported,
    /// Pre-stream failure (auth, network, other status).
    Failed(String),
}

/// Build the OpenRouter chat request body (pure — unit-tested).
fn build_chat_request(model: &str, msgs: &[Value], tools: &[Value], stream: bool) -> Value {
    let mut req = json!({
        "model": model,
        "messages": msgs,
        "temperature": 0,
        "stream": stream,
    });
    if !tools.is_empty() {
        req["tools"] = json!(tools);
        req["tool_choice"] = json!("auto");
    }
    req
}

/// Apply one SSE `data:` payload to the running accumulators. Returns the
/// content chunk (for live emission), if any.
fn apply_delta(content: &mut String, calls: &mut Vec<ToolCall>, payload: &str) -> Option<String> {
    let v: Value = serde_json::from_str(payload).ok()?;
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
    chunk
}

/// Accumulate full content + tool calls from a list of SSE `data:` payloads
/// (pure — unit-tested). Stops at `[DONE]`.
#[cfg(test)]
fn sse_accumulate(events: &[&str]) -> (String, Vec<ToolCall>) {
    let mut content = String::new();
    let mut calls: Vec<ToolCall> = Vec::new();
    for ev in events {
        if ev.trim() == "[DONE]" {
            break;
        }
        apply_delta(&mut content, &mut calls, ev);
    }
    (content, calls)
}

/// POST a streaming completion and consume the SSE body, emitting `chat_delta`
/// per content chunk.
fn openrouter_stream(
    app: &tauri::AppHandle,
    conv_id: &str,
    cfg: &fm_extract::LlmConfig,
    req: &Value,
    cancel: &AtomicBool,
) -> StreamOutcome {
    let client = match reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()
    {
        Ok(c) => c,
        Err(e) => return StreamOutcome::Failed(e.to_string()),
    };
    let resp = match client
        .post(OPENROUTER_CHAT_URL)
        .header("Authorization", format!("Bearer {}", cfg.api_key))
        .header("Content-Type", "application/json")
        .header("HTTP-Referer", "https://github.com/finmodel")
        .header("X-Title", "finmodel")
        .json(req)
        .send()
    {
        Ok(r) => r,
        Err(e) => return StreamOutcome::Failed(e.to_string()),
    };

    let status = resp.status();
    if !status.is_success() {
        let code = status.as_u16();
        let body = resp.text().unwrap_or_default();
        if code == 400 || code == 404 {
            return StreamOutcome::ToolsUnsupported;
        }
        let snippet: String = body.chars().take(300).collect();
        return StreamOutcome::Failed(format!("OpenRouter {code}: {snippet}"));
    }

    let mut content = String::new();
    let mut calls: Vec<ToolCall> = Vec::new();
    let reader = BufReader::new(resp);
    for line in reader.lines() {
        if cancel.load(Ordering::SeqCst) {
            break;
        }
        let line = match line {
            Ok(l) => l,
            Err(e) => {
                return StreamOutcome::Partial {
                    content,
                    error: e.to_string(),
                }
            }
        };
        let payload = match line.strip_prefix("data:") {
            Some(p) => p.trim(),
            None => continue,
        };
        if payload == "[DONE]" {
            break;
        }
        if payload.is_empty() {
            continue;
        }
        if let Some(chunk) = apply_delta(&mut content, &mut calls, payload) {
            emit(
                app,
                "chat_delta",
                json!({ "conversation_id": conv_id, "text": chunk }),
            );
        }
    }
    calls.retain(|c| !c.name.is_empty());
    StreamOutcome::Ok {
        content,
        tool_calls: calls,
    }
}

/// Build the LLM message array: system prompt + prior user/assistant text.
fn history_messages(conv: &Conversation) -> Vec<Value> {
    let mut msgs = vec![json!({ "role": "system", "content": SYSTEM_PROMPT })];
    for m in &conv.messages {
        match m.role.as_str() {
            "user" => msgs.push(json!({ "role": "user", "content": m.content })),
            "assistant" if !m.content.trim().is_empty() => {
                msgs.push(json!({ "role": "assistant", "content": m.content }))
            }
            _ => {} // card-only assistant messages carry no LLM text
        }
    }
    msgs
}

fn assistant_tool_call_message(content: &str, calls: &[ToolCall]) -> Value {
    json!({
        "role": "assistant",
        "content": if content.is_empty() { Value::Null } else { json!(content) },
        "tool_calls": calls.iter().map(|c| json!({
            "id": c.id,
            "type": "function",
            "function": { "name": c.name, "arguments": c.arguments },
        })).collect::<Vec<_>>(),
    })
}

fn run_llm_turn(
    app: &tauri::AppHandle,
    conv: &mut Conversation,
    cfg: &fm_extract::LlmConfig,
    cancel: &AtomicBool,
) -> Vec<ChatMsg> {
    let tools = tool_schemas();
    let mut messages = history_messages(conv);
    let mut appended: Vec<ChatMsg> = Vec::new();
    let mut use_tools = true;
    let mut rounds = 0usize;
    let no_tools: Vec<Value> = Vec::new();
    let user_msg = conv
        .messages
        .last()
        .map(|m| m.content.clone())
        .unwrap_or_default();

    loop {
        if cancel.load(Ordering::SeqCst) {
            push_assistant(conv, &mut appended, "(stopped)");
            break;
        }
        let req = build_chat_request(
            &cfg.model,
            &messages,
            if use_tools { &tools } else { &no_tools },
            true,
        );
        match openrouter_stream(app, &conv.id, cfg, &req, cancel) {
            StreamOutcome::ToolsUnsupported if use_tools => {
                // This model can't accept the tools param. Route the message
                // deterministically so a data query still runs a real tool
                // instead of a fabricated free-form answer; otherwise fall back
                // to a plain (tool-free) answer for non-data questions.
                if run_routed_tool(app, conv, &mut appended, &user_msg) {
                    break;
                }
                use_tools = false;
                messages.push(json!({
                    "role": "system",
                    "content": "(tools unavailable for this model — answer directly without tools)"
                }));
                continue;
            }
            StreamOutcome::ToolsUnsupported => {
                push_assistant(
                    conv,
                    &mut appended,
                    "This model rejected the request. Choose a different model in Settings.",
                );
                break;
            }
            StreamOutcome::Failed(err) => {
                emit(
                    app,
                    "chat_tool",
                    json!({ "conversation_id": conv.id, "name": "chat", "status": "error", "detail": err }),
                );
                push_assistant(
                    conv,
                    &mut appended,
                    "⚠ the model request failed — check your API key and model in Settings.",
                );
                break;
            }
            StreamOutcome::Partial { content, error } => {
                emit(
                    app,
                    "chat_tool",
                    json!({ "conversation_id": conv.id, "name": "chat", "status": "error", "detail": error }),
                );
                let msg = if content.trim().is_empty() {
                    "⚠ connection lost — partial reply kept".to_string()
                } else {
                    format!("{content}\n\n⚠ connection lost — partial reply kept")
                };
                push_assistant(conv, &mut appended, &msg);
                break;
            }
            StreamOutcome::Ok {
                content,
                tool_calls,
            } => {
                if tool_calls.is_empty() {
                    // A weak model may answer a data query WITHOUT calling a
                    // tool (risking fabricated numbers/links). If no tool ran
                    // this turn and the message is an EXPLICIT data request,
                    // drop the streamed text and run the real tool so figures
                    // come from a tool result, never the model. Bare
                    // definitional questions (e.g. "what is EBITDA") are left to
                    // the model — the loose web-search keywords don't count
                    // unless the user actually asked to search/find/look up.
                    if rounds == 0 {
                        if let Some((tool, _)) = route_fallback(&user_msg) {
                            let lm = user_msg.to_lowercase();
                            let explicit = tool != ToolName::WebSearch
                                || lm.contains("search")
                                || lm.contains("look up")
                                || lm.contains("find");
                            if explicit {
                                emit(app, "chat_reset", json!({ "conversation_id": conv.id }));
                                run_routed_tool(app, conv, &mut appended, &user_msg);
                                break;
                            }
                        }
                    }
                    push_assistant(conv, &mut appended, &content);
                    break;
                }
                // Record the assistant's tool-call turn for LLM context.
                messages.push(assistant_tool_call_message(&content, &tool_calls));
                if !content.trim().is_empty() {
                    push_assistant(conv, &mut appended, &content);
                }
                rounds += 1;
                for tc in &tool_calls {
                    let tool = ToolName::from_str(&tc.name);
                    emit(
                        app,
                        "chat_tool",
                        json!({ "conversation_id": conv.id, "name": tc.name, "status": "start", "detail": "" }),
                    );
                    let args: Value = serde_json::from_str(&tc.arguments).unwrap_or_else(|_| json!({}));
                    let (result_text, card) = match tool {
                        Some(t) => match run_tool(app, t, &args) {
                            Ok((txt, card)) => (txt, card),
                            Err(e) => (format!("Tool error: {e}"), error_card(&tc.name, &e)),
                        },
                        None => (
                            format!("Unknown tool: {}", tc.name),
                            error_card(&tc.name, "unknown tool"),
                        ),
                    };
                    let status = if card["type"] == json!("error") { "error" } else { "done" };
                    emit(
                        app,
                        "chat_tool",
                        json!({ "conversation_id": conv.id, "name": tc.name, "status": status, "card": card }),
                    );
                    push_card(conv, &mut appended, card);
                    messages.push(json!({
                        "role": "tool",
                        "tool_call_id": tc.id,
                        "content": result_text,
                    }));
                }
                if rounds >= MAX_TOOL_ROUNDS {
                    push_assistant(conv, &mut appended, "(stopped: tool limit reached)");
                    break;
                }
            }
        }
    }
    appended
}

/// Strip model pseudo-control tokens (e.g. `<|eom|>`, `<|eot_id|>`, `<|end|>`)
/// that some models leak into their text. Balanced `<| … |>` spans are removed.
fn strip_control_tokens(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find("<|") {
        out.push_str(&rest[..i]);
        match rest[i + 2..].find("|>") {
            Some(j) => rest = &rest[i + 2 + j + 2..],
            None => {
                out.push_str(&rest[i..]);
                rest = "";
                break;
            }
        }
    }
    out.push_str(rest);
    out.trim().to_string()
}

fn push_assistant(conv: &mut Conversation, appended: &mut Vec<ChatMsg>, content: &str) {
    let msg = ChatMsg {
        role: "assistant".into(),
        content: strip_control_tokens(content),
        card: None,
        ts: iso_now(),
    };
    conv.messages.push(msg.clone());
    appended.push(msg);
}

/// Deterministically route `user_msg` to a tool and execute it, emitting the
/// tool + intro-delta events and pushing the intro + card. Returns true if a
/// tool ran. Used by the no-key fallback AND as a safety net when a weak model
/// won't call tools (so a finance data query executes a real tool instead of
/// producing a fabricated free-form answer).
fn run_routed_tool(
    app: &tauri::AppHandle,
    conv: &mut Conversation,
    appended: &mut Vec<ChatMsg>,
    user_msg: &str,
) -> bool {
    let Some((tool, args)) = route_fallback(user_msg) else {
        return false;
    };
    emit(
        app,
        "chat_tool",
        json!({ "conversation_id": conv.id, "name": tool.as_str(), "status": "start", "detail": "" }),
    );
    match run_tool(app, tool, &args) {
        Ok((_text, card)) => {
            emit(
                app,
                "chat_tool",
                json!({ "conversation_id": conv.id, "name": tool.as_str(), "status": "done", "card": card }),
            );
            let intro = "Here's what I found:";
            emit(app, "chat_delta", json!({ "conversation_id": conv.id, "text": intro }));
            push_assistant(conv, appended, intro);
            push_card(conv, appended, card);
        }
        Err(e) => {
            emit(
                app,
                "chat_tool",
                json!({ "conversation_id": conv.id, "name": tool.as_str(), "status": "error", "detail": e }),
            );
            let msg = format!("Tool error: {e}");
            emit(app, "chat_delta", json!({ "conversation_id": conv.id, "text": msg }));
            push_assistant(conv, appended, &msg);
        }
    }
    true
}

fn push_card(conv: &mut Conversation, appended: &mut Vec<ChatMsg>, card: Value) {
    let msg = ChatMsg {
        role: "assistant".into(),
        content: String::new(),
        card: Some(card),
        ts: iso_now(),
    };
    conv.messages.push(msg.clone());
    appended.push(msg);
}

// ---------------------------------------------------------------------------
// No-key fallback router
// ---------------------------------------------------------------------------

fn run_fallback_turn(
    app: &tauri::AppHandle,
    conv: &mut Conversation,
    _cancel: &AtomicBool,
) -> Vec<ChatMsg> {
    let user = conv
        .messages
        .last()
        .map(|m| m.content.clone())
        .unwrap_or_default();
    let mut appended: Vec<ChatMsg> = Vec::new();
    if !run_routed_tool(app, conv, &mut appended, &user) {
        emit(
            app,
            "chat_delta",
            json!({ "conversation_id": conv.id, "text": FALLBACK_HELP }),
        );
        push_assistant(conv, &mut appended, FALLBACK_HELP);
    }
    appended
}

/// Deterministic keyword router used when no OpenRouter key is set. First match
/// wins on the lowercased message; ticker tokens are matched on original casing.
fn route_fallback(msg: &str) -> Option<(ToolName, Value)> {
    let m = msg.to_lowercase();
    let tickers = ticker_tokens(msg);
    let has = |kws: &[&str]| kws.iter().any(|k| m.contains(k));

    // 0. A quoted local path ending .pdf → analyze it directly.
    if let Some(path) = quoted_pdf_path(msg) {
        let label = std::path::Path::new(&path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("PDF")
            .to_string();
        return Some((ToolName::AnalyzePdf, json!({ "path": path, "label": label })));
    }
    // 1. benchmark / comps with >= 2 tickers.
    if has(&["benchmark", "compare", "peers", "comps"]) && tickers.len() >= 2 && !m.contains("build") {
        return Some((ToolName::BenchmarkPeers, json!({ "tickers": tickers })));
    }
    // 2. build / model / dcf with >= 1 ticker (extra tickers → peers; case tags).
    if has(&["build", "model", "dcf", "3-statement", "three statement"]) && !tickers.is_empty() {
        let mut args = json!({ "ticker": tickers[0] });
        let peers: Vec<String> = tickers.iter().skip(1).cloned().collect();
        if (has(&["with peers", " vs ", "versus", " with "]) || m.contains("peers")) && !peers.is_empty() {
            args["peers"] = json!(peers);
        }
        if has(&["downside", "bear"]) {
            args["case"] = json!("downside");
        } else if has(&["upside", "bull"]) {
            args["case"] = json!("upside");
        }
        return Some((ToolName::BuildModel, args));
    }
    // 3. qualitative filing content (10-K/10-Q text) with a ticker — before the
    //    generic web-search rule so it wins over "latest".
    if has(&["10-k", "10k", "10-q", "10q", "annual report", "md&a", "mda", "risk factors"])
        && !tickers.is_empty()
    {
        let form = if has(&["10-q", "10q"]) { "10-Q" } else { "10-K" };
        let mut args = json!({ "ticker": tickers[0], "form": form });
        if has(&["risk factors"]) {
            args["item"] = json!("1A");
        } else if has(&["md&a", "mda", "management discussion", "discussion and analysis"]) {
            args["item"] = json!("7");
        }
        return Some((ToolName::ReadFiling, args));
    }
    // 4. news / headlines.
    if has(&["news", "headlines"]) {
        let q = strip_keywords(msg, &["news", "headlines"]);
        return Some((ToolName::GetNews, json!({ "query": q })));
    }
    // 5. deal / M&A.
    if has(&["acqui", "merger", "deal", "takeover", "buyout"]) {
        return Some((ToolName::ResearchDeal, json!({ "query": msg.trim() })));
    }
    // 6. general web search.
    if has(&["search", "find", "look up", "what is", "who is", "latest"]) {
        return Some((ToolName::WebSearch, json!({ "query": msg.trim() })));
    }
    // 7. quote / filings with a ticker.
    if has(&["quote", "price"]) && !tickers.is_empty() {
        return Some((ToolName::GetQuote, json!({ "ticker": tickers[0] })));
    }
    if has(&["filings", "20-f"]) && !tickers.is_empty() {
        return Some((ToolName::ListFilings, json!({ "ticker": tickers[0] })));
    }
    None
}

/// First double-quoted substring ending in `.pdf` (case-insensitive), if any.
fn quoted_pdf_path(msg: &str) -> Option<String> {
    msg.split('"')
        .enumerate()
        .find(|(i, p)| i % 2 == 1 && p.trim().to_lowercase().ends_with(".pdf"))
        .map(|(_, p)| p.trim().to_string())
}

/// Extract ticker-like tokens: `[A-Z]{1,5}(\.[A-Z]{1,2})?`. The English words
/// "I" and "A" are excluded as they are never tickers in a sentence.
fn ticker_tokens(msg: &str) -> Vec<String> {
    let mut out = Vec::new();
    for raw in msg.split(|c: char| !(c.is_ascii_alphanumeric() || c == '.')) {
        let tok = raw.trim_matches('.');
        if is_ticker_like(tok) && tok != "I" && tok != "A" && !out.contains(&tok.to_string()) {
            out.push(tok.to_string());
        }
    }
    out
}

fn is_ticker_like(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    let (base, suffix) = match s.split_once('.') {
        Some((b, x)) => (b, Some(x)),
        None => (s, None),
    };
    if base.is_empty() || base.len() > 5 || !base.chars().all(|c| c.is_ascii_uppercase()) {
        return false;
    }
    if let Some(x) = suffix {
        if x.is_empty() || x.len() > 2 || !x.chars().all(|c| c.is_ascii_uppercase()) {
            return false;
        }
    }
    true
}

fn strip_keywords(msg: &str, kws: &[&str]) -> String {
    let mut out = msg.to_string();
    for k in kws {
        // case-insensitive removal
        loop {
            let lower = out.to_lowercase();
            match lower.find(k) {
                Some(i) => {
                    out.replace_range(i..i + k.len(), " ");
                }
                None => break,
            }
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

// ---------------------------------------------------------------------------
// Tools
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ToolName {
    BuildModel,
    BenchmarkPeers,
    WebSearch,
    ReadPage,
    GetNews,
    ResearchDeal,
    GetQuote,
    ListFilings,
    ReadFiling,
    AnalyzePdf,
}

impl ToolName {
    fn as_str(self) -> &'static str {
        match self {
            ToolName::BuildModel => "build_model",
            ToolName::BenchmarkPeers => "benchmark_peers",
            ToolName::WebSearch => "web_search",
            ToolName::ReadPage => "read_page",
            ToolName::GetNews => "get_news",
            ToolName::ResearchDeal => "research_deal",
            ToolName::GetQuote => "get_quote",
            ToolName::ListFilings => "list_filings",
            ToolName::ReadFiling => "read_filing",
            ToolName::AnalyzePdf => "analyze_pdf",
        }
    }
    fn from_str(s: &str) -> Option<Self> {
        Some(match s {
            "build_model" => ToolName::BuildModel,
            "benchmark_peers" => ToolName::BenchmarkPeers,
            "web_search" => ToolName::WebSearch,
            "read_page" => ToolName::ReadPage,
            "get_news" => ToolName::GetNews,
            "research_deal" => ToolName::ResearchDeal,
            "get_quote" => ToolName::GetQuote,
            "list_filings" => ToolName::ListFilings,
            "read_filing" => ToolName::ReadFiling,
            "analyze_pdf" => ToolName::AnalyzePdf,
            _ => return None,
        })
    }
}

fn error_card(tool: &str, message: &str) -> Value {
    json!({ "type": "error", "tool": tool, "message": message })
}

/// OpenAI function-tool schemas exposed to the LLM.
fn tool_schemas() -> Vec<Value> {
    fn f(name: &str, description: &str, params: Value) -> Value {
        json!({ "type": "function", "function": { "name": name, "description": description, "parameters": params } })
    }
    vec![
        f(
            "build_model",
            "Build a 3-statement + DCF Excel model for a ticker from SEC EDGAR. By default it presents an editable assumptions grid to the user; the user finalizes it manually. Set skip_review=true to build immediately without review.",
            json!({
                "type": "object",
                "properties": {
                    "ticker": { "type": "string", "description": "Ticker, e.g. AAPL or SAND.ST" },
                    "period": { "type": "string", "enum": ["annual", "quarter", "semi", "ltm"] },
                    "years": { "type": "integer", "description": "Projection years (1-10)" },
                    "skip_review": { "type": "boolean", "description": "Build immediately, skipping the assumptions grid" },
                    "peers": { "type": "array", "items": { "type": "string" }, "description": "Optional peer tickers for a trading-comps tab" },
                    "case": { "type": "string", "enum": ["base", "upside", "downside"], "description": "Scenario case (default base)" }
                },
                "required": ["ticker"]
            }),
        ),
        f(
            "benchmark_peers",
            "Benchmark a set of peer tickers (revenue, margins, ROE, leverage) into a comparison workbook.",
            json!({
                "type": "object",
                "properties": {
                    "tickers": { "type": "array", "items": { "type": "string" }, "description": "Peer tickers" },
                    "period": { "type": "string", "enum": ["annual", "quarter", "semi", "ltm"] },
                    "multiples": { "type": "boolean" },
                    "usd": { "type": "boolean" }
                },
                "required": ["tickers"]
            }),
        ),
        f(
            "web_search",
            "Search the web and return ranked results.",
            json!({ "type": "object", "properties": { "query": { "type": "string" } }, "required": ["query"] }),
        ),
        f(
            "read_page",
            "Read a web page and return its readable text (title + status).",
            json!({ "type": "object", "properties": { "url": { "type": "string" } }, "required": ["url"] }),
        ),
        f(
            "get_news",
            "Fetch recent news headlines for a ticker or query.",
            json!({
                "type": "object",
                "properties": { "query": { "type": "string" }, "limit": { "type": "integer" } },
                "required": ["query"]
            }),
        ),
        f(
            "research_deal",
            "Research an M&A deal (terms, rationale, sources) from public filings and news.",
            json!({ "type": "object", "properties": { "query": { "type": "string" } }, "required": ["query"] }),
        ),
        f(
            "get_quote",
            "Fetch the latest share price quote for a ticker.",
            json!({ "type": "object", "properties": { "ticker": { "type": "string" } }, "required": ["ticker"] }),
        ),
        f(
            "list_filings",
            "List recent SEC EDGAR filings for a US ticker.",
            json!({
                "type": "object",
                "properties": { "ticker": { "type": "string" }, "form": { "type": "string", "description": "e.g. 10-K, 10-Q, 8-K" } },
                "required": ["ticker"]
            }),
        ),
        f(
            "read_filing",
            "Read the actual text of a company's latest SEC filing (10-K/10-Q). Use for qualitative content — risk factors (item 1A), MD&A (item 7), business description. Never use web_search for filing content.",
            json!({
                "type": "object",
                "properties": {
                    "ticker": { "type": "string" },
                    "form": { "type": "string", "description": "e.g. 10-K, 10-Q (default 10-K)" },
                    "item": { "type": "string", "description": "Item id, e.g. 1A (risk factors), 7 (MD&A)" }
                },
                "required": ["ticker"]
            }),
        ),
        f(
            "analyze_pdf",
            "Analyze a local annual-report PDF file into a 3-statement + DCF model. Requires an OpenRouter API key.",
            json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Absolute path to a .pdf file" },
                    "label": { "type": "string", "description": "Company/ticker label for the workbook" }
                },
                "required": ["path", "label"]
            }),
        ),
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
fn run_tool(app: &tauri::AppHandle, tool: ToolName, args: &Value) -> Result<(String, Value), String> {
    match tool {
        ToolName::BuildModel => tool_build_model(app, args),
        ToolName::BenchmarkPeers => tool_benchmark(app, args),
        ToolName::WebSearch => tool_web_search(app, args),
        ToolName::ReadPage => tool_read_page(app, args),
        ToolName::GetNews => tool_get_news(args),
        ToolName::ResearchDeal => tool_research_deal(app, args),
        ToolName::GetQuote => tool_get_quote(args),
        ToolName::ListFilings => tool_list_filings(args),
        ToolName::ReadFiling => tool_read_filing(app, args),
        ToolName::AnalyzePdf => tool_analyze_pdf(app, args),
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
            "Presented an editable assumptions grid to the user; they will finalize it manually.".into(),
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
    let mut client = mcp_from_settings(app);
    let hits = match client.as_mut() {
        Some(_) => fm_research::web::web_search(&q, client.as_mut())
            .or_else(|_| fm_research::web::web_search(&q, None)),
        None => fm_research::web::web_search(&q, None),
    }
    .map_err(|e| e.to_string())?;

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
    let mut client = mcp_from_settings(app);
    let page = match client.as_mut() {
        Some(_) => fm_research::web::read_page_full(&url, None, client.as_mut())
            .or_else(|_| fm_research::web::read_page_full(&url, None, None)),
        None => fm_research::web::read_page_full(&url, None, None),
    }
    .map_err(|e| e.to_string())?;

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
    let mut client = mcp_from_settings(app);
    let deal = fm_research::agent::run_deal_research(&q, client.as_mut());
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
        if deal.target.is_empty() { String::new() } else { format!(" target={}", deal.target) },
        if deal.acquirer.is_empty() { String::new() } else { format!(" acquirer={}", deal.acquirer) },
        deal.sufficient,
        deal.sources_read.len(),
        truncate(&serde_json::to_string(&deal.summary).unwrap_or_default(), 1500),
    );
    Ok((truncate(&text, 2000), card))
}

fn tool_get_quote(args: &Value) -> Result<(String, Value), String> {
    let ticker = args["ticker"].as_str().unwrap_or("").trim().to_string();
    if ticker.is_empty() {
        return Err("get_quote requires a ticker".into());
    }
    let q = fm_fetch::fetch_quote(&ticker).map_err(|e| e.to_string())?;
    let text = format!("{} {:.2} {} (52w {:?}-{:?})", q.ticker, q.price, q.currency, q.week52_low, q.week52_high);
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
        text.push_str(&format!("- {} filed {} — {}\n", f.form_type, f.filing_date, f.url));
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
                (format!("{form} Item {want} for {ticker}:\n\n{clipped}"), json!(want), n)
            }
            None => (
                format!("Item {want} not found in {ticker} {form}. Available items: {}", ids.join(", ")),
                Value::Null,
                0,
            ),
        }
    } else {
        let head = truncate(&text, 4_000);
        let n = head.chars().count();
        (format!("{form} for {ticker}. Items: {}\n\nExcerpt:\n{head}", ids.join(", ")), Value::Null, n)
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

fn tool_analyze_pdf(app: &tauri::AppHandle, args: &Value) -> Result<(String, Value), String> {
    let path = args["path"].as_str().unwrap_or("").trim().to_string();
    if path.is_empty() {
        return Err("analyze_pdf requires a path".into());
    }
    let label = args["label"].as_str().unwrap_or("").trim().to_string();
    let label = if label.is_empty() {
        std::path::Path::new(&path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("PDF")
            .to_string()
    } else {
        label
    };
    let mut opts = fm_build::BuildOptions::default();
    opts.deck = true;
    let summary = crate::commands::model::analyze_pdf_blocking(app, &path, &label, opts)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_utc_epoch_and_known_dates() {
        assert_eq!(iso_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso_utc(1_700_000_000), "2023-11-14T22:13:20Z");
    }

    #[test]
    fn build_chat_request_shape_with_and_without_tools() {
        let msgs = vec![json!({ "role": "user", "content": "hi" })];
        let tools = vec![json!({ "type": "function" })];
        let req = build_chat_request("m", &msgs, &tools, true);
        assert_eq!(req["model"], json!("m"));
        assert_eq!(req["stream"], json!(true));
        assert_eq!(req["tool_choice"], json!("auto"));
        assert!(req["tools"].is_array());

        let bare = build_chat_request("m", &msgs, &[], false);
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
        let (content, calls) = sse_accumulate(&events);
        assert_eq!(content, "Hello");
        assert!(calls.is_empty());
    }

    #[test]
    fn sse_accumulate_split_tool_call_fragments() {
        let events = [
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","function":{"name":"build_model","arguments":"{\"tic"}}]}}]}"#,
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"ker\":\"AAPL\"}"}}]}}]}"#,
            "[DONE]",
        ];
        let (_content, calls) = sse_accumulate(&events);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "call_1");
        assert_eq!(calls[0].name, "build_model");
        assert_eq!(calls[0].arguments, "{\"ticker\":\"AAPL\"}");
        let args: Value = serde_json::from_str(&calls[0].arguments).unwrap();
        assert_eq!(args["ticker"], json!("AAPL"));
    }

    #[test]
    fn route_benchmark_before_build() {
        // "benchmark ... model" contains both keywords + 2 tickers → benchmark wins.
        let (tool, args) = route_fallback("benchmark AAPL, MSFT model").unwrap();
        assert_eq!(tool, ToolName::BenchmarkPeers);
        assert_eq!(args["tickers"], json!(["AAPL", "MSFT"]));
    }

    #[test]
    fn route_build_single_ticker() {
        let (tool, args) = route_fallback("build a dcf model for AAPL").unwrap();
        assert_eq!(tool, ToolName::BuildModel);
        assert_eq!(args["ticker"], json!("AAPL"));
    }

    #[test]
    fn route_news_strips_keyword() {
        let (tool, args) = route_fallback("news NVDA").unwrap();
        assert_eq!(tool, ToolName::GetNews);
        assert_eq!(args["query"], json!("NVDA"));
    }

    #[test]
    fn route_deal_search_quote_filings() {
        assert_eq!(route_fallback("the figma adobe merger").unwrap().0, ToolName::ResearchDeal);
        assert_eq!(route_fallback("search the web for margins").unwrap().0, ToolName::WebSearch);
        assert_eq!(route_fallback("quote AAPL").unwrap().0, ToolName::GetQuote);
        assert_eq!(route_fallback("show filings for AAPL").unwrap().0, ToolName::ListFilings);
    }

    #[test]
    fn route_no_match_returns_none() {
        assert!(route_fallback("tell me a joke about accounting").is_none());
    }

    #[test]
    fn route_build_with_peers() {
        let (tool, args) = route_fallback("Build NVDA with peers AMD, INTC, AVGO").unwrap();
        assert_eq!(tool, ToolName::BuildModel);
        assert_eq!(args["ticker"], json!("NVDA"));
        assert_eq!(args["peers"], json!(["AMD", "INTC", "AVGO"]));
    }

    #[test]
    fn route_build_downside_case() {
        let (tool, args) = route_fallback("Build the downside case for AMZN").unwrap();
        assert_eq!(tool, ToolName::BuildModel);
        assert_eq!(args["ticker"], json!("AMZN"));
        assert_eq!(args["case"], json!("downside"));
    }

    #[test]
    fn route_read_filing_risk_factors() {
        let (tool, args) = route_fallback("Read the risk factors in TSLA's latest 10-K").unwrap();
        assert_eq!(tool, ToolName::ReadFiling);
        assert_eq!(args["ticker"], json!("TSLA"));
        assert_eq!(args["form"], json!("10-K"));
        assert_eq!(args["item"], json!("1A"));
    }

    #[test]
    fn route_read_filing_mda() {
        let (tool, args) = route_fallback("show me the MD&A in AAPL's 10-K").unwrap();
        assert_eq!(tool, ToolName::ReadFiling);
        assert_eq!(args["item"], json!("7"));
    }

    #[test]
    fn route_analyze_pdf_quoted_path() {
        let (tool, args) =
            route_fallback("Analyze the filing PDF at \"C:/tmp/annual.pdf\" for TESTCO").unwrap();
        assert_eq!(tool, ToolName::AnalyzePdf);
        assert_eq!(args["path"], json!("C:/tmp/annual.pdf"));
        assert_eq!(args["label"], json!("annual"));
    }

    #[test]
    fn ticker_tokens_excludes_pronouns() {
        assert_eq!(ticker_tokens("I want AAPL and MSFT"), vec!["AAPL", "MSFT"]);
        assert_eq!(ticker_tokens("build SAND.ST please"), vec!["SAND.ST"]);
        assert!(ticker_tokens("build a model for apple").is_empty());
    }

    #[test]
    fn conversation_round_trip() {
        let dir = std::env::temp_dir().join(format!("finmodel-test-{}", now_ms()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("conv.json");
        let conv = Conversation {
            id: "abc-0001".into(),
            title: "Test".into(),
            created: iso_now(),
            updated: iso_now(),
            messages: vec![
                ChatMsg { role: "user".into(), content: "build AAPL".into(), card: None, ts: iso_now() },
                ChatMsg { role: "assistant".into(), content: String::new(), card: Some(json!({ "type": "model" })), ts: iso_now() },
            ],
        };
        write_conversation(&path, &conv).unwrap();
        let back = read_conversation(&path).unwrap();
        assert_eq!(back.id, "abc-0001");
        assert_eq!(back.messages.len(), 2);
        assert_eq!(back.messages[1].card.as_ref().unwrap()["type"], json!("model"));
        assert_eq!(preview_of(&conv), "build AAPL");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn title_truncation() {
        assert_eq!(title_from("short"), "short");
        let long: String = "x".repeat(60);
        let t = title_from(&long);
        assert_eq!(t.chars().count(), 49); // 48 + ellipsis
        assert!(t.ends_with('…'));
    }

    #[test]
    fn strip_control_tokens_removes_pseudo_tokens() {
        assert_eq!(strip_control_tokens("done.<|eom|>"), "done.");
        assert_eq!(strip_control_tokens("a<|eot_id|>b"), "ab");
        assert_eq!(strip_control_tokens("<|start|>hi<|end|>"), "hi");
        // unterminated marker is preserved (not silently swallowed)
        assert_eq!(strip_control_tokens("keep <| this"), "keep <| this");
        assert_eq!(strip_control_tokens("plain text"), "plain text");
    }
}
