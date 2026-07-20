//! LLM calling infrastructure for filing extraction.
//!
//! Ported from `src/extractor.py` — `_llm_complete()` and `_llm_complete_via_cli()`.
//! Supports:
//! - Claude Code CLI (`claude -p`) — fallback when no API key is set
//! - Provider API via HTTP (DeepSeek, Anthropic) when env keys are set
//!
//! # Important
//! On Windows, `claude` is a `.CMD` file and must be invoked via `cmd /c`.
//! The system prompt is written to a temp file to avoid shell quoting issues,
//! and user text is piped via stdin.

use std::io::Write;
use std::process::{Command, Stdio};

/// Errors from LLM operations.
#[derive(Debug, thiserror::Error)]
pub enum LlmError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Claude CLI exited with code {rc}: {stderr}")]
    CliError { rc: i32, stderr: String },
    #[error("UTF-8 decode error: {0}")]
    Utf8(#[from] std::string::FromUtf8Error),
}

/// Call an LLM with the given system and user prompts.
///
/// Provider selection (OpenRouter preferred — API keys don't expire like OAuth):
///   1. `OPENROUTER_API_KEY` set → OpenRouter (openai-compatible, any model)
///   2. `DEEPSEEK_API_KEY` set → DeepSeek (openai-compatible)
///   3. `ANTHROPIC_API_KEY` set → Anthropic API
///   4. None → Claude Code CLI (`claude -p`) — fragile, OAuth can expire
///
/// Model override: `FINMODEL_LLM_MODEL` env var (for OpenRouter, a model id like
/// `anthropic/claude-sonnet-4` or `openai/gpt-4o`).
pub fn llm_complete(
    system_text: &str,
    user_text: &str,
    max_tokens: u32,
) -> Result<String, LlmError> {
    let openrouter_key = std::env::var("OPENROUTER_API_KEY").unwrap_or_default();
    let deepseek_key = std::env::var("DEEPSEEK_API_KEY").unwrap_or_default();
    let anthropic_key = std::env::var("ANTHROPIC_API_KEY").unwrap_or_default();

    if !openrouter_key.trim().is_empty() {
        let model = std::env::var("FINMODEL_LLM_MODEL")
            .unwrap_or_else(|_| "anthropic/claude-sonnet-4".to_string());
        return llm_complete_openrouter(
            system_text,
            user_text,
            max_tokens,
            &model,
            openrouter_key.trim(),
        );
    }
    if !deepseek_key.trim().is_empty() {
        return llm_complete_deepseek(system_text, user_text, max_tokens);
    }
    if !anthropic_key.trim().is_empty() {
        return llm_complete_anthropic(system_text, user_text, max_tokens);
    }
    llm_complete_via_cli(system_text, user_text)
}

/// Explicit LLM credentials threaded per-request (no process-global env
/// mutation — safe under concurrent builds and sound on Windows).
#[derive(Clone, Debug, Default)]
pub struct LlmConfig {
    /// OpenRouter API key.
    pub api_key: String,
    /// OpenRouter model id (e.g. `anthropic/claude-sonnet-4`); blank → default.
    pub model: String,
}

/// Like [`llm_complete`] but with credentials passed explicitly. When `cfg` is
/// `Some` with a non-empty key it forces OpenRouter with that key/model,
/// mutating no shared state. `None` (or an empty key) falls back to the
/// env-based provider selection ([`llm_complete`]) — CLI path unchanged.
pub fn llm_complete_with(
    cfg: Option<&LlmConfig>,
    system_text: &str,
    user_text: &str,
    max_tokens: u32,
) -> Result<String, LlmError> {
    if let Some(c) = cfg {
        if !c.api_key.trim().is_empty() {
            let model = if c.model.trim().is_empty() {
                "anthropic/claude-sonnet-4".to_string()
            } else {
                c.model.trim().to_string()
            };
            return llm_complete_openrouter(
                system_text,
                user_text,
                max_tokens,
                &model,
                c.api_key.trim(),
            );
        }
    }
    llm_complete(system_text, user_text, max_tokens)
}

/// Build the CLI args shared across both platforms.
fn claude_args(sys_path: &str, model: &str) -> Vec<String> {
    vec![
        "--model".into(),
        model.into(),
        "--system-prompt-file".into(),
        sys_path.into(),
        "--output-format".into(),
        "text".into(),
        "-p".into(),
        "Process the piped input per the system instructions and return only the requested JSON."
            .into(),
    ]
}

