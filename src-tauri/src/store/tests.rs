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
        let p = std::env::temp_dir().join(format!("fmstore-{tag}-{}-{}", std::process::id(), n));
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
    db.insert_message(
        "m_a1",
        "c1",
        Some("m_root"),
        "assistant",
        None,
        "complete",
        NOW,
    )
    .unwrap();
    db.insert_message("m_u2", "c1", Some("m_a1"), "user", None, "complete", NOW)
        .unwrap();
    db.insert_message(
        "m_a2",
        "c1",
        Some("m_u2"),
        "assistant",
        None,
        "complete",
        NOW,
    )
    .unwrap();
    db.set_active_leaf("c1", "m_a2", NOW).unwrap();
    let path: Vec<String> = db
        .branch_path("c1")
        .unwrap()
        .into_iter()
        .map(|m| m.id)
        .collect();
    assert_eq!(path, vec!["m_root", "m_a1", "m_u2", "m_a2"]);

    // Edit at u2: create a sibling user message off a1, then a new assistant leaf.
    db.insert_message("m_u2b", "c1", Some("m_a1"), "user", None, "complete", NOW)
        .unwrap();
    db.insert_message(
        "m_a2b",
        "c1",
        Some("m_u2b"),
        "assistant",
        None,
        "complete",
        NOW,
    )
    .unwrap();
    db.set_active_leaf("c1", "m_a2b", NOW).unwrap();
    let path2: Vec<String> = db
        .branch_path("c1")
        .unwrap()
        .into_iter()
        .map(|m| m.id)
        .collect();
    assert_eq!(path2, vec!["m_root", "m_a1", "m_u2b", "m_a2b"]);
    // The old downstream subtree (m_u2/m_a2) is not on the active path.
    assert!(!path2.contains(&"m_u2".to_string()));
}

#[test]
fn fts_search_is_workspace_scoped_and_deletion_removes_index_rows() {
    let td = TempDir::new("fts");
    let db = mem_db(&td);
    // Two workspaces, each with a conversation + message + searchable part.
    db.create_workspace("wa", "A", "deal", "confidential", "", true, NOW)
        .unwrap();
    db.create_workspace("wb", "B", "deal", "confidential", "", true, NOW)
        .unwrap();
    db.create_conversation("ca", "wa", "t", NOW).unwrap();
    db.create_conversation("cb", "wb", "t", NOW).unwrap();
    db.insert_message("ma", "ca", None, "assistant", None, "complete", NOW)
        .unwrap();
    db.insert_message("mb", "cb", None, "assistant", None, "complete", NOW)
        .unwrap();
    db.insert_part(
        "pa",
        "ma",
        0,
        "text",
        "{}",
        Some("quarterly revenue growth accelerated"),
    )
    .unwrap();
    db.insert_part("pb", "mb", 0, "text", "{}", Some("revenue in workspace B"))
        .unwrap();

    // Scoped: searching workspace A never returns workspace B's message.
    let hits = db.search_messages("wa", "revenue", 10).unwrap();
    assert_eq!(hits, vec![("ca".to_string(), "ma".to_string())]);
    let hits_b = db.search_messages("wb", "revenue", 10).unwrap();
    assert_eq!(hits_b, vec![("cb".to_string(), "mb".to_string())]);

    // Deleting the part removes it from the FTS index.
    db.conn()
        .execute("DELETE FROM message_parts WHERE id='pa'", [])
        .unwrap();
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
    db.insert_run(
        "r1",
        "c1",
        None,
        None,
        "running",
        "preparing",
        None,
        None,
        NOW,
    )
    .unwrap();
    let s1 = db
        .append_event("e1", "r1", "run_started", "{}", NOW)
        .unwrap();
    let s2 = db
        .append_event("e2", "r1", "phase_changed", "{}", NOW)
        .unwrap();
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
    db.insert_run(
        "r1",
        "c1",
        None,
        None,
        "running",
        "awaiting_approval",
        None,
        None,
        NOW,
    )
    .unwrap();
    db.insert_pending("p1", "r1", Some("tc1"), "approval", "{}", NOW)
        .unwrap();
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

    let blob = db
        .publish_blob("2026/model.bin", b"hello bytes", NOW)
        .unwrap();
    let on_disk = td.path().join("blobs").join("2026/model.bin");
    assert!(on_disk.exists(), "published file must exist");
    assert_eq!(std::fs::read(&on_disk).unwrap(), b"hello bytes");

    // Two owners reference it.
    db.add_blob_ref(&blob.id, OWNER_ARTIFACT, "art1").unwrap();
    db.add_blob_ref(&blob.id, OWNER_ATTACHMENT, "att1").unwrap();

    // Removing one ref does NOT enqueue GC.
    assert!(!db
        .remove_blob_ref(&blob.id, OWNER_ARTIFACT, "art1", NOW)
        .unwrap());
    assert_eq!(db.run_blob_gc().unwrap(), 0);
    assert!(on_disk.exists(), "still referenced -> retained");

    // Removing the last ref enqueues GC; bytes reclaimed on run_blob_gc.
    assert!(db
        .remove_blob_ref(&blob.id, OWNER_ATTACHMENT, "att1", NOW)
        .unwrap());
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
    assert!(db
        .remove_blob_ref(&blob.id, OWNER_ARTIFACT, "none", NOW)
        .unwrap());
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
    assert!(
        !unregistered.contains(&"deck.pptx".to_string()),
        "registered blob is not flagged"
    );
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
    assert!(db
        .remove_blob_ref(&blob.id, OWNER_ARTIFACT, "a1", NOW)
        .unwrap());
    // Re-referenced before GC runs -> pending GC is cancelled, bytes survive.
    db.add_blob_ref(&blob.id, OWNER_ATTACHMENT, "a2").unwrap();
    assert_eq!(
        db.run_blob_gc().unwrap(),
        0,
        "resurrected blob must not be collected"
    );
    assert!(on_disk.exists(), "bytes retained after re-reference");
    assert!(db.get_blob(&blob.id).unwrap().is_some());
}

