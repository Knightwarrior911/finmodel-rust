//! Memory repository trait + implementations (Phase E).
//!
//! A narrow [`MemoryRepository`] so the pure `AgentMachine` tests can use an
//! in-memory fake while production uses SQLite. Not a backend/plugin contract:
//! just enough surface for capture and recall.

use std::collections::HashMap;

use crate::store::models::Memory;

/// Search result with rank.
#[derive(Clone, Debug)]
pub struct MemorySearchResult {
    pub memory: Memory,
    pub rank: f64,
}

/// Repository for scoped persistent memories.
///
/// Methods return [`MemoryError`] on backend failures.
pub trait MemoryRepository {
    /// Insert a new memory. Returns the assigned id.
    fn insert(&mut self, memory: NewMemory) -> Result<i64, MemoryError>;

    /// Fetch a memory by DB id.
    fn get(&self, id: i64) -> Result<Option<Memory>, MemoryError>;

    /// Fetch a memory by public_id (stable across resets in the fake).
    fn get_by_public_id(&self, public_id: &str) -> Result<Option<Memory>, MemoryError>;

    /// Full-text search across memories matching scope.
    fn search(
        &self,
        query: &str,
        scope: &MemoryScope,
    ) -> Result<Vec<MemorySearchResult>, MemoryError>;

    /// Supersede a memory: close its `valid_to` and link to `superseded_by`.
    fn supersede(&mut self, id: i64, superseded_by: i64, now: &str) -> Result<(), MemoryError>;

    /// Delete a memory by id.
    fn delete(&mut self, id: i64) -> Result<(), MemoryError>;

    /// Record a memory use in a run for recall explainability.
    fn record_use(&mut self, run_id: &str, memory_id: i64, rank: f64) -> Result<(), MemoryError>;
}

/// Scope filter for memory queries.
#[derive(Clone, Debug, Default)]
pub struct MemoryScope {
    pub workspace_id: Option<String>,
    pub conversation_id: Option<String>,
    /// Only `global`-scope memories (user preferences).
    pub global_only: bool,
}

/// Input for inserting a new memory.
#[derive(Clone, Debug)]
pub struct NewMemory {
    pub public_id: String,
    pub scope_type: String,
    pub workspace_id: Option<String>,
    pub conversation_id: Option<String>,
    pub kind: String,
    pub content: String,
    pub normalized_key: String,
    pub importance: f64,
    pub confidence: f64,
    pub source_type: String,
    pub source_ref: Option<String>,
    pub now: String,
}

/// Memory repository errors.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MemoryError {
    NotFound(i64),
    Backend(String),
}

impl std::fmt::Display for MemoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MemoryError::NotFound(id) => write!(f, "memory {id} not found"),
            MemoryError::Backend(msg) => write!(f, "memory backend: {msg}"),
        }
    }
}

// ── In-memory fake (for AgentMachine tests) ────────────────────────────

/// In-memory [`MemoryRepository`] — does not persist across restarts.
#[derive(Clone, Debug, Default)]
pub struct InMemoryMemoryRepository {
    memories: HashMap<i64, Memory>,
    uses: Vec<(String, i64, f64)>,
    next_id: i64,
}

impl InMemoryMemoryRepository {
    pub fn new() -> Self {
        Self::default()
    }
}

impl MemoryRepository for InMemoryMemoryRepository {
    fn insert(&mut self, m: NewMemory) -> Result<i64, MemoryError> {
        let id = self.next_id;
        self.next_id += 1;
        let mem = Memory {
            id,
            public_id: m.public_id,
            scope_type: m.scope_type,
            workspace_id: m.workspace_id,
            conversation_id: m.conversation_id,
            kind: m.kind,
            content: m.content,
            normalized_key: m.normalized_key,
            importance: m.importance,
            confidence: m.confidence,
            valid_from: Some(m.now.clone()),
            valid_to: None,
            source_type: m.source_type,
            source_ref: m.source_ref,
            superseded_by: None,
            created_at: m.now.clone(),
            updated_at: m.now,
        };
        self.memories.insert(id, mem);
        Ok(id)
    }

    fn get(&self, id: i64) -> Result<Option<Memory>, MemoryError> {
        Ok(self.memories.get(&id).cloned())
    }

    fn get_by_public_id(&self, public_id: &str) -> Result<Option<Memory>, MemoryError> {
        Ok(self
            .memories
            .values()
            .find(|m| m.public_id == public_id)
            .cloned())
    }

