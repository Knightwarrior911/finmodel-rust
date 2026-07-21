//! The conversation actor registry: the sole active-run authority after cutover.
//!
//! It enforces the plan's concurrency contract:
//! - at most one active run per conversation;
//! - at most [`MAX_ACTIVE_CONVERSATIONS`] conversations running at once;
//! - a global [`GLOBAL_SLOTS`] cap on concurrently executing provider/tool/child
//!   I/O operations, with at most [`PER_RUN_SLOTS`] per run;
//! - RAII deregistration so an error/panic can never leak an active entry.
//!
//! Actors, coordinators, and parent workflows must drop their execution slot
//! before awaiting children, approval, timers, or DB replies — the registry
//! only hands out slots for actively executing operations; holding one while
//! awaiting is a usage bug the scheduler avoids (a waiting parent holds none).

use std::collections::HashMap;
use std::sync::Arc;

use fm_agent::types::ApprovalResponse;
use parking_lot::Mutex;
use tokio::sync::oneshot;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio_util::sync::CancellationToken;

/// Maximum conversations with an active run simultaneously.
pub const MAX_ACTIVE_CONVERSATIONS: usize = 3;
/// Global cap on concurrently executing provider/tool/child operations.
pub const GLOBAL_SLOTS: usize = 8;
/// Per-run cap on concurrently executing operations.
pub const PER_RUN_SLOTS: usize = 4;

/// Why a run could not be started.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartError {
    /// The conversation already has an active run.
    ConversationBusy,
    /// Too many conversations are already running.
    TooManyActive,
}

impl std::fmt::Display for StartError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StartError::ConversationBusy => write!(f, "conversation already has an active run"),
            StartError::TooManyActive => write!(f, "too many active conversation runs"),
        }
    }
}
impl std::error::Error for StartError {}

#[derive(Clone)]
struct Entry {
    run_id: String,
    token: CancellationToken,
    /// Resumable pause signal, distinct from `token` (terminal cancel).
    interrupt: CancellationToken,
    /// Shared per-run child/tool I/O slots. Stored in the registry entry as
    /// well as the [`RunHandle`] so nested executors (such as a batch swarm)
    /// can acquire the SAME permits instead of creating an unbounded inner
    /// pool.
    per_run: Arc<Semaphore>,
}

type Map = Arc<Mutex<HashMap<String, Entry>>>;
/// Parked approvals keyed by run id — the driver awaits the receiver; the
/// `agent_approve` command resolves it. First answer wins (decision 7).
type Approvals = Arc<Mutex<HashMap<String, oneshot::Sender<ApprovalResponse>>>>;

/// The active-run authority + bounded execution slots.
#[derive(Clone)]
pub struct ActorRegistry {
    map: Map,
    global: Arc<Semaphore>,
    approvals: Approvals,
}

