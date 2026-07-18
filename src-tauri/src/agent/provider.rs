//! Provider streaming adapter (OpenRouter first).
//!
//! [`StreamAccumulator`] consumes OpenRouter/OpenAI SSE `data:` payloads and
//! accumulates assistant text plus fragmented tool calls (id/name/arguments
//! concatenated across chunks), capturing terminal metadata. This is the same
//! wire shape the legacy `chat.rs` handles, lifted into the agent driver so the
//! unified loop no longer depends on one provider-specific `tool_use` spelling.
//!
//! [`decide_stream_tool_calls`] implements the selection-time capability probe:
//! parallel tool-call streaming is enabled ONLY when a scripted two-call probe
//! actually observes streamed tool calls; unknown/stale capability stays serial.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// One accumulated tool call from `delta.tool_calls` fragments.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

/// Terminal metadata captured while streaming.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnMeta {
    pub finish_reason: Option<String>,
    pub native_finish_reason: Option<String>,
    pub model: Option<String>,
    pub provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<Value>,
    pub parse_errors: u32,
}

/// Accumulates a streamed completion.
#[derive(Clone, Debug, Default)]
pub struct StreamAccumulator {
    pub content: String,
    pub calls: Vec<AccToolCall>,
    pub meta: TurnMeta,
}

impl StreamAccumulator {
    pub fn new() -> Self {
        Self::default()
    }

    /// Apply one SSE payload (the text after `data: `). Returns the newly
    /// appended text chunk, if any (for ephemeral delta emission). Unparseable
    /// payloads increment `parse_errors` and are otherwise ignored — the raw
    /// payload is never stored.
    pub fn apply(&mut self, payload: &str) -> Option<String> {
        let p = payload.trim();
        if p.is_empty() || p == "[DONE]" {
            return None;
        }
        let v: Value = match serde_json::from_str(p) {
            Ok(v) => v,
            Err(_) => {
                self.meta.parse_errors += 1;
                return None;
            }
        };
        if let Some(m) = v.get("model").and_then(|m| m.as_str()) {
            self.meta.model = Some(m.to_string());
        }
        if let Some(pr) = v.get("provider").and_then(|m| m.as_str()) {
            self.meta.provider = Some(pr.to_string());
        }
        let choice0 = &v["choices"][0];
        let delta = &choice0["delta"];
        let mut chunk = None;
        if let Some(c) = delta.get("content").and_then(|c| c.as_str()) {
            if !c.is_empty() {
                self.content.push_str(c);
                chunk = Some(c.to_string());
            }
        }
        if let Some(tcs) = delta.get("tool_calls").and_then(|t| t.as_array()) {
            for tc in tcs {
                let idx = tc.get("index").and_then(|i| i.as_u64()).unwrap_or(0) as usize;
                while self.calls.len() <= idx {
                    self.calls.push(AccToolCall::default());
                }
                let slot = &mut self.calls[idx];
                if let Some(id) = tc.get("id").and_then(|i| i.as_str()) {
                    if !id.is_empty() {
                        slot.id = id.to_string();
                    }
                }
                if let Some(func) = tc.get("function") {
                    if let Some(name) = func.get("name").and_then(|n| n.as_str()) {
                        if !name.is_empty() {
                            slot.name = name.to_string();
                        }
                    }
                    if let Some(args) = func.get("arguments").and_then(|a| a.as_str()) {
                        slot.arguments.push_str(args);
                    }
                }
            }
        }
        if let Some(fr) = choice0.get("finish_reason").and_then(|f| f.as_str()) {
            if !fr.is_empty() {
                self.meta.finish_reason = Some(fr.to_string());
            }
        }
        if let Some(nfr) = choice0.get("native_finish_reason").and_then(|f| f.as_str()) {
            if !nfr.is_empty() {
                self.meta.native_finish_reason = Some(nfr.to_string());
            }
        }
        if v.get("usage").map(|u| u.is_object()).unwrap_or(false) {
            self.meta.usage = v.get("usage").cloned();
        }
        chunk
    }

    /// Complete tool calls: those with both a name and an id.
    pub fn complete_calls(&self) -> Vec<&AccToolCall> {
        self.calls
            .iter()
            .filter(|c| !c.name.is_empty() && !c.id.is_empty())
            .collect()
    }

    /// Whether the stop reason permits continuing to run tool calls.
    pub fn stop_allows_tools(&self) -> bool {
        match self.meta.finish_reason.as_deref() {
            Some("stop") | Some("length") | Some("content_filter") => false,
            // "tool_calls", None (mid-stream), or provider variants -> allow.
            _ => true,
        }
    }
}

/// Test/driver helper: fold a sequence of SSE payloads.
pub fn accumulate(payloads: &[&str]) -> StreamAccumulator {
    let mut acc = StreamAccumulator::new();
    for p in payloads {
        acc.apply(p);
    }
    acc
}

/// Normalized provider error categories. The adapter maps any provider's
/// redacted error string into one of these so loop policy (retry/failover in
/// Task 6.1) never depends on a provider-specific wire spelling.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderError {
    Auth,
    Billing,
    RateLimit,
    Capacity,
    Transport,
    Timeout,
    ContextOverflow,
    ContentFilter,
    ToolIncompatible,
    Unknown,
}