#[test]
fn interrupted_run_repair_on_startup() {
    let td = TempDir::new("repair");
    let db = mem_db(&td);
    let w = ws(&db);
    db.create_conversation("c1", &w, "t", NOW).unwrap();
    db.insert_run(
        "r1",
        "c1",
        None,
        None,
        "running",
        "executing",
        None,
        None,
        NOW,
    )
    .unwrap();
    db.insert_tool_invocation(
        "t1",
        "r1",
        None,
        None,
        "web_search",
        "running",
        "read_only",
        None,
        NOW,
    )
    .unwrap();
    let repaired = db.repair_interrupted_runs(NOW).unwrap();
    assert_eq!(repaired, 1);
    let run = db.get_run("r1").unwrap().unwrap();
    assert_eq!(run.status, "interrupted");
    assert_eq!(run.stop_reason.as_deref(), Some("interrupted"));
    let tool_status: String = db
        .conn()
        .query_row(
            "SELECT status FROM tool_invocations WHERE id='t1'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(tool_status, "interrupted");
}

#[test]
fn integrity_and_backup_roundtrip() {
    let td = TempDir::new("backup");
    let db = Db::open(&td.path().join("finmodel.db"), &td.path().join("blobs")).unwrap();
    let w = ws(&db);
    db.create_conversation("c1", &w, "hello backup", NOW)
        .unwrap();
    db.integrity_check().unwrap();
    db.foreign_key_check().unwrap();

    let dest = td.path().join("backup.db");
    db.backup_to(&dest).unwrap();
    let restored = rusqlite::Connection::open(&dest).unwrap();
    let title: String = restored
        .query_row("SELECT title FROM conversations WHERE id='c1'", [], |r| {
            r.get(0)
        })
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
    std::fs::write(
        json_dir.join("conv-1.json"),
        serde_json::to_string_pretty(&good).unwrap(),
    )
    .unwrap();
    std::fs::write(json_dir.join("broken.json"), "{ not valid json ").unwrap();

    let mut db = mem_db(&td);
    let w = db.ensure_default_personal_workspace(NOW).unwrap();
    let mut counter = 0u64;
    let mut gen = || {
        counter += 1;
        format!("id-{counter}")
    };
    let report =
        migrations::import_json_conversations(db.conn_mut(), &json_dir, &w, NOW, &mut gen).unwrap();
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
    assert_eq!(
        asst.context_summary.as_deref(),
        Some("Built NVDA 3-statement + DCF.")
    );
    let parts = db.message_parts(&asst.id).unwrap();
    let kinds: Vec<&str> = parts.iter().map(|p| p.kind.as_str()).collect();
    assert_eq!(
        kinds,
        vec!["text", "result"],
        "order preserved: prose then card"
    );

    // Idempotent: re-import skips the existing conversation, imports nothing new.
    let report2 =
        migrations::import_json_conversations(db.conn_mut(), &json_dir, &w, NOW, &mut gen).unwrap();
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
            db.insert_part("p1", "m1", 0, "text", "{}", Some("hi"))
                .unwrap();
            db.message_parts("m1").unwrap().len()
        })
        .await;
    assert_eq!(parts, 1);
}

