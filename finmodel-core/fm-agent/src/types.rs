//! Typed vocabulary shared by the pure [`crate::machine::AgentMachine`] reducer
//! and the Tauri driver: identifiers, phases, terminal reasons, structured
//! message parts, tool risk classes, the tool-result envelope, and the numeric
//! claim/provenance record.
//!
//! Everything here is runtime-agnostic and (de)serializable so the same shapes
//! flow through the reducer, the SQLite store, and the IPC event envelope.

use serde::{Deserialize, Serialize};

/// Opaque string identifiers. We keep UUID-shaped strings (as produced by the
/// web `crypto.randomUUID()` and [`crate::ids`]) rather than integer keys so the
/// reducer never depends on database row ids.
pub type ConversationId = String;
pub type RunId = String;
pub type MessageId = String;
/// Provider-assigned key that correlates a tool call with its result.
pub type ToolCallId = String;
/// A single execution attempt of a [`ToolCallId`]; a call may retry.
pub type AttemptId = String;
/// Groups independent read-only calls issued together.
pub type BatchId = String;
pub type SourceId = String;
pub type ArtifactId = String;
pub type InteractionId = String;

/// Workspace confidentiality tier (fixed product decision 11).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidentiality {
    /// Public/personal handling; minimized provider context permitted.
    Standard,
    /// Ordinary deal/company handling; no-training routing, no global memory.
    Confidential,
    /// Exceptional restricted-mandate handling; per-turn egress approval.
    Restricted,
}

impl Confidentiality {
    /// Restricted requires per-turn egress approval before any provider call.
    pub fn requires_egress_approval(self) -> bool {
        matches!(self, Confidentiality::Restricted)
    }
    /// Only Standard permits promoting a memory to global scope.
    pub fn allows_global_memory(self) -> bool {
        matches!(self, Confidentiality::Standard)
    }
}

/// Agent phases. Ordered progression with an optional [`Planning`] step and a
/// direct-answer shortcut (`Preparing -> Synthesizing`) for non-tool turns.
///
/// [`Planning`]: AgentPhase::Planning
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentPhase {
    Preparing,
    Planning,
    Executing,
    AwaitingApproval,
    Synthesizing,
    Verifying,
    Completed,
    Failed,
    Cancelled,
    Interrupted,
    BudgetLimited,
}

impl AgentPhase {
    /// Terminal phases emit exactly one matching terminal event and accept no
    /// further transitions.
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            AgentPhase::Completed
                | AgentPhase::Failed
                | AgentPhase::Cancelled
                | AgentPhase::Interrupted
                | AgentPhase::BudgetLimited
        )
    }

    /// The single durable terminal event kind this phase maps to, if terminal.
    pub fn terminal_event(self) -> Option<EventKind> {
        match self {
            AgentPhase::Completed => Some(EventKind::RunCompleted),
            AgentPhase::Failed => Some(EventKind::RunFailed),
            AgentPhase::Cancelled => Some(EventKind::RunCancelled),
            AgentPhase::Interrupted => Some(EventKind::RunInterrupted),
            AgentPhase::BudgetLimited => Some(EventKind::RunBudgetLimited),
            _ => None,
        }
    }
}

/// Why a run stopped, recorded on the terminal event and the `agent_runs` row.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "detail")]
pub enum StopReason {
    /// Model produced a final answer and verification passed (or was N/A).
    EndTurn,
    /// User (or a parent) cancelled the run.
    Cancelled,
    /// A budget (rounds, tokens, or deadline) was exhausted.
    Budget(BudgetKind),
    /// A non-recoverable error; carries an opaque, secret-free code.
    Error(String),
    /// Process died mid-run; recovered as interrupted on restart.
    Interrupted,
    /// Verification could not match a material claim after one repair.
    UnverifiedClaim,
}

/// Which budget dimension was exhausted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BudgetKind {
    Rounds,
    Tokens,
    Deadline,
}

