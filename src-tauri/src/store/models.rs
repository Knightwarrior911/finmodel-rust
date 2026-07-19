//! Typed row structs and small enums for the SQLite store. These mirror the
//! tables defined in [`super::migrations`]; the store actor converts between
//! `rusqlite::Row`s and these shapes so no raw SQL leaks past the store module.

use serde::{Deserialize, Serialize};

/// A workspace: deal, company, sector, or personal sandbox. Owns conversations,
/// standing instructions, sources, artifacts, and memory.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Workspace {
    pub id: String,
    pub name: String,
    /// `deal | company | sector | personal`.
    pub kind: String,
    /// `standard | confidential | restricted`.
    pub confidentiality: String,
    pub standing_instructions: String,
    pub memory_enabled: bool,
    pub created_at: String,
    pub updated_at: String,
}

/// A conversation inside a workspace.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Conversation {
    pub id: String,
    pub workspace_id: String,
    pub title: String,
    pub pinned: bool,
    pub archived: bool,
    pub summary: Option<String>,
    /// The active leaf message id; rendering/context walks parent links from here.
    pub active_leaf_message_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// A message. `ordinal` is global insertion order; the branch path walks
/// `parent_message_id` links from the conversation's active leaf.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub conversation_id: String,
    pub ordinal: i64,
    pub parent_message_id: Option<String>,
    /// `user | assistant | system`.
    pub role: String,
    /// Compact rendered text used for LLM history instead of full parts.
    pub context_summary: Option<String>,
    /// `complete | interrupted | draft`.
    pub status: String,
    pub created_at: String,
}

/// An ordered part of a message.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessagePart {
    pub id: String,
    pub message_id: String,
    pub ordinal: i64,
    /// One of the [`crate::store::PART_KINDS`] values.
    pub kind: String,
    pub payload_json: String,
    /// Sanitized visible text/labels for workspace-scoped FTS.
    pub search_text: Option<String>,
}

/// An agent run.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentRun {
    pub id: String,
    pub conversation_id: String,
    pub user_message_id: Option<String>,
    pub resumed_from_run_id: Option<String>,
    /// `running | completed | failed | cancelled | interrupted | budget_limited`.
    pub status: String,
    pub phase: String,
    pub model: Option<String>,
    pub policy: Option<String>,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub stop_reason: Option<String>,
    pub usage_json: Option<String>,
    /// Highest durable event sequence written for this run.
    pub last_sequence: i64,
}

/// A durable run event row.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunEvent {
    pub id: String,
    pub run_id: String,
    pub sequence: i64,
    pub kind: String,
    pub payload_json: String,
    pub created_at: String,
}

/// A tool invocation record. `canonical_args_json` retains local replay inputs
/// but never credentials; each logical call has exactly one terminal result.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolInvocation {
    pub id: String,
    pub run_id: String,
    pub parent_invocation_id: Option<String>,
    pub batch_id: Option<String>,
    pub tool_name: String,
    /// `queued | running | success | warning | error | cancelled | interrupted`.
    pub status: String,
    /// `read_only | local_create | local_overwrite | local_delete | export`.
    pub risk: String,
    pub canonical_args_json: Option<String>,
    pub result_summary_json: Option<String>,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub error_code: Option<String>,
}

/// A pending approval/disclosure interaction. First answer wins; no approve-forever.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingInteraction {
    pub id: String,
    pub run_id: String,
    pub tool_call_id: Option<String>,
    /// `approval | disclosure`.
    pub kind: String,
    pub request_json: String,
    /// `pending | resolved`.
    pub status: String,
    /// `approve_once | deny | create_new_version` when resolved.
    pub response_json: Option<String>,
    pub created_at: String,
    pub resolved_at: Option<String>,
}

/// A primary-source ledger entry.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Source {
    pub id: String,
    pub workspace_id: String,
    pub kind: String,
    pub canonical_uri: String,
    pub title: Option<String>,
    pub publisher: Option<String>,
    pub published_at: Option<String>,
    pub accessed_at: Option<String>,
    pub content_hash: Option<String>,
}

/// An artifact handle. Model context sees the opaque handle, never a path.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Artifact {
    pub id: String,
    pub workspace_id: String,
    pub conversation_id: Option<String>,
    pub run_id: Option<String>,
    pub kind: String,
    pub label: String,
    pub mime: String,
    pub blob_id: Option<String>,
    pub version: i64,
    pub parent_artifact_id: Option<String>,
    pub sha256: String,
    pub created_at: String,
}

/// A content-addressed blob record.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Blob {
    pub id: String,
    pub relative_path: String,
    pub sha256: String,
    pub byte_len: i64,
    pub created_at: String,
}

/// A scoped memory row.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Memory {
    pub id: i64,
    pub public_id: String,
    /// `global | workspace | conversation`.
    pub scope_type: String,
    pub workspace_id: Option<String>,
    pub conversation_id: Option<String>,
    pub kind: String,
    pub content: String,
    pub normalized_key: String,
    pub importance: f64,
    pub confidence: f64,
    pub valid_from: Option<String>,
    pub valid_to: Option<String>,
    pub source_type: String,
    pub source_ref: Option<String>,
    pub superseded_by: Option<i64>,
    pub created_at: String,
    pub updated_at: String,
}

/// A quarantined conversation file surfaced at startup.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Quarantined {
    pub filename: String,
    pub error: String,
}

/// Result of the JSON→SQLite migration pass.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MigrationReport {
    pub imported_conversations: usize,
    pub imported_messages: usize,
    pub skipped_existing: usize,
    pub quarantined: Vec<Quarantined>,
    /// Path of the ACL-protected timestamped JSON backup, if one was made.
    pub backup_dir: Option<String>,
}

/// One scheduled follow-through (Task 8.3), as surfaced to the UI and the tick.
#[derive(Clone, Debug, serde::Serialize)]
pub struct ScheduleRow {
    pub id: String,
    pub conversation_id: Option<String>,
    pub recurrence: Option<String>,
    pub next_due: String,
    /// JSON scope; `{"prompt": …}` for user-approved follow-ups.
    pub scope_json: String,
    pub status: String,
    pub last_outcome: Option<String>,
}
