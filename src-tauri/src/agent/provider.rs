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
        self.calls.iter().filter(|c| !c.name.is_empty() && !c.id.is_empty()).collect()
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
        assert_eq!(acc.meta.usage.as_ref().unwrap()["total_tokens"], serde_json::json!(15));
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
        assert!(decide_stream_tool_calls(Some(ToolStreamProbe { obs_first: true, obs_second: true })));
        assert!(!decide_stream_tool_calls(Some(ToolStreamProbe { obs_first: true, obs_second: false })));
        assert!(!decide_stream_tool_calls(Some(ToolStreamProbe { obs_first: false, obs_second: false })));
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
}