/// Structured message-part kinds. Backend part order is authoritative; live and
/// reload render the same ordered snapshot.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PartKind {
    Text,
    Attachment,
    Activity,
    Tool,
    Result,
    Sources,
    Artifact,
    Approval,
    Warning,
    Error,
    MemoryNotice,
}

/// Risk class of a tool executor; drives the approval policy (decision 7).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Risk {
    /// Reads only; auto-runs.
    ReadOnly,
    /// Writes a new immutable version to a new path in the output root; auto-runs.
    LocalCreate,
    /// Overwrites an existing path; pauses on approval.
    LocalOverwrite,
    /// Deletes; pauses on approval.
    LocalDelete,
    /// Exports outside the output root; pauses on approval.
    Export,
}

impl Risk {
    /// Whether a call of this risk may execute without a persisted approval.
    pub fn auto_runs(self) -> bool {
        matches!(self, Risk::ReadOnly | Risk::LocalCreate)
    }
}

/// Trust label attached to tool-produced text. External/PDF/filing/web/MCP text
/// is untrusted quoted data, never instructions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Trust {
    /// Produced by a local deterministic engine we control.
    Trusted,
    /// External content; treat as data only.
    Untrusted,
}

/// The response the user (or parent) gives to a pending approval. There is no
/// approve-forever; first answer wins (decision 7).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalResponse {
    ApproveOnce,
    Deny,
    CreateNewVersion,
}

/// A durable or ephemeral event kind. Every persisted run emits exactly one
/// terminal event; ephemeral kinds carry no durable sequence.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    // Durable
    RunStarted,
    PhaseChanged,
    PlanUpdated,
    AssistantCheckpoint,
    AttachmentAdded,
    ToolQueued,
    ToolStarted,
    ToolSucceeded,
    ToolWarning,
    ToolFailed,
    ToolCancelled,
    ApprovalRequested,
    ApprovalResolved,
    ResultPartAdded,
    ArtifactCreated,
    MemoryUpdated,
    RunCompleted,
    RunFailed,
    RunCancelled,
    RunInterrupted,
    RunBudgetLimited,
    // Ephemeral
    AssistantTextDelta,
    ToolProgress,
}

impl EventKind {
    /// Durable events persist to `run_events` with a monotonic sequence and are
    /// authoritative for replay; ephemeral events never determine terminal state.
    pub fn is_durable(self) -> bool {
        !matches!(self, EventKind::AssistantTextDelta | EventKind::ToolProgress)
    }

    /// Whether this is one of the five terminal run events.
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            EventKind::RunCompleted
                | EventKind::RunFailed
                | EventKind::RunCancelled
                | EventKind::RunInterrupted
                | EventKind::RunBudgetLimited
        )
    }
}

/// Durability marker on the IPC event envelope.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Durability {
    Durable,
    Ephemeral,
}

/// A source reference promoted into the primary-source ledger.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceRef {
    pub id: SourceId,
    pub kind: String,
    pub canonical_uri: String,
    pub title: Option<String>,
    pub publisher: Option<String>,
    /// Issuer/publication date, ISO-8601.
    pub published_at: Option<String>,
    /// When we retrieved it, ISO-8601.
    pub accessed_at: Option<String>,
}

/// An artifact handle. The model context sees only the opaque id/label, never a
/// filesystem path.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactRef {
    pub id: ArtifactId,
    pub kind: String,
    pub label: String,
    pub mime: String,
    pub version: u32,
    pub sha256: String,
}

