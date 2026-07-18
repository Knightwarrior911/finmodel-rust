//! SQLite persistence for the agentic analyst.
//!
//! [`Db`] is the synchronous core: it owns a `rusqlite::Connection`, applies the
//! fixed PRAGMAs, migrates the schema, and exposes typed CRUD/query methods so
//! no raw SQL leaks past this module. [`StoreHandle`] wraps a [`Db`] on a
//! dedicated blocking thread and serializes short transactions over an `mpsc`
//! channel, without exposing the `Connection` through Tauri state (the plan's
//! store-actor requirement).
//!
//! Design invariants enforced here:
//! - one `finmodel.db` in `app_config_dir`; `PRAGMA user_version` is the schema
//!   authority (see [`migrations`]);
//! - durable run events are strictly monotonic per run;
//! - a blob's last-reference removal enqueues GC in the same transaction; the
//!   bytes are reclaimed afterwards and retried on failure;
//! - every artifact/blob write goes to a same-directory temp file, is fsynced,
//!   then atomically renamed before the row commits.

pub mod memory;
pub mod migrations;
pub mod models;

use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};

pub use models::{
    AgentRun, Artifact, Blob, Conversation, Memory, Message, MessagePart, MigrationReport,
    PendingInteraction, Quarantined, RunEvent, Source, ToolInvocation, Workspace,
};

/// Explicit message-part kinds (mirrors `fm_agent::PartKind`).
pub const PART_KINDS: [&str; 11] = [
    "text",
    "attachment",
    "activity",
    "tool",
    "result",
    "sources",
    "artifact",
    "approval",
    "warning",
    "error",
    "memory_notice",
];

/// Store error type.
#[derive(Debug)]
pub enum StoreError {
    Sql(rusqlite::Error),
    Io(std::io::Error),
    Integrity(String),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StoreError::Sql(e) => write!(f, "sqlite: {e}"),
            StoreError::Io(e) => write!(f, "io: {e}"),
            StoreError::Integrity(m) => write!(f, "integrity: {m}"),
        }
    }
}
impl std::error::Error for StoreError {}
impl From<rusqlite::Error> for StoreError {
    fn from(e: rusqlite::Error) -> Self {
        StoreError::Sql(e)
    }
}
impl From<std::io::Error> for StoreError {
    fn from(e: std::io::Error) -> Self {
        StoreError::Io(e)
    }
}

pub type StoreResult<T> = Result<T, StoreError>;

/// Owner kinds for [`models::Blob`] references (`blob_refs.owner_kind`).
pub const OWNER_ARTIFACT: &str = "artifact";
pub const OWNER_ATTACHMENT: &str = "attachment";

/// The synchronous SQLite core.
pub struct Db {
    conn: Connection,
    /// Root directory for the managed content-addressed blob store.
    blob_root: PathBuf,
}

impl Db {
    /// Open (or create) the database at `path`, apply PRAGMAs, and migrate.
    /// `blob_root` is the managed blob directory (created if missing).
    pub fn open(path: &Path, blob_root: &Path) -> StoreResult<Self> {
        let mut conn = Connection::open(path)?;
        migrations::apply_connection_pragmas(&conn)?;
        // Header PRAGMAs must precede any page write (incl. the WAL switch).
        migrations::init_fresh_if_empty(&conn)?;
        migrations::enable_wal(&conn)?;
        migrations::migrate(&mut conn)?;
        std::fs::create_dir_all(blob_root)?;
        Ok(Db {
            conn,
            blob_root: blob_root.to_path_buf(),
        })
    }

    /// Open an in-memory database (tests). `blob_root` still points at a temp dir.
    /// In-memory databases do not support WAL, so it is skipped.
    pub fn open_in_memory(blob_root: &Path) -> StoreResult<Self> {
        let mut conn = Connection::open_in_memory()?;
        migrations::apply_connection_pragmas(&conn)?;
        migrations::init_fresh_if_empty(&conn)?;
        migrations::migrate(&mut conn)?;
        std::fs::create_dir_all(blob_root)?;
        Ok(Db {
            conn,
            blob_root: blob_root.to_path_buf(),
        })
    }

    /// Generate a fresh UUID-v4 id using OS entropy via SQLite's `randomblob`.
    fn new_id(&self) -> String {
        let bytes: Vec<u8> = self
            .conn
            .query_row("SELECT randomblob(16)", [], |r| r.get(0))
            .unwrap_or_else(|_| vec![0u8; 16]);
        let mut arr = [0u8; 16];
        arr.copy_from_slice(&bytes[..16]);
        fm_agent::ids::format_uuid_v4(arr)
    }

    // ---- Workspaces ----

    /// Insert a workspace with an explicit id.
    #[allow(clippy::too_many_arguments)]
    pub fn create_workspace(
        &self,
        id: &str,
        name: &str,
        kind: &str,
        confidentiality: &str,
        standing_instructions: &str,
        memory_enabled: bool,
        now: &str,
    ) -> StoreResult<()> {
        self.conn.execute(
            "INSERT INTO workspaces (id, name, kind, confidentiality, standing_instructions, memory_enabled, created_at, updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?7)",
            params![id, name, kind, confidentiality, standing_instructions, memory_enabled as i64, now],
        )?;
        Ok(())
    }

    /// Ensure the default Standard `Personal` workspace exists; return its id.
    pub fn ensure_default_personal_workspace(&self, now: &str) -> StoreResult<String> {
        if let Some(id) = self
            .conn
            .query_row(
                "SELECT id FROM workspaces WHERE kind='personal' ORDER BY created_at LIMIT 1",
                [],
                |r| r.get::<_, String>(0),
            )
            .optional()?
        {
            return Ok(id);
        }
        let id = self.new_id();
        self.create_workspace(&id, "Personal", "personal", "standard", "", true, now)?;
        Ok(id)
    }

    pub fn get_workspace(&self, id: &str) -> StoreResult<Option<Workspace>> {
        Ok(self
            .conn
            .query_row(
                "SELECT id,name,kind,confidentiality,standing_instructions,memory_enabled,created_at,updated_at
                 FROM workspaces WHERE id=?1",
                [id],
                |r| {
                    Ok(Workspace {
                        id: r.get(0)?,
                        name: r.get(1)?,
                        kind: r.get(2)?,
                        confidentiality: r.get(3)?,
                        standing_instructions: r.get(4)?,
                        memory_enabled: r.get::<_, i64>(5)? != 0,
                        created_at: r.get(6)?,
                        updated_at: r.get(7)?,
                    })
                },
            )
            .optional()?)
    }