impl ProviderError {
    /// Transient categories worth an idempotent retry with backoff.
    pub fn is_retryable(self) -> bool {
        matches!(
            self,
            ProviderError::RateLimit
                | ProviderError::Capacity
                | ProviderError::Transport
                | ProviderError::Timeout
        )
    }

    /// Categories that justify rotating to an explicit fallback model/provider.
    pub fn is_failover(self) -> bool {
        matches!(
            self,
            ProviderError::Auth
                | ProviderError::Billing
                | ProviderError::RateLimit
                | ProviderError::Capacity
                | ProviderError::ToolIncompatible
        )
    }
}

/// Classify a redacted provider error string into a normalized category. Uses
/// case-insensitive substring matching so it survives provider wording drift.
pub fn classify_provider_error(msg: &str) -> ProviderError {
    let m = msg.to_lowercase();
    let has = |needles: &[&str]| needles.iter().any(|n| m.contains(n));
    if has(&[
        "tools_unsupported",
        "tool use is not",
        "does not support tool",
        "no endpoints found that support tool",
    ]) {
        ProviderError::ToolIncompatible
    } else if has(&[
        "context",
        "maximum context",
        "context_length",
        "too many tokens",
        "reduce the length",
    ]) {
        ProviderError::ContextOverflow
    } else if has(&["content_filter", "moderation", "flagged", "safety"]) {
        ProviderError::ContentFilter
    } else if has(&[
        "401",
        "unauthor",
        "invalid api key",
        "no auth",
        "missing api key",
    ]) {
        ProviderError::Auth
    } else if has(&[
        "402",
        "quota",
        "billing",
        "insufficient",
        "credit",
        "payment required",
    ]) {
        ProviderError::Billing
    } else if has(&["429", "rate limit", "rate-limit", "too many requests"]) {
        ProviderError::RateLimit
    } else if has(&["timeout", "timed out", "deadline"]) {
        ProviderError::Timeout
    } else if has(&[
        "500",
        "502",
        "503",
        "504",
        "overloaded",
        "capacity",
        "unavailable",
        "server error",
    ]) {
        ProviderError::Capacity
    } else if has(&[
        "network",
        "connection",
        "connreset",
        "connection reset",
        "dns",
        "transport",
    ]) {
        ProviderError::Transport
    } else {
        ProviderError::Unknown
    }
}

/// The selection-time capability probe result for a model.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolStreamProbe {
    /// The first scripted probe observed streamed tool calls.
    pub obs_first: bool,
    /// The second scripted probe observed streamed tool calls.
    pub obs_second: bool,
}

