//! Child-agent delegation (`delegate_analysis`): a junior analyst runs a
//! self-contained subtask in its OWN context with the read-only tool belt
//! and returns a compact findings brief.
//!
//! Why: multi-company deep dives in the main loop pile every tool result
//! into the orchestrator's context until pruning eats the early evidence.
//! A delegated slice keeps its raw evidence here, in the child's messages,
//! and hands the parent only the brief — the parent's context stays clean
//! over long sessions (the OMP "first-class subagents" lever, scoped to
//! this product's needs).
//!
//! Safety shape: the child's belt is read-only tools minus itself (no
//! recursion, nothing that creates or exports), rounds are hard-capped,
//! the PARENT's cancel token aborts the child mid-stream, and the child
//! streams under a synthetic run id the UI never paints. The child's
//! spend rides home on the card's `usage` object, which the driver feeds
//! into the conversation CostGuard — delegated work counts against the
//! same ceiling as everything else.

use serde_json::{json, Value};

/// Hard cap on child rounds — a delegation is a slice, not a session.
const MAX_ROUNDS: usize = 4;
/// Per-round provider deadline.
const ROUND_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);
/// The parent sees at most this much of the brief inline (the card keeps
/// the full text).
const BRIEF_CAP: usize = 1400;

const CHILD_PROMPT: &str = "You are a junior analyst inside finmodel handling ONE delegated subtask. Work it with the read-only tools available (financials, quotes, filings, research, news, web) and nothing else. Rules: every material figure must come from a tool result in THIS conversation; independent lookups go in one turn as parallel tool calls; be quick - you have a small round budget. When the evidence is in, reply with a compact findings brief: lead with the answer, then the key figures (with period labels), then one line on sources. No preamble, no restating the task.";

/// The child's tool belt: read-only, minus the delegate tool itself.
pub(crate) fn child_tool_belt() -> Vec<Value> {
    // Excluded even though read-only: recursion (delegate_analysis) and
    // deliverable/meta tools (draft_memo, use_skill) — a child returns a
    // findings brief, never artifacts or behavior changes.
    const EXCLUDED: [&str; 3] = ["delegate_analysis", "draft_memo", "use_skill"];
    crate::agent::tools::ToolRegistry::shared()
        .agent_schemas_read_only()
        .into_iter()
        .filter(|v| {
            v.pointer("/function/name")
                .and_then(|n| n.as_str())
                .map_or(false, |n| !EXCLUDED.contains(&n))
        })
        .collect()
}

/// Boundary-safe truncation for the parent-facing brief.
pub(crate) fn cap_brief(s: &str, cap: usize) -> String {
    if s.len() <= cap {
        return s.to_string();
    }
    let mut cut = cap;
    while cut > 0 && !s.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}…", &s[..cut])
}

/// One step of the child's visible work trail: which tool, on what, and the
/// first line of what came back — enough for the parent UI's collapsed
/// "how the junior analyst worked" section, without the raw payloads.
pub(crate) fn trail_entry(tool: &str, args: &Value, summary: &str) -> Value {
    let subject = ["ticker", "company", "query", "task", "url", "cik"]
        .iter()
        .find_map(|k| args.get(*k).and_then(|v| v.as_str()))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let first_line = summary.lines().next().unwrap_or("").trim();
    json!({
        "tool": tool,
        "subject": subject,
        "note": cap_brief(first_line, 140),
    })
}

/// Accumulate one round's provider usage into the running child total.
pub(crate) fn add_usage(total: &mut (u64, u64, f64, bool), usage: Option<&Value>) {
    let Some(u) = usage else { return };
    total.0 += u.get("prompt_tokens").and_then(|x| x.as_u64()).unwrap_or(0);
    total.1 += u
        .get("completion_tokens")
        .and_then(|x| x.as_u64())
        .unwrap_or(0);
    if let Some(c) = u
        .get("cost")
        .and_then(|x| x.as_f64())
        .filter(|v| v.is_finite() && *v >= 0.0)
    {
        total.2 += c;
        total.3 = true;
    }
}

