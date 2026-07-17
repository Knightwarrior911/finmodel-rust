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

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{Emitter, Manager};

use crate::commands::mcp::McpManager;
use crate::commands::settings::read_settings;
use crate::error::{AppError, AppResult};

const OPENROUTER_CHAT_URL: &str = "https://openrouter.ai/api/v1/chat/completions";
/// Shared tool-call budget across native and application-routed multi-action paths.
const MAX_TOOL_ROUNDS: usize = 8;
/// Non-research chat overall wall-clock deadline (Phase 1.4 / 3.1).
const CHAT_DEADLINE_SECS: u64 = 120;
/// Shared total-token budget across native tool rounds in one chat turn.
/// Charged from provider `usage.total_tokens`; missing usage uses a conservative
/// fallback so unbounded spend cannot hide behind absent counters.
const MAX_TURN_TOKENS: u64 = 32_000;
/// Cap retained provider/error text before UI/persistence (roadmap: 8 KiB).
const MAX_ERROR_CHARS: usize = 8 * 1024;

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
    /// Compact rendered text for the LLM history (a summary/answer digest — NEVER
    /// raw pages or full card payloads). When present, used instead of `content`
    /// in [`history_messages`].
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub llm_context: Option<String>,
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

/// Roadmap 3.6: hard cap on stored conversation files (never auto-evicted).
const MAX_CONVERSATION_FILES: usize = 500;

/// Count `.json` conversation files currently on disk.
fn conversation_file_count(app: &tauri::AppHandle) -> usize {
    let dir = match conv_dir(app) {
        Ok(d) => d,
        Err(_) => return 0,
    };
    std::fs::read_dir(&dir)
        .map(|rd| {
            rd.filter_map(Result::ok)
                .filter(|e| {
                    e.path()
                        .extension()
                        .and_then(|x| x.to_str())
                        .map(|x| x.eq_ignore_ascii_case("json"))
                        .unwrap_or(false)
                })
                .count()
        })
        .unwrap_or(0)
}

/// Durable write: serialize to a same-directory temp file, fsync it, then
/// atomically replace the target (Windows-safe `rename` overwrites). A crash
/// mid-write leaves the previous good file intact, never a truncated one.
fn write_conversation(path: &std::path::Path, conv: &Conversation) -> AppResult<()> {
    use std::io::Write;
    let data = serde_json::to_string_pretty(conv)?;
    // Roadmap 3.6: 2 MiB/file cap. Refuse to grow past it rather than truncate;
    // the existing good file stays intact and the user is told to archive/delete.
    const MAX_CONV_BYTES: usize = 2 * 1024 * 1024;
    if data.len() > MAX_CONV_BYTES {
        return Err(AppError::Config(
            "conversation exceeds 2 MiB — start a new chat or delete old messages".into(),
        ));
    }
    let tmp = path.with_extension("json.tmp");
    {
        let mut f = std::fs::File::create(&tmp).map_err(|e| AppError::Io(e.to_string()))?;
        f.write_all(data.as_bytes())
            .map_err(|e| AppError::Io(e.to_string()))?;
        f.sync_all().ok();
    }
    std::fs::rename(&tmp, path).map_err(|e| AppError::Io(e.to_string()))?;
    Ok(())
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
    run_id: Option<String>,
) -> AppResult<String> {
    tauri::async_runtime::spawn_blocking(move || {
        chat_send_blocking(&app, conversation_id, message, run_id)
    })
    .await
    .map_err(|e| AppError::Engine(format!("chat task failed: {e}")))?
}

/// Request cancellation of a specific conversation's run (Phase 3.1).
/// Idempotent. Requires both `conversation_id` and `run_id`; missing ids are a
/// no-op so a malformed Stop never kills another chat's run. A mismatched
/// conversation_id never cancels another chat's run.
#[tauri::command(rename_all = "snake_case")]
pub fn chat_cancel(
    app: tauri::AppHandle,
    conversation_id: Option<String>,
    run_id: Option<String>,
) -> AppResult<String> {
    let reg = app.state::<crate::commands::run::RunRegistry>();
    if let (Some(cid), Some(rid)) = (conversation_id.as_deref(), run_id.as_deref()) {
        if !cid.trim().is_empty() && crate::commands::run::valid_run_id(rid) {
            let _ = reg.cancel(cid, rid);
        }
    }
    // Missing/malformed ids → intentional no-op (never cancel_all).
    Ok(json!({ "ok": true }).to_string())
}

fn chat_send_blocking(
    app: &tauri::AppHandle,
    conversation_id: Option<String>,
    message: String,
    run_id: Option<String>,
) -> AppResult<String> {
    let text = message.trim().to_string();
    if text.is_empty() {
        return Err(AppError::Config("Type a message.".into()));
    }

    // Load or create the conversation first so we can key the run by its id.
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

    // Roadmap 3.6: at most 500 conversation files; when full, never auto-evict —
    // require the user to explicitly delete/archive. Only gates NEW conversations.
    let is_new_conv = conv_path(app, &conv.id)
        .map(|p| !p.exists())
        .unwrap_or(true);
    if is_new_conv && conversation_file_count(app) >= MAX_CONVERSATION_FILES {
        return Err(AppError::Config(
            "conversation limit reached (500) — delete old chats to start a new one".into(),
        ));
    }

    // Per-(conversation, run) ownership (Phase 3.1). One active run per chat;
    // independent conversations may run concurrently.
    let reg = app.state::<crate::commands::run::RunRegistry>();
    let run_id = run_id
        .filter(|r| crate::commands::run::valid_run_id(r))
        .unwrap_or_else(crate::commands::run::gen_run_id);
    let run = reg.start(&conv.id, &run_id).map_err(|e| match e {
        crate::commands::run::RunError::Duplicate => {
            AppError::Config("A chat turn is already running in this conversation.".into())
        }
        crate::commands::run::RunError::BadFormat => AppError::Config("Invalid run_id.".into()),
        crate::commands::run::RunError::BadConversation => {
            AppError::Config("Invalid conversation_id.".into())
        }
    })?;
    let cancel = run.cancel.clone();

    // Append the user message; set the title from the first message.
    if conv.title.is_empty() {
        conv.title = title_from(&text);
    }
    conv.messages.push(ChatMsg {
        role: "user".into(),
        content: text.clone(),
        card: None,
        llm_context: None,
        ts: iso_now(),
    });

    let settings = read_settings(app);
    let has_key = !settings.openrouter_api_key.trim().is_empty();

    let appended = if has_key {
        let cfg = fm_extract::LlmConfig {
            api_key: settings.openrouter_api_key.trim().to_string(),
            model: settings.model.trim().to_string(),
        };
        // Application-owned routing (Phase 1.1): recognized single intents
        // execute without asking the model to choose a tool. Research enters
        // the search→read→synthesize pipeline. Only DirectAnswer (and rare
        // multi-action fallthrough) uses the native tool-calling loop.
        match route_intent(&text, false) {
            Intent::Research => run_research_turn(app, &mut conv, &cfg, &cancel, &text, &run_id),
            Intent::DirectAnswer => run_llm_turn(app, &mut conv, &cfg, &cancel, &run_id),
            // Build / benchmark / filings / news / quote / PDF: deterministic
            // app execution via the keyword router. If the keyword router fails
            // to materialize args (should be rare — intent already matched),
            // fall through to the LLM as a safety net.
            _ => {
                let mut appended = Vec::new();
                if run_routed_tool(app, &mut conv, &mut appended, &text, &run_id) {
                    appended
                } else {
                    run_llm_turn(app, &mut conv, &cfg, &cancel, &run_id)
                }
            }
        }
    } else {
        run_fallback_turn(app, &mut conv, &cancel, &run_id)
    };

    conv.updated = iso_now();
    write_conversation(&conv_path(app, &conv.id)?, &conv)?;
    emit_chat(app, "chat_done", &conv.id, &run_id, json!({}));

    Ok(json!({ "conversation_id": conv.id, "messages": appended }).to_string())
}

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
        meta: TurnMeta,
    },
    /// Mid-stream network failure — whatever streamed is kept.
    Partial { content: String, error: String },
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

/// Shared call / token / deadline budgets for a chat turn (Phase 1.4).
#[derive(Clone, Copy, Debug)]
struct TurnBudgets {
    max_tool_rounds: usize,
    deadline_secs: u64,
    max_tokens: u64,
}

