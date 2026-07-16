//! Bounded in-memory research run state + retry (Phase 3.6).
//!
//! After a research turn reaches a terminal answer/digest, the driver stores the
//! run's read ledger keyed by `run_id`. A retry mints a NEW `run_id` (retries
//! always create a new run — the parent's events stay dead) and seeds the new
//! run from the parent per the requested phase:
//! - `Searching`    → reuse the plan, clear the ledger (re-search downstream).
//! - `Reading`      → keep successful reads, drop failed/unread for re-fetch.
//! - `Synthesizing` → reuse the whole read ledger (only re-run synthesis).
//!
//! `run_research_turn` consumes the seed: a Reading/Synthesizing seed with a
//! non-empty ledger drives the pipeline through a [`SeededBackend`] so the new
//! run SKIPS the network search/read stages and re-runs synthesis on the stored
//! sources. A Searching seed (or none) runs the pipeline fresh.
//!
//! Bounds: 8 runs max, 30-minute TTL, LRU eviction.

use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::commands::cache::BoundedCache;
use crate::commands::run::valid_run_id;
use fm_research::research::{SourceRecord, SourceStatus};

/// Which phase a retry resumes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RetryPhase {
    Searching,
    Reading,
    Synthesizing,
}

impl RetryPhase {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "searching" | "search" => Some(Self::Searching),
            "reading" | "read" => Some(Self::Reading),
            "synthesizing" | "synthesis" | "synthesize" => Some(Self::Synthesizing),
            _ => None,
        }
    }
}

/// Stored state for one research run.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StoredRun {
    pub question: String,
    /// Plan queries (empty = no plan / question used directly).
    pub plan: Vec<String>,
    /// The read ledger (validated sources with statuses/excerpts).
    pub ledger: Vec<SourceRecord>,
    /// Optional backend override recorded for this run.
    pub backend_override: Option<String>,
    /// Which phase this run should resume from (None = fresh run).
    pub resume_from: Option<RetryPhase>,
    /// The parent run this was retried from (None = original).
    pub parent: Option<String>,
}

/// Roadmap bounds: 8 runs, 30-minute TTL.
const MAX_RUNS: usize = 8;
const TTL_SECS: i64 = 30 * 60;

/// Managed state: bounded run ledger cache.
pub struct ResearchRunState(pub Mutex<BoundedCache<String, StoredRun>>);

impl Default for ResearchRunState {
    fn default() -> Self {
        Self(Mutex::new(BoundedCache::new(MAX_RUNS, TTL_SECS)))
    }
}

impl ResearchRunState {
    /// Store (or replace) a run's state.
    pub fn put(&self, run_id: &str, run: StoredRun, now: i64) {
        if let Ok(mut c) = self.0.lock() {
            c.insert(run_id.to_string(), run, now);
        }
    }

    /// Fetch a run's stored state (bumps recency).
    pub fn get(&self, run_id: &str, now: i64) -> Option<StoredRun> {
        self.0
            .lock()
            .ok()
            .and_then(|mut c| c.get(&run_id.to_string(), now).cloned())
    }

    /// Prepare a retry: validate the parent exists, mint a new run_id, seed the
    /// new run per `phase`, store it, and return the new run_id. Errors when the
    /// parent is unknown/expired or the new id is malformed.
    pub fn retry(
        &self,
        parent_run_id: &str,
        phase: RetryPhase,
        backend_override: Option<String>,
        new_run_id: &str,
        now: i64,
    ) -> Result<String, String> {
        if !valid_run_id(new_run_id) {
            return Err("invalid new run id".into());
        }
        let parent = self
            .get(parent_run_id, now)
            .ok_or_else(|| "parent run not found or expired".to_string())?;

        let ledger = match phase {
            // Reuse the plan, clear reads so the new run re-searches downstream.
            RetryPhase::Searching => Vec::new(),
            // Keep successful reads; drop failed/unread so they are retried.
            RetryPhase::Reading => parent
                .ledger
                .iter()
                .filter(|s| s.status == SourceStatus::Read && s.excerpt.is_some())
                .cloned()
                .collect(),
            // Reuse the whole read ledger; only re-run synthesis.
            RetryPhase::Synthesizing => parent.ledger.clone(),
        };
        let seeded = StoredRun {
            question: parent.question.clone(),
            plan: parent.plan.clone(),
            ledger,
            backend_override,
            resume_from: Some(phase),
            parent: Some(parent_run_id.to_string()),
        };
        self.put(new_run_id, seeded, now);
        Ok(new_run_id.to_string())
    }
}