/// The card-borne usage object the parent driver charges to the CostGuard.
pub(crate) fn usage_value(total: &(u64, u64, f64, bool)) -> Value {
    let mut u = json!({
        "prompt_tokens": total.0,
        "completion_tokens": total.1,
    });
    if total.3 {
        u["cost"] = json!(total.2);
    }
    u
}

/// Run one delegated subtask to completion. Returns `(parent_summary, card)`.
/// The parent summary is the compact brief; the card carries the full brief
/// plus provenance (work trail, tools used, usage).
pub(crate) async fn run_delegate_loop(
    app: &tauri::AppHandle,
    cfg: &fm_extract::LlmConfig,
    task: &str,
    conversation_id: &str,
    cancel: &tokio_util::sync::CancellationToken,
) -> Result<(String, Value), String> {
    if cfg.api_key.trim().is_empty() {
        return Err("delegation needs the OpenRouter key configured in Settings".into());
    }
    let tools = child_tool_belt();
    let mut messages = vec![
        json!({ "role": "system", "content": CHILD_PROMPT }),
        json!({ "role": "user", "content": task }),
    ];
    // Synthetic run id: the UI filters stream deltas by run id, so the
    // child's reasoning never paints into the parent's answer stream.
    let child_run = format!("delegate:{}", &uuid_ish());
    let mut tools_used: Vec<String> = Vec::new();
    let mut trail: Vec<Value> = Vec::new();
    let mut usage_total: (u64, u64, f64, bool) = (0, 0, 0.0, false);
    let mut last_prose = String::new();

    for round in 0..MAX_ROUNDS {
        if cancel.is_cancelled() {
            return Err("delegation cancelled".into());
        }
        // The LAST round runs without tools: whatever evidence exists must
        // become the brief instead of another fan-out.
        let belt: &[Value] = if round + 1 == MAX_ROUNDS { &[] } else { &tools };
        let req = crate::commands::chat::build_chat_request(
            &cfg.model,
            &messages,
            belt,
            true,
            belt.len() > 1,
        );
        let acc = crate::commands::chat::stream_completion_for_agent(
            app,
            conversation_id,
            &child_run,
            cfg,
            &req,
            cancel,
            ROUND_TIMEOUT,
        )
        .await
        .map_err(|e| format!("delegated analyst call failed: {e}"))?;
        add_usage(&mut usage_total, acc.meta.usage.as_ref());

        let calls = acc.complete_calls();
        if calls.is_empty() {
            last_prose = acc.content.trim().to_string();
            break;
        }
        // Append the assistant tool-call turn, then execute each call via
        // the SAME registry surface the parent uses (validated, read-only).
        let call_msgs: Vec<Value> = calls
            .iter()
            .map(|c| {
                json!({
                    "id": c.id,
                    "type": "function",
                    "function": { "name": c.name, "arguments": c.arguments },
                })
            })
            .collect();
        messages.push(json!({ "role": "assistant", "content": null, "tool_calls": call_msgs }));
        let registry = crate::agent::tools::ToolRegistry::shared();
        for c in &calls {
            if cancel.is_cancelled() {
                return Err("delegation cancelled".into());
            }
            let args: Value = serde_json::from_str(&c.arguments).unwrap_or(json!({}));
            let content = match registry.validate_call(&c.name, &args) {
                Err(e) => crate::agent::executors::tool_error_content(
                    registry,
                    &crate::agent::executors::ExecuteError::Validation(e),
                ),
                Ok(()) => {
                    // Belt filter above guarantees read-only; double-check
                    // here so a hallucinated name can never widen the belt.
                    let read_only = registry
                        .get(&c.name)
                        .map(|s| matches!(s.risk, fm_agent::types::Risk::ReadOnly))
                        .unwrap_or(false);
                    if !read_only {
                        format!("Tool error: `{}` is not available to a delegated analyst (read-only tools only).", c.name)
                    } else {
                        match crate::commands::chat::run_tool(
                            app,
                            &c.name,
                            &args,
                            task,
                            conversation_id,
                            cancel,
                        ) {
                            Ok((summary, _card)) => {
                                tools_used.push(c.name.clone());
                                trail.push(trail_entry(&c.name, &args, &summary));
                                summary
                            }
                            Err(e) => format!("Tool error: {e}"),
                        }
                    }
                }
            };
            messages.push(json!({
                "role": "tool",
                "tool_call_id": c.id,
                "content": content,
            }));
        }
    }

    if last_prose.is_empty() {
        return Err("the delegated analyst ran out of rounds without a brief".into());
    }
    tools_used.sort_unstable();
    tools_used.dedup();
    let brief = cap_brief(&last_prose, BRIEF_CAP);
    let card = json!({
        "type": "delegate",
        "task": task,
        "findings": last_prose,
        "tools_used": tools_used,
        "trail": trail,
        "usage": usage_value(&usage_total),
    });
    let summary = format!("Delegated analysis finished. Findings brief:\n{brief}");
    Ok((summary, card))
}