/// Spawn a subprocess, pipe user_text to its stdin, and collect output.
fn spawn_and_pipe(
    mut child: std::process::Child,
    user_text: &str,
) -> Result<(i32, String, String), LlmError> {
    // Write user text to stdin
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(user_text.as_bytes());
        // Drop stdin handle so the child can finish reading
        drop(stdin);
    }

    let output = child.wait_with_output()?;
    let out_raw = String::from_utf8_lossy(&output.stdout).to_string();
    let err_raw = String::from_utf8_lossy(&output.stderr).to_string();
    let rc = output.status.code().unwrap_or(-1);
    Ok((rc, out_raw, err_raw))
}

/// Write system prompt to a temp file and return its path.
fn write_sys_prompt(system_text: &str) -> Result<std::path::PathBuf, LlmError> {
    let tmp_dir = std::env::temp_dir();
    let sys_path = tmp_dir.join(format!("fm_extract_sys_{}.txt", std::process::id()));
    let mut f = std::fs::File::create(&sys_path)?;
    f.write_all(system_text.as_bytes())?;
    Ok(sys_path)
}

/// Call Claude via the Claude Code CLI.
///
/// Ported from `_llm_complete_via_cli()` in `src/extractor.py`.
fn llm_complete_via_cli(system_text: &str, user_text: &str) -> Result<String, LlmError> {
    let sys_path = write_sys_prompt(system_text)?;
    let model = std::env::var("FINMODEL_LLM_MODEL").unwrap_or_else(|_| "opus".to_string());

    let result = if cfg!(target_os = "windows") {
        let args = claude_args(&sys_path.to_string_lossy(), &model);
        let child = Command::new("cmd")
            .arg("/c")
            .arg("claude")
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        spawn_and_pipe(child, user_text)?
    } else {
        let args = claude_args(&sys_path.to_string_lossy(), &model);
        let child = Command::new("claude")
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        spawn_and_pipe(child, user_text)?
    };

    // Always clean up temp file
    let _ = std::fs::remove_file(&sys_path);

    let (rc, out_raw, err_raw) = result;

    if rc != 0 {
        let err_trimmed: String = err_raw.chars().take(400).collect();
        return Err(LlmError::CliError {
            rc,
            stderr: err_trimmed,
        });
    }

    let out = out_raw.trim().to_string();
    // Strip markdown code fences if present (matches Python behavior)
    Ok(strip_code_fences(&out))
}

/// Remove markdown code fences from LLM output.
/// Matches `_llm_complete_via_cli` lines 121-127 in Python.
fn strip_code_fences(s: &str) -> String {
    let s = s.trim();
    if s.starts_with("```") {
        let lines: Vec<&str> = s.lines().collect();
        if lines.len() <= 1 {
            return String::new();
        }
        let mut inner: Vec<&str> = lines[1..].to_vec();
        if let Some(last) = inner.last() {
            if last.trim() == "```" {
                inner.pop();
            }
        }
        inner.join("\n").trim().to_string()
    } else {
        s.to_string()
    }
}

/// Call DeepSeek API (openai-compatible).
fn llm_complete_deepseek(
    _system_text: &str,
    _user_text: &str,
    _max_tokens: u32,
) -> Result<String, LlmError> {
    Err(LlmError::Io(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "DeepSeek API caller not yet implemented; use Claude CLI or set ANTHROPIC_API_KEY",
    )))
}

/// Call Anthropic API directly.
fn llm_complete_anthropic(
    _system_text: &str,
    _user_text: &str,
    _max_tokens: u32,
) -> Result<String, LlmError> {
    Err(LlmError::Io(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "Anthropic API caller not yet implemented; use Claude CLI",
    )))
}

// ---------------------------------------------------------------------------
// OpenRouter provider (production-grade — API key, openai-compatible)
// ---------------------------------------------------------------------------

const OPENROUTER_CHAT_URL: &str = "https://openrouter.ai/api/v1/chat/completions";
const OPENROUTER_MODELS_URL: &str = "https://openrouter.ai/api/v1/models";