/// Retry a research run. Returns `{ "run_id": "<new>" }`. The UI then calls
/// `chat_send` with the returned run id; the parent's events stay dead.
#[tauri::command(rename_all = "snake_case")]
pub fn research_retry(
    app: tauri::AppHandle,
    parent_run_id: String,
    phase: String,
    backend_override: Option<String>,
) -> crate::error::AppResult<String> {
    use crate::error::AppError;
    use tauri::Manager;
    let phase = RetryPhase::parse(&phase)
        .ok_or_else(|| AppError::Config("phase must be searching|reading|synthesizing".into()))?;
    let state = app.state::<ResearchRunState>();
    let new_id = crate::commands::run::gen_run_id();
    let now = crate::commands::model::now_secs();
    let run_id = state
        .retry(&parent_run_id, phase, backend_override, &new_id, now)
        .map_err(AppError::Config)?;
    Ok(serde_json::json!({ "run_id": run_id }).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use fm_research::research::{SourceBackend, SourceKind};

    fn src(id: &str, status: SourceStatus, has_excerpt: bool) -> SourceRecord {
        SourceRecord {
            id: id.into(),
            requested_url: format!("https://ex.com/{id}"),
            final_url: None,
            canonical_url: format!("https://ex.com/{id}"),
            title: id.into(),
            domain: "ex.com".into(),
            retrieved_at: "t".into(),
            status,
            kind: SourceKind::Primary,
            backend: SourceBackend::BasicHttp,
            snippet: None,
            excerpt: has_excerpt.then(|| "quote".to_string()),
            error_code: None,
        }
    }

    fn parent_run() -> StoredRun {
        StoredRun {
            question: "why".into(),
            plan: vec!["q1".into(), "q2".into()],
            ledger: vec![
                src("S1", SourceStatus::Read, true),
                src("S2", SourceStatus::Failed, false),
                src("S3", SourceStatus::Read, false),
            ],
            backend_override: None,
            resume_from: None,
            parent: None,
        }
    }

    #[test]
    fn retry_unknown_parent_errors() {
        let st = ResearchRunState::default();
        let err = st
            .retry(
                "p",
                RetryPhase::Searching,
                None,
                &crate::commands::run::gen_run_id(),
                0,
            )
            .unwrap_err();
        assert!(err.contains("parent"));
    }

    #[test]
    fn searching_reuses_plan_clears_ledger() {
        let st = ResearchRunState::default();
        st.put("parent-x", parent_run(), 0);
        let new = crate::commands::run::gen_run_id();
        let id = st
            .retry("parent-x", RetryPhase::Searching, None, &new, 0)
            .unwrap();
        let seeded = st.get(&id, 0).unwrap();
        assert_eq!(seeded.plan, vec!["q1", "q2"]);
        assert!(seeded.ledger.is_empty());
        assert_eq!(seeded.resume_from, Some(RetryPhase::Searching));
        assert_eq!(seeded.parent.as_deref(), Some("parent-x"));
    }

    #[test]
    fn reading_keeps_only_successful_reads() {
        let st = ResearchRunState::default();
        st.put("parent-x", parent_run(), 0);
        let new = crate::commands::run::gen_run_id();
        let id = st
            .retry("parent-x", RetryPhase::Reading, None, &new, 0)
            .unwrap();
        let seeded = st.get(&id, 0).unwrap();
        // Only S1 (Read + excerpt) survives; S2 (Failed) and S3 (no excerpt) drop.
        assert_eq!(seeded.ledger.len(), 1);
        assert_eq!(seeded.ledger[0].id, "S1");
    }

    #[test]
    fn synthesizing_reuses_full_ledger() {
        let st = ResearchRunState::default();
        st.put("parent-x", parent_run(), 0);
        let new = crate::commands::run::gen_run_id();
        let id = st
            .retry(
                "parent-x",
                RetryPhase::Synthesizing,
                Some("basic".into()),
                &new,
                0,
            )
            .unwrap();
        let seeded = st.get(&id, 0).unwrap();
        assert_eq!(seeded.ledger.len(), 3);
        assert_eq!(seeded.backend_override.as_deref(), Some("basic"));
    }

    #[test]
    fn retry_rejects_bad_new_id() {
        let st = ResearchRunState::default();
        st.put("parent-x", parent_run(), 0);
        let err = st
            .retry("parent-x", RetryPhase::Searching, None, "not-a-uuid", 0)
            .unwrap_err();
        assert!(err.contains("invalid"));
    }

    #[test]
    fn bounds_evict_beyond_capacity() {
        let st = ResearchRunState::default();
        for i in 0..(MAX_RUNS + 4) {
            st.put(&format!("run-{i}"), parent_run(), i as i64);
        }
        assert!(st.0.lock().unwrap().len() <= MAX_RUNS);
    }
}