/// Cheap unique id for the child's synthetic run (no uuid crate dependency
/// needed here — collision space is per-process, per-run).
fn uuid_ish() -> String {
    let mut b = [0u8; 16];
    rand::Rng::fill(&mut rand::thread_rng(), &mut b);
    fm_agent::ids::format_uuid_v4(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn child_belt_is_read_only_and_never_recursive() {
        let belt = child_tool_belt();
        assert!(!belt.is_empty(), "child needs research tools");
        let names: Vec<&str> = belt
            .iter()
            .filter_map(|v| v.pointer("/function/name").and_then(|n| n.as_str()))
            .collect();
        assert!(!names.contains(&"delegate_analysis"), "no recursion: {names:?}");
        assert!(!names.contains(&"use_skill"), "meta tools stay out");
        // Write-risk tools must be absent from the child belt entirely.
        for banned in ["build_model", "draft_memo"] {
            assert!(!names.contains(&banned), "{banned} leaked into the child belt");
        }
        // The research surface is present (the point of delegation).
        assert!(names.contains(&"research"));
        assert!(names.contains(&"get_financials"));
    }

    #[test]
    fn brief_cap_is_utf8_safe() {
        let s = "€".repeat(600); // 3 bytes each — 1800 bytes, cap mid-char.
        let capped = cap_brief(&s, 1400);
        assert!(capped.ends_with('…'));
        assert!(capped.len() <= 1404);
        // No panic and no broken char: round-trips as valid UTF-8 by type.
        assert!(cap_brief("short", 1400) == "short");
    }

    #[test]
    fn trail_entries_and_usage_roll_up_for_the_parent() {
        // Trail: tool + subject + first line of the result, capped.
        let e = trail_entry(
            "get_financials",
            &json!({"ticker": "SAP"}),
            "SAP SE — annual report (20-F/IFRS)\nRevenue: FY2025 €36.8B",
        );
        assert_eq!(e["tool"], "get_financials");
        assert_eq!(e["subject"], "SAP");
        assert_eq!(e["note"], "SAP SE — annual report (20-F/IFRS)");
        // Usage: rounds sum; reported cost carries through only when present.
        let mut t = (0u64, 0u64, 0.0f64, false);
        add_usage(&mut t, Some(&json!({"prompt_tokens": 900, "completion_tokens": 100})));
        add_usage(
            &mut t,
            Some(&json!({"prompt_tokens": 1100, "completion_tokens": 200, "cost": 0.004})),
        );
        add_usage(&mut t, None);
        let u = usage_value(&t);
        assert_eq!(u["prompt_tokens"], 2000);
        assert_eq!(u["completion_tokens"], 300);
        assert_eq!(u["cost"], 0.004);
        // No reported cost anywhere → no cost field (the guard estimates
        // from catalog prices instead of trusting a fabricated zero).
        let t2 = (10u64, 5u64, 0.0f64, false);
        assert!(usage_value(&t2).get("cost").is_none());
    }
}