/// A model available on OpenRouter (subset of fields from the /models endpoint).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OpenRouterModel {
    /// Model id used in API calls, e.g. "anthropic/claude-sonnet-4".
    pub id: String,
    /// Human-readable name.
    #[serde(default)]
    pub name: String,
    /// Maximum context length in tokens.
    #[serde(default)]
    pub context_length: Option<u64>,
    /// Pricing info (per-token strings, USD).
    #[serde(default)]
    pub pricing: Option<OpenRouterPricing>,
    /// Parameters the model/provider advertises support for (OpenRouter
    /// `/models` `supported_parameters`), e.g. `tools`, `structured_outputs`.
    #[serde(default)]
    pub supported_parameters: Vec<String>,
    /// Input/output modality badges (OpenRouter `/models` `architecture`).
    #[serde(default)]
    pub architecture: Option<OpenRouterArchitecture>,
}

/// The `architecture` object on an OpenRouter catalog entry. Only the
/// modality fields are kept; both spellings are tolerated (older payloads
/// carried a combined `modality` string like `text+image->text`).
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct OpenRouterArchitecture {
    #[serde(default)]
    pub input_modalities: Vec<String>,
    #[serde(default)]
    pub modality: String,
}

impl OpenRouterModel {
    /// Whether the model advertises support for `param`.
    pub fn supports(&self, param: &str) -> bool {
        self.supported_parameters.iter().any(|p| p == param)
    }
    /// Native OpenRouter tool-calling support (`tools` parameter).
    pub fn native_tools(&self) -> bool {
        self.supports("tools")
    }
    /// Strict structured-output support (`response_format` json_schema).
    pub fn strict_json(&self) -> bool {
        self.supports("structured_outputs") || self.supports("response_format")
    }
    /// Whether the model can accept image inputs (vision). Checks the
    /// structured `input_modalities` list first, then falls back to the
    /// combined `modality` string's input half (`text+image->text`).
    pub fn vision(&self) -> bool {
        let Some(a) = &self.architecture else {
            return false;
        };
        if !a.input_modalities.is_empty() {
            return a.input_modalities.iter().any(|m| m == "image");
        }
        a.modality
            .split("->")
            .next()
            .map(|inputs| inputs.contains("image"))
            .unwrap_or(false)
    }
    /// Prompt price in USD per 1M tokens, when the catalog carries a
    /// parseable per-token price. Unparseable/absent → None (callers MUST
    /// treat unknown price as ineligible for cost-based routing).
    pub fn prompt_per_mtok(&self) -> Option<f64> {
        per_mtok(self.pricing.as_ref().map(|p| p.prompt.as_str()))
    }
    /// Completion price in USD per 1M tokens (same contract as prompt).
    pub fn completion_per_mtok(&self) -> Option<f64> {
        per_mtok(self.pricing.as_ref().map(|p| p.completion.as_str()))
    }
}

/// Parse an OpenRouter per-token USD price string into $/1M tokens.
/// Empty, negative, or non-numeric prices → None.
fn per_mtok(per_token: Option<&str>) -> Option<f64> {
    let s = per_token?.trim();
    if s.is_empty() {
        return None;
    }
    let v: f64 = s.parse().ok()?;
    if !v.is_finite() || v < 0.0 {
        return None;
    }
    Some(v * 1_000_000.0)
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OpenRouterPricing {
    #[serde(default)]
    pub prompt: String,
    #[serde(default)]
    pub completion: String,
}

/// Fetch the live list of available models from OpenRouter.
///
/// This is the dynamic model catalog — never a hardcoded list. Requires a
/// valid `api_key`. Returns models sorted by id.
pub fn list_openrouter_models(api_key: &str) -> Result<Vec<OpenRouterModel>, LlmError> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| LlmError::Io(std::io::Error::other(e.to_string())))?;
    let resp = client
        .get(OPENROUTER_MODELS_URL)
        .header("Authorization", format!("Bearer {api_key}"))
        .send()
        .map_err(|e| LlmError::Io(std::io::Error::other(e.to_string())))?
        .error_for_status()
        .map_err(|e| LlmError::Io(std::io::Error::other(e.to_string())))?;
    let body: serde_json::Value = resp
        .json()
        .map_err(|e| LlmError::Io(std::io::Error::other(e.to_string())))?;
    Ok(parse_models_response(&body))
}

/// Parse the OpenRouter /models response into a sorted list of models.
/// Pure function — unit-testable without network.
fn parse_models_response(body: &serde_json::Value) -> Vec<OpenRouterModel> {
    let mut models: Vec<OpenRouterModel> = body
        .get("data")
        .and_then(|d| d.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|m| serde_json::from_value::<OpenRouterModel>(m.clone()).ok())
                .collect()
        })
        .unwrap_or_default();
    models.sort_by(|a, b| a.id.cmp(&b.id));
    models
}

