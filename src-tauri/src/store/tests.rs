//! Phase A store tests: foreign keys, branch-path search/context, FTS deletion,
//! last-reference blob reclamation/retry, atomic publish/reconcile, backup,
//! integrity, PRAGMA hooks, interrupted-run repair, and JSON migration.

use super::migrations;
use super::*;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

/// A unique temp directory removed on drop.
struct TempDir(PathBuf);
impl TempDir {
    fn new(tag: &str) -> Self {
        static N: AtomicU64 = AtomicU64::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let p = std::env::temp_dir().join(format!(
            "fmstore-{tag}-{}-{}",
            std::process::id(),
            n
        ));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        TempDir(p)
    }
    fn path(&self) -> &std::path::Path {
        &self.0
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const NOW: &str = "2026-07-17T00:00:00Z";

fn mem_db(td: &TempDir) -> Db {
    Db::open_in_memory(&td.path().join("blobs")).unwrap()
}

fn ws(db: &Db) -> String {
    let id = "ws-1".to_string();
    db.create_workspace(&id, "Deal A", "deal", "confidential", "", true, NOW)
        .unwrap();
    id
}

#[test]
fn foreign_keys_are_enforced() {
    let td = TempDir::new("fk");
    let db = mem_db(&td);
    // conversation referencing a nonexistent workspace must be rejected.
    let err = db.create_conversation("c1", "no-such-ws", "t", NOW);
    assert!(err.is_err(), "FK violation should reject");
}

#[test]
fn header_pragmas_are_set_on_fresh_db() {
    let td = TempDir::new("pragmas");
    let db = Db::open(&td.path().join("finmodel.db"), &td.path().join("blobs")).unwrap();
    let av: i64 = db
        .conn()
        .query_row("PRAGMA auto_vacuum", [], |r| r.get(0))
        .unwrap();
    assert_eq!(av, 2, "auto_vacuum should be INCREMENTAL(2)");
    let sd: i64 = db
        .conn()
        .query_row("PRAGMA secure_delete", [], |r| r.get(0))
        .unwrap();
    assert_eq!(sd, 1, "secure_delete should be ON");
    let fk: i64 = db
        .conn()
        .query_row("PRAGMA foreign_keys", [], |r| r.get(0))
        .unwrap();
    assert_eq!(fk, 1, "foreign_keys should be ON");
    let jm: String = db
        .conn()
        .query_row("PRAGMA journal_mode", [], |r| r.get(0))
        .unwrap();
    assert_eq!(jm.to_lowercase(), "wal", "journal_mode should be WAL");
    let uv = migrations::user_version(db.conn()).unwrap();
    assert_eq!(uv, migrations::SCHEMA_VERSION);
}

#[test]
fn branch_path_walks_active_leaf_and_switching_hides_old_subtree() {
    let td = TempDir::new("branch");
    let db = mem_db(&td);
    let w = ws(&db);
    db.create_conversation("c1", &w, "t", NOW).unwrap();
    // root(user) -> a1(assistant) -> u2(user) -> a2(assistant)
    db.insert_message("m_root", "c1", None, "user", None, "complete", NOW)
        .unwrap();
    db.insert_message("m_a1", "c1", Some("m_root"), "assistant", None, "complete", NOW)
        .unwrap();
    db.insert_message("m_u2", "c1", Some("m_a1"), "user", None, "complete", NOW)
        .unwrap();
    db.insert_message("m_a2", "c1", Some("m_u2"), "assistant", None, "complete", NOW)
        .unwrap();
    db.set_active_leaf("c1", "m_a2", NOW).unwrap();
    let path: Vec<String> = db.branch_path("c1").unwrap().into_iter().map(|m| m.id).collect();
    assert_eq!(path, vec!["m_root", "m_a1", "m_u2", "m_a2"]);

    // Edit at u2: create a sibling user message off a1, then a new assistant leaf.
    db.insert_message("m_u2b", "c1", Some("m_a1"), "user", None, "complete", NOW)
        .unwrap();
    db.insert_message("m_a2b", "c1", Some("m_u2b"), "assistant", None, "complete", NOW)
        .unwrap();
    db.set_active_leaf("c1", "m_a2b", NOW).unwrap();
    let path2: Vec<String> = db.branch_path("c1").unwrap().into_iter().map(|m| m.id).collect();
    assert_eq!(path2, vec!["m_root", "m_a1", "m_u2b", "m_a2b"]);
    // The old downstream subtree (m_u2/m_a2) is not on the active path.
    assert!(!path2.contains(&"m_u2".to_string()));
}

#[test]
fn fts_search_is_workspace_scoped_and_deletion_removes_index_rows() {
    let td = TempDir::new("fts");
    let db = mem_db(&td);
    // Two workspaces, each with a conversation + message + searchable part.
    db.create_workspace("wa", "A", "deal", "confidential", "", true, NOW).unwrap();
    db.create_workspace("wb", "B", "deal", "confidential", "", true, NOW).unwrap();
    db.create_conversation("ca", "wa", "t", NOW).unwrap();
    db.create_conversation("cb", "wb", "t", NOW).unwrap();
    db.insert_message("ma", "ca", None, "assistant", None, "complete", NOW).unwrap();
    db.insert_message("mb", "cb", None, "assistant", None, "complete", NOW).unwrap();
    db.insert_part("pa", "ma", 0, "text", "{}", Some("quarterly revenue growth accelerated"))
        .unwrap();
    db.insert_part("pb", "mb", 0, "text", "{}", Some("revenue in workspace B"))
        .unwrap();

    // Scoped: searching workspace A never returns workspace B's message.
    let hits = db.search_messages("wa", "revenue", 10).unwrap();
    assert_eq!(hits, vec![("ca".to_string(), "ma".to_string())]);
    let hits_b = db.search_messages("wb", "revenue", 10).unwrap();
    assert_eq!(hits_b, vec![("cb".to_string(), "mb".to_string())]);

    // Deleting the part removes it from the FTS index.
    db.conn().execute("DELETE FROM message_parts WHERE id='pa'", []).unwrap();
    let after = db.search_messages("wa", "revenue", 10).unwrap();
    assert!(after.is_empty(), "deleted part must not match FTS");
    db.fts_check().unwrap();
}

#[test]
fn monotonic_run_event_sequences() {
    let td = TempDir::new("events");
    let mut db = mem_db(&td);
    let w = ws(&db);
    db.create_conversation("c1", &w, "t", NOW).unwrap();
    db.insert_run("r1", "c1", None, None, "running", "preparing", None, None, NOW)
        .unwrap();
    let s1 = db.append_event("e1", "r1", "run_started", "{}", NOW).unwrap();
    let s2 = db.append_event("e2", "r1", "phase_changed", "{}", NOW).unwrap();
    assert_eq!((s1, s2), (1, 2));
    assert_eq!(db.get_run("r1").unwrap().unwrap().last_sequence, 2);
    assert_eq!(db.events_after("r1", 0).unwrap().len(), 2);
    let tail = db.events_after("r1", 1).unwrap();
    assert_eq!(tail.len(), 1);
    assert_eq!(tail[0].sequence, 2);
    // Duplicate sequence is rejected by the unique index.
    let dup = db.conn().execute(
        "INSERT INTO run_events (id,run_id,sequence,kind,payload_json,created_at) VALUES ('x','r1',1,'k','{}',?1)",
        [NOW],
    );
    assert!(dup.is_err(), "duplicate (run_id,sequence) must be rejected");
}

#[test]
fn first_answer_wins_on_pending_interaction() {
    let td = TempDir::new("pending");
    let db = mem_db(&td);
    let w = ws(&db);
    db.create_conversation("c1", &w, "t", NOW).unwrap();
    db.insert_run("r1", "c1", None, None, "running", "awaiting_approval", None, None, NOW)
        .unwrap();
    db.insert_pending("p1", "r1", Some("tc1"), "approval", "{}", NOW).unwrap();
    assert!(db.resolve_pending("p1", "\"approve_once\"", NOW).unwrap());
    // A second resolution loses.
    assert!(!db.resolve_pending("p1", "\"deny\"", NOW).unwrap());
}

#[test]
fn blob_last_reference_reclamation_and_ref_hold() {
    let td = TempDir::new("blob");
    let mut db = mem_db(&td);
    let w = ws(&db);
    db.create_conversation("c1", &w, "t", NOW).unwrap();

    let blob = db.publish_blob("2026/model.bin", b"hello bytes", NOW).unwrap();
    let on_disk = td.path().join("blobs").join("2026/model.bin");
    assert!(on_disk.exists(), "published file must exist");
    assert_eq!(std::fs::read(&on_disk).unwrap(), b"hello bytes");

    // Two owners reference it.
    db.add_blob_ref(&blob.id, OWNER_ARTIFACT, "art1").unwrap();
    db.add_blob_ref(&blob.id, OWNER_ATTACHMENT, "att1").unwrap();

    // Removing one ref does NOT enqueue GC.
    assert!(!db.remove_blob_ref(&blob.id, OWNER_ARTIFACT, "art1", NOW).unwrap());
    assert_eq!(db.run_blob_gc().unwrap(), 0);
    assert!(on_disk.exists(), "still referenced -> retained");

    // Removing the last ref enqueues GC; bytes reclaimed on run_blob_gc.
    assert!(db.remove_blob_ref(&blob.id, OWNER_ATTACHMENT, "att1", NOW).unwrap());
    assert!(on_disk.exists(), "not yet collected");
    assert_eq!(db.run_blob_gc().unwrap(), 1);
    assert!(!on_disk.exists(), "bytes reclaimed");
    assert!(db.get_blob(&blob.id).unwrap().is_none(), "blob row removed");
}

#[test]
fn blob_gc_retries_when_file_missing_still_reclaims_row() {
    let td = TempDir::new("blobgc2");
    let mut db = mem_db(&td);
    let blob = db.publish_blob("x/y.bin", b"data", NOW).unwrap();
    // Externally remove the file, then queue GC: reclamation still succeeds.
    std::fs::remove_file(td.path().join("blobs").join("x/y.bin")).unwrap();
    assert!(db.remove_blob_ref(&blob.id, OWNER_ARTIFACT, "none", NOW).unwrap());
    assert_eq!(db.run_blob_gc().unwrap(), 1);
    assert!(db.get_blob(&blob.id).unwrap().is_none());
}

#[test]
fn atomic_publish_and_reconcile_stale_temps() {
    let td = TempDir::new("reconcile");
    let db = mem_db(&td);
    let blob = db.publish_blob("deck.pptx", b"PPTX", NOW).unwrap();
    assert_eq!(blob.byte_len, 4);
    // Leave a stale temp and an unregistered final file.
    let blobs = td.path().join("blobs");
    std::fs::write(blobs.join(".tmp-orphan-123"), b"junk").unwrap();
    std::fs::write(blobs.join("unregistered.bin"), b"loose").unwrap();
    let (deleted, unregistered) = db.reconcile_blob_dir().unwrap();
    assert_eq!(deleted, 1, "stale temp deleted");
    assert!(unregistered.contains(&"unregistered.bin".to_string()));
    assert!(!unregistered.contains(&"deck.pptx".to_string()), "registered blob is not flagged");
    assert!(!blobs.join(".tmp-orphan-123").exists());
}

#[test]
fn blob_re_reference_cancels_pending_gc() {
    let td = TempDir::new("resurrect");
    let mut db = mem_db(&td);
    let blob = db.publish_blob("r/z.bin", b"payload", NOW).unwrap();
    let on_disk = td.path().join("blobs").join("r/z.bin");
    db.add_blob_ref(&blob.id, OWNER_ARTIFACT, "a1").unwrap();
    // Last ref removed -> GC enqueued.
    assert!(db.remove_blob_ref(&blob.id, OWNER_ARTIFACT, "a1", NOW).unwrap());
    // Re-referenced before GC runs -> pending GC is cancelled, bytes survive.
    db.add_blob_ref(&blob.id, OWNER_ATTACHMENT, "a2").unwrap();
    assert_eq!(db.run_blob_gc().unwrap(), 0, "resurrected blob must not be collected");
    assert!(on_disk.exists(), "bytes retained after re-reference");
    assert!(db.get_blob(&blob.id).unwrap().is_some());
}

#[test]
fn interrupted_run_repair_on_startup() {
    let td = TempDir::new("repair");
    let db = mem_db(&td);
    let w = ws(&db);
    db.create_conversation("c1", &w, "t", NOW).unwrap();
    db.insert_run("r1", "c1", None, None, "running", "executing", None, None, NOW)
        .unwrap();
    db.insert_tool_invocation("t1", "r1", None, None, "web_search", "running", "read_only", None, NOW)
        .unwrap();
    let repaired = db.repair_interrupted_runs(NOW).unwrap();
    assert_eq!(repaired, 1);
    let run = db.get_run("r1").unwrap().unwrap();
    assert_eq!(run.status, "interrupted");
    assert_eq!(run.stop_reason.as_deref(), Some("interrupted"));
    let tool_status: String = db
        .conn()
        .query_row("SELECT status FROM tool_invocations WHERE id='t1'", [], |r| r.get(0))
        .unwrap();
    assert_eq!(tool_status, "interrupted");
}

#[test]
fn integrity_and_backup_roundtrip() {
    let td = TempDir::new("backup");
    let db = Db::open(&td.path().join("finmodel.db"), &td.path().join("blobs")).unwrap();
    let w = ws(&db);
    db.create_conversation("c1", &w, "hello backup", NOW).unwrap();
    db.integrity_check().unwrap();
    db.foreign_key_check().unwrap();

    let dest = td.path().join("backup.db");
    db.backup_to(&dest).unwrap();
    let restored = rusqlite::Connection::open(&dest).unwrap();
    let title: String = restored
        .query_row("SELECT title FROM conversations WHERE id='c1'", [], |r| r.get(0))
        .unwrap();
    assert_eq!(title, "hello backup");
}

#[test]
fn json_migration_groups_parts_is_idempotent_and_quarantines_bad_files() {
    let td = TempDir::new("migrate");
    let json_dir = td.path().join("conversations");
    std::fs::create_dir_all(&json_dir).unwrap();
    // A conversation: user, then two consecutive assistant messages (grouped).
    let good = serde_json::json!({
        "id": "conv-1",
        "title": "Old chat",
        "created": "2026-01-01T00:00:00Z",
        "updated": "2026-01-02T00:00:00Z",
        "messages": [
            {"role":"user","content":"build NVDA model","ts":"2026-01-01T00:00:00Z"},
            {"role":"assistant","content":"Working on it.","ts":"2026-01-01T00:00:01Z"},
            {"role":"assistant","content":"","card":{"kind":"model","ticker":"NVDA","title":"NVDA model"},"llm_context":"Built NVDA 3-statement + DCF.","ts":"2026-01-01T00:00:02Z"}
        ]
    });
    std::fs::write(json_dir.join("conv-1.json"), serde_json::to_string_pretty(&good).unwrap()).unwrap();
    std::fs::write(json_dir.join("broken.json"), "{ not valid json ").unwrap();

    let mut db = mem_db(&td);
    let w = db.ensure_default_personal_workspace(NOW).unwrap();
    let mut counter = 0u64;
    let mut gen = || {
        counter += 1;
        format!("id-{counter}")
    };
    let report = migrations::import_json_conversations(
        db.conn_mut(),
        &json_dir,
        &w,
        NOW,
        &mut gen,
    )
    .unwrap();
    assert_eq!(report.imported_conversations, 1);
    assert_eq!(report.imported_messages, 2, "user + one grouped assistant");
    assert_eq!(report.quarantined.len(), 1);
    assert_eq!(report.quarantined[0].filename, "broken.json");

    // The grouped assistant message keeps order: text part then result (card) part,
    // and its context_summary is the llm_context.
    let path = db.branch_path("conv-1").unwrap();
    assert_eq!(path.len(), 2);
    assert_eq!(path[0].role, "user");
    let asst = &path[1];
    assert_eq!(asst.role, "assistant");
    assert_eq!(asst.context_summary.as_deref(), Some("Built NVDA 3-statement + DCF."));
    let parts = db.message_parts(&asst.id).unwrap();
    let kinds: Vec<&str> = parts.iter().map(|p| p.kind.as_str()).collect();
    assert_eq!(kinds, vec!["text", "result"], "order preserved: prose then card");

    // Idempotent: re-import skips the existing conversation, imports nothing new.
    let report2 = migrations::import_json_conversations(
        db.conn_mut(),
        &json_dir,
        &w,
        NOW,
        &mut gen,
    )
    .unwrap();
    assert_eq!(report2.imported_conversations, 0);
    assert_eq!(report2.skipped_existing, 1);
    // Still exactly one conversation row.
    let n: i64 = db
        .conn()
        .query_row("SELECT count(*) FROM conversations", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 1);
}

#[tokio::test]
async fn store_actor_serializes_calls() {
    let td = TempDir::new("actor");
    let db = mem_db(&td);
    let w = ws(&db);
    db.create_conversation("c1", &w, "t", NOW).unwrap();
    let handle = StoreHandle::spawn(db);
    // Write then read through the actor.
    handle
        .call(|db| {
            db.insert_message("m1", "c1", None, "user", None, "complete", NOW)
                .unwrap();
        })
        .await;
    let parts = handle
        .call(|db| {
            db.insert_part("p1", "m1", 0, "text", "{}", Some("hi")).unwrap();
            db.message_parts("m1").unwrap().len()
        })
        .await;
    assert_eq!(parts, 1);
}