#[test]
fn source_dedup_and_citation_linkage() {
    let td = TempDir::new("cite");
    let db = mem_db(&td);
    let w = ws(&db);
    db.create_conversation("c1", &w, "t", NOW).unwrap();
    db.insert_message("m1", "c1", None, "assistant", None, "complete", NOW)
        .unwrap();
    db.insert_part("p1", "m1", 0, "result", "{}", None).unwrap();
    // Same source id inserted twice → deduped (INSERT OR IGNORE, Task 4.1).
    db.insert_source(
        "src-x",
        &w,
        "web",
        "https://a.com",
        Some("A"),
        None,
        None,
        None,
        None,
    )
    .unwrap();
    db.insert_source(
        "src-x",
        &w,
        "web",
        "https://a.com",
        Some("A"),
        None,
        None,
        None,
        None,
    )
    .unwrap();
    let n: i64 = db
        .conn()
        .query_row("SELECT count(*) FROM sources WHERE id='src-x'", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(n, 1, "source deduped by id");
    // Citation links the part to the source with claim provenance.
    let claim = fm_agent::types::Claim {
        claim_key: "k".into(),
        entity: "NVDA".into(),
        normalized_value: "60922".into(),
        unit: "usd_m".into(),
        currency: Some("USD".into()),
        scale: "1e6".into(),
        period: "FY2024".into(),
        locator: "10-K p.1".into(),
        source_id: "src-x".into(),
        quote_hash: "h".into(),
    };
    db.insert_citation("p1", 0, "src-x", &claim).unwrap();
    let (sid, key): (String, String) = db
        .conn()
        .query_row(
            "SELECT source_id, claim_key FROM citations WHERE message_part_id='p1' AND ordinal=0",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(sid, "src-x");
    assert_eq!(key, "k");
}

#[test]
fn delegations_persist_and_recover() {
    let td = TempDir::new("deleg");
    let db = mem_db(&td);
    let w = ws(&db);
    db.create_conversation("c1", &w, "t", NOW).unwrap();
    db.insert_run(
        "parent",
        "c1",
        None,
        None,
        "running",
        "executing",
        None,
        None,
        NOW,
    )
    .unwrap();
    // Dispatch persisted BEFORE the child runs (queued).
    db.insert_delegation("d1", "parent", Some("tc1"), "{\"task\":\"NVDA\"}", NOW)
        .unwrap();
    assert_eq!(db.delegations_in_status("parent", "queued").unwrap(), 1);
    // Child registered → running.
    db.insert_run(
        "child",
        "c1",
        None,
        None,
        "running",
        "executing",
        None,
        None,
        NOW,
    )
    .unwrap();
    db.set_delegation_child("d1", "child").unwrap();
    assert_eq!(db.delegations_in_status("parent", "running").unwrap(), 1);
    // Restart recovery: a still-running delegation becomes outcome_unknown —
    // never assumed succeeded or failed (Task 5.1).
    assert_eq!(db.recover_dead_delegations(NOW).unwrap(), 1);
    assert_eq!(
        db.delegations_in_status("parent", "outcome_unknown")
            .unwrap(),
        1
    );
    // A finished delegation is preserved and untouched by later recovery.
    db.insert_delegation("d2", "parent", None, "{}", NOW)
        .unwrap();
    db.finish_delegation("d2", "succeeded", Some("{\"ok\":true}"), None, NOW)
        .unwrap();
    assert_eq!(db.delegations_in_status("parent", "succeeded").unwrap(), 1);
}

#[test]
fn child_result_delivery_is_at_least_once() {
    let td = TempDir::new("deliv");
    let db = mem_db(&td);
    let w = ws(&db);
    db.create_conversation("c1", &w, "t", NOW).unwrap();
    db.insert_run(
        "parent",
        "c1",
        None,
        None,
        "running",
        "executing",
        None,
        None,
        NOW,
    )
    .unwrap();
    db.insert_delegation("d1", "parent", None, "{}", NOW)
        .unwrap();
    db.finish_delegation("d1", "succeeded", Some("{\"ans\":1}"), None, NOW)
        .unwrap();

    // A finished-but-undelivered result is visible for delivery.
    assert_eq!(
        db.undelivered_completed_delegations("parent")
            .unwrap()
            .len(),
        1
    );
    // Two consumers race the claim; exactly one wins (at-most-once claim).
    assert!(db.claim_delegation_delivery("d1", "A", NOW).unwrap());
    assert!(!db.claim_delegation_delivery("d1", "B", NOW).unwrap());
    // A non-owner can neither ack nor release the claim.
    assert!(!db.ack_delegation_delivery("d1", "B").unwrap());
    assert!(!db.release_delegation_claim("d1", "B").unwrap());
    // A failed append releases the owner's claim → still undelivered, reclaimable.
    assert!(db.release_delegation_claim("d1", "A").unwrap());
    assert_eq!(
        db.undelivered_completed_delegations("parent")
            .unwrap()
            .len(),
        1
    );
    // Re-claim, then a crash before ack: a later restart reclaims the stale claim
    // (claimed at or before the cutoff); a fresh claim is not stolen.
    assert!(db
        .claim_delegation_delivery("d1", "C", "2020-01-01T00:00:00Z")
        .unwrap());
    assert_eq!(
        db.reclaim_stale_deliveries("2020-06-01T00:00:00Z").unwrap(),
        1
    );
    // Final successful delivery: claim → ack → delivered, no longer pending.
    assert!(db.claim_delegation_delivery("d1", "D", NOW).unwrap());
    assert!(db.ack_delegation_delivery("d1", "D").unwrap());
    assert!(db
        .undelivered_completed_delegations("parent")
        .unwrap()
        .is_empty());
    // A delivered result is never re-claimed (exactly-once effect at the parent).
    assert!(!db.claim_delegation_delivery("d1", "E", NOW).unwrap());
    assert_eq!(db.reclaim_stale_deliveries(NOW).unwrap(), 0);
}

#[test]
fn commitments_persist_and_schedule_claim_is_exclusive() {
    let td = TempDir::new("commit");
    let db = mem_db(&td);
    let w = ws(&db);
    db.insert_commitment(
        "cm1",
        None,
        None,
        &w,
        "recheck after earnings",
        Some("after_next_earnings"),
        0.9,
        NOW,
    )
    .unwrap();
    assert_eq!(db.commitments_in_status("pending").unwrap(), 1);
    // A due schedule (next_due in the past).
    db.insert_schedule(
        "s1",
        Some("cm1"),
        None,
        "UTC",
        None,
        "2020-01-01T00:00:00Z",
        "{}",
        None,
        Some("appr1"),
        NOW,
    )
    .unwrap();
    // Two workers race the same due row; the guarded UPDATE lets only one win.
    let a = db.claim_due_schedule(NOW, "workerA").unwrap();
    let b = db.claim_due_schedule(NOW, "workerB").unwrap();
    assert_eq!(a.as_deref(), Some("s1"));
    assert_eq!(b, None, "second worker gets nothing — no double claim");
}

#[test]
fn pending_approvals_are_durable_and_fail_closed() {
    let td = TempDir::new("pend");
    let db = mem_db(&td);
    let w = ws(&db);
    db.create_conversation("c1", &w, "t", NOW).unwrap();
    db.insert_run(
        "r1",
        "c1",
        None,
        None,
        "running",
        "awaiting_approval",
        None,
        None,
        NOW,
    )
    .unwrap();
    db.insert_pending(
        "p1",
        "r1",
        Some("tc1"),
        "approval",
        "{\"risk\":\"export\"}",
        NOW,
    )
    .unwrap();
    // Survives "restart": still queryable as unresolved.
    let unresolved = db.unresolved_pending("r1").unwrap();
    assert_eq!(unresolved.len(), 1);
    assert_eq!(unresolved[0].0, "p1");
    // First resolve wins; a second is a no-op (first answer wins).
    assert!(db
        .resolve_pending("p1", "{\"response\":\"approve_once\"}", NOW)
        .unwrap());
    assert!(!db
        .resolve_pending("p1", "{\"response\":\"deny\"}", NOW)
        .unwrap());
    assert!(db.unresolved_pending("r1").unwrap().is_empty());
    // Expiry fails closed for a stale pending row.
    db.insert_pending("p2", "r1", None, "approval", "{}", "2020-01-01T00:00:00Z")
        .unwrap();
    assert_eq!(db.expire_pending("2020-06-01T00:00:00Z", NOW).unwrap(), 1);
    assert!(db.unresolved_pending("r1").unwrap().is_empty());
}

#[test]
fn oversized_result_spills_to_blob_with_bounded_preview() {
    let td = TempDir::new("spill");
    let db = mem_db(&td);
    // A small result stays inline: no blob, exact text preserved.
    let (preview, blob) = db.spill_result("r1", "tc1", "small", 64, NOW).unwrap();
    assert_eq!(preview, "small");
    assert!(blob.is_none());
    // A large result spills: preview is bounded + the full bytes are recoverable.
    let big = "x".repeat(5000);
    let (preview, blob) = db.spill_result("r1", "tc2", &big, 100, NOW).unwrap();
    let id = blob.expect("large result spills to a blob");
    // Preview is bounded to the budget (+ the single ellipsis marker char).
    assert!(preview.chars().take_while(|c| *c == 'x').count() == 100);
    assert!(preview.ends_with('…'));
    assert!(preview.len() < big.len());
    // The full result is persisted and recoverable by the opaque id.
    let stored = db.get_blob(&id).unwrap().expect("blob row exists");
    assert_eq!(stored.byte_len, big.len() as i64);
}

#[test]
fn skill_lifecycle_ages_and_excludes_from_default_context() {
    let td = TempDir::new("skills");
    let db = mem_db(&td);
    // Register two skills far in the past; both start active.
    db.upsert_skill("alpha", 1, "2020-01-01T00:00:00Z").unwrap();
    db.upsert_skill("beta", 1, "2020-01-01T00:00:00Z").unwrap();
    assert_eq!(db.active_skill_names().unwrap(), vec!["alpha", "beta"]);
    // Using `beta` stamps recent use, so aging won't stale it.
    db.record_skill_use("beta", "2020-06-01T00:00:00Z").unwrap();

    // Aging sweep: stale cutoff catches the unused `alpha`; nothing archived yet.
    let (staled, archived) = db
        .age_skills(
            "2020-03-01T00:00:00Z",
            "2020-02-01T00:00:00Z",
            "2020-06-02T00:00:00Z",
        )
        .unwrap();
    assert_eq!((staled, archived), (1, 0));
    // Stale skills are excluded from default context; active ones remain.
    assert_eq!(db.active_skill_names().unwrap(), vec!["beta"]);
    assert_eq!(
        db.skill_lifecycle_state("alpha").unwrap().unwrap().0,
        "stale"
    );

    // A second sweep past the archive cutoff moves stale `alpha` → archived
    // (one transition per sweep: beta, recently used, stays active).
    let (staled, archived) = db
        .age_skills(
            "2020-07-01T00:00:00Z",
            "2020-07-01T00:00:00Z",
            "2020-07-02T00:00:00Z",
        )
        .unwrap();
    assert_eq!(archived, 1);
    assert_eq!(
        db.skill_lifecycle_state("alpha").unwrap().unwrap().0,
        "archived"
    );
    // beta was active-and-recently-used → became stale this sweep (its last_used
    // 2020-06 is <= the 2020-07 stale cutoff), proving aging is disuse-based.
    assert_eq!(staled, 1);
    // inactive_skill_names is the catalog-exclusion set: alpha (archived) + beta
    // (stale) are both excluded, mirroring active_skill_names' complement (7.3).
    {
        let mut inactive = db.inactive_skill_names().unwrap();
        inactive.sort();
        assert_eq!(inactive, vec!["alpha".to_string(), "beta".to_string()]);
    }

    // Restore brings an archived skill back into default context, inspectable.
    assert!(db.restore_skill("alpha", "2020-08-01T00:00:00Z").unwrap());
    assert!(db
        .active_skill_names()
        .unwrap()
        .contains(&"alpha".to_string()));

    // Supersession archives the old and registers the new with lineage.
    db.supersede_skill("alpha", "alpha-v2", 2, "2020-09-01T00:00:00Z")
        .unwrap();
    assert_eq!(
        db.skill_lifecycle_state("alpha").unwrap().unwrap().0,
        "archived"
    );
    let (state, _uses, ver, supersedes) = db.skill_lifecycle_state("alpha-v2").unwrap().unwrap();
    assert_eq!(state, "active");
    assert_eq!(ver, 2);
    assert_eq!(supersedes.as_deref(), Some("alpha"));

    // Reviving via use: stale → active on record_skill_use.
    db.upsert_skill("gamma", 1, "2020-01-01T00:00:00Z").unwrap();
    db.age_skills(
        "2020-03-01T00:00:00Z",
        "2019-01-01T00:00:00Z",
        "2020-06-02T00:00:00Z",
    )
    .unwrap();
    assert_eq!(
        db.skill_lifecycle_state("gamma").unwrap().unwrap().0,
        "stale"
    );
    assert!(db
        .record_skill_use("gamma", "2020-10-01T00:00:00Z")
        .unwrap());
    assert_eq!(
        db.skill_lifecycle_state("gamma").unwrap().unwrap().0,
        "active"
    );
}

#[test]
fn memory_pin_round_trips_via_v5_column() {
    let td = TempDir::new("mempin");
    let db = mem_db(&td);
    let w = ws(&db);
    let id = db
        .insert_memory(
            "m-pub-1",
            "workspace",
            Some(&w),
            None,
            "preference",
            "prefers concise answers",
            "concise",
            0.5,
            0.9,
            "manual",
            None,
            NOW,
        )
        .unwrap();
    // The v5 `pinned` column defaults to unpinned.
    assert!(!db.is_memory_pinned(id).unwrap());
    // Pin persists true; unpin returns to false (reversible, Task 7.2).
    assert!(db.set_memory_pinned(id, true).unwrap());
    assert!(db.is_memory_pinned(id).unwrap());
    assert!(db.set_memory_pinned(id, false).unwrap());
    assert!(!db.is_memory_pinned(id).unwrap());
    // An unknown id matches no row.
    assert!(!db.set_memory_pinned(9999, true).unwrap());
}

#[test]
fn memory_edit_updates_content() {
    let td = TempDir::new("memedit");
    let db = mem_db(&td);
    let w = ws(&db);
    let id = db
        .insert_memory(
            "m-e",
            "workspace",
            Some(&w),
            None,
            "preference",
            "prefers dcf",
            "concise",
            0.5,
            0.9,
            "manual",
            None,
            NOW,
        )
        .unwrap();
    assert_eq!(
        db.memory_content(id).unwrap().as_deref(),
        Some("prefers dcf")
    );
    // Edit persists the new content (Task 7.2).
    assert!(db.update_memory_value(id, "prefers comps", NOW).unwrap());
    assert_eq!(
        db.memory_content(id).unwrap().as_deref(),
        Some("prefers comps")
    );
    // Unknown id matches no row.
    assert!(!db.update_memory_value(9999, "x", NOW).unwrap());
}

#[test]
fn schedule_lifecycle_list_cancel_rearm() {
    let td = TempDir::new("sched");
    let db = mem_db(&td);
    let now = "2026-07-19T10:00:00Z";
    db.insert_schedule(
        "sch1",
        None,
        None,
        "UTC",
        Some("daily"),
        "2026-07-19T09:00:00Z",
        r#"{"prompt":"morning brief"}"#,
        None,
        None,
        now,
    )
    .unwrap();
    db.insert_schedule(
        "sch2",
        None,
        None,
        "UTC",
        None,
        "2026-07-20T09:00:00Z",
        r#"{"prompt":"one shot"}"#,
        None,
        None,
        now,
    )
    .unwrap();

    let rows = db.list_schedules().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].id, "sch1", "soonest due first");
    assert_eq!(rows[0].recurrence.as_deref(), Some("daily"));

    // Claim the due one; re-arm it for tomorrow (recurring).
    let claimed = db.claim_due_schedule(now, "tick").unwrap();
    assert_eq!(claimed.as_deref(), Some("sch1"));
    db.rearm_schedule("sch1", "2026-07-20T09:00:00Z").unwrap();
    let st = db.schedule_state("sch1").unwrap().unwrap();
    assert_eq!(st.0, "pending");
    // Re-armed row is claimable again at its NEW due time, not before.
    assert_eq!(db.claim_due_schedule(now, "tick").unwrap(), None);

    // Cancel removes it from the list and from claiming.
    db.cancel_schedule("sch1").unwrap();
    db.cancel_schedule("sch2").unwrap();
    assert!(db.list_schedules().unwrap().is_empty());
    assert_eq!(
        db.claim_due_schedule("2026-07-21T00:00:00Z", "tick")
            .unwrap(),
        None
    );
}

