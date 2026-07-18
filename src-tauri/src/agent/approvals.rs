//! Approval-expiry sweep (Task 4.3): fail-closed denial of stale parked approvals.
//!
//! A write-risk tool parks an approval and the driver awaits its oneshot. If the
//! user walks away, the pending row is expired in the store (fail-closed) — but
//! that alone would leave the awaiting driver blocked until its long safety
//! timeout. This sweep closes that gap: it expires the stale rows AND signals
//! `Deny` to each affected run's parked oneshot, so `await_approval` returns
//! promptly and never hangs.

use fm_agent::types::ApprovalResponse;

use crate::agent::registry::ActorRegistry;
use crate::store::{StoreHandle, StoreResult};

/// Expire pending interactions created at/before `cutoff` and `Deny` each affected
/// run's parked approval oneshot (Task 4.3). Returns the run ids denied. Runs
/// with an expired row but no live waiter are still expired in the store (a
/// resolve with no waiter is a harmless no-op), so a later `await_approval` on a
/// re-parked run still sees the row gone and fails closed.
pub async fn expire_and_deny_stale_approvals(
    store: &StoreHandle,
    registry: &ActorRegistry,
    cutoff: &str,
    now: &str,
) -> StoreResult<Vec<String>> {
    let (c, n) = (cutoff.to_string(), now.to_string());
    let run_ids = store.call(move |db| db.expire_pending_runs(&c, &n)).await?;
    for run_id in &run_ids {
        // Deny the parked waiter; a no-op if this run had none parked.
        registry.resolve_approval(run_id, ApprovalResponse::Deny);
    }
    Ok(run_ids)
}
