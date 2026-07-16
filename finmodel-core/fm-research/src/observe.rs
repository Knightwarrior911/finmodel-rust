//! Local, bounded, redacted observability for research/tool runs (Phase 0.3).
//!
//! Pure and runtime-agnostic: the desktop app persists a [`ResearchMetrics`] log
//! under its config dir and exports a redacted bundle ONLY on an explicit
//! Settings action. A [`ResearchTrace`] carries strictly opaque identifiers,
//! categories, counts, timings, and provider-supplied token usage — NEVER
//! prompts, page/source text, API keys, local paths, or generated artifacts.
//!
//! Deleting the metrics log must change no conversation or app behavior: this
//! module is a side-channel with no back-edge into any execution path.

use serde::{Deserialize, Serialize};

/// Retain window: prune traces older than 30 days.
pub const MAX_AGE_SECS: i64 = 30 * 24 * 60 * 60;
/// Hard cap: above this, evict oldest-first.
pub const MAX_TRACES: usize = 500;
/// Current on-disk metrics schema version.
pub const METRICS_SCHEMA_VERSION: u32 = 1;

/// Terminal outcome category of a run (no free-form text).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunOutcome {
    Direct,
    Answer,
    Digest,
    Error,
    Cancelled,
    ToolContract,
}

/// Coarse error category — never a message body or provider payload.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCategory {
    #[default]
    None,
    Auth,
    RateLimit,
    Policy,
    Transport,
    Timeout,
    Schema,
    Ssrf,
    Blocked,
    Synthesis,
    Other,
}

/// Per-stage durations of the research pipeline, in milliseconds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StageDurations {
    pub planning_ms: u64,
    pub searching_ms: u64,
    pub reading_ms: u64,
    pub synthesizing_ms: u64,
}

/// Source status tallies for a run (counts only — never URLs, domains, or text).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceCounts {
    pub read: u32,
    pub blocked: u32,
    pub thin: u32,
    pub failed: u32,
}

/// Provider-supplied token usage, as reported by the provider.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenUsage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
}

/// Scrub a would-be identifier so it can never smuggle a path, URL, secret, or
/// other content into the trace. Anything containing a path/URL separator or
/// obvious secret marker collapses to `"[redacted]"`; the value is also length
/// capped so a stray blob can't bloat the log.
pub fn redact_identifier(s: &str) -> String {
    let looks_unsafe = s.contains('/')
        || s.contains('\\')
        || s.contains("://")
        || s.contains(' ')
        || s.to_ascii_lowercase().contains("key")
        || s.to_ascii_lowercase().contains("token")
        || s.to_ascii_lowercase().contains("bearer");
    if looks_unsafe {
        return "[redacted]".to_string();
    }
    let mut out = s.to_string();
    if out.len() > 96 {
        out.truncate(96);
    }
    out
}

/// One run's trace. Contains ONLY opaque identifiers, categories, counts,
/// timings, and provider usage. Construct via [`ResearchTrace::new`] +
/// [`ResearchTrace::redacted`] before persisting.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResearchTrace {
    /// Opaque run identifier (a UUID, not derived from any user content).
    pub run_id: String,
    /// Unix seconds the run finished (used for pruning).
    pub finished_at: i64,
    /// Public model catalog id (e.g. `anthropic/claude-3.5-sonnet`).
    pub model_id: String,
    /// Public provider id.
    pub provider_id: String,
    /// Router intent (snake_case route name).
    pub intent: String,
    /// Research depth, when the run was a research run.
    pub depth: Option<String>,
    pub stages: StageDurations,
    pub sources: SourceCounts,
    pub outcome: RunOutcome,
    /// Whether the strict/structured schema path was used and validated.
    pub schema_ok: bool,
    /// Whether the run terminated via cancellation.
    pub cancelled: bool,
    pub error: ErrorCategory,
    pub usage: TokenUsage,
}

impl ResearchTrace {
    /// Minimal constructor; callers set the remaining fields directly.
    pub fn new(
        run_id: impl Into<String>,
        finished_at: i64,
        intent: impl Into<String>,
        outcome: RunOutcome,
    ) -> Self {
        Self {
            run_id: run_id.into(),
            finished_at,
            model_id: String::new(),
            provider_id: String::new(),
            intent: intent.into(),
            depth: None,
            stages: StageDurations::default(),
            sources: SourceCounts::default(),
            outcome,
            schema_ok: false,
            cancelled: false,
            error: ErrorCategory::None,
            usage: TokenUsage::default(),
        }
    }

    /// Return a copy with every free-form string field passed through
    /// [`redact_identifier`], so a path/URL/secret can never reach the log even
    /// if a caller populated a field carelessly.
    pub fn redacted(&self) -> Self {
        let mut t = self.clone();
        t.model_id = redact_identifier(&t.model_id);
        t.provider_id = redact_identifier(&t.provider_id);
        t.intent = redact_identifier(&t.intent);
        if let Some(d) = &t.depth {
            t.depth = Some(redact_identifier(d));
        }
        t
    }
}

