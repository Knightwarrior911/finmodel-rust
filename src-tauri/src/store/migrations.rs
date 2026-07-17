//! Schema, PRAGMAs, versioned migrations, and the JSON→SQLite conversation
//! import. `PRAGMA user_version` is the sole schema-version authority.
//!
//! Ordering matters: `auto_vacuum` and `secure_delete` are persistent header
//! settings that must be established on a fresh database *before* any table
//! exists, so [`init_fresh_db`] runs them while `user_version == 0`. Per-
//! connection PRAGMAs (`foreign_keys`, `busy_timeout`, `synchronous`) are
//! reapplied by [`apply_connection_pragmas`] on every open.

use rusqlite::{Connection, Transaction};
use serde_json::Value;

use super::models::{MigrationReport, Quarantined};

/// Current schema version this build understands.
pub const SCHEMA_VERSION: i64 = 1;

/// Per-connection PRAGMAs. Safe to call on every open.
pub fn apply_connection_pragmas(conn: &Connection) -> rusqlite::Result<()> {
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.pragma_update(None, "busy_timeout", 5000)?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    Ok(())
}

/// Establish persistent header PRAGMAs (`auto_vacuum`, `secure_delete`) on a
/// brand-new, zero-page database. These MUST run before any write — including
/// the `journal_mode=WAL` switch and any table creation — because `auto_vacuum`
/// only takes effect while the database has no pages. Safe no-op otherwise.
pub fn init_fresh_if_empty(conn: &Connection) -> rusqlite::Result<()> {
    let has_tables: i64 = conn.query_row(
        "SELECT count(*) FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'",
        [],
        |r| r.get(0),
    )?;
    if has_tables == 0 && user_version(conn)? == 0 {
        conn.pragma_update(None, "auto_vacuum", "INCREMENTAL")?;
        conn.pragma_update(None, "secure_delete", "ON")?;
    }
    Ok(())
}

/// Enable WAL journaling (persistent; no-op on in-memory databases, which do
/// not support WAL). Call after [`init_fresh_if_empty`].
pub fn enable_wal(conn: &Connection) -> rusqlite::Result<()> {
    let _: String = conn.query_row("PRAGMA journal_mode=WAL", [], |r| r.get(0))?;
    Ok(())
}

/// Read `PRAGMA user_version`.
pub fn user_version(conn: &Connection) -> rusqlite::Result<i64> {
    conn.query_row("PRAGMA user_version", [], |r| r.get(0))
}

/// Open-time migration: apply ordered versions up to [`SCHEMA_VERSION`] in one
/// transaction each. Returns the resulting version.
pub fn migrate(conn: &mut Connection) -> rusqlite::Result<i64> {
    let mut v = user_version(conn)?;
    while v < SCHEMA_VERSION {
        let tx = conn.transaction()?;
        match v {
            0 => apply_v1(&tx)?,
            other => {
                return Err(rusqlite::Error::SqliteFailure(
                    rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_ERROR),
                    Some(format!("no migration from version {other}")),
                ))
            }
        }
        tx.pragma_update(None, "user_version", v + 1)?;
        tx.commit()?;
        v += 1;
    }
    Ok(v)
}