#[test]
fn conversation_spend_sums_run_costs_and_survives_finish() {
    let td = TempDir::new("spend");
    let db = mem_db(&td);
    let w = ws(&db);
    db.create_conversation("c1", &w, "t", NOW).unwrap();
    db.create_conversation("c2", &w, "t", NOW).unwrap();
    for (run, conv) in [("r1", "c1"), ("r2", "c1"), ("r3", "c2")] {
        db.insert_run(
            run,
            conv,
            None,
            None,
            "running",
            "preparing",
            None,
            None,
            NOW,
        )
        .unwrap();
    }
    db.set_run_usage(
        "r1",
        r#"{"prompt_tokens":10,"completion_tokens":5,"cost_usd":0.25}"#,
    )
    .unwrap();
    db.set_run_usage("r2", r#"{"cost_usd":0.50}"#).unwrap();
    // Another conversation's spend never bleeds in.
    db.set_run_usage("r3", r#"{"cost_usd":9.99}"#).unwrap();
    assert!((db.conversation_spend_usd("c1").unwrap() - 0.75).abs() < 1e-9);
    // finish_run with usage_json=None must KEEP the driver's snapshot.
    db.finish_run("r1", "completed", "done", None, None, NOW)
        .unwrap();
    assert!((db.conversation_spend_usd("c1").unwrap() - 0.75).abs() < 1e-9);
    // Junk rows count as zero, never poison the sum.
    db.set_run_usage("r2", "not json").unwrap();
    assert!((db.conversation_spend_usd("c1").unwrap() - 0.25).abs() < 1e-9);
}