impl TurnBudgets {
    fn chat_default() -> Self {
        Self {
            max_tool_rounds: MAX_TOOL_ROUNDS,
            deadline_secs: CHAT_DEADLINE_SECS,
            max_tokens: MAX_TURN_TOKENS,
        }
    }
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

/// Build the OpenRouter chat request body (pure — unit-tested).
/// When `tools` is non-empty, also sets `tool_choice: "auto"` and
/// `parallel_tool_calls: false` (Phase 1.3: one tool call at a time).
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
        req["parallel_tool_calls"] = json!(false);
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

/// POST a streaming completion and consume the SSE body, emitting `chat_delta`
/// per content chunk. Cancellation is an explicit [`StreamOutcome::Cancelled`].
/// `timeout` is the remaining wall-clock budget for this request (shared turn
/// deadline minus elapsed); never a fresh full CHAT_DEADLINE_SECS.
///
/// Sync wrapper: drives the async stream on the Tauri runtime so the existing
/// blocking `spawn_blocking` chat path can call it.
fn openrouter_stream(
    app: &tauri::AppHandle,
    conv_id: &str,
    run_id: &str,
    cfg: &fm_extract::LlmConfig,
    req: &Value,
    cancel: &tokio_util::sync::CancellationToken,
    timeout: std::time::Duration,
) -> StreamOutcome {
    tauri::async_runtime::block_on(openrouter_stream_async(
        app, conv_id, run_id, cfg, req, cancel, timeout,
    ))
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

    // Race connect against cancel + overall remaining budget.
    let send_fut = client
        .post(OPENROUTER_CHAT_URL)
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
                return if content.is_empty() {
                    StreamOutcome::Failed(redact_provider_error(None, "no progress timeout"))
                } else {
                    StreamOutcome::Partial {
                        content,
                        error: redact_provider_error(None, "no progress timeout"),
                    }
                };
            }
            Ok(None) => break, // EOF
            Ok(Some(Err(e))) => {
                return if content.is_empty() {
                    StreamOutcome::Failed(redact_provider_error(None, &e.to_string()))
                } else {
                    StreamOutcome::Partial {
                        content,
                        error: redact_provider_error(None, &e.to_string()),
                    }
                };
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
                        emit_chat(app, "chat_delta", conv_id, run_id, json!({ "text": chunk }));
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

/// Build the LLM message array: system prompt + prior user/assistant text.
fn history_messages(conv: &Conversation) -> Vec<Value> {
    // Per-request context budget (roadmap 3.6): cap total llm_context at 40,000
    // chars, retaining the NEWEST *complete turns*. A turn is a user message plus
    // the assistant/card replies that follow it (until the next user message);
    // whole turns are dropped together so a reply never survives without its
    // prompt. When any older turn is dropped, a single `[older turns omitted]`
    // system note is inserted after the system prompt.
    const MAX_REQUEST_CHARS: usize = 40_000;

    // Render each message to its (role, capped-text) LLM form, or None.
    let rendered: Vec<Option<(&str, String)>> = conv
        .messages
        .iter()
        .map(|m| {
            let text = cap_context(m.llm_context.as_deref().unwrap_or(&m.content));
            match m.role.as_str() {
                "user" => Some(("user", text)),
                "assistant" if !text.trim().is_empty() => Some(("assistant", text)),
                _ => None, // card-only assistant messages carry no LLM text
            }
        })
        .collect();

    // Group into turns: each turn starts at a user message and includes the
    // following non-user messages. Leading non-user messages form turn 0.
    let mut turns: Vec<Vec<Value>> = Vec::new();
    let mut turn_cost: Vec<usize> = Vec::new();
    for (i, m) in conv.messages.iter().enumerate() {
        if m.role == "user" || turns.is_empty() {
            turns.push(Vec::new());
            turn_cost.push(0);
        }
        if let Some((role, text)) = &rendered[i] {
            let cost = text.chars().count();
            let idx = turns.len() - 1;
            turns[idx].push(json!({ "role": role, "content": text }));
            turn_cost[idx] += cost;
        }
    }

    // Keep whole turns newest-first until the budget is exhausted.
    let mut kept_rev: Vec<Vec<Value>> = Vec::new();
    let mut used = 0usize;
    let mut dropped = false;
    for idx in (0..turns.len()).rev() {
        if turns[idx].is_empty() {
            continue;
        }
        if used + turn_cost[idx] > MAX_REQUEST_CHARS && !kept_rev.is_empty() {
            dropped = true;
            break;
        }
        used += turn_cost[idx];
        kept_rev.push(std::mem::take(&mut turns[idx]));
    }

    let today = &iso_now()[..10];
    let system = format!(
        "{SYSTEM_PROMPT}\n\nToday's date is {today} (UTC). You do not have reliable knowledge of events after your training cutoff, so for anything current, recent, \"latest\", or time-bound, rely on tool results rather than your own memory."
    );
    let mut msgs = vec![json!({ "role": "system", "content": system })];
    if dropped {
        msgs.push(json!({ "role": "system", "content": "[older turns omitted]" }));
    }
    for turn in kept_rev.into_iter().rev() {
        msgs.extend(turn);
    }
    msgs
}

/// Per-message context cap (chars). Keeps a single message from dominating the
/// prompt; the per-request budget is enforced by the caller (Phase 3).
fn cap_context(s: &str) -> String {
    const MAX: usize = 8000;
    if s.chars().count() <= MAX {
        s.to_string()
    } else {
        let head: String = s.chars().take(MAX).collect();
        format!("{head}…")
    }
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
    cancel: &tokio_util::sync::CancellationToken,
    run_id: &str,
) -> Vec<ChatMsg> {
    use std::time::Instant;
    // Phase 1.3: only expose tools when the selected model has a *tested*
    // native_tools capability. Missing/failed probes → app routing + no tools.
    let settings = read_settings(app);
    let tools_ok = settings
        .model_capability
        .as_ref()
        .map(|c| c.model_id == cfg.model && c.native_tools)
        .unwrap_or(false);
    let tools = if tools_ok { tool_schemas() } else { Vec::new() };
    let budgets = TurnBudgets::chat_default();
    let started = Instant::now();
    let mut tokens_used: u64 = 0;
    let mut messages = history_messages(conv);
    let mut appended: Vec<ChatMsg> = Vec::new();
    let mut use_tools = tools_ok;
    // LLM response rounds (for the weak-model first-pass safety net only).
    let mut rounds = 0usize;
    // Count of individual tool executions this turn (not LLM rounds). An
    // adversarial model that packs many tool_calls into one response is still
    // capped at budgets.max_tool_rounds before the next run_tool.
    let mut tool_calls_used = 0usize;
    let no_tools: Vec<Value> = Vec::new();
    let user_msg = conv
        .messages
        .last()
        .map(|m| m.content.clone())
        .unwrap_or_default();

    /// Charge provider usage against the turn budget. Missing usage →
    /// conservative fallback so a silent provider cannot unbounded-spend.
    fn charge_usage(tokens_used: &mut u64, meta: &TurnMeta) {
        const FALLBACK: u64 = 2_000;
        let n = meta
            .usage
            .as_ref()
            .and_then(|u| u.get("total_tokens"))
            .and_then(|t| t.as_u64())
            .filter(|&n| n > 0)
            .unwrap_or(FALLBACK);
        *tokens_used = tokens_used.saturating_add(n);
    }

    loop {
        if cancel.is_cancelled() {
            push_assistant(conv, &mut appended, "(stopped)");
            break;
        }
        if started.elapsed().as_secs() >= budgets.deadline_secs {
            push_assistant(
                conv,
                &mut appended,
                "(stopped: chat deadline elapsed — try a shorter question or another model)",
            );
            break;
        }
        if tokens_used >= budgets.max_tokens {
            push_assistant(
                conv,
                &mut appended,
                "(stopped: token budget exhausted for this turn)",
            );
            break;
        }
        let remaining =
            std::time::Duration::from_secs(budgets.deadline_secs).saturating_sub(started.elapsed());
        if remaining.is_zero() {
            push_assistant(
                conv,
                &mut appended,
                "(stopped: chat deadline elapsed — try a shorter question or another model)",
            );
            break;
        }
        let req = build_chat_request(
            &cfg.model,
            &messages,
            if use_tools { &tools } else { &no_tools },
            true,
        );
        match openrouter_stream(app, &conv.id, run_id, cfg, &req, cancel, remaining) {
            StreamOutcome::ToolsUnsupported if use_tools => {
                // This model can't accept the tools param. Route the message
                // deterministically so a data query still runs a real tool
                // instead of a fabricated free-form answer; otherwise fall back
                // to a plain (tool-free) answer for non-data questions.
                if run_routed_tool(app, conv, &mut appended, &user_msg, run_id) {
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
                emit_chat(
                    app,
                    "chat_tool",
                    &conv.id,
                    run_id,
                    json!({ "name": "chat", "status": "error", "detail": err }),
                );
                push_assistant(
                    conv,
                    &mut appended,
                    "⚠ the model request failed — check your API key and model in Settings.",
                );
                break;
            }
            StreamOutcome::Partial { content, error } => {
                emit_chat(
                    app,
                    "chat_tool",
                    &conv.id,
                    run_id,
                    json!({ "name": "chat", "status": "error", "detail": error }),
                );
                let msg = if content.trim().is_empty() {
                    "⚠ connection lost — partial reply kept".to_string()
                } else {
                    format!("{content}\n\n⚠ connection lost — partial reply kept")
                };
                push_assistant(conv, &mut appended, &msg);
                break;
            }
            StreamOutcome::Cancelled { content } => {
                // Explicit Stop: terminate the whole turn. Keep any partial
                // streamed prose, then a visible stopped marker.
                if !content.trim().is_empty() {
                    push_assistant(conv, &mut appended, &content);
                }
                push_assistant(conv, &mut appended, "(stopped)");
                break;
            }
            StreamOutcome::Ok {
                content,
                tool_calls,
                meta,
            } => {
                charge_usage(&mut tokens_used, &meta);
                // Malformed terminal stream with no content and no tools —
                // stop visibly rather than spinning.
                if tool_calls.is_empty()
                    && content.trim().is_empty()
                    && meta.finish_reason.is_none()
                    && meta.sse_parse_errors > 0
                {
                    push_assistant(
                        conv,
                        &mut appended,
                        "(stopped: malformed stream — no usable content)",
                    );
                    break;
                }
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
                                emit_chat(app, "chat_reset", &conv.id, run_id, json!({}));
                                run_routed_tool(app, conv, &mut appended, &user_msg, run_id);
                                break;
                            }
                        }
                    }
                    let final_text = match meta.finish_reason.as_deref() {
                        Some("length") => format!(
                            "{content}\n\n⚠ (stopped: response truncated — the model hit its length limit)"
                        ),
                        Some("content_filter") => {
                            format!("{content}\n\n⚠ (stopped: filtered by the model)")
                        }
                        _ => content.clone(),
                    };
                    push_assistant(conv, &mut appended, &final_text);
                    break;
                }
                // Record the assistant's tool-call turn for LLM context.
                messages.push(assistant_tool_call_message(&content, &tool_calls));
                if !content.trim().is_empty() {
                    push_assistant(conv, &mut appended, &content);
                }
                for tc in &tool_calls {
                    if cancel.is_cancelled() {
                        push_assistant(conv, &mut appended, "(stopped)");
                        return appended;
                    }
                    if started.elapsed().as_secs() >= budgets.deadline_secs {
                        push_assistant(conv, &mut appended, "(stopped: chat deadline elapsed)");
                        return appended;
                    }
                    if tool_calls_used >= budgets.max_tool_rounds {
                        push_assistant(conv, &mut appended, "(stopped: tool limit reached)");
                        return appended;
                    }
                    tool_calls_used += 1;
                    let tool = ToolName::from_str(&tc.name);
                    emit_chat(
                        app,
                        "chat_tool",
                        &conv.id,
                        run_id,
                        json!({ "name": tc.name, "status": "start", "detail": "" }),
                    );
                    let (result_text, card) = match tool {
                        Some(t) => match validate_tool_args(t, &tc.arguments) {
                            // Strict decode + semantic validation replaces the old
                            // silent `{}` fallback. Invalid arguments never execute;
                            // the tool-error result is fed back for one repair.
                            Ok(args) => match run_tool(app, t, &args, &user_msg, &conv.id) {
                                Ok((txt, card)) => (txt, card),
                                Err(e) => (format!("Tool error: {e}"), error_card(&tc.name, &e)),
                            },
                            Err(e) => (
                                format!(
                                    "Invalid arguments for {}: {e}. Fix the arguments and call the tool again.",
                                    tc.name
                                ),
                                tool_contract_card(&tc.name, &e),
                            ),
                        },
                        None => (
                            format!("Unknown tool: {}", tc.name),
                            error_card(&tc.name, "unknown tool"),
                        ),
                    };
                    let status = if card["type"] == json!("error")
                        || card["type"] == json!("tool_contract")
                    {
                        "error"
                    } else {
                        "done"
                    };
                    emit_chat(
                        app,
                        "chat_tool",
                        &conv.id,
                        run_id,
                        json!({ "name": tc.name, "status": status, "card": card }),
                    );
                    push_card(conv, &mut appended, card);
                    messages.push(json!({
                        "role": "tool",
                        "tool_call_id": tc.id,
                        "content": result_text,
                    }));
                }
                // LLM "round" counter only for the weak-model first-pass safety net.
                rounds = rounds.saturating_add(1);
                if tool_calls_used >= budgets.max_tool_rounds {
                    push_assistant(conv, &mut appended, "(stopped: tool limit reached)");
                    break;
                }
                if tokens_used >= budgets.max_tokens {
                    push_assistant(
                        conv,
                        &mut appended,
                        "(stopped: token budget exhausted for this turn)",
                    );
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
        llm_context: None,
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
    run_id: &str,
) -> bool {
    // 1) Explicit artifact handle in the message: Analyze PDF [art-…]
    if let Some(id) = artifact_id_in_message(user_msg) {
        let label = user_msg
            .split_whitespace()
            .last()
            .unwrap_or("PDF")
            .to_string();
        let args = json!({ "artifact_id": id, "label": label });
        emit_chat(
            app,
            "chat_tool",
            &conv.id,
            run_id,
            json!({ "name": "analyze_pdf", "status": "start", "detail": "" }),
        );
        match run_tool(app, ToolName::AnalyzePdf, &args, user_msg, &conv.id) {
            Ok((_text, card)) => {
                emit_chat(
                    app,
                    "chat_tool",
                    &conv.id,
                    run_id,
                    json!({ "name": "analyze_pdf", "status": "done", "card": card }),
                );
                let intro = routed_intro(&card);
                emit_chat(
                    app,
                    "chat_delta",
                    &conv.id,
                    run_id,
                    json!({ "text": &intro }),
                );
                push_assistant(conv, appended, &intro);
                push_card(conv, appended, card);
            }
            Err(e) => {
                emit_chat(
                    app,
                    "chat_tool",
                    &conv.id,
                    run_id,
                    json!({ "name": "analyze_pdf", "status": "error", "detail": e }),
                );
                let msg = format!("Tool error: {e}");
                emit_chat(app, "chat_delta", &conv.id, run_id, json!({ "text": msg }));
                push_assistant(conv, appended, &msg);
            }
        }
        return true;
    }
    // 2) Quoted user PDF path → mint a conversation-scoped artifact handle first.

    if let Some(path) = quoted_pdf_path(user_msg) {
        if let Some(reg) = app.try_state::<crate::commands::artifacts::ArtifactRegistry>() {
            let label = std::path::Path::new(&path)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("PDF")
                .to_string();
            match reg.register_user_pdf(&path, &label, &conv.id) {
                Ok(id) => {
                    let args = json!({ "artifact_id": id, "label": label });
                    emit_chat(
                        app,
                        "chat_tool",
                        &conv.id,
                        run_id,
                        json!({ "name": "analyze_pdf", "status": "start", "detail": "" }),
                    );
                    match run_tool(app, ToolName::AnalyzePdf, &args, user_msg, &conv.id) {
                        Ok((_text, card)) => {
                            emit_chat(
                                app,
                                "chat_tool",
                                &conv.id,
                                run_id,
                                json!({ "name": "analyze_pdf", "status": "done", "card": card }),
                            );
                            let intro = routed_intro(&card);
                            emit_chat(
                                app,
                                "chat_delta",
                                &conv.id,
                                run_id,
                                json!({ "text": &intro }),
                            );
                            push_assistant(conv, appended, &intro);
                            push_card(conv, appended, card);
                        }
                        Err(e) => {
                            emit_chat(
                                app,
                                "chat_tool",
                                &conv.id,
                                run_id,
                                json!({ "name": "analyze_pdf", "status": "error", "detail": e }),
                            );
                            let msg = format!("Tool error: {e}");
                            emit_chat(app, "chat_delta", &conv.id, run_id, json!({ "text": msg }));
                            push_assistant(conv, appended, &msg);
                        }
                    }
                    return true;
                }
                Err(e) => {
                    push_assistant(conv, appended, &format!("Could not register PDF: {e}"));
                    return true;
                }
            }
        }
    }
    let Some((tool, args)) = route_fallback(user_msg) else {
        return false;
    };
    emit_chat(
        app,
        "chat_tool",
        &conv.id,
        run_id,
        json!({ "name": tool.as_str(), "status": "start", "detail": "" }),
    );
    match run_tool(app, tool, &args, user_msg, &conv.id) {
        Ok((_text, card)) => {
            emit_chat(
                app,
                "chat_tool",
                &conv.id,
                run_id,
                json!({ "name": tool.as_str(), "status": "done", "card": card }),
            );
            let intro = routed_intro(&card);
            emit_chat(
                app,
                "chat_delta",
                &conv.id,
                run_id,
                json!({ "text": &intro }),
            );
            push_assistant(conv, appended, &intro);
            push_card(conv, appended, card);
        }
        Err(e) => {
            emit_chat(
                app,
                "chat_tool",
                &conv.id,
                run_id,
                json!({ "name": tool.as_str(), "status": "error", "detail": e }),
            );
            let msg = format!("Tool error: {e}");
            emit_chat(app, "chat_delta", &conv.id, run_id, json!({ "text": msg }));
            push_assistant(conv, appended, &msg);
        }
    }
    true
}

/// A complete, self-contained lead-in for a deterministically-routed tool card.
/// Never ends in a dangling colon: the card is the answer, so this reads as a
/// finished sentence whether it renders above (on reload) or below (live) the
/// card. Counts come from the card so an empty result reads as an honest miss
/// rather than a broken, truncated reply.
fn routed_intro(card: &Value) -> String {
    match card.get("type").and_then(|t| t.as_str()) {
        Some("news") => {
            let n = card
                .get("items")
                .and_then(|v| v.as_array())
                .map(|a| a.len())
                .unwrap_or(0);
            match n {
                0 => "I didn't find any recent headlines for that — try broadening the topic or widening the time window.".to_string(),
                1 => "I found 1 recent headline on this topic.".to_string(),
                _ => format!("I found {n} recent headlines on this topic."),
            }
        }
        Some("search") => {
            let n = card
                .get("hits")
                .and_then(|v| v.as_array())
                .map(|a| a.len())
                .unwrap_or(0);
            match n {
                0 => "I didn't find any web results for that.".to_string(),
                1 => "I found 1 web result for your query.".to_string(),
                _ => format!("I found {n} web results for your query."),
            }
        }
        _ => "Here's what I found.".to_string(),
    }
}

fn push_card(conv: &mut Conversation, appended: &mut Vec<ChatMsg>, card: Value) {
    let msg = ChatMsg {
        role: "assistant".into(),
        content: String::new(),
        card: Some(card),
        llm_context: None,
        ts: iso_now(),
    };
    conv.messages.push(msg.clone());
    appended.push(msg);
}

/// Push an assistant message carrying a compact `llm_context` (used for the
/// research answer, so follow-up turns see a summary, never raw pages).
fn push_assistant_with_context(
    conv: &mut Conversation,
    appended: &mut Vec<ChatMsg>,
    content: &str,
    llm_context: Option<String>,
) {
    let msg = ChatMsg {
        role: "assistant".into(),
        content: strip_control_tokens(content),
        card: None,
        llm_context,
        ts: iso_now(),
    };
    conv.messages.push(msg.clone());
    appended.push(msg);
}

/// Persist a completed research run's read ledger for a possible retry
/// (roadmap 3.6). Bounded/TTL'd by [`ResearchRunState`].
fn store_research_run(
    app: &tauri::AppHandle,
    run_id: &str,
    question: &str,
    sources: &[fm_research::research::SourceRecord],
) {
    use crate::commands::research_state::{ResearchRunState, StoredRun};
    let state = app.state::<ResearchRunState>();
    state.put(
        run_id,
        StoredRun {
            question: question.to_string(),
            plan: Vec::new(),
            ledger: sources.to_vec(),
            backend_override: None,
            resume_from: None,
            parent: None,
        },
        crate::commands::model::now_secs(),
    );
}

/// Application-controlled research turn: run the search→read→synthesize pipeline
/// and render a cited answer or honest digest. A Reading/Synthesizing retry seed
/// (from [`research_retry`]) drives a `SeededBackend` that skips network
/// search/read and re-synthesizes the stored ledger.
fn run_research_turn(
    app: &tauri::AppHandle,
    conv: &mut Conversation,
    cfg: &fm_extract::LlmConfig,
    cancel: &tokio_util::sync::CancellationToken,
    user_msg: &str,
    run_id: &str,
) -> Vec<ChatMsg> {
    use crate::commands::research::{run_research, HttpBackend, OpenRouterSynthesizer};
    use fm_research::machine::{Action, ResearchBudgets, ResearchMachine};
    use fm_research::research::{ResearchDepth, ResearchMode, ResearchOutput, ResearchToolArgs};

    let mut appended: Vec<ChatMsg> = Vec::new();
    let depth = ResearchDepth::Standard;
    let tickers = ticker_tokens(user_msg);
    let (mode, deal_target, deal_acquirer) = select_research_mode(user_msg, &tickers);
    let mut request = ResearchToolArgs {
        question: user_msg.to_string(),
        mode,
        tickers,
        depth,
    }
    .into_request(user_msg);
    // Application fills deal parties from the original user text — never the model.
    if mode == ResearchMode::Deal {
        request.target = Some(deal_target).filter(|s| !s.trim().is_empty());
        request.acquirer = Some(deal_acquirer).filter(|s| !s.trim().is_empty());
    }
    let budgets = ResearchBudgets::from_depth(depth);
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
    // Phase 1.3: strict structured synthesis only when Test model certified it.
    let strict_json = read_settings(app)
        .model_capability
        .as_ref()
        .map(|c| c.model_id == cfg.model && c.strict_json)
        .unwrap_or(false);
    let synth = OpenRouterSynthesizer {
        api_key: cfg.api_key.clone(),
        model: cfg.model.clone(),
        strict_json,
        request: request.clone(),
    };

    emit_chat(
        app,
        "chat_tool",
        &conv.id,
        run_id,
        json!({ "name": "research", "status": "start", "detail": "Researching…" }),
    );
    // Per-stage progress → a dedicated event that only updates the polite
    // progress region (never spawns tool-status nodes). Phase 4.2.
    let progress = |label: &str| {
        emit_chat(
            app,
            "chat_progress",
            &conv.id,
            run_id,
            json!({ "text": label }),
        );
    };
    // Consume a Reading/Synthesizing retry seed: skip network search/read and
    // re-synthesize the stored ledger via a SeededBackend (roadmap 3.6).
    let seed = app
        .state::<crate::commands::research_state::ResearchRunState>()
        .get(run_id, crate::commands::model::now_secs());
    let use_seed = seed
        .as_ref()
        .map(|s| {
            matches!(
                s.resume_from,
                Some(crate::commands::research_state::RetryPhase::Reading)
                    | Some(crate::commands::research_state::RetryPhase::Synthesizing)
            ) && !s.ledger.is_empty()
        })
        .unwrap_or(false);
    let terminal = if use_seed {
        let seeded = crate::commands::research::SeededBackend {
            ledger: seed.expect("seed present").ledger,
        };
        tauri::async_runtime::block_on(run_research(
            machine, request, &seeded, &synth, cancel, &progress,
        ))
    } else {
        tauri::async_runtime::block_on(run_research(
            machine, request, &backend, &synth, cancel, &progress,
        ))
    };

    match terminal {
        Action::Done(ResearchOutput::Answer(a)) => {
            let card = json!({ "type": "research_answer", "answer": serde_json::to_value(&a).unwrap_or(Value::Null) });
            emit_chat(
                app,
                "chat_tool",
                &conv.id,
                run_id,
                json!({ "name": "research", "status": "done", "card": card.clone() }),
            );
            // Inline [S#] markers on each paragraph + a consulted-source list, so
            // citations are visible now (the rich card renders in Phase 4 UI).
            let cite_markers = |p: &fm_research::research::CitedParagraph| -> String {
                let mut ids: Vec<&str> = p.citations.iter().map(|c| c.source_id.as_str()).collect();
                ids.dedup();
                if ids.is_empty() {
                    String::new()
                } else {
                    format!(" [{}]", ids.join(", "))
                }
            };
            let mut prose = format!("{}{}", a.summary.text, cite_markers(&a.summary));
            for sec in &a.sections {
                prose.push_str(&format!("\n\n**{}**\n", sec.heading));
                for p in &sec.paragraphs {
                    prose.push_str(&format!("{}{}\n", p.text, cite_markers(p)));
                }
            }
            if !a.sources.is_empty() {
                prose.push_str("\n\nSources:");
                for s in &a.sources {
                    prose.push_str(&format!("\n- {} {} ({:?})", s.id, s.domain, s.status));
                }
            }
            push_assistant_with_context(conv, &mut appended, &prose, Some(prose.clone()));
            push_card(conv, &mut appended, card);
            // Store the read ledger for a possible retry (roadmap 3.6).
            store_research_run(app, run_id, &a.question, &a.sources);
        }
        Action::Done(ResearchOutput::Digest(d)) => {
            let card = json!({ "type": "research_digest", "digest": serde_json::to_value(&d).unwrap_or(Value::Null) });
            emit_chat(
                app,
                "chat_tool",
                &conv.id,
                run_id,
                json!({ "name": "research", "status": "done", "card": card.clone() }),
            );
            let note = format!("Source digest — no synthesis. {}", d.limitations.join(" "));
            push_assistant(conv, &mut appended, &note);
            push_card(conv, &mut appended, card);
            // A digest means synthesis was impossible; store the run (empty read
            // ledger) so a retry re-runs fresh rather than reusing nothing.
            store_research_run(app, run_id, &d.question, &[]);
        }
        Action::Cancelled => push_assistant(conv, &mut appended, "(stopped)"),
        Action::Error { code } => {
            emit_chat(
                app,
                "chat_tool",
                &conv.id,
                run_id,
                json!({ "name": "research", "status": "error", "detail": code.clone() }),
            );
            push_assistant(conv, &mut appended, &format!("Research error: {code}"));
        }
        // Non-terminal actions never escape the driver.
        _ => {}
    }
    appended
}

/// Pure research-mode router. Ticker-bearing questions pick a single/multi-company
/// mode by intent precedence (filing → earnings → comparison → company → web); a
/// ticker-free M&A question with a clean parsed target routes to Deal. Returns the
/// mode plus the parsed deal target/acquirer (empty for non-deal modes).
fn select_research_mode(
    user_msg: &str,
    tickers: &[String],
) -> (fm_research::research::ResearchMode, String, String) {
    use fm_research::research::ResearchMode;
    fn is_filing_intent(m: &str) -> bool {
        let m = m.to_lowercase();
        m.contains("10-k")
            || m.contains("10-q")
            || m.contains("8-k")
            || m.contains("20-f")
            || m.contains("annual report")
            || m.contains("quarterly report")
            || m.contains("risk factor")
            || m.contains("md&a")
            || m.contains("proxy statement")
            || (m.contains("filing") && (m.contains("sec") || m.contains("item")))
    }
    fn is_company_brief_intent(m: &str) -> bool {
        let m = m.to_lowercase();
        m.contains("company brief")
            || m.contains("company profile")
            || m.contains("company overview")
            || m.contains("company snapshot")
            || m.contains("brief on")
            || m.contains("profile of")
            || m.contains("overview of")
    }
    fn is_earnings_intent(m: &str) -> bool {
        let m = m.to_lowercase();
        m.contains("earnings")
            || m.contains("quarterly results")
            || m.contains("latest quarter")
            || m.contains("last quarter")
            || m.contains("guidance")
            || (m.contains("beat") && m.contains("estimate"))
    }
    fn is_comparison_intent(m: &str) -> bool {
        let m = m.to_lowercase();
        m.contains(" vs ")
            || m.contains(" versus ")
            || m.contains("compare")
            || m.contains("comparison")
    }
    fn is_deal_intent(m: &str) -> bool {
        let m = m.to_lowercase();
        m.contains("acquire")
            || m.contains("acquisition")
            || m.contains("merger")
            || m.contains("merges with")
            || m.contains("buyout")
            || m.contains("takeover")
    }
    // Guards parse_ma_query's greedy fallbacks: a real target is short and not an
    // interrogative fragment.
    fn deal_target_is_sane(t: &str) -> bool {
        let t = t.trim();
        if t.is_empty() || t.split_whitespace().count() > 5 {
            return false;
        }
        let first = t.split_whitespace().next().unwrap_or("").to_lowercase();
        !matches!(
            first.as_str(),
            "how"
                | "what"
                | "why"
                | "when"
                | "where"
                | "does"
                | "do"
                | "is"
                | "are"
                | "was"
                | "were"
                | "the"
                | "a"
                | "an"
        )
    }
    let (deal_target, deal_acquirer) = if tickers.is_empty() && is_deal_intent(user_msg) {
        fm_research::agent::parse_ma_query(user_msg, "")
    } else {
        (String::new(), String::new())
    };
    let mode = if !tickers.is_empty() {
        if is_filing_intent(user_msg) {
            ResearchMode::Filing
        } else if is_earnings_intent(user_msg) {
            ResearchMode::Earnings
        } else if tickers.len() >= 2 && is_comparison_intent(user_msg) {
            ResearchMode::Comparison
        } else if is_company_brief_intent(user_msg) {
            ResearchMode::Company
        } else {
            ResearchMode::Web
        }
    } else if deal_target_is_sane(&deal_target) {
        ResearchMode::Deal
    } else {
        ResearchMode::Web
    };
    (mode, deal_target, deal_acquirer)
}

// ---------------------------------------------------------------------------
// No-key fallback router
// ---------------------------------------------------------------------------

fn run_fallback_turn(
    app: &tauri::AppHandle,
    conv: &mut Conversation,
    _cancel: &tokio_util::sync::CancellationToken,
    run_id: &str,
) -> Vec<ChatMsg> {
    let user = conv
        .messages
        .last()
        .map(|m| m.content.clone())
        .unwrap_or_default();
    let mut appended: Vec<ChatMsg> = Vec::new();
    if !run_routed_tool(app, conv, &mut appended, &user, run_id) {
        emit_chat(
            app,
            "chat_delta",
            &conv.id,
            run_id,
            json!({ "text": FALLBACK_HELP }),
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

    // 0. Quoted local .pdf path: no longer materializes a raw-path tool arg.
    // Intent routing still maps these to AnalyzePdf; the app registers a
    // conversation-scoped artifact handle before execution.
    // (route_fallback is pure — handle minting happens in run_routed_tool.)
    // 1. benchmark / comps with >= 2 tickers.
    if has(&["benchmark", "compare", "peers", "comps"])
        && tickers.len() >= 2
        && !m.contains("build")
    {
        return Some((ToolName::BenchmarkPeers, json!({ "tickers": tickers })));
    }
    // 2. build / model / dcf with >= 1 ticker (extra tickers → peers; case tags).
    if has(&["build", "model", "dcf", "3-statement", "three statement"]) && !tickers.is_empty() {
        let mut args = json!({ "ticker": tickers[0] });
        let peers: Vec<String> = tickers.iter().skip(1).cloned().collect();
        if (has(&["with peers", " vs ", "versus", " with "]) || m.contains("peers"))
            && !peers.is_empty()
        {
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
    if has(&[
        "10-k",
        "10k",
        "10-q",
        "10q",
        "annual report",
        "md&a",
        "mda",
        "risk factors",
    ]) && !tickers.is_empty()
    {
        let form = if has(&["10-q", "10q"]) {
            "10-Q"
        } else {
            "10-K"
        };
        let mut args = json!({ "ticker": tickers[0], "form": form });
        if has(&["risk factors"]) {
            args["item"] = json!("1A");
        } else if has(&[
            "md&a",
            "mda",
            "management discussion",
            "discussion and analysis",
        ]) {
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
/// Extract an `art-…` handle from a user message of the form
/// `Analyze PDF [art-…]`. Returns None if no valid 36-char handle is present.
fn artifact_id_in_message(msg: &str) -> Option<String> {
    // Find `art-` then take 32 hex chars after it (total 36).
    let bytes = msg.as_bytes();
    let needle = b"art-";
    let mut i = 0;
    while i + 36 <= bytes.len() {
        if &bytes[i..i + 4] == needle {
            let cand = &msg[i..i + 36];
            if cand.chars().skip(4).all(|c| c.is_ascii_hexdigit()) {
                return Some(cand.to_string());
            }
        }
        i += 1;
    }
    None
}

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
    Research,
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
            ToolName::Research => "research",
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
            "research" => ToolName::Research,
            _ => return None,
        })
    }
}

// ---------------------------------------------------------------------------
// Typed intent router + argument registry (Phase 1.1 / 1.2)
// ---------------------------------------------------------------------------

/// The typed intent the application resolves for a turn. Explicit factual /
/// current / entity / numeric questions route to [`Intent::Research`] rather than
/// depending on a model to call `web_search`, or falling through to free-form
/// prose. Raw-artifact intents require explicit "show source text" / "list
/// headlines" wording.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Intent {
    AnalyzePdf,
    ReadFiling,
    Filings,
    News,
    BuildModel,
    BenchmarkPeers,
    Research,
    Quote,
    DirectAnswer,
}

/// Resolve a user message to a typed [`Intent`] with explicit precedence:
/// local-PDF action; explicit raw-artifact (filing text / news list / filings
/// list); unambiguous artifact command (build / benchmark); any current /
/// entity / numeric / factual question to Research; then DirectAnswer.
pub(crate) fn route_intent(msg: &str, has_local_pdf: bool) -> Intent {
    let lm = msg.to_lowercase();
    let tickers = ticker_tokens(msg);

    // 1. A local PDF attachment / quoted .pdf path is a file action.
    if has_local_pdf || quoted_pdf_path(msg).is_some() {
        return Intent::AnalyzePdf;
    }
    // 2. Explicit "read/show/open the source text of a filing".
    let wants_read = lm.contains("read") || lm.contains("show") || lm.contains("open");
    let filing_sig = lm.contains("10-k")
        || lm.contains("10-q")
        || lm.contains("risk factor")
        || lm.contains("md&a")
        || lm.contains("item 1a")
        || lm.contains("item 7")
        || (lm.contains("filing") && (lm.contains("text") || lm.contains("source")));
    if wants_read && filing_sig && !tickers.is_empty() {
        return Intent::ReadFiling;
    }
    // 3. Explicit "list the filings".
    if lm.contains("filing") && (lm.contains("list") || lm.contains("recent")) {
        return Intent::Filings;
    }
    // 4. Explicit "news / headlines".
    if lm.contains("news") || lm.contains("headline") {
        return Intent::News;
    }
    // 5. Any current/entity/numeric/factual QUESTION → application Research,
    // taking precedence over the non-current artifact commands below (a
    // current-flavored "build the latest NVDA model" or "compare KO and PEP" is
    // research; a bare "build AAPL model" is an artifact command).
    if is_research_question(&lm, &tickers) {
        return Intent::Research;
    }
    // 6. Unambiguous non-current artifact commands.
    if lm.contains("build") && !tickers.is_empty() {
        return Intent::BuildModel;
    }
    if (lm.contains("benchmark") || lm.contains("comps")) && tickers.len() >= 2 {
        return Intent::BenchmarkPeers;
    }
    // 7. A bare "get the quote/price" raw lookup (no question framing).
    if (lm.contains("quote") || lm.contains("share price")) && !tickers.is_empty() {
        return Intent::Quote;
    }
    // 8. Nothing data-bearing recognized.
    Intent::DirectAnswer
}

/// Whether a message is a current/entity/numeric/factual question that the
/// application should research, rather than a bare definitional question left to
/// the model.
fn is_research_question(lm: &str, tickers: &[String]) -> bool {
    const RESEARCH_VERBS: &[&str] = &[
        "research",
        "analyze",
        "analyse",
        "compare",
        "investment case",
        "outlook",
        "catalyst",
        "thesis",
        "should i",
        "how is",
        "how are",
        "what's happening",
        "tell me about",
        "deal",
        "merger",
        "acquisition",
        "earnings",
        "guidance",
    ];
    const CURRENT_SIGNALS: &[&str] = &[
        "current",
        "latest",
        "today",
        "now",
        "recent",
        "this year",
        "price",
        "consensus",
    ];
    let has_verb = RESEARCH_VERBS.iter().any(|k| lm.contains(k));
    let has_current = CURRENT_SIGNALS.iter().any(|k| lm.contains(k));
    let is_question = lm.contains('?')
        || lm.starts_with("what")
        || lm.starts_with("why")
        || lm.starts_with("how")
        || lm.starts_with("is ")
        || lm.starts_with("are ")
        || lm.starts_with("does ")
        || lm.starts_with("who ");
    // A research verb always qualifies. Otherwise a question or current signal
    // qualifies only when it references a concrete entity (ticker present).
    has_verb || ((is_question || has_current) && !tickers.is_empty())
}

// ── Typed argument structs (strict: reject unknown/missing/malformed) ────────

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
#[allow(dead_code)]
enum ArgPeriod {
    Annual,
    Quarter,
    Semi,
    Ltm,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
#[allow(dead_code)]
enum ArgCase {
    Base,
    Upside,
    Downside,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
struct BuildModelArgs {
    ticker: String,
    #[serde(default)]
    period: Option<ArgPeriod>,
    #[serde(default)]
    years: Option<i64>,
    #[serde(default)]
    skip_review: Option<bool>,
    #[serde(default)]
    peers: Option<Vec<String>>,
    #[serde(default)]
    case: Option<ArgCase>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
struct BenchmarkArgs {
    tickers: Vec<String>,
    #[serde(default)]
    period: Option<ArgPeriod>,
    #[serde(default)]
    multiples: Option<bool>,
    #[serde(default)]
    usd: Option<bool>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct QueryArgs {
    query: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadPageArgs {
    url: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
struct NewsArgs {
    query: String,
    #[serde(default)]
    limit: Option<i64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TickerArgs {
    ticker: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
struct ListFilingsArgs {
    ticker: String,
    #[serde(default)]
    form: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
struct ReadFilingArgs {
    ticker: String,
    #[serde(default)]
    form: Option<String>,
    #[serde(default)]
    item: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AnalyzePdfArgs {
    /// Opaque handle from the trusted PDF picker / app-side user-path registration.
    artifact_id: String,
    #[serde(default)]
    #[allow(dead_code)]
    label: String,
}

fn require_ticker(s: &str) -> Result<(), String> {
    let t = s.trim();
    if t.is_empty() {
        return Err("ticker is empty".into());
    }
    if !is_ticker_like(t) {
        return Err(format!("'{t}' is not a valid ticker"));
    }
    Ok(())
}

fn require_query(s: &str) -> Result<(), String> {
    if s.trim().is_empty() {
        return Err("query is empty".into());
    }
    if s.chars().count() > 2000 {
        return Err("query is too long (max 2000 chars)".into());
    }
    Ok(())
}

/// Strictly decode + semantically validate a tool call's raw argument JSON.
/// Replaces the old silent `{}` fallback: malformed JSON, unknown fields,
/// missing required fields, bad enums, out-of-range numbers, and invalid
/// ticker/URL syntax are all rejected BEFORE the tool executes. On success
/// returns the parsed argument object for the executor.
fn validate_tool_args(tool: ToolName, raw: &str) -> Result<Value, String> {
    let raw = raw.trim();
    let raw = if raw.is_empty() { "{}" } else { raw };
    let v: Value =
        serde_json::from_str(raw).map_err(|e| format!("arguments are not valid JSON: {e}"))?;
    let decode = |r: &str| -> Result<(), String> {
        match tool {
            ToolName::BuildModel => {
                let a: BuildModelArgs = serde_json::from_str(r).map_err(dec_err)?;
                require_ticker(&a.ticker)?;
                if let Some(y) = a.years {
                    if !(1..=10).contains(&y) {
                        return Err(format!("years must be 1-10, got {y}"));
                    }
                }
                if let Some(ps) = &a.peers {
                    if ps.len() > 20 {
                        return Err("too many peers (max 20)".into());
                    }
                    for p in ps {
                        require_ticker(p)?;
                    }
                }
            }
            ToolName::BenchmarkPeers => {
                let a: BenchmarkArgs = serde_json::from_str(r).map_err(dec_err)?;
                if a.tickers.is_empty() {
                    return Err("tickers is empty".into());
                }
                if a.tickers.len() > 30 {
                    return Err("too many tickers (max 30)".into());
                }
                for t in &a.tickers {
                    require_ticker(t)?;
                }
            }
            ToolName::WebSearch | ToolName::ResearchDeal => {
                let a: QueryArgs = serde_json::from_str(r).map_err(dec_err)?;
                require_query(&a.query)?;
            }
            ToolName::GetNews => {
                let a: NewsArgs = serde_json::from_str(r).map_err(dec_err)?;
                require_query(&a.query)?;
                if let Some(l) = a.limit {
                    if !(1..=50).contains(&l) {
                        return Err(format!("limit must be 1-50, got {l}"));
                    }
                }
            }
            ToolName::ReadPage => {
                let a: ReadPageArgs = serde_json::from_str(r).map_err(dec_err)?;
                fm_research::validate_request_url(&a.url)
                    .map_err(|e| format!("invalid url: {e:?}"))?;
            }
            ToolName::GetQuote => {
                let a: TickerArgs = serde_json::from_str(r).map_err(dec_err)?;
                require_ticker(&a.ticker)?;
            }
            ToolName::ListFilings => {
                let a: ListFilingsArgs = serde_json::from_str(r).map_err(dec_err)?;
                require_ticker(&a.ticker)?;
            }
            ToolName::ReadFiling => {
                let a: ReadFilingArgs = serde_json::from_str(r).map_err(dec_err)?;
                require_ticker(&a.ticker)?;
            }
            ToolName::AnalyzePdf => {
                let a: AnalyzePdfArgs = serde_json::from_str(r).map_err(dec_err)?;
                if a.artifact_id.trim().is_empty() {
                    return Err("artifact_id is empty".into());
                }
                if !a.artifact_id.starts_with("art-") || a.artifact_id.len() != 36 {
                    return Err("artifact_id must be a registered handle".into());
                }
            }
            ToolName::Research => {
                // Compact schema is question/mode/tickers/depth only. Mode-specific
                // fields (deal target/acquirer) are filled by the application from
                // the original user text at execution — not validated here.
                let a: fm_research::research::ResearchToolArgs =
                    serde_json::from_str(r).map_err(dec_err)?;
                if a.question.trim().is_empty() {
                    return Err("question is empty".into());
                }
                if a.tickers.len() > 8 {
                    return Err("too many tickers (max 8)".into());
                }
                for t in &a.tickers {
                    require_ticker(t)?;
                }
            }
        }
        Ok(())
    };
    decode(raw)?;
    Ok(v)
}

fn dec_err(e: serde_json::Error) -> String {
    format!("invalid arguments: {e}")
}

/// A `tool_contract` failure card — surfaced when a tool call's arguments fail
/// strict validation, so nothing executes on malformed input.
fn tool_contract_card(tool: &str, message: &str) -> Value {
    json!({ "type": "tool_contract", "tool": tool, "message": message })
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
            "get_news",
            "Fetch recent news headlines for a ticker or query.",
            json!({
                "type": "object",
                "properties": { "query": { "type": "string" }, "limit": { "type": "integer" } },
                "required": ["query"]
            }),
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
            "Analyze a local annual-report PDF (registered via the file picker) into a 3-statement + DCF model. Requires an OpenRouter API key and a picker-minted artifact_id — never a raw filesystem path.",
            json!({
                "type": "object", "additionalProperties": false,
                "properties": {
                    "artifact_id": { "type": "string", "description": "Opaque handle from pick_pdf_artifact" },
                    "label": { "type": "string", "description": "Company/ticker label for the workbook" }
                },
                "required": ["artifact_id"]
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
/// `user_msg` is the ORIGINAL user text — the `research` tool normalizes against
/// it rather than the model's rewritten `question` argument.
/// `conversation_id` is a trusted app value used only for artifact ownership.
fn run_tool(
    app: &tauri::AppHandle,
    tool: ToolName,
    args: &Value,
    user_msg: &str,
    conversation_id: &str,
) -> Result<(String, Value), String> {
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
        ToolName::AnalyzePdf => tool_analyze_pdf(app, args, conversation_id),
        ToolName::Research => tool_research(app, args, user_msg),
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

    let ta: ResearchToolArgs =
        serde_json::from_value(args.clone()).map_err(|e| format!("invalid arguments: {e}"))?;
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
        assert_eq!(req["parallel_tool_calls"], json!(false));
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
    fn turn_budgets_default_covers_rounds_deadline_tokens() {
        let b = TurnBudgets::chat_default();
        assert_eq!(b.max_tool_rounds, MAX_TOOL_ROUNDS);
        assert_eq!(b.deadline_secs, CHAT_DEADLINE_SECS);
        assert_eq!(b.max_tokens, MAX_TURN_TOKENS);
        assert!(b.max_tokens > 0);
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
        assert_eq!(
            route_fallback("the figma adobe merger").unwrap().0,
            ToolName::ResearchDeal
        );
        assert_eq!(
            route_fallback("search the web for margins").unwrap().0,
            ToolName::WebSearch
        );
        assert_eq!(route_fallback("quote AAPL").unwrap().0, ToolName::GetQuote);
        assert_eq!(
            route_fallback("show filings for AAPL").unwrap().0,
            ToolName::ListFilings
        );
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
    fn route_analyze_pdf_quoted_path_no_longer_in_fallback() {
        let hit = route_fallback("Analyze the filing PDF at \"C:/tmp/annual.pdf\" for TESTCO");
        assert!(
            hit.is_none() || hit.map(|(t, _)| t != ToolName::AnalyzePdf).unwrap_or(true),
            "raw-path AnalyzePdf must not come from route_fallback"
        );
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
                ChatMsg {
                    role: "user".into(),
                    content: "build AAPL".into(),
                    card: None,
                    llm_context: None,
                    ts: iso_now(),
                },
                ChatMsg {
                    role: "assistant".into(),
                    content: String::new(),
                    card: Some(json!({ "type": "model" })),
                    llm_context: None,
                    ts: iso_now(),
                },
            ],
        };
        write_conversation(&path, &conv).unwrap();
        let back = read_conversation(&path).unwrap();
        assert_eq!(back.id, "abc-0001");
        assert_eq!(back.messages.len(), 2);
        assert_eq!(
            back.messages[1].card.as_ref().unwrap()["type"],
            json!("model")
        );
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

    // ── Phase 1.1 typed intent router ──────────────────────────────────────
    #[test]
    fn router_factual_questions_go_to_research() {
        assert_eq!(
            route_intent("Research the current investment case for Nvidia.", false),
            Intent::Research
        );
        assert_eq!(
            route_intent("What is the current share price of AAPL?", false),
            Intent::Research
        );
        assert_eq!(
            route_intent("compare KO and PEP on margins", false),
            Intent::Research
        );
        assert_eq!(
            route_intent("analyze the Adobe Figma deal", false),
            Intent::Research
        );
        // A current-flavored build request is research-first per the precedence.
        assert_eq!(
            route_intent("build the latest NVDA model", false),
            Intent::Research
        );
    }

    #[test]
    fn router_artifact_commands() {
        assert_eq!(
            route_intent("build a dcf model for AAPL", false),
            Intent::BuildModel
        );
        assert_eq!(
            route_intent("benchmark AAPL, MSFT, NVDA", false),
            Intent::BenchmarkPeers
        );
        assert_eq!(
            route_intent("analyze the filing at \"C:/tmp/x.pdf\"", false),
            Intent::AnalyzePdf
        );
        assert_eq!(route_intent("anything", true), Intent::AnalyzePdf);
    }

    #[test]
    fn router_raw_artifact_requires_explicit_wording() {
        assert_eq!(
            route_intent("read the risk factors in TSLA's latest 10-K", false),
            Intent::ReadFiling
        );
        assert_eq!(
            route_intent("list recent filings for AAPL", false),
            Intent::Filings
        );
        assert_eq!(route_intent("news NVDA", false), Intent::News);
    }

    #[test]
    fn router_definitional_stays_direct() {
        assert_eq!(route_intent("what is EBITDA", false), Intent::DirectAnswer);
        assert_eq!(
            route_intent("tell me a joke about accounting", false),
            Intent::DirectAnswer
        );
    }

    // ── Phase 1.2 strict argument validation (no silent {}) ────────────────
    #[test]
    fn valid_args_decode() {
        assert!(validate_tool_args(ToolName::BuildModel, r#"{"ticker":"AAPL","years":5}"#).is_ok());
        assert!(
            validate_tool_args(ToolName::BenchmarkPeers, r#"{"tickers":["AAPL","MSFT"]}"#).is_ok()
        );
        assert!(validate_tool_args(ToolName::WebSearch, r#"{"query":"nvidia outlook"}"#).is_ok());
        assert!(
            validate_tool_args(ToolName::ReadPage, r#"{"url":"https://example.com/x"}"#).is_ok()
        );
        assert!(validate_tool_args(
            ToolName::Research,
            r#"{"question":"What is AAPL revenue?","mode":"company","tickers":["AAPL"],"depth":"quick"}"#
        )
        .is_ok());
    }

    #[test]
    fn malformed_json_is_rejected_not_coerced_to_empty() {
        let e = validate_tool_args(ToolName::BuildModel, "{not json").unwrap_err();
        assert!(e.contains("not valid JSON"), "got: {e}");
    }

    #[test]
    fn missing_required_and_unknown_fields_rejected() {
        assert!(
            validate_tool_args(ToolName::BuildModel, "{}").is_err(),
            "missing ticker"
        );
        assert!(
            validate_tool_args(ToolName::BuildModel, r#"{"ticker":"AAPL","bogus":1}"#).is_err(),
            "unknown field"
        );
    }

    #[test]
    fn semantic_validation_rejects_bad_values() {
        assert!(validate_tool_args(ToolName::BuildModel, r#"{"ticker":"not a ticker"}"#).is_err());
        assert!(
            validate_tool_args(ToolName::BuildModel, r#"{"ticker":"AAPL","years":99}"#).is_err()
        );
        assert!(validate_tool_args(
            ToolName::BuildModel,
            r#"{"ticker":"AAPL","period":"weekly"}"#
        )
        .is_err());
        assert!(validate_tool_args(ToolName::BenchmarkPeers, r#"{"tickers":[]}"#).is_err());
        assert!(
            validate_tool_args(ToolName::AnalyzePdf, r#"{"path":"/tmp/x.pdf","label":"X"}"#)
                .is_err(),
            "raw path rejected"
        );
        assert!(
            validate_tool_args(
                ToolName::AnalyzePdf,
                r#"{"artifact_id":"art-0123456789abcdef0123456789abcdef"}"#
            )
            .is_ok(),
            "valid artifact_id accepted"
        );
        assert!(
            validate_tool_args(ToolName::ReadPage, r#"{"url":"http://127.0.0.1/"}"#).is_err(),
            "SSRF target"
        );
        assert!(
            validate_tool_args(
                ToolName::Research,
                r#"{"question":"","mode":"web","tickers":[],"depth":"quick"}"#
            )
            .is_err(),
            "empty question"
        );
        assert!(
            validate_tool_args(
                ToolName::Research,
                r#"{"question":"x","mode":"web","tickers":["not a ticker"],"depth":"quick"}"#
            )
            .is_err(),
            "bad ticker"
        );
        // Deal mode is accepted at the compact-schema layer; parties are filled
        // from the original user text at execution (not required in the args).
        assert!(
            validate_tool_args(
                ToolName::Research,
                r#"{"question":"Intel acquires Mobileye","mode":"deal","tickers":[],"depth":"quick"}"#
            )
            .is_ok(),
            "deal compact args ok without model-supplied target"
        );
        // Unknown fields (old target/acquirer) are rejected by deny_unknown_fields.
        assert!(
            validate_tool_args(
                ToolName::Research,
                r#"{"question":"Intel acquires Mobileye","mode":"deal","tickers":[],"depth":"quick","target":"Mobileye"}"#
            )
            .is_err(),
            "target is not a compact-schema field"
        );
    }

    #[test]
    fn conv_id_resolver_accepts_app_ids_rejects_traversal() {
        // The app-generated shape passes.
        assert!(validate_conv_id(&new_conversation().id).is_ok());
        assert!(validate_conv_id("1752345678901-ab12").is_ok());
        // Traversal / separators / reserved / wrong shape are rejected.
        assert!(validate_conv_id("../secrets").is_err());
        assert!(validate_conv_id("..\\..\\win").is_err());
        assert!(validate_conv_id("a/b").is_err());
        assert!(validate_conv_id("CON").is_err());
        assert!(validate_conv_id("nul").is_err());
        assert!(validate_conv_id("1752-zz99").is_err()); // non-hex suffix
        assert!(validate_conv_id("1752-ab1").is_err()); // wrong hex length
        assert!(validate_conv_id("").is_err());
    }

    #[test]
    fn history_prefers_llm_context_and_caps_length() {
        let conv = Conversation {
            id: "1-0000".into(),
            title: String::new(),
            created: iso_now(),
            updated: iso_now(),
            messages: vec![
                ChatMsg {
                    role: "user".into(),
                    content: "q".into(),
                    card: None,
                    llm_context: None,
                    ts: iso_now(),
                },
                // A card-carrying assistant message with a compact llm_context now
                // contributes that summary (not the raw/blank content).
                ChatMsg {
                    role: "assistant".into(),
                    content: String::new(),
                    card: Some(json!({"type":"research_answer"})),
                    llm_context: Some("Summary: NVDA thesis.".into()),
                    ts: iso_now(),
                },
            ],
        };
        let msgs = history_messages(&conv);
        // system + user + assistant(llm_context).
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[2]["content"], json!("Summary: NVDA thesis."));
        // Cap: an over-long context is truncated with an ellipsis.
        let long = "x".repeat(9000);
        assert!(cap_context(&long).chars().count() <= 8001);
        assert!(cap_context(&long).ends_with('…'));
    }

    #[test]
    fn history_drops_whole_oldest_turns_with_marker() {
        // Per-message cap is 8000 chars, so ~8k per turn (assistant context).
        // Six turns (~48k) exceed the 40k request budget; the oldest whole turns
        // are dropped and a single marker is inserted. A reply never survives
        // without its user prompt.
        let big = "y".repeat(8000);
        let mut messages = Vec::new();
        for i in 0..6 {
            messages.push(ChatMsg {
                role: "user".into(),
                content: format!("q{i}"),
                card: None,
                llm_context: None,
                ts: iso_now(),
            });
            messages.push(ChatMsg {
                role: "assistant".into(),
                content: String::new(),
                card: None,
                llm_context: Some(big.clone()),
                ts: iso_now(),
            });
        }
        let conv = Conversation {
            id: "1-0000".into(),
            title: String::new(),
            created: iso_now(),
            updated: iso_now(),
            messages,
        };
        let msgs = history_messages(&conv);
        assert_eq!(msgs[0]["role"], json!("system"));
        assert_eq!(msgs[1]["content"], json!("[older turns omitted]"));
        let joined = serde_json::to_string(&msgs).unwrap();
        // Oldest turn dropped whole (both its user prompt and reply gone).
        assert!(!joined.contains("\"q0\""), "oldest turn dropped");
        // Newest turn retained.
        assert!(joined.contains("\"q5\""), "newest turn retained");
        // Every retained user prompt is followed by its assistant reply
        // (turns never split): count user == count assistant among kept.
        let users = msgs.iter().filter(|m| m["role"] == json!("user")).count();
        let asts = msgs
            .iter()
            .filter(|m| m["role"] == json!("assistant"))
            .count();
        assert_eq!(users, asts, "kept turns are complete");
    }

    #[test]
    fn research_mode_router_precedence_and_deal_hijack_guard() {
        use fm_research::research::ResearchMode;
        let nvda = vec!["NVDA".to_string()];
        let two = vec!["NVDA".to_string(), "AMD".to_string()];
        let m = |msg: &str, t: &[String]| select_research_mode(msg, t).0;
        // Ticker-bearing intents.
        assert_eq!(
            m("What are NVDA's risk factors in its 10-K?", &nvda),
            ResearchMode::Filing
        );
        assert_eq!(
            m("Review NVDA latest quarter earnings", &nvda),
            ResearchMode::Earnings
        );
        assert_eq!(m("Compare NVDA vs AMD", &two), ResearchMode::Comparison);
        assert_eq!(
            m("Give a company brief on NVDA", &nvda),
            ResearchMode::Company
        );
        assert_eq!(m("What does NVDA do?", &nvda), ResearchMode::Web);
        // Comparison needs two tickers; one ticker + "compare" stays Web.
        assert_eq!(m("compare NVDA to peers", &nvda), ResearchMode::Web);
        // Ticker-free clean M&A → Deal; parties parsed.
        let (mode, target, acquirer) =
            select_research_mode("Microsoft acquires Activision Blizzard", &[]);
        assert_eq!(mode, ResearchMode::Deal);
        assert!(target.contains("Activision") && acquirer.contains("Microsoft"));
        // Deal-hijack guard: a ticker question that merely mentions an acquisition
        // must NOT route to Deal (greedy parse would yield an interrogative target).
        assert_eq!(
            m(
                "How does NVDA's acquisition of Mellanox affect earnings?",
                &nvda
            ),
            ResearchMode::Earnings
        );
        // Ticker-free non-deal factual question → Web.
        assert_eq!(
            m("What is the outlook for AI chips?", &[]),
            ResearchMode::Web
        );
    }
}