/// Build the JSON request body for an OpenRouter chat completion.
/// Pure function — unit-testable.
fn build_openrouter_request(
    system_text: &str,
    user_text: &str,
    max_tokens: u32,
    model: &str,
) -> serde_json::Value {
    serde_json::json!({
        "model": model,
        "messages": [
            { "role": "system", "content": system_text },
            { "role": "user", "content": user_text }
        ],
        "temperature": 0,
        "max_tokens": max_tokens
    })
}

/// Extract the assistant message content from an OpenRouter chat response.
/// Pure function — unit-testable.
fn parse_openrouter_response(body: &serde_json::Value) -> Result<String, LlmError> {
    body.get("choices")
        .and_then(|c| c.as_array())
        .and_then(|arr| arr.first())
        .and_then(|choice| choice.get("message"))
        .and_then(|msg| msg.get("content"))
        .and_then(|content| content.as_str())
        .map(|s| strip_code_fences(s.trim()))
        .ok_or_else(|| {
            // Surface an API error message if present
            let err_msg = body
                .get("error")
                .and_then(|e| e.get("message"))
                .and_then(|m| m.as_str())
                .unwrap_or("no choices in OpenRouter response");
            LlmError::CliError {
                rc: 1,
                stderr: err_msg.to_string(),
            }
        })
}