/// The persisted, aggregate metrics log. Bounded by age and count.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ResearchMetrics {
    pub schema_version: u32,
    pub traces: Vec<ResearchTrace>,
}

impl Default for ResearchMetrics {
    fn default() -> Self {
        Self {
            schema_version: METRICS_SCHEMA_VERSION,
            traces: Vec::new(),
        }
    }
}

impl ResearchMetrics {
    /// Record a (redacted) trace, then prune. `now` = unix seconds (injected).
    pub fn record(&mut self, trace: ResearchTrace, now: i64) {
        self.traces.push(trace.redacted());
        self.prune(now);
    }

    /// Drop traces older than [`MAX_AGE_SECS`], then evict oldest-first above
    /// [`MAX_TRACES`]. Eviction sorts by `finished_at` so it is oldest-first
    /// regardless of insertion order.
    pub fn prune(&mut self, now: i64) {
        self.traces.retain(|t| now - t.finished_at <= MAX_AGE_SECS);
        if self.traces.len() > MAX_TRACES {
            self.traces.sort_by_key(|t| t.finished_at);
            let excess = self.traces.len() - MAX_TRACES;
            self.traces.drain(0..excess);
        }
    }

    /// A fully-redacted JSON diagnostics bundle for the Settings export action.
    /// Re-redacts every trace defensively before serializing.
    pub fn export_bundle(&self) -> serde_json::Value {
        let traces: Vec<ResearchTrace> = self.traces.iter().map(ResearchTrace::redacted).collect();
        serde_json::json!({
            "schema_version": self.schema_version,
            "kind": "finmodel-research-diagnostics",
            "trace_count": traces.len(),
            "traces": traces,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trace_at(id: &str, finished_at: i64) -> ResearchTrace {
        ResearchTrace::new(id, finished_at, "research", RunOutcome::Answer)
    }

    #[test]
    fn redaction_strips_paths_urls_and_secrets() {
        let mut t = trace_at("run1", 100);
        t.model_id = "C:/Users/x/secret/key.txt".into();
        t.provider_id = "https://evil.example/steal".into();
        t.intent = "my-api-key-abc".into();
        let r = t.redacted();
        assert_eq!(r.model_id, "[redacted]");
        assert_eq!(r.provider_id, "[redacted]");
        assert_eq!(r.intent, "[redacted]");
        // A clean catalog id survives.
        let mut ok = trace_at("run2", 100);
        ok.model_id = "anthropic".into(); // no slash/space
        assert_eq!(ok.redacted().model_id, "anthropic");
    }

    #[test]
    fn record_redacts_before_storing() {
        let mut m = ResearchMetrics::default();
        let mut t = trace_at("run1", 100);
        t.provider_id = "/etc/passwd".into();
        m.record(t, 100);
        assert_eq!(m.traces[0].provider_id, "[redacted]");
    }

    #[test]
    fn prune_removes_traces_older_than_30_days() {
        let mut m = ResearchMetrics::default();
        let now = 1_000_000_000;
        m.traces.push(trace_at("old", now - MAX_AGE_SECS - 1));
        m.traces.push(trace_at("fresh", now - 10));
        m.prune(now);
        let ids: Vec<&str> = m.traces.iter().map(|t| t.run_id.as_str()).collect();
        assert_eq!(ids, vec!["fresh"]);
    }

    #[test]
    fn prune_caps_at_max_traces_oldest_first() {
        let mut m = ResearchMetrics::default();
        let now = 2_000_000_000;
        // Insert 505 recent traces with strictly increasing finished_at.
        for i in 0..(MAX_TRACES as i64 + 5) {
            m.traces.push(trace_at(&format!("r{i}"), now - 1000 + i));
        }
        m.prune(now);
        assert_eq!(m.traces.len(), MAX_TRACES);
        // The five oldest (r0..r4) were evicted; the newest survive.
        assert_eq!(m.traces.first().unwrap().run_id, "r5");
        assert_eq!(
            m.traces.last().unwrap().run_id,
            format!("r{}", MAX_TRACES + 4)
        );
    }

    #[test]
    fn export_bundle_contains_only_allowed_keys() {
        let mut m = ResearchMetrics::default();
        m.record(trace_at("run1", 100), 100);
        let bundle = m.export_bundle();
        assert_eq!(bundle["schema_version"], METRICS_SCHEMA_VERSION);
        assert_eq!(bundle["trace_count"], 1);
        // A trace serializes to exactly the whitelisted field set — no content.
        let keys: Vec<&str> = bundle["traces"][0]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        let expected = [
            "run_id",
            "finished_at",
            "model_id",
            "provider_id",
            "intent",
            "depth",
            "stages",
            "sources",
            "outcome",
            "schema_ok",
            "cancelled",
            "error",
            "usage",
        ];
        for k in &keys {
            assert!(expected.contains(k), "unexpected trace field leaked: {k}");
        }
        assert_eq!(keys.len(), expected.len());
    }
}