    fn search(
        &self,
        query: &str,
        scope: &MemoryScope,
    ) -> Result<Vec<MemorySearchResult>, MemoryError> {
        let q = query.to_lowercase();
        let mut results: Vec<MemorySearchResult> = self
            .memories
            .values()
            .filter(|m| {
                if scope.global_only && m.scope_type != "global" {
                    return false;
                }
                if let Some(ref wid) = scope.workspace_id {
                    if m.workspace_id.as_ref() != Some(wid) {
                        return false;
                    }
                }
                if let Some(ref cid) = scope.conversation_id {
                    if m.conversation_id.as_ref() != Some(cid) {
                        return false;
                    }
                }
                // FTS-style token match (simple contains).
                let text = format!(
                    "{} {}",
                    m.content.to_lowercase(),
                    m.normalized_key.to_lowercase()
                );
                q.split_whitespace().all(|token| text.contains(token))
            })
            .map(|m| MemorySearchResult {
                memory: m.clone(),
                rank: m.importance,
            })
            .collect();
        results.sort_by(|a, b| {
            b.rank
                .partial_cmp(&a.rank)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        Ok(results)
    }

    fn supersede(&mut self, id: i64, superseded_by: i64, now: &str) -> Result<(), MemoryError> {
        let m = self
            .memories
            .get_mut(&id)
            .ok_or(MemoryError::NotFound(id))?;
        m.valid_to = Some(now.to_string());
        m.superseded_by = Some(superseded_by);
        m.updated_at = now.to_string();
        Ok(())
    }

    fn delete(&mut self, id: i64) -> Result<(), MemoryError> {
        self.memories.remove(&id).ok_or(MemoryError::NotFound(id))?;
        Ok(())
    }

    fn record_use(&mut self, run_id: &str, memory_id: i64, rank: f64) -> Result<(), MemoryError> {
        self.uses.push((run_id.to_string(), memory_id, rank));
        Ok(())
    }
}

// ── SQLite backend (wraps Db directly) ─────────────────────────────────

use crate::store::Db;

/// SQLite-backed [`MemoryRepository`] using the `memories` + `memory_fts`
/// tables defined in [`crate::store::migrations`].
pub struct SqliteMemoryRepository<'a> {
    db: &'a Db,
}

impl<'a> SqliteMemoryRepository<'a> {
    pub fn new(db: &'a Db) -> Self {
        Self { db }
    }
}

impl MemoryRepository for SqliteMemoryRepository<'_> {
    fn insert(&mut self, m: NewMemory) -> Result<i64, MemoryError> {
        self.db
            .insert_memory(
                &m.public_id,
                &m.scope_type,
                m.workspace_id.as_deref(),
                m.conversation_id.as_deref(),
                &m.kind,
                &m.content,
                &m.normalized_key,
                m.importance,
                m.confidence,
                &m.source_type,
                m.source_ref.as_deref(),
                &m.now,
            )
            .map_err(|e| MemoryError::Backend(e.to_string()))
    }

