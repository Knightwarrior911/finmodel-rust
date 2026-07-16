//! Per-run ownership + targeted cancellation (Phase 3.1).
//!
//! The UI creates `run_id = crypto.randomUUID()` and passes it with
//! `conversation_id` to `chat_send`; Stop calls idempotent
//! `chat_cancel(conversation_id, run_id)`. The backend owns exactly one
//! [`tokio_util::sync::CancellationToken`] per active run, keyed by
//! `(conversation_id, run_id)`, so a Stop targets a specific conversation's
//! run — never another chat's.
//!
//! A [`RunGuard`] deregisters its run on drop (RAII) — an error or panic
//! mid-run can never leak an "active" entry that would block future retries.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex;
use tokio_util::sync::CancellationToken;

/// Composite key: conversation owns its runs.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct RunKey {
    conversation_id: String,
    run_id: String,
}

type Tokens = Arc<Mutex<HashMap<RunKey, CancellationToken>>>;

/// Managed state: the live set of run cancellation tokens.
#[derive(Default)]
pub struct RunRegistry {
    tokens: Tokens,
}

/// Why a run could not be started.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunError {
    /// `run_id` is not a valid UUID-shaped identifier.
    BadFormat,
    /// A run with this `(conversation_id, run_id)` is already active.
    Duplicate,
    /// `conversation_id` is empty or invalid.
    BadConversation,
}

/// An active run's handle. Holds the cancellation token; deregisters on drop.
#[derive(Debug)]
pub struct RunGuard {
    tokens: Tokens,
    key: RunKey,
    /// The cancellation token the execution loop selects on.
    pub cancel: CancellationToken,
}

impl Drop for RunGuard {
    fn drop(&mut self) {
        self.tokens.lock().remove(&self.key);
    }
}

impl RunRegistry {
    /// Register `(conversation_id, run_id)`, returning a guard whose `cancel`
    /// token the async loop races against I/O. Rejects malformed ids and
    /// duplicates. Also rejects a second concurrent run for the same
    /// conversation (one active run per chat).
    pub fn start(&self, conversation_id: &str, run_id: &str) -> Result<RunGuard, RunError> {
        if conversation_id.trim().is_empty() {
            return Err(RunError::BadConversation);
        }
        if !valid_run_id(run_id) {
            return Err(RunError::BadFormat);
        }
        let key = RunKey {
            conversation_id: conversation_id.to_string(),
            run_id: run_id.to_string(),
        };
        let mut tokens = self.tokens.lock();
        if tokens.contains_key(&key) {
            return Err(RunError::Duplicate);
        }
        // One active run per conversation.
        if tokens
            .keys()
            .any(|k| k.conversation_id == key.conversation_id)
        {
            return Err(RunError::Duplicate);
        }
        let cancel = CancellationToken::new();
        tokens.insert(key.clone(), cancel.clone());
        Ok(RunGuard {
            tokens: Arc::clone(&self.tokens),
            key,
            cancel,
        })
    }

    /// Cancel a specific conversation's run (idempotent). Returns whether the
    /// run was active **and** owned by that conversation. A mismatched
    /// conversation_id never cancels another chat's run.
    pub fn cancel(&self, conversation_id: &str, run_id: &str) -> bool {
        let key = RunKey {
            conversation_id: conversation_id.to_string(),
            run_id: run_id.to_string(),
        };
        match self.tokens.lock().get(&key) {
            Some(c) => {
                c.cancel();
                true
            }
            None => false,
        }
    }

    /// Cancel every active run for a single conversation. Returns how many
    /// runs were signaled.
    pub fn cancel_conversation(&self, conversation_id: &str) -> usize {
        let tokens = self.tokens.lock();
        let mut n = 0usize;
        for (k, c) in tokens.iter() {
            if k.conversation_id == conversation_id {
                c.cancel();
                n += 1;
            }
        }
        n
    }

    /// Cancel every active run — last-resort path when no ids are supplied.
    pub fn cancel_all(&self) -> usize {
        let tokens = self.tokens.lock();
        let n = tokens.len();
        for c in tokens.values() {
            c.cancel();
        }
        n
    }

    /// Whether any run is active for `conversation_id`.
    pub fn conversation_busy(&self, conversation_id: &str) -> bool {
        self.tokens
            .lock()
            .keys()
            .any(|k| k.conversation_id == conversation_id)
    }
}