impl Default for ActorRegistry {
    fn default() -> Self {
        ActorRegistry {
            map: Arc::new(Mutex::new(HashMap::new())),
            global: Arc::new(Semaphore::new(GLOBAL_SLOTS)),
            approvals: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl ActorRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Acquire one execution slot for the active run in `conversation_id`.
    ///
    /// Nested executors cannot borrow the parent [`RunHandle`] directly, so
    /// they resolve its shared per-run semaphore through the registry. `None`
    /// means the conversation is not owned by an active unified-agent run.
    /// The returned permit releases both global and per-run capacity on drop.
    pub async fn acquire_active_slot(&self, conversation_id: &str) -> Option<SlotPermit> {
        let per_run = self.map.lock().get(conversation_id)?.per_run.clone();
        let global = self.global.clone().acquire_owned().await.ok()?;
        let per_run = per_run.acquire_owned().await.ok()?;
        Some(SlotPermit {
            _global: global,
            _per_run: per_run,
        })
    }

    /// Park an approval for `run_id`; the returned receiver resolves when
    /// `resolve_approval` is called (or drops to `Deny` on cancel/timeout).
    pub fn park_approval(&self, run_id: &str) -> oneshot::Receiver<ApprovalResponse> {
        let (tx, rx) = oneshot::channel();
        self.approvals.lock().insert(run_id.to_string(), tx);
        rx
    }

    /// Resolve a parked approval. Returns true iff a waiter was signalled.
    pub fn resolve_approval(&self, run_id: &str, response: ApprovalResponse) -> bool {
        if let Some(tx) = self.approvals.lock().remove(run_id) {
            tx.send(response).is_ok()
        } else {
            false
        }
    }

    /// Register a new active run. Fails if the conversation is busy or the
    /// active-conversation cap is reached. The returned [`RunHandle`]
    /// deregisters on drop.
    pub fn start_run(&self, conversation_id: &str, run_id: &str) -> Result<RunHandle, StartError> {
        let mut map = self.map.lock();
        if map.contains_key(conversation_id) {
            return Err(StartError::ConversationBusy);
        }
        if map.len() >= MAX_ACTIVE_CONVERSATIONS {
            return Err(StartError::TooManyActive);
        }
        let token = CancellationToken::new();
        let interrupt = CancellationToken::new();
        let per_run = Arc::new(Semaphore::new(PER_RUN_SLOTS));
        map.insert(
            conversation_id.to_string(),
            Entry {
                run_id: run_id.to_string(),
                token: token.clone(),
                interrupt: interrupt.clone(),
                per_run: per_run.clone(),
            },
        );
        Ok(RunHandle {
            map: self.map.clone(),
            conversation_id: conversation_id.to_string(),
            run_id: run_id.to_string(),
            token,
            interrupt,
            global: self.global.clone(),
            per_run,
        })
    }

    /// Idempotently cancel a specific conversation's run. Returns true if a
    /// matching active run was found and signalled.
    pub fn cancel(&self, conversation_id: &str, run_id: &str) -> bool {
        let map = self.map.lock();
        match map.get(conversation_id) {
            Some(e) if e.run_id == run_id => {
                e.token.cancel();
                true
            }
            _ => false,
        }
    }

    /// Idempotently pause (resumably interrupt) a specific conversation's run.
    /// Fires the interrupt token, distinct from the terminal cancel token, so the
    /// run ends `RunInterrupted` (resumable) rather than `RunCancelled`. Returns
    /// true if a matching active run was found and signalled.
    pub fn pause(&self, conversation_id: &str, run_id: &str) -> bool {
        let map = self.map.lock();
        match map.get(conversation_id) {
            Some(e) if e.run_id == run_id => {
                e.interrupt.cancel();
                true
            }
            _ => false,
        }
    }

    /// Number of conversations with an active run.
    pub fn active_count(&self) -> usize {
        self.map.lock().len()
    }

    /// The active run id for a conversation, if any.
    pub fn active_run(&self, conversation_id: &str) -> Option<String> {
        self.map
            .lock()
            .get(conversation_id)
            .map(|e| e.run_id.clone())
    }

    /// All active `(conversation_id, run_id)` pairs.
    pub fn active_runs(&self) -> Vec<(String, String)> {
        self.map
            .lock()
            .iter()
            .map(|(c, e)| (c.clone(), e.run_id.clone()))
            .collect()
    }

    /// Global execution slots currently available.
    pub fn global_available(&self) -> usize {
        self.global.available_permits()
    }
}

/// An active run's handle. Holds its cancellation token and per-run slot pool;
/// deregisters the conversation on drop (RAII).
pub struct RunHandle {
    map: Map,
    conversation_id: String,
    run_id: String,
    token: CancellationToken,
    interrupt: CancellationToken,
    global: Arc<Semaphore>,
    per_run: Arc<Semaphore>,
}

impl RunHandle {
    pub fn run_id(&self) -> &str {
        &self.run_id
    }

    pub fn cancellation_token(&self) -> CancellationToken {
        self.token.clone()
    }

    pub fn is_cancelled(&self) -> bool {
        self.token.is_cancelled()
    }

    /// The resumable interrupt (pause) token, distinct from cancellation.
    pub fn interrupt_token(&self) -> CancellationToken {
        self.interrupt.clone()
    }

    /// Per-run execution slots currently available.
    pub fn per_run_available(&self) -> usize {
        self.per_run.available_permits()
    }

    /// Acquire one execution slot (one global + one per-run permit). Await this
    /// only immediately before executing an operation, never while waiting on a
    /// child/approval/timer/DB — a waiting parent must hold no slot.
    pub async fn acquire_slot(&self) -> SlotPermit {
        let global = self
            .global
            .clone()
            .acquire_owned()
            .await
            .expect("global sem");
        let per_run = self
            .per_run
            .clone()
            .acquire_owned()
            .await
            .expect("per-run sem");
        SlotPermit {
            _global: global,
            _per_run: per_run,
        }
    }

    /// Non-blocking slot acquisition (tests / opportunistic scheduling).
    pub fn try_acquire_slot(&self) -> Option<SlotPermit> {
        let global = self.global.clone().try_acquire_owned().ok()?;
        let per_run = self.per_run.clone().try_acquire_owned().ok()?;
        Some(SlotPermit {
            _global: global,
            _per_run: per_run,
        })
    }
}

impl Drop for RunHandle {
    fn drop(&mut self) {
        let mut map = self.map.lock();
        // Only remove if we still own this conversation's entry (matching run).
        if let Some(e) = map.get(&self.conversation_id) {
            if e.run_id == self.run_id {
                map.remove(&self.conversation_id);
            }
        }
    }
}

/// A held execution slot; releases both permits on drop.
pub struct SlotPermit {
    _global: OwnedSemaphorePermit,
    _per_run: OwnedSemaphorePermit,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_run_per_conversation() {
        let reg = ActorRegistry::new();
        let _h = reg.start_run("c1", "r1").unwrap();
        assert!(matches!(
            reg.start_run("c1", "r2"),
            Err(StartError::ConversationBusy)
        ));
    }

    #[test]
    fn at_most_three_active_conversations() {
        let reg = ActorRegistry::new();
        let _a = reg.start_run("c1", "r1").unwrap();
        let _b = reg.start_run("c2", "r2").unwrap();
        let _c = reg.start_run("c3", "r3").unwrap();
        assert_eq!(reg.active_count(), 3);
        assert!(matches!(
            reg.start_run("c4", "r4"),
            Err(StartError::TooManyActive)
        ));
    }

    #[test]
    fn raii_deregistration_frees_the_conversation() {
        let reg = ActorRegistry::new();
        {
            let _h = reg.start_run("c1", "r1").unwrap();
            assert_eq!(reg.active_count(), 1);
        }
        assert_eq!(reg.active_count(), 0);
        // Free again for a new run.
        let _h2 = reg.start_run("c1", "r2").unwrap();
        assert_eq!(reg.active_run("c1").as_deref(), Some("r2"));
    }

    #[test]
    fn cancel_targets_the_specific_run() {
        let reg = ActorRegistry::new();
        let h = reg.start_run("c1", "r1").unwrap();
        // Wrong run id does nothing.
        assert!(!reg.cancel("c1", "rX"));
        assert!(!h.is_cancelled());
        // Right one cancels.
        assert!(reg.cancel("c1", "r1"));
        assert!(h.is_cancelled());
    }

    #[test]
    fn per_run_slots_capped_at_four() {
        let reg = ActorRegistry::new();
        let h = reg.start_run("c1", "r1").unwrap();
        let mut permits = Vec::new();
        for _ in 0..PER_RUN_SLOTS {
            permits.push(h.try_acquire_slot().expect("slot"));
        }
        assert_eq!(h.per_run_available(), 0);
        // Fifth slot for the same run is refused.
        assert!(h.try_acquire_slot().is_none());
        // Dropping one frees it (a waiting parent that released holds none).
        permits.pop();
        assert_eq!(h.per_run_available(), 1);
        assert!(h.try_acquire_slot().is_some());
    }

    #[test]
    fn global_slots_capped_across_runs() {
        let reg = ActorRegistry::new();
        // Three runs, up to 4 each, but global cap is 8.
        let h1 = reg.start_run("c1", "r1").unwrap();
        let h2 = reg.start_run("c2", "r2").unwrap();
        let h3 = reg.start_run("c3", "r3").unwrap();
        let mut permits = Vec::new();
        // 4 from r1, 4 from r2 -> 8 global used.
        for _ in 0..4 {
            permits.push(h1.try_acquire_slot().unwrap());
        }
        for _ in 0..4 {
            permits.push(h2.try_acquire_slot().unwrap());
        }
        assert_eq!(reg.global_available(), 0);
        // r3 has per-run capacity but the global pool is exhausted.
        assert!(h3.try_acquire_slot().is_none());
        // Release one global slot -> r3 can proceed.
        permits.pop();
        assert!(h3.try_acquire_slot().is_some());
    }

    #[tokio::test]
    async fn acquire_slot_awaits_and_releases() {
        let reg = ActorRegistry::new();
        let h = reg.start_run("c1", "r1").unwrap();
        let p = h.acquire_slot().await;
        assert_eq!(h.per_run_available(), PER_RUN_SLOTS - 1);
        drop(p);
        assert_eq!(h.per_run_available(), PER_RUN_SLOTS);
    }

    #[tokio::test]
    async fn nested_executor_acquires_the_active_runs_shared_slots() {
        let reg = ActorRegistry::new();
        assert!(
            reg.acquire_active_slot("missing").await.is_none(),
            "no orphan slot pool for inactive conversations"
        );
        let h = reg.start_run("c1", "r1").unwrap();
        let p = reg
            .acquire_active_slot("c1")
            .await
            .expect("active run slot");
        assert_eq!(h.per_run_available(), PER_RUN_SLOTS - 1);
        assert_eq!(reg.global_available(), GLOBAL_SLOTS - 1);
        drop(p);
        assert_eq!(h.per_run_available(), PER_RUN_SLOTS);
        assert_eq!(reg.global_available(), GLOBAL_SLOTS);
    }
}