    fn get(&self, id: i64) -> Result<Option<Memory>, MemoryError> {
        let result = self.db.conn.query_row(
            "SELECT id,public_id,scope_type,workspace_id,conversation_id,kind,content,
                    normalized_key,importance,confidence,valid_from,valid_to,
                    source_type,source_ref,superseded_by,created_at,updated_at
             FROM memories WHERE id=?1",
            [id],
            |r| {
                Ok(Memory {
                    id: r.get(0)?,
                    public_id: r.get(1)?,
                    scope_type: r.get(2)?,
                    workspace_id: r.get(3)?,
                    conversation_id: r.get(4)?,
                    kind: r.get(5)?,
                    content: r.get(6)?,
                    normalized_key: r.get(7)?,
                    importance: r.get(8)?,
                    confidence: r.get(9)?,
                    valid_from: r.get(10)?,
                    valid_to: r.get(11)?,
                    source_type: r.get(12)?,
                    source_ref: r.get(13)?,
                    superseded_by: r.get(14)?,
                    created_at: r.get(15)?,
                    updated_at: r.get(16)?,
                })
            },
        );
        match result {
            Ok(m) => Ok(Some(m)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(MemoryError::Backend(e.to_string())),
        }
    }

    fn get_by_public_id(&self, public_id: &str) -> Result<Option<Memory>, MemoryError> {
        let result = self.db.conn.query_row(
            "SELECT id,public_id,scope_type,workspace_id,conversation_id,kind,content,
                    normalized_key,importance,confidence,valid_from,valid_to,
                    source_type,source_ref,superseded_by,created_at,updated_at
             FROM memories WHERE public_id=?1",
            [public_id],
            |r| {
                Ok(Memory {
                    id: r.get(0)?,
                    public_id: r.get(1)?,
                    scope_type: r.get(2)?,
                    workspace_id: r.get(3)?,
                    conversation_id: r.get(4)?,
                    kind: r.get(5)?,
                    content: r.get(6)?,
                    normalized_key: r.get(7)?,
                    importance: r.get(8)?,
                    confidence: r.get(9)?,
                    valid_from: r.get(10)?,
                    valid_to: r.get(11)?,
                    source_type: r.get(12)?,
                    source_ref: r.get(13)?,
                    superseded_by: r.get(14)?,
                    created_at: r.get(15)?,
                    updated_at: r.get(16)?,
                })
            },
        );
        match result {
            Ok(m) => Ok(Some(m)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(MemoryError::Backend(e.to_string())),
        }
    }

    fn search(
        &self,
        query: &str,
        scope: &MemoryScope,
    ) -> Result<Vec<MemorySearchResult>, MemoryError> {
        let mut conditions = vec!["memory_fts MATCH ?1".to_string()];
        let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = vec![Box::new(query.to_string())];
        let mut param_idx = 2;

        if scope.global_only {
            conditions.push(format!("m.scope_type = ?{param_idx}"));
            params.push(Box::new("global".to_string()));
            param_idx += 1;
        }
        if let Some(ref wid) = scope.workspace_id {
            conditions.push(format!("m.workspace_id = ?{param_idx}"));
            params.push(Box::new(wid.clone()));
            param_idx += 1;
        }
        if let Some(ref cid) = scope.conversation_id {
            conditions.push(format!("m.conversation_id = ?{param_idx}"));
            params.push(Box::new(cid.clone()));
        }

        let sql = format!(
            "SELECT m.id,m.public_id,m.scope_type,m.workspace_id,m.conversation_id,m.kind,m.content,
                    m.normalized_key,m.importance,m.confidence,m.valid_from,m.valid_to,
                    m.source_type,m.source_ref,m.superseded_by,m.created_at,m.updated_at,
                    rank
             FROM memory_fts
             JOIN memories m ON m.id = memory_fts.rowid
             WHERE {} AND m.valid_to IS NULL
             ORDER BY rank
             LIMIT 20",
            conditions.join(" AND ")
        );

        let mut stmt = self
            .db
            .conn
            .prepare(&sql)
            .map_err(|e| MemoryError::Backend(e.to_string()))?;
        let param_refs: Vec<&dyn rusqlite::types::ToSql> =
            params.iter().map(|p| p.as_ref()).collect();

        let rows = stmt
            .query_map(param_refs.as_slice(), |r| {
                Ok(MemorySearchResult {
                    memory: Memory {
                        id: r.get(0)?,
                        public_id: r.get(1)?,
                        scope_type: r.get(2)?,
                        workspace_id: r.get(3)?,
                        conversation_id: r.get(4)?,
                        kind: r.get(5)?,
                        content: r.get(6)?,
                        normalized_key: r.get(7)?,
                        importance: r.get(8)?,
                        confidence: r.get(9)?,
                        valid_from: r.get(10)?,
                        valid_to: r.get(11)?,
                        source_type: r.get(12)?,
                        source_ref: r.get(13)?,
                        superseded_by: r.get(14)?,
                        created_at: r.get(15)?,
                        updated_at: r.get(16)?,
                    },
                    rank: r.get::<_, f64>(17)?,
                })
            })
            .map_err(|e| MemoryError::Backend(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| MemoryError::Backend(e.to_string()))?;

        Ok(rows)
    }

    fn supersede(&mut self, id: i64, superseded_by: i64, now: &str) -> Result<(), MemoryError> {
        let affected = self
            .db
            .conn
            .execute(
                "UPDATE memories SET valid_to=?1, superseded_by=?2, updated_at=?1 WHERE id=?3",
                rusqlite::params![now, superseded_by, id],
            )
            .map_err(|e| MemoryError::Backend(e.to_string()))?;
        if affected == 0 {
            return Err(MemoryError::NotFound(id));
        }
        Ok(())
    }

    fn delete(&mut self, id: i64) -> Result<(), MemoryError> {
        let affected = self
            .db
            .conn
            .execute("DELETE FROM memories WHERE id=?1", [id])
            .map_err(|e| MemoryError::Backend(e.to_string()))?;
        if affected == 0 {
            return Err(MemoryError::NotFound(id));
        }
        Ok(())
    }

    fn record_use(&mut self, run_id: &str, memory_id: i64, rank: f64) -> Result<(), MemoryError> {
        self.db
            .conn
            .execute(
                "INSERT OR IGNORE INTO memory_uses (run_id, memory_id, rank, created_at)
                 VALUES (?1, ?2, ?3, datetime('now'))",
                rusqlite::params![run_id, memory_id, rank],
            )
            .map_err(|e| MemoryError::Backend(e.to_string()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Db;
    use tempfile::TempDir;

    fn sample_memory() -> NewMemory {
        NewMemory {
            public_id: "mem1".into(),
            scope_type: "workspace".into(),
            workspace_id: Some("ws1".into()),
            conversation_id: Some("conv1".into()),
            kind: "preference".into(),
            content: "User prefers P/E multiple over EV/EBITDA".into(),
            normalized_key: "preference:valuation:multiple".into(),
            importance: 0.8,
            confidence: 0.9,
            source_type: "user".into(),
            source_ref: Some("turn-42".into()),
            now: "2026-07-17T12:00:00Z".into(),
        }
    }

    // ── In-memory tests ──

    #[test]
    fn in_memory_insert_and_get() {
        let mut repo = InMemoryMemoryRepository::new();
        let id = repo.insert(sample_memory()).unwrap();
        assert_eq!(id, 0);
        let m = repo.get(id).unwrap().unwrap();
        assert_eq!(m.public_id, "mem1");
        assert_eq!(m.content, "User prefers P/E multiple over EV/EBITDA");
    }

    #[test]
    fn in_memory_get_by_public_id() {
        let mut repo = InMemoryMemoryRepository::new();
        repo.insert(sample_memory()).unwrap();
        let m = repo.get_by_public_id("mem1").unwrap().unwrap();
        assert_eq!(m.id, 0);
    }

    #[test]
    fn in_memory_search_scope() {
        let mut repo = InMemoryMemoryRepository::new();
        repo.insert(NewMemory {
            public_id: "g1".into(),
            scope_type: "global".into(),
            content: "Dark theme preferred".into(),
            normalized_key: "theme:dark".into(),
            ..sample_memory()
        })
        .unwrap();

        let results = repo
            .search(
                "dark",
                &MemoryScope {
                    global_only: true,
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].memory.public_id, "g1");
    }

    #[test]
    fn in_memory_search_no_cross_workspace() {
        let mut repo = InMemoryMemoryRepository::new();
        repo.insert(NewMemory {
            public_id: "w1".into(),
            workspace_id: Some("ws1".into()),
            content: "Deal data".into(),
            normalized_key: "deal:data".into(),
            ..sample_memory()
        })
        .unwrap();
        repo.insert(NewMemory {
            public_id: "w2".into(),
            workspace_id: Some("ws2".into()),
            content: "Deal data".into(),
            normalized_key: "deal:data".into(),
            ..sample_memory()
        })
        .unwrap();

        let results = repo
            .search(
                "Deal",
                &MemoryScope {
                    workspace_id: Some("ws1".into()),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].memory.public_id, "w1");
    }

    #[test]
    fn in_memory_supersede() {
        let mut repo = InMemoryMemoryRepository::new();
        let id1 = repo.insert(sample_memory()).unwrap();
        let id2 = repo
            .insert(NewMemory {
                public_id: "mem2".into(),
                content: "User prefers EV/EBITDA".into(),
                normalized_key: "preference:valuation:multiple".into(),
                ..sample_memory()
            })
            .unwrap();

        repo.supersede(id1, id2, "2026-07-17T13:00:00Z").unwrap();
        let m1 = repo.get(id1).unwrap().unwrap();
        assert_eq!(m1.valid_to, Some("2026-07-17T13:00:00Z".into()));
        assert_eq!(m1.superseded_by, Some(1));
    }

    #[test]
    fn in_memory_delete() {
        let mut repo = InMemoryMemoryRepository::new();
        let id = repo.insert(sample_memory()).unwrap();
        assert!(repo.get(id).unwrap().is_some());
        repo.delete(id).unwrap();
        assert!(repo.get(id).unwrap().is_none());
    }

    #[test]
    fn in_memory_record_use() {
        let mut repo = InMemoryMemoryRepository::new();
        let id = repo.insert(sample_memory()).unwrap();
        repo.record_use("run1", id, 0.9).unwrap();
    }

    #[test]
    fn in_memory_search_empty_query_returns_all() {
        let mut repo = InMemoryMemoryRepository::new();
        repo.insert(sample_memory()).unwrap();
        let results = repo.search("", &MemoryScope::default()).unwrap();
        assert_eq!(results.len(), 1);
    }

    // ── SQLite helpers ──

    fn sqlite_setup() -> (Db, String, TempDir) {
        let td = TempDir::new().unwrap();
        let db = Db::open_in_memory(&td.path().join("blobs")).unwrap();
        let ws_id = "ws1".to_string();
        db.create_workspace(
            &ws_id,
            "Test",
            "personal",
            "standard",
            "",
            true,
            "2026-07-17T12:00:00Z",
        )
        .unwrap();
        (db, ws_id, td)
    }

    fn sqlite_mem(ws: &str) -> NewMemory {
        NewMemory {
            public_id: "mem1".into(),
            scope_type: "workspace".into(),
            workspace_id: Some(ws.to_string()),
            conversation_id: None,
            kind: "preference".into(),
            content: "PE multiple preference".into(),
            normalized_key: "pref:val:pe".into(),
            importance: 0.8,
            confidence: 0.9,
            source_type: "user".into(),
            source_ref: Some("t42".into()),
            now: "2026-07-17T12:00:00Z".into(),
        }
    }

    #[test]
    fn sqlite_insert_and_get() {
        let (db, ws, _td) = sqlite_setup();
        let mut repo = SqliteMemoryRepository::new(&db);
        let id = repo.insert(sqlite_mem(&ws)).unwrap();
        assert!(id > 0);
        let m = repo.get(id).unwrap().unwrap();
        assert_eq!(m.public_id, "mem1");
    }

    #[test]
    fn sqlite_get_nonexistent() {
        let (db, _ws, _td) = sqlite_setup();
        let repo = SqliteMemoryRepository::new(&db);
        assert!(repo.get(999).unwrap().is_none());
    }

    #[test]
    fn sqlite_get_by_public_id() {
        let (db, ws, _td) = sqlite_setup();
        let mut repo = SqliteMemoryRepository::new(&db);
        repo.insert(sqlite_mem(&ws)).unwrap();
        let m = repo.get_by_public_id("mem1").unwrap().unwrap();
        assert!(m.id > 0);
    }

    #[test]
    fn sqlite_supersede() {
        let (db, ws, _td) = sqlite_setup();
        let mut repo = SqliteMemoryRepository::new(&db);
        let id1 = repo.insert(sqlite_mem(&ws)).unwrap();
        let id2 = repo
            .insert(NewMemory {
                public_id: "mem2".into(),
                content: "Updated preference".into(),
                normalized_key: "pref:val:ev".into(),
                ..sqlite_mem(&ws)
            })
            .unwrap();
        repo.supersede(id1, id2, "2026-07-17T14:00:00Z").unwrap();
        let m1 = repo.get(id1).unwrap().unwrap();
        assert_eq!(m1.valid_to, Some("2026-07-17T14:00:00Z".into()));
        assert_eq!(m1.superseded_by, Some(id2));
    }

    #[test]
    fn sqlite_delete() {
        let (db, ws, _td) = sqlite_setup();
        let mut repo = SqliteMemoryRepository::new(&db);
        let id = repo.insert(sqlite_mem(&ws)).unwrap();
        repo.delete(id).unwrap();
        assert!(repo.get(id).unwrap().is_none());
    }

    #[test]
    fn sqlite_search() {
        let (db, ws, _td) = sqlite_setup();
        let mut repo = SqliteMemoryRepository::new(&db);
        repo.insert(sqlite_mem(&ws)).unwrap();
        repo.insert(NewMemory {
            public_id: "mem2".into(),
            content: "Revenue growth is the key driver".into(),
            normalized_key: "driver:rev:growth".into(),
            ..sqlite_mem(&ws)
        })
        .unwrap();
        let results = repo
            .search(
                "PE",
                &MemoryScope {
                    workspace_id: Some(ws.clone()),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].memory.public_id, "mem1");
    }

    #[test]
    fn sqlite_record_use() {
        let (db, ws, _td) = sqlite_setup();
        let mut repo = SqliteMemoryRepository::new(&db);
        let id = repo.insert(sqlite_mem(&ws)).unwrap();
        // Bypass FK for soft telemetry (run/conversation are test scaffolding).
        db.conn.execute_batch("PRAGMA foreign_keys=OFF;").unwrap();
        db.conn
            .execute(
                "INSERT INTO agent_runs (id, conversation_id, status, phase, started_at)
             VALUES ('run1', 'conv_fk_bypass', 'completed', 'completed', '2026-07-17T12:00:00Z')",
                [],
            )
            .unwrap();
        db.conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
        repo.record_use("run1", id, 0.85).unwrap();
    }
}