    /// Add an allowlisted public entity to a workspace (user or trusted resolution).
    pub fn add_public_entity(
        &self,
        workspace_id: &str,
        entity_id: &str,
        canonical_name: &str,
    ) -> StoreResult<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO workspace_public_entities (workspace_id, entity_id, canonical_name)
             VALUES (?1,?2,?3)",
            params![workspace_id, entity_id, canonical_name],
        )?;
        Ok(())
    }

    // ---- Conversations & messages ----

    pub fn create_conversation(
        &self,
        id: &str,
        workspace_id: &str,
        title: &str,
        now: &str,
    ) -> StoreResult<()> {
        self.conn.execute(
            "INSERT INTO conversations (id, workspace_id, title, pinned, archived, summary, active_leaf_message_id, created_at, updated_at)
             VALUES (?1,?2,?3,0,0,NULL,NULL,?4,?4)",
            params![id, workspace_id, title, now],
        )?;
        Ok(())
    }

    /// Next global insertion ordinal for a conversation.
    fn next_ordinal(&self, conversation_id: &str) -> StoreResult<i64> {
        let n: i64 = self.conn.query_row(
            "SELECT COALESCE(MAX(ordinal), -1) + 1 FROM messages WHERE conversation_id=?1",
            [conversation_id],
            |r| r.get(0),
        )?;
        Ok(n)
    }

    /// Insert a message; returns its assigned ordinal.
    pub fn insert_message(
        &self,
        id: &str,
        conversation_id: &str,
        parent_message_id: Option<&str>,
        role: &str,
        context_summary: Option<&str>,
        status: &str,
        now: &str,
    ) -> StoreResult<i64> {
        let ordinal = self.next_ordinal(conversation_id)?;
        self.conn.execute(
            "INSERT INTO messages (id, conversation_id, ordinal, parent_message_id, role, context_summary, status, created_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            params![id, conversation_id, ordinal, parent_message_id, role, context_summary, status, now],
        )?;
        Ok(ordinal)
    }

    /// Insert an ordered message part. `search_text` (when present) is indexed
    /// by `message_fts` via triggers.
    pub fn insert_part(
        &self,
        id: &str,
        message_id: &str,
        ordinal: i64,
        kind: &str,
        payload_json: &str,
        search_text: Option<&str>,
    ) -> StoreResult<()> {
        debug_assert!(PART_KINDS.contains(&kind), "unknown part kind {kind}");
        self.conn.execute(
            "INSERT INTO message_parts (id, message_id, ordinal, kind, payload_json, search_text)
             VALUES (?1,?2,?3,?4,?5,?6)",
            params![id, message_id, ordinal, kind, payload_json, search_text],
        )?;
        Ok(())
    }

    pub fn set_active_leaf(
        &self,
        conversation_id: &str,
        message_id: &str,
        now: &str,
    ) -> StoreResult<()> {
        self.conn.execute(
            "UPDATE conversations SET active_leaf_message_id=?1, updated_at=?2 WHERE id=?3",
            params![message_id, now, conversation_id],
        )?;
        Ok(())
    }

    /// The conversation's current active-leaf message id, if any.
    pub fn active_leaf_id(&self, conversation_id: &str) -> StoreResult<Option<String>> {
        let leaf: Option<String> = self
            .conn
            .query_row(
                "SELECT active_leaf_message_id FROM conversations WHERE id=?1",
                [conversation_id],
                |r| r.get(0),
            )
            .optional()?
            .flatten();
        Ok(leaf)
    }

    /// The active root→leaf branch path for a conversation (rendering/context
    /// walks `parent_message_id` links up from the active leaf).
    pub fn branch_path(&self, conversation_id: &str) -> StoreResult<Vec<Message>> {
        let leaf: Option<String> = self
            .conn
            .query_row(
                "SELECT active_leaf_message_id FROM conversations WHERE id=?1",
                [conversation_id],
                |r| r.get(0),
            )
            .optional()?
            .flatten();
        let Some(mut cur) = leaf else {
            return Ok(Vec::new());
        };
        let mut path = Vec::new();
        loop {
            let m = self
                .conn
                .query_row(
                    "SELECT id,conversation_id,ordinal,parent_message_id,role,context_summary,status,created_at
                     FROM messages WHERE id=?1",
                    [&cur],
                    Self::row_to_message,
                )
                .optional()?;
            let Some(m) = m else { break };
            let parent = m.parent_message_id.clone();
            path.push(m);
            match parent {
                Some(p) => cur = p,
                None => break,
            }
        }
        path.reverse();
        Ok(path)
    }

    /// A conversation's title, if it exists.
    pub fn conversation_title(&self, id: &str) -> StoreResult<Option<String>> {
        let t: Option<String> = self
            .conn
            .query_row("SELECT title FROM conversations WHERE id=?1", [id], |r| {
                r.get(0)
            })
            .optional()?;
        Ok(t)
    }

    /// Sidebar rows for a workspace, newest first: id, title, updated, and a
    /// short preview drawn from the latest message's text/result part.
    pub fn list_conversations(
        &self,
        workspace_id: &str,
    ) -> StoreResult<Vec<(String, String, String, String, Option<String>)>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, title, updated_at, project_id FROM conversations
             WHERE workspace_id=?1 AND archived=0
             ORDER BY updated_at DESC",
        )?;
        let rows: Vec<(String, String, String, Option<String>)> = stmt
            .query_map([workspace_id], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })?
            .collect::<rusqlite::Result<_>>()?;
        let mut out = Vec::with_capacity(rows.len());
        for (id, title, updated, project_id) in rows {
            let preview = self.conversation_preview(&id).unwrap_or_default();
            out.push((id, title, updated, preview, project_id));
        }
        Ok(out)
    }

    /// Newest message's text preview (first ~80 chars), for the sidebar.
    fn conversation_preview(&self, conversation_id: &str) -> StoreResult<String> {
        let text: Option<String> = self
            .conn
            .query_row(
                "SELECT p.search_text FROM message_parts p
                 JOIN messages m ON m.id = p.message_id
                 WHERE m.conversation_id=?1 AND p.search_text IS NOT NULL
                 ORDER BY m.ordinal DESC, p.ordinal DESC LIMIT 1",
                [conversation_id],
                |r| r.get(0),
            )
            .optional()?;
        Ok(text
            .map(|t| t.chars().take(80).collect::<String>())
            .unwrap_or_default())
    }

    /// Rename a conversation.
    pub fn rename_conversation(&self, id: &str, title: &str, now: &str) -> StoreResult<()> {
        self.conn.execute(
            "UPDATE conversations SET title=?1, updated_at=?2 WHERE id=?3",
            params![title, now, id],
        )?;
        Ok(())
    }

    /// Delete a conversation and all its cascade-owned rows (messages, parts,
    /// runs, events, invocations; FTS via triggers).
    pub fn delete_conversation(&self, id: &str) -> StoreResult<()> {
        self.conn
            .execute("DELETE FROM conversations WHERE id=?1", [id])?;
        Ok(())
    }

    // ---- Projects (conversation folders) ----

    pub fn create_project(
        &self,
        id: &str,
        workspace_id: &str,
        name: &str,
        now: &str,
    ) -> StoreResult<()> {
        self.conn.execute(
            "INSERT INTO projects (id, workspace_id, name, created_at) VALUES (?1,?2,?3,?4)",
            params![id, workspace_id, name, now],
        )?;
        Ok(())
    }

    /// Projects in a workspace, `(id, name)`, alphabetical.
    pub fn list_projects(&self, workspace_id: &str) -> StoreResult<Vec<(String, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name FROM projects WHERE workspace_id=?1 ORDER BY name COLLATE NOCASE",
        )?;
        let rows = stmt
            .query_map([workspace_id], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }

    pub fn rename_project(&self, id: &str, name: &str) -> StoreResult<()> {
        self.conn
            .execute("UPDATE projects SET name=?1 WHERE id=?2", params![name, id])?;
        Ok(())
    }

    /// Delete a project; its conversations become loose (`project_id = NULL`).
    pub fn delete_project(&self, id: &str) -> StoreResult<()> {
        self.conn.execute(
            "UPDATE conversations SET project_id=NULL WHERE project_id=?1",
            [id],
        )?;
        self.conn
            .execute("DELETE FROM projects WHERE id=?1", [id])?;
        Ok(())
    }

    /// Assign (or clear, with `None`) a conversation's project folder.
    pub fn set_conversation_project(
        &self,
        conversation_id: &str,
        project_id: Option<&str>,
        now: &str,
    ) -> StoreResult<()> {
        self.conn.execute(
            "UPDATE conversations SET project_id=?1, updated_at=?2 WHERE id=?3",
            params![project_id, now, conversation_id],
        )?;
        Ok(())
    }

    /// The project a conversation belongs to, if any.
    pub fn conversation_project(&self, conversation_id: &str) -> StoreResult<Option<String>> {
        let p: Option<Option<String>> = self
            .conn
            .query_row(
                "SELECT project_id FROM conversations WHERE id=?1",
                [conversation_id],
                |r| r.get(0),
            )
            .optional()?;
        Ok(p.flatten())
    }
    /// The workspace a conversation belongs to, if the conversation exists.
    pub fn conversation_workspace(&self, conversation_id: &str) -> StoreResult<Option<String>> {
        let w: Option<String> = self
            .conn
            .query_row(
                "SELECT workspace_id FROM conversations WHERE id=?1",
                [conversation_id],
                |r| r.get(0),
            )
            .optional()?;
        Ok(w)
    }

    fn row_to_message(r: &rusqlite::Row) -> rusqlite::Result<Message> {
        Ok(Message {
            id: r.get(0)?,
            conversation_id: r.get(1)?,
            ordinal: r.get(2)?,
            parent_message_id: r.get(3)?,
            role: r.get(4)?,
            context_summary: r.get(5)?,
            status: r.get(6)?,
            created_at: r.get(7)?,
        })
    }

    /// Parts of a message in order.
    pub fn message_parts(&self, message_id: &str) -> StoreResult<Vec<MessagePart>> {
        let mut stmt = self.conn.prepare(
            "SELECT id,message_id,ordinal,kind,payload_json,search_text
             FROM message_parts WHERE message_id=?1 ORDER BY ordinal",
        )?;
        let rows = stmt
            .query_map([message_id], |r| {
                Ok(MessagePart {
                    id: r.get(0)?,
                    message_id: r.get(1)?,
                    ordinal: r.get(2)?,
                    kind: r.get(3)?,
                    payload_json: r.get(4)?,
                    search_text: r.get(5)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Workspace-scoped full-text search over message part visible text. Returns
    /// `(conversation_id, message_id)` pairs, best matches first. Never crosses
    /// the workspace boundary.
    pub fn search_messages(
        &self,
        workspace_id: &str,
        query: &str,
        limit: i64,
    ) -> StoreResult<Vec<(String, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT c.id, m.id
             FROM message_fts f
             JOIN message_parts p ON p.rowid = f.rowid
             JOIN messages m ON m.id = p.message_id
             JOIN conversations c ON c.id = m.conversation_id
             WHERE c.workspace_id = ?1 AND f.search_text MATCH ?2
             ORDER BY bm25(message_fts)
             LIMIT ?3",
        )?;
        let rows = stmt
            .query_map(params![workspace_id, query, limit], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    // ---- Runs & events ----

    #[allow(clippy::too_many_arguments)]
    pub fn insert_run(
        &self,
        id: &str,
        conversation_id: &str,
        user_message_id: Option<&str>,
        resumed_from_run_id: Option<&str>,
        status: &str,
        phase: &str,
        model: Option<&str>,
        policy: Option<&str>,
        now: &str,
    ) -> StoreResult<()> {
        self.conn.execute(
            "INSERT INTO agent_runs (id, conversation_id, user_message_id, resumed_from_run_id, status, phase, model, policy, started_at, last_sequence)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,0)",
            params![id, conversation_id, user_message_id, resumed_from_run_id, status, phase, model, policy, now],
        )?;
        Ok(())
    }

    /// Append a durable event with the next monotonic sequence for the run, and
    /// bump `agent_runs.last_sequence`, atomically. Returns the assigned sequence.
    pub fn append_event(
        &mut self,
        event_id: &str,
        run_id: &str,
        kind: &str,
        payload_json: &str,
        now: &str,
    ) -> StoreResult<i64> {
        let tx = self.conn.transaction()?;
        let seq: i64 = tx.query_row(
            "SELECT last_sequence + 1 FROM agent_runs WHERE id=?1",
            [run_id],
            |r| r.get(0),
        )?;
        tx.execute(
            "INSERT INTO run_events (id, run_id, sequence, kind, payload_json, created_at)
             VALUES (?1,?2,?3,?4,?5,?6)",
            params![event_id, run_id, seq, kind, payload_json, now],
        )?;
        tx.execute(
            "UPDATE agent_runs SET last_sequence=?1 WHERE id=?2",
            params![seq, run_id],
        )?;
        tx.commit()?;
        Ok(seq)
    }

    /// Durable events strictly after `sequence`, ascending — closes the
    /// snapshot/subscription gap on attach/reload.
    pub fn events_after(&self, run_id: &str, sequence: i64) -> StoreResult<Vec<RunEvent>> {
        let mut stmt = self.conn.prepare(
            "SELECT id,run_id,sequence,kind,payload_json,created_at
             FROM run_events WHERE run_id=?1 AND sequence>?2 ORDER BY sequence",
        )?;
        let rows = stmt
            .query_map(params![run_id, sequence], |r| {
                Ok(RunEvent {
                    id: r.get(0)?,
                    run_id: r.get(1)?,
                    sequence: r.get(2)?,
                    kind: r.get(3)?,
                    payload_json: r.get(4)?,
                    created_at: r.get(5)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Finalize a run with a terminal status/stop reason.
    pub fn finish_run(
        &self,
        run_id: &str,
        status: &str,
        phase: &str,
        stop_reason: Option<&str>,
        usage_json: Option<&str>,
        now: &str,
    ) -> StoreResult<()> {
        self.conn.execute(
            "UPDATE agent_runs SET status=?1, phase=?2, stop_reason=?3, usage_json=?4, finished_at=?5 WHERE id=?6",
            params![status, phase, stop_reason, usage_json, now, run_id],
        )?;
        Ok(())
    }

    pub fn get_run(&self, run_id: &str) -> StoreResult<Option<AgentRun>> {
        Ok(self
            .conn
            .query_row(
                "SELECT id,conversation_id,user_message_id,resumed_from_run_id,status,phase,model,policy,started_at,finished_at,stop_reason,usage_json,last_sequence
                 FROM agent_runs WHERE id=?1",
                [run_id],
                |r| {
                    Ok(AgentRun {
                        id: r.get(0)?,
                        conversation_id: r.get(1)?,
                        user_message_id: r.get(2)?,
                        resumed_from_run_id: r.get(3)?,
                        status: r.get(4)?,
                        phase: r.get(5)?,
                        model: r.get(6)?,
                        policy: r.get(7)?,
                        started_at: r.get(8)?,
                        finished_at: r.get(9)?,
                        stop_reason: r.get(10)?,
                        usage_json: r.get(11)?,
                        last_sequence: r.get(12)?,
                    })
                },
            )
            .optional()?)
    }

    /// The most recent run for a conversation (by insertion/start order), if any.
    pub fn latest_run_for_conversation(
        &self,
        conversation_id: &str,
    ) -> StoreResult<Option<AgentRun>> {
        let id: Option<String> = self
            .conn
            .query_row(
                "SELECT id FROM agent_runs WHERE conversation_id=?1 ORDER BY started_at DESC, rowid DESC LIMIT 1",
                [conversation_id],
                |r| r.get(0),
            )
            .optional()?;
        match id {
            Some(rid) => self.get_run(&rid),
            None => Ok(None),
        }
    }

    // ---- Tool invocations & approvals ----

    #[allow(clippy::too_many_arguments)]
    pub fn insert_tool_invocation(
        &self,
        id: &str,
        run_id: &str,
        parent_invocation_id: Option<&str>,
        batch_id: Option<&str>,
        tool_name: &str,
        status: &str,
        risk: &str,
        canonical_args_json: Option<&str>,
        now: &str,
    ) -> StoreResult<()> {
        self.conn.execute(
            "INSERT INTO tool_invocations (id, run_id, parent_invocation_id, batch_id, tool_name, status, risk, canonical_args_json, started_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            params![id, run_id, parent_invocation_id, batch_id, tool_name, status, risk, canonical_args_json, now],
        )?;
        Ok(())
    }

    pub fn finish_tool_invocation(
        &self,
        id: &str,
        status: &str,
        result_summary_json: Option<&str>,
        error_code: Option<&str>,
        now: &str,
    ) -> StoreResult<()> {
        self.conn.execute(
            "UPDATE tool_invocations SET status=?1, result_summary_json=?2, error_code=?3, finished_at=?4 WHERE id=?5",
            params![status, result_summary_json, error_code, now, id],
        )?;
        Ok(())
    }

    pub fn insert_pending(
        &self,
        id: &str,
        run_id: &str,
        tool_call_id: Option<&str>,
        kind: &str,
        request_json: &str,
        now: &str,
    ) -> StoreResult<()> {
        self.conn.execute(
            "INSERT INTO pending_interactions (id, run_id, tool_call_id, kind, request_json, status, created_at)
             VALUES (?1,?2,?3,?4,?5,'pending',?6)",
            params![id, run_id, tool_call_id, kind, request_json, now],
        )?;
        Ok(())
    }

    /// Resolve a pending interaction; first answer wins. Returns true iff this
    /// call performed the transition (was still pending).
    pub fn resolve_pending(&self, id: &str, response_json: &str, now: &str) -> StoreResult<bool> {
        let n = self.conn.execute(
            "UPDATE pending_interactions SET status='resolved', response_json=?1, resolved_at=?2
             WHERE id=?3 AND status='pending'",
            params![response_json, now, id],
        )?;
        Ok(n == 1)
    }

    /// Still-pending interactions for a run — replayed on restart so a parked
    /// approval survives an app restart (Task 4.3).
    pub fn unresolved_pending(&self, run_id: &str) -> StoreResult<Vec<(String, String, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, kind, request_json FROM pending_interactions WHERE run_id=?1 AND status='pending' ORDER BY created_at",
        )?;
        let rows = stmt.query_map(params![run_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        Ok(rows.filter_map(|x| x.ok()).collect())
    }

    /// Fail closed: mark pending interactions created at/before `cutoff` as
    /// `expired` so a walked-away user's approval never wedges the run or later
    /// silently executes (Task 4.3). Returns the number expired.
    pub fn expire_pending(&self, cutoff: &str, now: &str) -> StoreResult<usize> {
        let n = self.conn.execute(
            "UPDATE pending_interactions SET status='expired', resolved_at=?1 WHERE status='pending' AND created_at<=?2",
            params![now, cutoff],
        )?;
        Ok(n)
    }

    /// Like [`Db::expire_pending`], but returns the distinct run ids affected so a
    /// caller can DENY their parked approval oneshots promptly (Task 4.3), instead
    /// of leaving the run to wait out the driver's safety timeout.
    pub fn expire_pending_runs(&self, cutoff: &str, now: &str) -> StoreResult<Vec<String>> {
        let run_ids: Vec<String> = {
            let mut stmt = self.conn.prepare(
                "SELECT DISTINCT run_id FROM pending_interactions WHERE status='pending' AND created_at<=?1",
            )?;
            let rows = stmt.query_map(params![cutoff], |r| r.get::<_, String>(0))?;
            rows.filter_map(|x| x.ok()).collect()
        };
        self.conn.execute(
            "UPDATE pending_interactions SET status='expired', resolved_at=?1 WHERE status='pending' AND created_at<=?2",
            params![now, cutoff],
        )?;
        Ok(run_ids)
    }

    // ---- Sources & citations ----

    #[allow(clippy::too_many_arguments)]
    pub fn insert_source(
        &self,
        id: &str,
        workspace_id: &str,
        kind: &str,
        canonical_uri: &str,
        title: Option<&str>,
        publisher: Option<&str>,
        published_at: Option<&str>,
        accessed_at: Option<&str>,
        content_hash: Option<&str>,
    ) -> StoreResult<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO sources (id, workspace_id, kind, canonical_uri, title, publisher, published_at, accessed_at, content_hash)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            params![id, workspace_id, kind, canonical_uri, title, publisher, published_at, accessed_at, content_hash],
        )?;
        Ok(())
    }

    /// Link a message part to a source with claim provenance (Task 4.1). Keyed by
    /// `(message_part_id, ordinal)` with `INSERT OR REPLACE`, so re-rendering a
    /// part re-links idempotently.
    pub fn insert_citation(
        &self,
        message_part_id: &str,
        ordinal: i64,
        source_id: &str,
        claim: &fm_agent::types::Claim,
    ) -> StoreResult<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO citations (message_part_id, ordinal, source_id, claim_key, entity, normalized_value, unit, currency, scale, period, locator, quote_hash)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
            params![
                message_part_id,
                ordinal,
                source_id,
                claim.claim_key,
                claim.entity,
                claim.normalized_value,
                claim.unit,
                claim.currency,
                claim.scale,
                claim.period,
                claim.locator,
                claim.quote_hash
            ],
        )?;
        Ok(())
    }

    // ---- Delegations (durable child runs, Task 5.1) ----

    /// Persist a child delegation BEFORE the child executes (status `queued`,
    /// delivery `pending`), so a crash mid-dispatch is recoverable.
    pub fn insert_delegation(
        &self,
        id: &str,
        parent_run_id: &str,
        parent_tool_call_id: Option<&str>,
        task_json: &str,
        now: &str,
    ) -> StoreResult<()> {
        self.conn.execute(
            "INSERT INTO delegations (id, parent_run_id, parent_tool_call_id, status, task_json, dispatched_at, delivery_state)
             VALUES (?1,?2,?3,'queued',?4,?5,'pending')",
            params![id, parent_run_id, parent_tool_call_id, task_json, now],
        )?;
        Ok(())
    }

    /// Link the child run once it is registered, and mark the delegation running.
    pub fn set_delegation_child(&self, id: &str, child_run_id: &str) -> StoreResult<()> {
        self.conn.execute(
            "UPDATE delegations SET child_run_id=?1, status='running' WHERE id=?2",
            params![child_run_id, id],
        )?;
        Ok(())
    }

    /// Finalize a delegation with its terminal status + result/error.
    pub fn finish_delegation(
        &self,
        id: &str,
        status: &str,
        result_json: Option<&str>,
        error_code: Option<&str>,
        now: &str,
    ) -> StoreResult<()> {
        self.conn.execute(
            "UPDATE delegations SET status=?1, result_json=?2, error_code=?3, finished_at=?4 WHERE id=?5",
            params![status, result_json, error_code, now, id],
        )?;
        Ok(())
    }

    /// Count delegations for a parent run in a given status (recovery/inspection).
    pub fn delegations_in_status(&self, parent_run_id: &str, status: &str) -> StoreResult<i64> {
        Ok(self.conn.query_row(
            "SELECT count(*) FROM delegations WHERE parent_run_id=?1 AND status=?2",
            params![parent_run_id, status],
            |r| r.get(0),
        )?)
    }

    /// On restart, mark still-`running` delegations as `outcome_unknown` — a dead
    /// owner's child is never assumed succeeded or failed (Task 5.1). Returns the
    /// number recovered.
    pub fn recover_dead_delegations(&self, now: &str) -> StoreResult<usize> {
        let n = self.conn.execute(
            "UPDATE delegations SET status='outcome_unknown', finished_at=?1 WHERE status IN ('queued','running')",
            params![now],
        )?;
        Ok(n)
    }

    // ---- Child result delivery (at-least-once, Task 5.3) ----
    //
    // A terminal delegation carries `delivery_state='pending'` until its result is
    // appended to the parent exactly once. Delivery is a `delivery_state` CAS keyed
    // by an owner `claim_id`: `pending→claimed` is at-most-once (two consumers
    // racing one row never both win); `claimed→delivered` acks after the parent
    // append; `claimed→pending` releases on a failed append. Only the owning
    // `claim_id` can ack or release, and `claimed_at` timestamps the claim so a
    // crash between claim and ack is recovered by `reclaim_stale_deliveries`
    // (time-based, so a live owner's fresh claim is never stolen). A killed owner
    // thus loses no result and never double-synthesizes (the parent append is
    // idempotent under the `delivered` guard + tool_call_id dedup).

    /// Terminal statuses whose single result must reach the parent.
    const DELIVERABLE: &'static str =
        "status IN ('succeeded','warning','failed','outcome_unknown')";

    /// Claim a terminal, undelivered delegation for delivery. `claim_id` records
    /// the owner and `now` stamps `claimed_at`. Returns true iff won.
    pub fn claim_delegation_delivery(
        &self,
        id: &str,
        claim_id: &str,
        now: &str,
    ) -> StoreResult<bool> {
        let n = self.conn.execute(
            &format!(
                "UPDATE delegations SET delivery_state='claimed', claim_id=?1, claimed_at=?2
                 WHERE id=?3 AND delivery_state='pending' AND {}",
                Self::DELIVERABLE
            ),
            params![claim_id, now, id],
        )?;
        Ok(n == 1)
    }

    /// Acknowledge delivery after the result was appended to the parent. Only the
    /// owning `claim_id` can ack.
    pub fn ack_delegation_delivery(&self, id: &str, claim_id: &str) -> StoreResult<bool> {
        let n = self.conn.execute(
            "UPDATE delegations SET delivery_state='delivered'
             WHERE id=?1 AND delivery_state='claimed' AND claim_id=?2",
            params![id, claim_id],
        )?;
        Ok(n == 1)
    }

    /// Release a claim on a failed append (claimed→pending) so delivery retries.
    /// Only the owning `claim_id` can release; the claim fields are cleared.
    pub fn release_delegation_claim(&self, id: &str, claim_id: &str) -> StoreResult<bool> {
        let n = self.conn.execute(
            "UPDATE delegations SET delivery_state='pending', claim_id=NULL, claimed_at=NULL
             WHERE id=?1 AND delivery_state='claimed' AND claim_id=?2",
            params![id, claim_id],
        )?;
        Ok(n == 1)
    }

    /// Terminal, still-undelivered delegations for a parent (id, result_json).
    pub fn undelivered_completed_delegations(
        &self,
        parent_run_id: &str,
    ) -> StoreResult<Vec<(String, Option<String>)>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT id, result_json FROM delegations
             WHERE parent_run_id=?1 AND delivery_state='pending' AND {}",
            Self::DELIVERABLE
        ))?;
        let rows = stmt.query_map(params![parent_run_id], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.filter_map(|x| x.ok()).collect())
    }

    /// Reclaim claims stamped at or before `cutoff` (owner died mid-delivery) back
    /// to `pending` so the result is redelivered. Returns the number reclaimed.
    pub fn reclaim_stale_deliveries(&self, cutoff: &str) -> StoreResult<usize> {
        let n = self.conn.execute(
            "UPDATE delegations SET delivery_state='pending', claim_id=NULL, claimed_at=NULL
             WHERE delivery_state='claimed' AND claimed_at<=?1",
            params![cutoff],
        )?;
        Ok(n)
    }
    // ---- Commitments + schedules (proactive follow-through, Task 8.1) ----

    /// Persist a follow-up commitment (default status `pending`).
    #[allow(clippy::too_many_arguments)]
    pub fn insert_commitment(
        &self,
        id: &str,
        run_id: Option<&str>,
        message_id: Option<&str>,
        workspace_id: &str,
        text: &str,
        due_semantics: Option<&str>,
        confidence: f64,
        now: &str,
    ) -> StoreResult<()> {
        self.conn.execute(
            "INSERT INTO commitments (id, run_id, message_id, workspace_id, text, status, due_semantics, confidence, created_at)
             VALUES (?1,?2,?3,?4,?5,'pending',?6,?7,?8)",
            params![id, run_id, message_id, workspace_id, text, due_semantics, confidence, now],
        )?;
        Ok(())
    }

    /// Count commitments in a status (restart preserves pending ones).
    pub fn commitments_in_status(&self, status: &str) -> StoreResult<i64> {
        Ok(self.conn.query_row(
            "SELECT count(*) FROM commitments WHERE status=?1",
            params![status],
            |r| r.get(0),
        )?)
    }

    /// Persist an approved schedule (status `pending`, unclaimed).
    #[allow(clippy::too_many_arguments)]
    pub fn insert_schedule(
        &self,
        id: &str,
        commitment_id: Option<&str>,
        conversation_id: Option<&str>,
        timezone: &str,
        recurrence: Option<&str>,
        next_due: &str,
        scope_json: &str,
        budget_json: Option<&str>,
        approval_id: Option<&str>,
        now: &str,
    ) -> StoreResult<()> {
        self.conn.execute(
            "INSERT INTO schedules (id, commitment_id, conversation_id, timezone, recurrence, next_due, scope_json, budget_json, approval_id, status, created_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,'pending',?10)",
            params![id, commitment_id, conversation_id, timezone, recurrence, next_due, scope_json, budget_json, approval_id, now],
        )?;
        Ok(())
    }

    /// Transactionally claim ONE due schedule for `owner`. The guarded
    /// `WHERE claim_owner IS NULL` makes the claim atomic, so two workers racing
    /// the same due row never both win (Task 8.1). Returns the claimed id.
    pub fn claim_due_schedule(&self, now: &str, owner: &str) -> StoreResult<Option<String>> {
        loop {
            let candidate: Option<String> = self
                .conn
                .query_row(
                    "SELECT id FROM schedules WHERE status='pending' AND claim_owner IS NULL AND next_due <= ?1 ORDER BY next_due LIMIT 1",
                    params![now],
                    |r| r.get(0),
                )
                .optional()?;
            let Some(id) = candidate else {
                return Ok(None);
            };
            let won = self.conn.execute(
                "UPDATE schedules SET claim_owner=?1, claimed_at=?2, status='claimed' WHERE id=?3 AND claim_owner IS NULL",
                params![owner, now, id],
            )?;
            if won == 1 {
                return Ok(Some(id));
            }
            // Contended: another worker claimed it first; try the next due row.
        }
    }

    /// Finalize a claimed schedule as `done` with `last_outcome`, clearing the
    /// claim (Task 8.3). One-shot follow-through: a recurring schedule is instead
    /// re-armed by the caller before this.
    pub fn finish_schedule(&self, id: &str, last_outcome: &str, _now: &str) -> StoreResult<()> {
        self.conn.execute(
            "UPDATE schedules SET status='done', last_outcome=?1, claim_owner=NULL, claimed_at=NULL WHERE id=?2",
            params![last_outcome, id],
        )?;
        Ok(())
    }

    /// Record a failed schedule attempt (Task 8.3). Increments `attempts`; if it
    /// reaches `max_attempts` the schedule is terminal (`failed`), otherwise it
    /// returns to `pending` with a backed-off `retry_next_due` and a cleared claim
    /// so a later sweep retries it. Returns `(attempts, terminal)`.
    pub fn fail_schedule_attempt(
        &self,
        id: &str,
        max_attempts: i64,
        retry_next_due: &str,
    ) -> StoreResult<(i64, bool)> {
        let attempts: i64 = self.conn.query_row(
            "SELECT attempts FROM schedules WHERE id=?1",
            params![id],
            |r| r.get(0),
        )?;
        let attempts = attempts + 1;
        let terminal = attempts >= max_attempts;
        if terminal {
            self.conn.execute(
                "UPDATE schedules SET status='failed', attempts=?1, last_outcome='failed', claim_owner=NULL, claimed_at=NULL WHERE id=?2",
                params![attempts, id],
            )?;
        } else {
            self.conn.execute(
                "UPDATE schedules SET status='pending', attempts=?1, last_outcome='retry', claim_owner=NULL, claimed_at=NULL, next_due=?2 WHERE id=?3",
                params![attempts, retry_next_due, id],
            )?;
        }
        Ok((attempts, terminal))
    }

    /// A schedule's `(status, attempts, last_outcome)` for inspection/tests.
    pub fn schedule_state(&self, id: &str) -> StoreResult<Option<(String, i64, Option<String>)>> {
        self.conn
            .query_row(
                "SELECT status, attempts, last_outcome FROM schedules WHERE id=?1",
                params![id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()
            .map_err(Into::into)
    }

    // ---- Skill lifecycle (deterministic aging + supersession, Task 7.3) ----

    /// Register a skill (or bump its `source_version`) as `active` on save. A new
    /// or edited SKILL.md revives a stale skill and records the source version.
    pub fn upsert_skill(&self, name: &str, source_version: i64, now: &str) -> StoreResult<()> {
        self.conn.execute(
            "INSERT INTO skill_lifecycle (name, state, source_version, created_at, updated_at)
             VALUES (?1,'active',?2,?3,?3)
             ON CONFLICT(name) DO UPDATE SET
               state='active',
               source_version=max(source_version, excluded.source_version),
               updated_at=excluded.updated_at",
            params![name, source_version, now],
        )?;
        Ok(())
    }

    /// Record a use: increment `use_count`, stamp `last_used`, and revive a
    /// `stale` skill to `active` (an archived skill needs an explicit restore).
    pub fn record_skill_use(&self, name: &str, now: &str) -> StoreResult<bool> {
        let n = self.conn.execute(
            "UPDATE skill_lifecycle
             SET use_count=use_count+1, last_used=?2, updated_at=?2,
                 state=CASE WHEN state='stale' THEN 'active' ELSE state END
             WHERE name=?1",
            params![name, now],
        )?;
        Ok(n == 1)
    }

    /// Deterministic aging (Task 7.3): first archive already-`stale` skills unused
    /// since `archive_cutoff`, then mark `active` skills unused since
    /// `stale_cutoff` as `stale`. Disuse is measured from `last_used`, falling
    /// back to `created_at` for never-used skills. At most one transition per
    /// skill per sweep. Returns `(staled, archived)`.
    pub fn age_skills(
        &self,
        stale_cutoff: &str,
        archive_cutoff: &str,
        now: &str,
    ) -> StoreResult<(usize, usize)> {
        let archived = self.conn.execute(
            "UPDATE skill_lifecycle SET state='archived', updated_at=?2
             WHERE state='stale' AND COALESCE(last_used, created_at) <= ?1",
            params![archive_cutoff, now],
        )?;
        let staled = self.conn.execute(
            "UPDATE skill_lifecycle SET state='stale', updated_at=?2
             WHERE state='active' AND COALESCE(last_used, created_at) <= ?1",
            params![stale_cutoff, now],
        )?;
        Ok((staled, archived))
    }

    /// Restore a stale/archived skill to `active` (inspectable → usable again).
    pub fn restore_skill(&self, name: &str, now: &str) -> StoreResult<bool> {
        let n = self.conn.execute(
            "UPDATE skill_lifecycle SET state='active', updated_at=?2 WHERE name=?1",
            params![name, now],
        )?;
        Ok(n == 1)
    }

    /// Supersede `old` with `new`: archive the old skill and register the new one
    /// as active, retaining lineage (`supersedes`) and a bumped source version.
    pub fn supersede_skill(
        &self,
        old: &str,
        new: &str,
        source_version: i64,
        now: &str,
    ) -> StoreResult<()> {
        self.conn.execute(
            "UPDATE skill_lifecycle SET state='archived', updated_at=?2 WHERE name=?1",
            params![old, now],
        )?;
        self.conn.execute(
            "INSERT INTO skill_lifecycle (name, state, source_version, supersedes, created_at, updated_at)
             VALUES (?1,'active',?2,?3,?4,?4)
             ON CONFLICT(name) DO UPDATE SET
               state='active', source_version=?2, supersedes=?3, updated_at=?4",
            params![new, source_version, old, now],
        )?;
        Ok(())
    }

    /// Names of `active` skills only — the set eligible for default context
    /// injection (stale/archived are excluded but remain inspectable).
    pub fn active_skill_names(&self) -> StoreResult<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT name FROM skill_lifecycle WHERE state='active' ORDER BY name")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        Ok(rows.filter_map(|x| x.ok()).collect())
    }

    /// Names of skills excluded from default context (state `stale` or
    /// `archived`). A hand-dropped skill file with no lifecycle row is NOT here,
    /// so it stays visible — only aged-out skills are hidden (Task 7.3).
    pub fn inactive_skill_names(&self) -> StoreResult<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT name FROM skill_lifecycle WHERE state IN ('stale','archived')")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        Ok(rows.filter_map(|x| x.ok()).collect())
    }

    /// A skill's `(state, use_count, source_version, supersedes)` for
    /// inspection/restore UIs and tests.
    pub fn skill_lifecycle_state(
        &self,
        name: &str,
    ) -> StoreResult<Option<(String, i64, i64, Option<String>)>> {
        self.conn
            .query_row(
                "SELECT state, use_count, source_version, supersedes FROM skill_lifecycle WHERE name=?1",
                params![name],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()
            .map_err(Into::into)
    }

    // ---- Blobs, refs, GC (content-addressed, atomic publish) ----

    /// Atomically publish `bytes` to the managed blob store under `relative_path`
    /// (relative to `blob_root`). Writes a same-directory temp file, flushes and
    /// fsyncs it, atomically renames into place, then inserts the blob row. On
    /// any failure the temp file is removed and no row is committed.
    pub fn publish_blob(&self, relative_path: &str, bytes: &[u8], now: &str) -> StoreResult<Blob> {
        let final_path = self.blob_root.join(relative_path);
        if let Some(parent) = final_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp_path = with_tmp_suffix(&final_path);
        // Write + flush + fsync the temp file.
        {
            use std::io::Write;
            let mut f = std::fs::File::create(&tmp_path)?;
            f.write_all(bytes)?;
            f.flush()?;
            f.sync_all()?;
        }
        // Atomic rename into place.
        if let Err(e) = std::fs::rename(&tmp_path, &final_path) {
            let _ = std::fs::remove_file(&tmp_path);
            return Err(e.into());
        }
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        let sha = hex(&hasher.finalize());
        let id = self.new_id();
        if let Err(e) = self.conn.execute(
            "INSERT INTO blobs (id, relative_path, sha256, byte_len, created_at) VALUES (?1,?2,?3,?4,?5)",
            params![id, relative_path, sha, bytes.len() as i64, now],
        ) {
            // Registration failed after the file landed: remove the orphan file.
            let _ = std::fs::remove_file(&final_path);
            return Err(e.into());
        }
        Ok(Blob {
            id,
            relative_path: relative_path.to_string(),
            sha256: sha,
            byte_len: bytes.len() as i64,
            created_at: now.to_string(),
        })
    }

    /// Spill an oversized tool result to the blob store, returning a bounded,
    /// char-boundary-safe preview plus the opaque blob id for range reads
    /// (Task 3.4). Results within `preview_budget` bytes are returned inline with
    /// no blob, so only genuinely large results incur a blob write; the full
    /// result is always recoverable by the returned id.
    pub fn spill_result(
        &self,
        run_id: &str,
        tool_call_id: &str,
        result: &str,
        preview_budget: usize,
        now: &str,
    ) -> StoreResult<(String, Option<String>)> {
        if result.len() <= preview_budget {
            return Ok((result.to_string(), None));
        }
        let rel = format!("results/{run_id}/{tool_call_id}.txt");
        let blob = self.publish_blob(&rel, result.as_bytes(), now)?;
        // Trim to a char boundary at or below the budget so the preview is valid
        // UTF-8 and never exceeds the budget (plus the ellipsis marker).
        let mut end = preview_budget.min(result.len());
        while end > 0 && !result.is_char_boundary(end) {
            end -= 1;
        }
        Ok((format!("{}…", &result[..end]), Some(blob.id)))
    }

    /// Add a reference to a blob from an owner (artifact/attachment).
    pub fn add_blob_ref(&self, blob_id: &str, owner_kind: &str, owner_id: &str) -> StoreResult<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO blob_refs (blob_id, owner_kind, owner_id) VALUES (?1,?2,?3)",
            params![blob_id, owner_kind, owner_id],
        )?;
        // Resurrection: a re-referenced blob must not be garbage-collected.
        self.conn
            .execute("DELETE FROM blob_gc WHERE blob_id=?1", [blob_id])?;
        Ok(())
    }

    /// Remove a reference. If it was the last reference, enqueue GC for the blob
    /// in the same transaction (bytes are reclaimed later by [`run_blob_gc`]).
    pub fn remove_blob_ref(
        &mut self,
        blob_id: &str,
        owner_kind: &str,
        owner_id: &str,
        now: &str,
    ) -> StoreResult<bool> {
        let tx = self.conn.transaction()?;
        tx.execute(
            "DELETE FROM blob_refs WHERE blob_id=?1 AND owner_kind=?2 AND owner_id=?3",
            params![blob_id, owner_kind, owner_id],
        )?;
        let remaining: i64 = tx.query_row(
            "SELECT count(*) FROM blob_refs WHERE blob_id=?1",
            [blob_id],
            |r| r.get(0),
        )?;
        let enqueued = if remaining == 0 {
            tx.execute(
                "INSERT OR IGNORE INTO blob_gc (blob_id, queued_at) VALUES (?1,?2)",
                params![blob_id, now],
            )?;
            true
        } else {
            false
        };
        tx.commit()?;
        Ok(enqueued)
    }

    /// Process the GC queue: for each queued blob, remove the bytes then the blob
    /// row. Byte-removal failures are retained with `last_error` for retry.
    /// Returns the number of blobs fully reclaimed.
    pub fn run_blob_gc(&mut self) -> StoreResult<usize> {
        let queued: Vec<(String, String)> = {
            let mut stmt = self.conn.prepare(
                "SELECT g.blob_id, b.relative_path FROM blob_gc g JOIN blobs b ON b.id=g.blob_id
                 WHERE NOT EXISTS (SELECT 1 FROM blob_refs r WHERE r.blob_id = g.blob_id)",
            )?;
            let rows = stmt
                .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            rows
        };
        let mut reclaimed = 0usize;
        for (blob_id, rel) in queued {
            let path = self.blob_root.join(&rel);
            match std::fs::remove_file(&path) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => {
                    // Retain for retry; record the error.
                    self.conn.execute(
                        "UPDATE blob_gc SET last_error=?1 WHERE blob_id=?2",
                        params![e.to_string(), blob_id],
                    )?;
                    continue;
                }
            }
            let tx = self.conn.transaction()?;
            tx.execute("DELETE FROM blobs WHERE id=?1", [&blob_id])?;
            tx.execute("DELETE FROM blob_gc WHERE blob_id=?1", [&blob_id])?;
            tx.commit()?;
            reclaimed += 1;
        }
        Ok(reclaimed)
    }

    pub fn get_blob(&self, blob_id: &str) -> StoreResult<Option<Blob>> {
        Ok(self
            .conn
            .query_row(
                "SELECT id,relative_path,sha256,byte_len,created_at FROM blobs WHERE id=?1",
                [blob_id],
                |r| {
                    Ok(Blob {
                        id: r.get(0)?,
                        relative_path: r.get(1)?,
                        sha256: r.get(2)?,
                        byte_len: r.get(3)?,
                        created_at: r.get(4)?,
                    })
                },
            )
            .optional()?)
    }

    /// Delete stale `.tmp-*` files under `blob_root` at startup, and report
    /// final files present on disk that are not registered as blobs
    /// (for reconciliation). Returns `(deleted_temps, unregistered_finals)`.
    pub fn reconcile_blob_dir(&self) -> StoreResult<(usize, Vec<String>)> {
        let mut deleted = 0usize;
        let mut unregistered = Vec::new();
        let known: std::collections::HashSet<String> = {
            let mut stmt = self.conn.prepare("SELECT relative_path FROM blobs")?;
            let rows = stmt
                .query_map([], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<std::collections::HashSet<_>>>()?;
            rows
        };
        for entry in walk_files(&self.blob_root) {
            let name = entry
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
            if is_tmp_name(&name) {
                if std::fs::remove_file(&entry).is_ok() {
                    deleted += 1;
                }
                continue;
            }
            if let Ok(rel) = entry.strip_prefix(&self.blob_root) {
                let rel = rel.to_string_lossy().replace('\\', "/");
                if !known.contains(&rel) {
                    unregistered.push(rel);
                }
            }
        }
        Ok((deleted, unregistered))
    }

    // ---- Artifacts ----

    #[allow(clippy::too_many_arguments)]
    pub fn insert_artifact(
        &self,
        id: &str,
        workspace_id: &str,
        conversation_id: Option<&str>,
        run_id: Option<&str>,
        kind: &str,
        label: &str,
        mime: &str,
        blob_id: Option<&str>,
        version: i64,
        parent_artifact_id: Option<&str>,
        sha256: &str,
        now: &str,
    ) -> StoreResult<()> {
        self.conn.execute(
            "INSERT INTO artifacts (id, workspace_id, conversation_id, run_id, kind, label, mime, blob_id, version, parent_artifact_id, sha256, created_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
            params![id, workspace_id, conversation_id, run_id, kind, label, mime, blob_id, version, parent_artifact_id, sha256, now],
        )?;
        if let Some(bid) = blob_id {
            self.add_blob_ref(bid, OWNER_ARTIFACT, id)?;
        }
        Ok(())
    }

    // ---- Memories ----

    #[allow(clippy::too_many_arguments)]
    pub fn insert_memory(
        &self,
        public_id: &str,
        scope_type: &str,
        workspace_id: Option<&str>,
        conversation_id: Option<&str>,
        kind: &str,
        content: &str,
        normalized_key: &str,
        importance: f64,
        confidence: f64,
        source_type: &str,
        source_ref: Option<&str>,
        now: &str,
    ) -> StoreResult<i64> {
        self.conn.execute(
            "INSERT INTO memories (public_id, scope_type, workspace_id, conversation_id, kind, content, normalized_key, importance, confidence, valid_from, source_type, source_ref, created_at, updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?10,?10)",
            params![public_id, scope_type, workspace_id, conversation_id, kind, content, normalized_key, importance, confidence, now, source_type, source_ref],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Pin or unpin a memory (Task 7.2). A pinned memory is protected from
    /// automatic forgetting. Returns true iff a row matched.
    pub fn set_memory_pinned(&self, id: i64, pinned: bool) -> StoreResult<bool> {
        let n = self.conn.execute(
            "UPDATE memories SET pinned=?1 WHERE id=?2",
            params![i64::from(pinned), id],
        )?;
        Ok(n == 1)
    }

    /// Whether a memory is pinned (for inspection/tests).
    pub fn is_memory_pinned(&self, id: i64) -> StoreResult<bool> {
        let p: i64 = self.conn.query_row(
            "SELECT pinned FROM memories WHERE id=?1",
            params![id],
            |r| r.get(0),
        )?;
        Ok(p != 0)
    }

    /// Edit a memory's content (Task 7.2 — user correction). Bumps `updated_at`.
    /// Returns true iff a row matched.
    pub fn update_memory_value(&self, id: i64, value: &str, now: &str) -> StoreResult<bool> {
        let n = self.conn.execute(
            "UPDATE memories SET content=?1, updated_at=?2 WHERE id=?3",
            params![value, now, id],
        )?;
        Ok(n == 1)
    }

    /// A memory's current content (for inspection/tests).
    pub fn memory_content(&self, id: i64) -> StoreResult<Option<String>> {
        self.conn
            .query_row(
                "SELECT content FROM memories WHERE id=?1",
                params![id],
                |r| r.get(0),
            )
            .optional()
            .map_err(Into::into)
    }

    // ---- Startup recovery & integrity ----

    /// Mark `running` runs as `interrupted` and give dangling (`queued`/
    /// `running`) tool invocations an interrupted terminal result. Returns the
    /// number of runs repaired.
    pub fn repair_interrupted_runs(&self, now: &str) -> StoreResult<usize> {
        let runs = self.conn.execute(
            "UPDATE agent_runs SET status='interrupted', phase='interrupted', finished_at=?1, stop_reason='interrupted'
             WHERE status='running'",
            [now],
        )?;
        self.conn.execute(
            "UPDATE tool_invocations SET status='interrupted', finished_at=?1, error_code='interrupted'
             WHERE status IN ('queued','running')",
            [now],
        )?;
        Ok(runs)
    }

    /// `PRAGMA foreign_key_check` — returns Err if any violation exists.
    pub fn foreign_key_check(&self) -> StoreResult<()> {
        let mut stmt = self.conn.prepare("PRAGMA foreign_key_check")?;
        let rows: Vec<String> = stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        if rows.is_empty() {
            Ok(())
        } else {
            Err(StoreError::Integrity(format!(
                "foreign key violations in: {}",
                rows.join(", ")
            )))
        }
    }

    /// `PRAGMA integrity_check`.
    pub fn integrity_check(&self) -> StoreResult<()> {
        let res: String = self
            .conn
            .query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
        if res == "ok" {
            Ok(())
        } else {
            Err(StoreError::Integrity(res))
        }
    }

    /// Rebuild + verify the FTS indexes (`'rebuild'` then `'integrity-check'`).
    pub fn fts_check(&self) -> StoreResult<()> {
        self.conn
            .execute_batch("INSERT INTO message_fts(message_fts) VALUES('integrity-check');")?;
        self.conn
            .execute_batch("INSERT INTO memory_fts(memory_fts) VALUES('integrity-check');")?;
        Ok(())
    }

    /// Back up the whole database to `dest` using the online backup API.
    pub fn backup_to(&self, dest: &Path) -> StoreResult<()> {
        let mut out = Connection::open(dest)?;
        let backup = rusqlite::backup::Backup::new(&self.conn, &mut out)?;
        backup.run_to_completion(64, std::time::Duration::from_millis(0), None)?;
        Ok(())
    }

    /// Checkpoint and truncate the WAL, then run one incremental vacuum pass —
    /// the explicit privacy-deletion reclamation step.
    pub fn privacy_reclaim(&self) -> StoreResult<()> {
        self.conn
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE); PRAGMA incremental_vacuum;")?;
        Ok(())
    }

    /// Escape hatch for integration code that needs a raw query during Phase A
    /// bring-up. Prefer typed methods.
    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    /// Mutable connection accessor (needed for the JSON importer's transactions).
    pub fn conn_mut(&mut self) -> &mut Connection {
        &mut self.conn
    }
}

/// Tauri-managed store state: the actor handle plus the migrated default
/// workspace id. Model/network/tool work never holds the connection — all
/// access goes through [`StoreHandle`].
pub struct AppStore {
    pub handle: StoreHandle,
    pub default_workspace_id: String,
}
/// Current time as an ISO-8601 UTC timestamp (lexicographically sortable).
pub fn now_iso() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    iso_utc(secs)
}

/// ISO-8601 UTC timestamp `secs` seconds before now — for expiry cutoffs
/// (e.g. the approval-expiry sweep, Task 4.3).
pub fn iso_seconds_ago(secs: i64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    iso_utc(now - secs)
}

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

/// Open the store under `config_dir`, migrate, ensure the default Personal
/// workspace, repair interrupted runs, reconcile the blob dir, and idempotently
/// import legacy JSON conversations. The JSON directory remains the live source
/// of truth until the Phase G cutover, so this import is non-destructive.
pub fn init(config_dir: &Path) -> StoreResult<(StoreHandle, MigrationReport, String)> {
    let db_path = config_dir.join("finmodel.db");
    let blob_root = config_dir.join("blobs");
    let mut db = Db::open(&db_path, &blob_root)?;
    let now = now_iso();
    let workspace_id = db.ensure_default_personal_workspace(&now)?;
    db.repair_interrupted_runs(&now)?;
    let _ = db.reconcile_blob_dir()?;
    let json_dir = config_dir.join("conversations");
    let mut gen = || {
        let mut bytes = [0u8; 16];
        rand::Rng::fill(&mut rand::thread_rng(), &mut bytes);
        fm_agent::ids::format_uuid_v4(bytes)
    };
    let report = migrations::import_json_conversations(
        db.conn_mut(),
        &json_dir,
        &workspace_id,
        &now,
        &mut gen,
    )?;
    let handle = StoreHandle::spawn(db);
    Ok((handle, report, workspace_id))
}
// ---- helpers ----

fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(HEX[(b >> 4) as usize] as char);
        s.push(HEX[(b & 0x0f) as usize] as char);
    }
    s
}

fn with_tmp_suffix(final_path: &Path) -> PathBuf {
    let name = final_path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("blob");
    let tmp = format!(".tmp-{name}-{}", std::process::id());
    final_path.with_file_name(tmp)
}

fn is_tmp_name(name: &str) -> bool {
    name.starts_with(".tmp-")
}

/// Recursively collect regular files under `root` (shallow, iterative).
fn walk_files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.is_file() {
                out.push(p);
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Store actor: owns a Db on a dedicated blocking thread and serializes access.
// ---------------------------------------------------------------------------

type Job = Box<dyn FnOnce(&mut Db) + Send>;

/// A handle to the store actor. Cloneable; every clone talks to the one Db.
#[derive(Clone)]
pub struct StoreHandle {
    tx: std::sync::mpsc::Sender<Job>,
}

impl StoreHandle {
    /// Spawn the store actor thread owning `db`.
    pub fn spawn(mut db: Db) -> Self {
        let (tx, rx) = std::sync::mpsc::channel::<Job>();
        std::thread::Builder::new()
            .name("finmodel-store".into())
            .spawn(move || {
                while let Ok(job) = rx.recv() {
                    job(&mut db);
                }
            })
            .expect("spawn store actor");
        StoreHandle { tx }
    }

    /// Run `f` against the Db on the actor thread and await its result. The
    /// closure runs inside the single-owner thread, so transactions serialize.
    pub async fn call<T, F>(&self, f: F) -> T
    where
        T: Send + 'static,
        F: FnOnce(&mut Db) -> T + Send + 'static,
    {
        let (otx, orx) = tokio::sync::oneshot::channel();
        let job: Job = Box::new(move |db| {
            let _ = otx.send(f(db));
        });
        self.tx.send(job).expect("store actor alive");
        orx.await.expect("store actor replied")
    }

    /// Fire-and-forget variant for durable writes whose result is not awaited.
    pub fn dispatch<F>(&self, f: F)
    where
        F: FnOnce(&mut Db) + Send + 'static,
    {
        let _ = self.tx.send(Box::new(f));
    }
}

#[cfg(test)]
mod tests;