/// Whether `s` is a UUID-shaped run id (`8-4-4-4-12` hex with hyphens), as
/// produced by the web `crypto.randomUUID()`.
pub fn valid_run_id(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 36 {
        return false;
    }
    // 8-4-4-4-12
    let hyphens = [8usize, 13, 18, 23];
    for (i, &c) in b.iter().enumerate() {
        if hyphens.contains(&i) {
            if c != b'-' {
                return false;
            }
        } else if !c.is_ascii_hexdigit() {
            return false;
        }
    }
    true
}

/// Generate a fresh UUID-v4-shaped run id (fallback when the UI omits one).
pub fn gen_run_id() -> String {
    let mut bytes = [0u8; 16];
    for b in &mut bytes {
        *b = rand::random();
    }
    // Set version 4 + variant bits.
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0],
        bytes[1],
        bytes[2],
        bytes[3],
        bytes[4],
        bytes[5],
        bytes[6],
        bytes[7],
        bytes[8],
        bytes[9],
        bytes[10],
        bytes[11],
        bytes[12],
        bytes[13],
        bytes[14],
        bytes[15],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_registers_and_guard_drop_deregisters() {
        let reg = RunRegistry::default();
        let g = reg.start("conv-a", &gen_run_id()).unwrap();
        assert!(reg.conversation_busy("conv-a"));
        drop(g);
        assert!(!reg.conversation_busy("conv-a"));
    }

    #[test]
    fn bad_format_rejected() {
        let reg = RunRegistry::default();
        assert_eq!(
            reg.start("conv-a", "not-a-uuid").unwrap_err(),
            RunError::BadFormat
        );
        assert_eq!(
            reg.start("", &gen_run_id()).unwrap_err(),
            RunError::BadConversation
        );
    }

    #[test]
    fn duplicate_run_id_rejected_while_active() {
        let reg = RunRegistry::default();
        let id = gen_run_id();
        let _g = reg.start("conv-a", &id).unwrap();
        assert_eq!(reg.start("conv-a", &id).unwrap_err(), RunError::Duplicate);
        // Same conversation, different run id — still rejected (one per chat).
        assert_eq!(
            reg.start("conv-a", &gen_run_id()).unwrap_err(),
            RunError::Duplicate
        );
        // Different conversation is fine.
        let _h = reg.start("conv-b", &gen_run_id()).unwrap();
    }

    #[test]
    fn cancel_targets_specific_run_and_is_idempotent() {
        let reg = RunRegistry::default();
        let id = gen_run_id();
        let g = reg.start("conv-a", &id).unwrap();
        assert!(reg.cancel("conv-a", &id));
        assert!(g.cancel.is_cancelled());
        // Idempotent.
        assert!(reg.cancel("conv-a", &id));
    }

    #[test]
    fn mismatched_conversation_does_not_cancel() {
        let reg = RunRegistry::default();
        let id = gen_run_id();
        let g = reg.start("conv-a", &id).unwrap();
        // Wrong conversation: must not cancel.
        assert!(!reg.cancel("conv-other", &id));
        assert!(!g.cancel.is_cancelled());
        // Right conversation works.
        assert!(reg.cancel("conv-a", &id));
        assert!(g.cancel.is_cancelled());
    }

    #[test]
    fn cancel_all_signals_every_active_run() {
        let reg = RunRegistry::default();
        let a = reg.start("conv-a", &gen_run_id()).unwrap();
        let b = reg.start("conv-b", &gen_run_id()).unwrap();
        assert_eq!(reg.cancel_all(), 2);
        assert!(a.cancel.is_cancelled());
        assert!(b.cancel.is_cancelled());
    }

    #[test]
    fn generated_ids_are_valid_and_unique() {
        let mut seen = std::collections::HashSet::new();
        for _ in 0..64 {
            let id = gen_run_id();
            assert!(valid_run_id(&id), "bad id: {id}");
            assert!(seen.insert(id));
        }
    }

    #[test]
    fn rejects_malformed_run_ids() {
        assert!(!valid_run_id(""));
        assert!(!valid_run_id("1234"));
        assert!(!valid_run_id("xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx"));
        assert!(!valid_run_id("12345678-1234-1234-1234-12345678901")); // short
    }
}