/// The v1 schema: all tables, FTS5 external-content indexes, and sync triggers.
fn apply_v1(tx: &Transaction) -> rusqlite::Result<()> {
    tx.execute_batch(
        r#"
CREATE TABLE workspaces (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  kind TEXT NOT NULL,
  confidentiality TEXT NOT NULL,
  standing_instructions TEXT NOT NULL DEFAULT '',
  memory_enabled INTEGER NOT NULL DEFAULT 1,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE workspace_public_entities (
  workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
  entity_id TEXT NOT NULL,
  canonical_name TEXT NOT NULL,
  PRIMARY KEY (workspace_id, entity_id)
);

CREATE TABLE conversations (
  id TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
  title TEXT NOT NULL DEFAULT '',
  pinned INTEGER NOT NULL DEFAULT 0,
  archived INTEGER NOT NULL DEFAULT 0,
  summary TEXT,
  active_leaf_message_id TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
CREATE INDEX idx_conversations_workspace ON conversations(workspace_id);

CREATE TABLE messages (
  id TEXT PRIMARY KEY,
  conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
  ordinal INTEGER NOT NULL,
  parent_message_id TEXT REFERENCES messages(id) ON DELETE CASCADE,
  role TEXT NOT NULL,
  context_summary TEXT,
  status TEXT NOT NULL DEFAULT 'complete',
  created_at TEXT NOT NULL,
  UNIQUE (conversation_id, ordinal)
);
CREATE INDEX idx_messages_conversation ON messages(conversation_id);
CREATE INDEX idx_messages_parent ON messages(parent_message_id);

CREATE TABLE message_parts (
  id TEXT PRIMARY KEY,
  message_id TEXT NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
  ordinal INTEGER NOT NULL,
  kind TEXT NOT NULL,
  payload_json TEXT NOT NULL,
  search_text TEXT
);
CREATE INDEX idx_parts_message ON message_parts(message_id);

CREATE TABLE agent_runs (
  id TEXT PRIMARY KEY,
  conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
  user_message_id TEXT REFERENCES messages(id) ON DELETE SET NULL,
  resumed_from_run_id TEXT REFERENCES agent_runs(id) ON DELETE SET NULL,
  status TEXT NOT NULL,
  phase TEXT NOT NULL,
  model TEXT,
  policy TEXT,
  started_at TEXT NOT NULL,
  finished_at TEXT,
  stop_reason TEXT,
  usage_json TEXT,
  last_sequence INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX idx_runs_conversation ON agent_runs(conversation_id);
CREATE INDEX idx_runs_status ON agent_runs(status);

CREATE TABLE run_events (
  id TEXT PRIMARY KEY,
  run_id TEXT NOT NULL REFERENCES agent_runs(id) ON DELETE CASCADE,
  sequence INTEGER NOT NULL,
  kind TEXT NOT NULL,
  payload_json TEXT NOT NULL,
  created_at TEXT NOT NULL,
  UNIQUE (run_id, sequence)
);

CREATE TABLE tool_invocations (
  id TEXT PRIMARY KEY,
  run_id TEXT NOT NULL REFERENCES agent_runs(id) ON DELETE CASCADE,
  parent_invocation_id TEXT REFERENCES tool_invocations(id) ON DELETE CASCADE,
  batch_id TEXT,
  tool_name TEXT NOT NULL,
  status TEXT NOT NULL,
  risk TEXT NOT NULL,
  canonical_args_json TEXT,
  result_summary_json TEXT,
  started_at TEXT,
  finished_at TEXT,
  error_code TEXT
);
CREATE INDEX idx_tools_run ON tool_invocations(run_id);

CREATE TABLE pending_interactions (
  id TEXT PRIMARY KEY,
  run_id TEXT NOT NULL REFERENCES agent_runs(id) ON DELETE CASCADE,
  tool_call_id TEXT,
  kind TEXT NOT NULL,
  request_json TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'pending',
  response_json TEXT,
  created_at TEXT NOT NULL,
  resolved_at TEXT
);
CREATE INDEX idx_pending_run ON pending_interactions(run_id);

CREATE TABLE sources (
  id TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
  kind TEXT NOT NULL,
  canonical_uri TEXT NOT NULL,
  title TEXT,
  publisher TEXT,
  published_at TEXT,
  accessed_at TEXT,
  content_hash TEXT
);
CREATE INDEX idx_sources_workspace ON sources(workspace_id);

CREATE TABLE citations (
  message_part_id TEXT NOT NULL REFERENCES message_parts(id) ON DELETE CASCADE,
  ordinal INTEGER NOT NULL,
  source_id TEXT NOT NULL REFERENCES sources(id) ON DELETE CASCADE,
  claim_key TEXT NOT NULL,
  entity TEXT,
  normalized_value TEXT,
  unit TEXT,
  currency TEXT,
  scale TEXT,
  period TEXT,
  locator TEXT,
  quote_hash TEXT,
  PRIMARY KEY (message_part_id, ordinal)
);

CREATE TABLE blobs (
  id TEXT PRIMARY KEY,
  relative_path TEXT NOT NULL,
  sha256 TEXT NOT NULL,
  byte_len INTEGER NOT NULL,
  created_at TEXT NOT NULL
);

CREATE TABLE blob_refs (
  blob_id TEXT NOT NULL REFERENCES blobs(id) ON DELETE CASCADE,
  owner_kind TEXT NOT NULL,
  owner_id TEXT NOT NULL,
  PRIMARY KEY (blob_id, owner_kind, owner_id)
);

CREATE TABLE blob_gc (
  blob_id TEXT PRIMARY KEY,
  queued_at TEXT NOT NULL,
  last_error TEXT
);

CREATE TABLE artifacts (
  id TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
  conversation_id TEXT REFERENCES conversations(id) ON DELETE SET NULL,
  run_id TEXT REFERENCES agent_runs(id) ON DELETE SET NULL,
  kind TEXT NOT NULL,
  label TEXT NOT NULL,
  mime TEXT NOT NULL,
  blob_id TEXT REFERENCES blobs(id) ON DELETE SET NULL,
  version INTEGER NOT NULL DEFAULT 1,
  parent_artifact_id TEXT REFERENCES artifacts(id) ON DELETE SET NULL,
  sha256 TEXT NOT NULL,
  created_at TEXT NOT NULL
);
CREATE INDEX idx_artifacts_workspace ON artifacts(workspace_id);

CREATE TABLE memories (
  id INTEGER PRIMARY KEY,
  public_id TEXT NOT NULL UNIQUE,
  scope_type TEXT NOT NULL,
  workspace_id TEXT REFERENCES workspaces(id) ON DELETE CASCADE,
  conversation_id TEXT REFERENCES conversations(id) ON DELETE CASCADE,
  kind TEXT NOT NULL,
  content TEXT NOT NULL,
  normalized_key TEXT NOT NULL,
  importance REAL NOT NULL DEFAULT 0.5,
  confidence REAL NOT NULL DEFAULT 0.5,
  valid_from TEXT,
  valid_to TEXT,
  source_type TEXT NOT NULL,
  source_ref TEXT,
  superseded_by INTEGER REFERENCES memories(id) ON DELETE SET NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
CREATE INDEX idx_memories_scope ON memories(scope_type, workspace_id, conversation_id);
CREATE INDEX idx_memories_key ON memories(normalized_key);

CREATE TABLE memory_uses (
  run_id TEXT NOT NULL REFERENCES agent_runs(id) ON DELETE CASCADE,
  memory_id INTEGER NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
  rank INTEGER NOT NULL,
  created_at TEXT NOT NULL,
  PRIMARY KEY (run_id, memory_id)
);

-- FTS5 external-content index over message part visible text.
CREATE VIRTUAL TABLE message_fts USING fts5(
  search_text,
  content='message_parts',
  content_rowid='rowid'
);
CREATE TRIGGER message_parts_ai AFTER INSERT ON message_parts BEGIN
  INSERT INTO message_fts(rowid, search_text) VALUES (new.rowid, new.search_text);
END;
CREATE TRIGGER message_parts_ad AFTER DELETE ON message_parts BEGIN
  INSERT INTO message_fts(message_fts, rowid, search_text) VALUES('delete', old.rowid, old.search_text);
END;
CREATE TRIGGER message_parts_au AFTER UPDATE ON message_parts BEGIN
  INSERT INTO message_fts(message_fts, rowid, search_text) VALUES('delete', old.rowid, old.search_text);
  INSERT INTO message_fts(rowid, search_text) VALUES (new.rowid, new.search_text);
END;

-- FTS5 external-content index over memory content + normalized key.
CREATE VIRTUAL TABLE memory_fts USING fts5(
  content,
  normalized_key,
  content='memories',
  content_rowid='id'
);
CREATE TRIGGER memories_ai AFTER INSERT ON memories BEGIN
  INSERT INTO memory_fts(rowid, content, normalized_key) VALUES (new.id, new.content, new.normalized_key);
END;
CREATE TRIGGER memories_ad AFTER DELETE ON memories BEGIN
  INSERT INTO memory_fts(memory_fts, rowid, content, normalized_key) VALUES('delete', old.id, old.content, old.normalized_key);
END;
CREATE TRIGGER memories_au AFTER UPDATE ON memories BEGIN
  INSERT INTO memory_fts(memory_fts, rowid, content, normalized_key) VALUES('delete', old.id, old.content, old.normalized_key);
  INSERT INTO memory_fts(rowid, content, normalized_key) VALUES (new.id, new.content, new.normalized_key);
END;
"#,
    )
}

/// Escape a string for embedding as a single-quoted SQLite literal in triggers
/// is unnecessary here; kept intentionally small — no dynamic SQL in v1.
// (No dynamic DDL; migrations are static.)

/// Import legacy JSON conversations into the given (already-migrated) DB.
///
/// - Idempotent by conversation id: a conversation whose id already exists is
///   skipped and counted.
/// - Consecutive assistant messages between user turns are grouped into one
///   logical assistant message; `content` becomes a `text` part and any `card`
///   becomes a `result` part, in order. `llm_context` becomes the assistant
///   message `context_summary`.
/// - The last imported message is set as the conversation active leaf.
/// - Malformed JSON files are quarantined by filename, never discarded.
pub fn import_json_conversations(
    conn: &mut Connection,
    json_dir: &std::path::Path,
    default_workspace_id: &str,
    now: &str,
    new_id: &mut dyn FnMut() -> String,
) -> rusqlite::Result<MigrationReport> {
    let mut report = MigrationReport::default();
    let entries = match std::fs::read_dir(json_dir) {
        Ok(e) => e,
        Err(_) => return Ok(report), // no legacy dir: nothing to import
    };

    let mut files: Vec<std::path::PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().map(|x| x == "json").unwrap_or(false))
        .collect();
    files.sort();

    for path in files {
        let fname = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("<unknown>")
            .to_string();
        let raw = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => {
                report.quarantined.push(Quarantined {
                    filename: fname,
                    error: format!("read error: {e}"),
                });
                continue;
            }
        };
        let json: Value = match serde_json::from_str(&raw) {
            Ok(v) => v,
            Err(e) => {
                report.quarantined.push(Quarantined {
                    filename: fname,
                    error: format!("parse error: {e}"),
                });
                continue;
            }
        };
        match import_one(conn, &json, default_workspace_id, now, new_id) {
            Ok(ImportOutcome::Imported { messages }) => {
                report.imported_conversations += 1;
                report.imported_messages += messages;
            }
            Ok(ImportOutcome::Skipped) => report.skipped_existing += 1,
            Err(e) => report.quarantined.push(Quarantined {
                filename: fname,
                error: format!("import error: {e}"),
            }),
        }
    }
    Ok(report)
}

enum ImportOutcome {
    Imported { messages: usize },
    Skipped,
}

fn import_one(
    conn: &mut Connection,
    json: &Value,
    workspace_id: &str,
    now: &str,
    new_id: &mut dyn FnMut() -> String,
) -> rusqlite::Result<ImportOutcome> {
    let conv_id = json
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| bad("conversation missing id"))?
        .to_string();

    // Idempotent by id.
    let exists: i64 = conn.query_row(
        "SELECT count(*) FROM conversations WHERE id = ?1",
        [&conv_id],
        |r| r.get(0),
    )?;
    if exists > 0 {
        return Ok(ImportOutcome::Skipped);
    }

    let title = json
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let created = json
        .get("created")
        .and_then(|v| v.as_str())
        .unwrap_or(now)
        .to_string();
    let updated = json
        .get("updated")
        .and_then(|v| v.as_str())
        .unwrap_or(now)
        .to_string();
    let msgs = json
        .get("messages")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let tx = conn.transaction()?;
    tx.execute(
        "INSERT INTO conversations (id, workspace_id, title, pinned, archived, summary, active_leaf_message_id, created_at, updated_at)
         VALUES (?1, ?2, ?3, 0, 0, NULL, NULL, ?4, ?5)",
        rusqlite::params![conv_id, workspace_id, title, created, updated],
    )?;

    // Group consecutive assistant messages into one logical assistant message.
    let mut ordinal: i64 = 0;
    let mut parent: Option<String> = None;
    let mut last_message_id: Option<String> = None;
    let mut imported_messages = 0usize;

    let mut i = 0usize;
    while i < msgs.len() {
        let role = msgs[i].get("role").and_then(|v| v.as_str()).unwrap_or("");
        if role == "assistant" {
            // Collect the run of consecutive assistant messages.
            let mut group: Vec<&Value> = Vec::new();
            while i < msgs.len()
                && msgs[i].get("role").and_then(|v| v.as_str()) == Some("assistant")
            {
                group.push(&msgs[i]);
                i += 1;
            }
            let mid = new_id();
            // context_summary: prefer the last non-empty llm_context in the group.
            let ctx = group
                .iter()
                .rev()
                .find_map(|m| m.get("llm_context").and_then(|v| v.as_str()))
                .map(|s| s.to_string());
            let ts = group
                .first()
                .and_then(|m| m.get("ts").and_then(|v| v.as_str()))
                .unwrap_or(now);
            tx.execute(
                "INSERT INTO messages (id, conversation_id, ordinal, parent_message_id, role, context_summary, status, created_at)
                 VALUES (?1, ?2, ?3, ?4, 'assistant', ?5, 'complete', ?6)",
                rusqlite::params![mid, conv_id, ordinal, parent, ctx, ts],
            )?;
            // Ordered parts: for each message, content -> text part, card -> result part.
            let mut part_ord: i64 = 0;
            for m in &group {
                if let Some(content) = m.get("content").and_then(|v| v.as_str()) {
                    if !content.is_empty() {
                        insert_part(&tx, new_id(), &mid, part_ord, "text",
                            &Value::String(content.to_string()), Some(content))?;
                        part_ord += 1;
                    }
                }
                if let Some(card) = m.get("card") {
                    if !card.is_null() {
                        let st = card_search_text(card);
                        insert_part(&tx, new_id(), &mid, part_ord, "result", card, st.as_deref())?;
                        part_ord += 1;
                    }
                }
            }
            parent = Some(mid.clone());
            last_message_id = Some(mid);
            ordinal += 1;
            imported_messages += 1;
        } else {
            // user (or system) message: one message, one text part.
            let mid = new_id();
            let content = msgs[i].get("content").and_then(|v| v.as_str()).unwrap_or("");
            let ts = msgs[i].get("ts").and_then(|v| v.as_str()).unwrap_or(now);
            let role_norm = if role == "user" { "user" } else { "system" };
            tx.execute(
                "INSERT INTO messages (id, conversation_id, ordinal, parent_message_id, role, context_summary, status, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, NULL, 'complete', ?6)",
                rusqlite::params![mid, conv_id, ordinal, parent, role_norm, ts],
            )?;
            insert_part(&tx, new_id(), &mid, 0, "text",
                &Value::String(content.to_string()), Some(content))?;
            parent = Some(mid.clone());
            last_message_id = Some(mid);
            ordinal += 1;
            imported_messages += 1;
            i += 1;
        }
    }

    if let Some(leaf) = &last_message_id {
        tx.execute(
            "UPDATE conversations SET active_leaf_message_id = ?1 WHERE id = ?2",
            rusqlite::params![leaf, conv_id],
        )?;
    }
    tx.commit()?;
    Ok(ImportOutcome::Imported {
        messages: imported_messages,
    })
}

fn insert_part(
    tx: &Transaction,
    id: String,
    message_id: &str,
    ordinal: i64,
    kind: &str,
    payload: &Value,
    search_text: Option<&str>,
) -> rusqlite::Result<()> {
    let payload_json = serde_json::to_string(payload).unwrap_or_else(|_| "null".into());
    tx.execute(
        "INSERT INTO message_parts (id, message_id, ordinal, kind, payload_json, search_text)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![id, message_id, ordinal, kind, payload_json, search_text],
    )?;
    Ok(())
}

/// Extract sanitized searchable text from a legacy card payload (title/label/
/// ticker style fields), best-effort.
fn card_search_text(card: &Value) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    for key in ["title", "label", "ticker", "kind", "summary", "name"] {
        if let Some(s) = card.get(key).and_then(|v| v.as_str()) {
            if !s.is_empty() {
                parts.push(s.to_string());
            }
        }
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" "))
    }
}

fn bad(msg: &str) -> rusqlite::Error {
    rusqlite::Error::SqliteFailure(
        rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_ERROR),
        Some(msg.to_string()),
    )
}