/// Decide whether parallel tool-call streaming may be enabled. Only true when
/// BOTH scripted observations saw streamed tool calls — unknown/stale stays
/// serial. OpenRouter metadata alone is NOT treated as authoritative.
pub fn decide_stream_tool_calls(probe: Option<ToolStreamProbe>) -> bool {
    match probe {
        Some(p) => p.obs_first && p.obs_second,
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accumulates_content_deltas_and_ignores_done() {
        let acc = accumulate(&[
            r#"{"choices":[{"delta":{"content":"Hel"}}]}"#,
            r#"{"choices":[{"delta":{"content":"lo"}}]}"#,
            "[DONE]",
            r#"{"choices":[{"delta":{"content":" world"}}]}"#,
        ]);
        assert_eq!(acc.content, "Hello world");
        assert!(acc.calls.is_empty());
    }

    #[test]
    fn accumulates_split_tool_call_fragments() {
        let acc = accumulate(&[
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","function":{"name":"get_quote","arguments":"{\"tic"}}]}}]}"#,
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"ker\":\"AAPL\"}"}}]}}]}"#,
            "[DONE]",
        ]);
        assert_eq!(acc.calls.len(), 1);
        assert_eq!(acc.calls[0].id, "call_1");
        assert_eq!(acc.calls[0].name, "get_quote");
        assert_eq!(acc.calls[0].arguments, r#"{"ticker":"AAPL"}"#);
        assert_eq!(acc.complete_calls().len(), 1);
    }

    #[test]
    fn accumulates_two_parallel_tool_calls() {
        let acc = accumulate(&[
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"a","function":{"name":"get_quote","arguments":"{}"}}]}}]}"#,
            r#"{"choices":[{"delta":{"tool_calls":[{"index":1,"id":"b","function":{"name":"get_news","arguments":"{}"}}]}}]}"#,
            "[DONE]",
        ]);
        assert_eq!(acc.calls.len(), 2);
        assert_eq!(acc.calls[1].name, "get_news");
    }

    #[test]
    fn captures_finish_reason_usage_and_parse_errors() {
        let acc = accumulate(&[
            r#"{"model":"anthropic/claude-sonnet-4","provider":"Anthropic","choices":[{"delta":{"content":"Hi"}}]}"#,
            r#"{"choices":[{"delta":{},"finish_reason":"length","native_finish_reason":"max_tokens"}]}"#,
            r#"{"choices":[],"usage":{"total_tokens":15}}"#,
            "not-json",
            "[DONE]",
        ]);
        assert_eq!(acc.content, "Hi");
        assert_eq!(acc.meta.finish_reason.as_deref(), Some("length"));
        assert_eq!(acc.meta.native_finish_reason.as_deref(), Some("max_tokens"));
        assert_eq!(acc.meta.model.as_deref(), Some("anthropic/claude-sonnet-4"));
        assert_eq!(
            acc.meta.usage.as_ref().unwrap()["total_tokens"],
            serde_json::json!(15)
        );
        assert_eq!(acc.meta.parse_errors, 1);
    }

    #[test]
    fn stop_reason_gates_tool_continuation() {
        let mut acc = StreamAccumulator::new();
        acc.meta.finish_reason = Some("stop".into());
        assert!(!acc.stop_allows_tools());
        acc.meta.finish_reason = Some("tool_calls".into());
        assert!(acc.stop_allows_tools());
        acc.meta.finish_reason = None;
        assert!(acc.stop_allows_tools());
    }

    #[test]
    fn probe_requires_both_observations() {
        assert!(decide_stream_tool_calls(Some(ToolStreamProbe {
            obs_first: true,
            obs_second: true
        })));
        assert!(!decide_stream_tool_calls(Some(ToolStreamProbe {
            obs_first: true,
            obs_second: false
        })));
        assert!(!decide_stream_tool_calls(Some(ToolStreamProbe {
            obs_first: false,
            obs_second: false
        })));
        // Unknown capability stays serial.
        assert!(!decide_stream_tool_calls(None));
    }

    #[test]
    fn incomplete_tool_call_is_not_complete() {
        // Name but no id -> not yet a complete call.
        let acc = accumulate(&[
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"name":"get_quote","arguments":"{}"}}]}}]}"#,
            "[DONE]",
        ]);
        assert_eq!(acc.complete_calls().len(), 0);
    }

    #[test]
    fn openrouter_and_deepseek_yield_identical_normalized_transcript() {
        // Same logical stream from two OpenAI-compatible providers: the wire
        // differs only in cosmetic model/provider tags and chunk splitting; the
        // normalized transcript (content + complete tool calls) must be identical.
        let openrouter = accumulate(&[
            r#"{"model":"deepseek/deepseek-chat","provider":"DeepSeek","choices":[{"delta":{"content":"NVDA rev "}}]}"#,
            r#"{"model":"deepseek/deepseek-chat","provider":"DeepSeek","choices":[{"delta":{"tool_calls":[{"index":0,"id":"c1","function":{"name":"get_financials","arguments":"{\"ticker\":\"NV"}}]}}]}"#,
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"DA\"}"}}]}}]}"#,
            r#"{"choices":[{"delta":{},"finish_reason":"tool_calls"}]}"#,
            "[DONE]",
        ]);
        let deepseek_direct = accumulate(&[
            r#"{"model":"deepseek-chat","choices":[{"delta":{"content":"NVDA "}}]}"#,
            r#"{"model":"deepseek-chat","choices":[{"delta":{"content":"rev "}}]}"#,
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"c1","function":{"name":"get_financials"}}]}}]}"#,
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{\"ticker\":\"NVDA\"}"}}]}}]}"#,
            r#"{"choices":[{"delta":{},"finish_reason":"tool_calls"}]}"#,
            "[DONE]",
        ]);
        assert_eq!(openrouter.content, deepseek_direct.content);
        assert_eq!(
            openrouter.complete_calls(),
            deepseek_direct.complete_calls()
        );
        assert_eq!(
            openrouter.meta.finish_reason,
            deepseek_direct.meta.finish_reason
        );
    }

    #[test]
    fn provider_errors_classify_into_normalized_categories() {
        assert_eq!(
            classify_provider_error("tools_unsupported"),
            ProviderError::ToolIncompatible
        );
        assert_eq!(
            classify_provider_error("HTTP 401 Unauthorized"),
            ProviderError::Auth
        );
        assert_eq!(
            classify_provider_error("429 Too Many Requests"),
            ProviderError::RateLimit
        );
        assert_eq!(
            classify_provider_error("insufficient quota / billing"),
            ProviderError::Billing
        );
        assert_eq!(
            classify_provider_error("maximum context length exceeded"),
            ProviderError::ContextOverflow
        );
        assert_eq!(
            classify_provider_error("503 service unavailable, overloaded"),
            ProviderError::Capacity
        );
        assert_eq!(
            classify_provider_error("connection reset by peer"),
            ProviderError::Transport
        );
        assert_eq!(
            classify_provider_error("request timed out"),
            ProviderError::Timeout
        );
        assert_eq!(
            classify_provider_error("content_filter triggered"),
            ProviderError::ContentFilter
        );
        assert_eq!(
            classify_provider_error("weird glorp"),
            ProviderError::Unknown
        );
        assert!(ProviderError::RateLimit.is_retryable());
        assert!(ProviderError::Auth.is_failover() && !ProviderError::Auth.is_retryable());
    }
}