/// Call OpenRouter's chat completions endpoint (openai-compatible).
fn llm_complete_openrouter(
    system_text: &str,
    user_text: &str,
    max_tokens: u32,
    model: &str,
    api_key: &str,
) -> Result<String, LlmError> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()
        .map_err(|e| LlmError::Io(std::io::Error::other(e.to_string())))?;
    let request_body = build_openrouter_request(system_text, user_text, max_tokens, model);
    let resp = client
        .post(OPENROUTER_CHAT_URL)
        .header("Authorization", format!("Bearer {api_key}"))
        .header("Content-Type", "application/json")
        .header("HTTP-Referer", "https://github.com/finmodel")
        .header("X-Title", "finmodel")
        .json(&request_body)
        .send()
        .map_err(|e| LlmError::Io(std::io::Error::other(e.to_string())))?;
    let body: serde_json::Value = resp
        .json()
        .map_err(|e| LlmError::Io(std::io::Error::other(e.to_string())))?;
    parse_openrouter_response(&body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_code_fences_plain_text() {
        let input = "{\"key\": \"value\"}";
        assert_eq!(strip_code_fences(input), input);
    }

    #[test]
    fn test_strip_code_fences_with_fences() {
        let input = "```json\n{\"key\": \"value\"}\n```";
        let expected = "{\"key\": \"value\"}";
        assert_eq!(strip_code_fences(input), expected);
    }

    #[test]
    fn test_strip_code_fences_no_lang() {
        let input = "```\nplain text\n```";
        assert_eq!(strip_code_fences(input), "plain text");
    }

    #[test]
    fn test_strip_code_fences_trailing_newline() {
        let input = "```\nhello\n```\n";
        assert_eq!(strip_code_fences(input), "hello");
    }

    #[test]
    fn test_strip_code_fences_empty_fence() {
        let input = "```";
        assert_eq!(strip_code_fences(input), "");
    }

    #[test]
    fn test_build_openrouter_request_structure() {
        let req = build_openrouter_request("SYS", "USER", 8192, "anthropic/claude-sonnet-4");
        assert_eq!(req["model"], "anthropic/claude-sonnet-4");
        assert_eq!(req["temperature"], 0);
        assert_eq!(req["max_tokens"], 8192);
        assert_eq!(req["messages"][0]["role"], "system");
        assert_eq!(req["messages"][0]["content"], "SYS");
        assert_eq!(req["messages"][1]["role"], "user");
        assert_eq!(req["messages"][1]["content"], "USER");
    }

    #[test]
    fn test_parse_openrouter_response_success() {
        let body = serde_json::json!({
            "choices": [{ "message": { "role": "assistant", "content": "{\"revenue\": [100]}" } }]
        });
        let result = parse_openrouter_response(&body).expect("should parse");
        assert_eq!(result, "{\"revenue\": [100]}");
    }

    #[test]
    fn test_parse_openrouter_response_strips_fences() {
        let body = serde_json::json!({
            "choices": [{ "message": { "content": "```json\n{\"x\": 1}\n```" } }]
        });
        let result = parse_openrouter_response(&body).expect("should parse");
        assert_eq!(result, "{\"x\": 1}");
    }

    #[test]
    fn test_parse_openrouter_response_error() {
        let body = serde_json::json!({
            "error": { "message": "invalid api key", "code": 401 }
        });
        let result = parse_openrouter_response(&body);
        assert!(result.is_err());
        match result {
            Err(LlmError::CliError { stderr, .. }) => assert!(stderr.contains("invalid api key")),
            _ => panic!("expected CliError with API message"),
        }
    }

    #[test]
    fn test_parse_models_response() {
        let body = serde_json::json!({
            "data": [
                { "id": "openai/gpt-4o", "name": "GPT-4o", "context_length": 128000,
                  "pricing": { "prompt": "0.0000025", "completion": "0.00001" } },
                { "id": "anthropic/claude-sonnet-4", "name": "Claude Sonnet 4", "context_length": 200000 }
            ]
        });
        let models = parse_models_response(&body);
        assert_eq!(models.len(), 2);
        // Sorted by id: anthropic before openai
        assert_eq!(models[0].id, "anthropic/claude-sonnet-4");
        assert_eq!(models[0].context_length, Some(200000));
        assert_eq!(models[1].id, "openai/gpt-4o");
        assert_eq!(models[1].pricing.as_ref().unwrap().prompt, "0.0000025");
    }

    #[test]
    fn test_parse_models_response_empty() {
        let body = serde_json::json!({ "data": [] });
        assert_eq!(parse_models_response(&body).len(), 0);
        let body2 = serde_json::json!({});
        assert_eq!(parse_models_response(&body2).len(), 0);
    }
    #[test]
    fn vision_from_architecture_both_shapes() {
        let body = serde_json::json!({
            "data": [
                { "id": "a/eyes", "architecture": { "input_modalities": ["text", "image"] } },
                { "id": "b/legacy", "architecture": { "modality": "text+image->text" } },
                { "id": "c/text", "architecture": { "input_modalities": ["text"] } },
                { "id": "d/legacy-text", "architecture": { "modality": "text->text" } },
                { "id": "e/none" },
                // Output-side "image" must NOT count as vision input.
                { "id": "f/imagegen", "architecture": { "modality": "text->image" } }
            ]
        });
        let models = parse_models_response(&body);
        let vision: Vec<&str> = models
            .iter()
            .filter(|m| m.vision())
            .map(|m| m.id.as_str())
            .collect();
        assert_eq!(vision, vec!["a/eyes", "b/legacy"]);
    }

    #[test]
    fn per_mtok_prices_parse_defensively() {
        let m: OpenRouterModel = serde_json::from_value(serde_json::json!(
            { "id": "x", "pricing": { "prompt": "0.0000025", "completion": "0.00001" } }
        ))
        .unwrap();
        assert_eq!(m.prompt_per_mtok(), Some(2.5));
        assert_eq!(m.completion_per_mtok(), Some(10.0));
        // Unparseable, negative, or missing prices are None — never 0.
        let bad: OpenRouterModel = serde_json::from_value(serde_json::json!(
            { "id": "y", "pricing": { "prompt": "n/a", "completion": "-1" } }
        ))
        .unwrap();
        assert_eq!(bad.prompt_per_mtok(), None);
        assert_eq!(bad.completion_per_mtok(), None);
        let none: OpenRouterModel =
            serde_json::from_value(serde_json::json!({ "id": "z" })).unwrap();
        assert_eq!(none.completion_per_mtok(), None);
    }

    #[test]
    fn model_capabilities_from_supported_parameters() {
        let body = serde_json::json!({
            "data": [
                { "id": "a/native", "supported_parameters": ["tools", "tool_choice", "structured_outputs", "temperature"] },
                { "id": "b/textonly", "supported_parameters": ["temperature", "max_tokens"] },
                { "id": "c/unknown" }
            ]
        });
        let models = parse_models_response(&body);
        let native = models.iter().find(|m| m.id == "a/native").unwrap();
        assert!(native.native_tools());
        assert!(native.strict_json());
        let text_only = models.iter().find(|m| m.id == "b/textonly").unwrap();
        assert!(!text_only.native_tools());
        assert!(!text_only.strict_json());
        // A model with no advertised params defaults to no capabilities → the
        // application-controlled typed-JSON path (never assume native support).
        let unknown = models.iter().find(|m| m.id == "c/unknown").unwrap();
        assert!(unknown.supported_parameters.is_empty());
        assert!(!unknown.native_tools());
    }
}