/// A material numeric claim under the fixed provenance rules (decision 5). The
/// verifier must match every applicable field against primary-source evidence.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Claim {
    /// Stable key for dedup/supersession (e.g. `nvda.revenue.fy2024`).
    pub claim_key: String,
    pub entity: String,
    /// Canonical decimal value as a string to avoid float drift.
    pub normalized_value: String,
    pub unit: String,
    pub currency: Option<String>,
    /// Scale multiplier applied to reach the canonical value (e.g. `1e6`).
    pub scale: String,
    /// Fiscal period / as-of date on the issuer calendar.
    pub period: String,
    /// Where in the source the figure lives (page, table, XBRL tag, …).
    pub locator: String,
    /// The source backing this claim.
    pub source_id: SourceId,
    /// Hash of the quoted evidence span.
    pub quote_hash: String,
}

/// The uniform envelope every tool executor returns.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolResultEnvelope {
    /// Model-facing typed JSON.
    pub data: serde_json::Value,
    /// Result message-part payload for the UI.
    pub display: serde_json::Value,
    /// Compact replay context stored for later turns.
    pub summary: String,
    pub sources: Vec<SourceRef>,
    pub artifacts: Vec<ArtifactRef>,
    pub warnings: Vec<String>,
    /// Trust label for any external text in `data`/`display`.
    pub trust: Trust,
}

impl ToolResultEnvelope {
    /// A trusted, source-free success envelope with the given typed data.
    pub fn ok(data: serde_json::Value, summary: impl Into<String>) -> Self {
        ToolResultEnvelope {
            data: data.clone(),
            display: data,
            summary: summary.into(),
            sources: Vec::new(),
            artifacts: Vec::new(),
            warnings: Vec::new(),
            trust: Trust::Trusted,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_phases_map_to_one_event() {
        for (phase, kind) in [
            (AgentPhase::Completed, EventKind::RunCompleted),
            (AgentPhase::Failed, EventKind::RunFailed),
            (AgentPhase::Cancelled, EventKind::RunCancelled),
            (AgentPhase::Interrupted, EventKind::RunInterrupted),
            (AgentPhase::BudgetLimited, EventKind::RunBudgetLimited),
        ] {
            assert!(phase.is_terminal());
            assert_eq!(phase.terminal_event(), Some(kind));
            assert!(kind.is_terminal());
            assert!(kind.is_durable());
        }
    }

    #[test]
    fn non_terminal_phases_have_no_terminal_event() {
        for phase in [
            AgentPhase::Preparing,
            AgentPhase::Planning,
            AgentPhase::Executing,
            AgentPhase::AwaitingApproval,
            AgentPhase::Synthesizing,
            AgentPhase::Verifying,
        ] {
            assert!(!phase.is_terminal());
            assert_eq!(phase.terminal_event(), None);
        }
    }

    #[test]
    fn risk_auto_run_policy() {
        assert!(Risk::ReadOnly.auto_runs());
        assert!(Risk::LocalCreate.auto_runs());
        assert!(!Risk::LocalOverwrite.auto_runs());
        assert!(!Risk::LocalDelete.auto_runs());
        assert!(!Risk::Export.auto_runs());
    }

    #[test]
    fn ephemeral_events_are_not_durable() {
        assert!(!EventKind::AssistantTextDelta.is_durable());
        assert!(!EventKind::ToolProgress.is_durable());
        assert!(EventKind::RunStarted.is_durable());
        assert!(EventKind::ToolSucceeded.is_durable());
    }

    #[test]
    fn confidentiality_policy() {
        assert!(!Confidentiality::Standard.requires_egress_approval());
        assert!(Confidentiality::Restricted.requires_egress_approval());
        assert!(Confidentiality::Standard.allows_global_memory());
        assert!(!Confidentiality::Confidential.allows_global_memory());
        assert!(!Confidentiality::Restricted.allows_global_memory());
    }

    #[test]
    fn envelope_roundtrips_json() {
        let env = ToolResultEnvelope::ok(serde_json::json!({"price": 123.45}), "quote AAPL 123.45");
        let s = serde_json::to_string(&env).unwrap();
        let back: ToolResultEnvelope = serde_json::from_str(&s).unwrap();
        assert_eq!(env, back);
        assert_eq!(back.trust, Trust::Trusted);
    }
}
