//! One-level child delegation supervisor (Task 5.2).
//!
//! Replaces the label-only `SubagentPool` accounting with a supervisor that
//! actually executes a child to terminal via [`run_turn`] — the same pump, store,
//! and sink as any run — then composes the durable delegation lifecycle:
//! persist-before-execute (5.1), terminal-status recovery, and at-least-once
//! delivery of the child's single result to the parent (5.3). Children are one
//! level deep: a child never spawns grandchildren.

use std::future::Future;

use fm_agent::machine::AgentMachine;
use fm_agent::types::EventKind;

use crate::agent::actor::{run_turn, Driver, TurnOutcome};
use crate::agent::events::EventSink;
use crate::store::{now_iso, StoreHandle};

/// Children are one level deep (a parent's depth must be `< MAX_CHILD_DEPTH`).
pub const MAX_CHILD_DEPTH: u32 = 1;

/// The delivery-claim owner recorded for a supervisor-driven child (5.3 CAS).
const CLAIM_OWNER: &str = "supervisor";

/// Identifiers a caller mints for one child dispatch (explicit so dispatch is
/// deterministic and crash-recoverable).
#[derive(Clone, Debug)]
pub struct ChildDispatch {
    pub delegation_id: String,
    pub child_run_id: String,
}

/// Why a child could not be dispatched.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChildError {
    /// One-level-deep limit: a child cannot spawn its own children.
    DepthExceeded,
}

/// Map a child's terminal turn to its delegation status. A cancelled child has no
/// result to deliver; a partial completion is delivered as a `warning`; an
/// interrupted / budget-limited child's outcome is not proven (`outcome_unknown`).
fn delegation_status(out: &TurnOutcome) -> &'static str {
    match out.event {
        EventKind::RunCompleted if out.partial => "warning",
        EventKind::RunCompleted => "succeeded",
        EventKind::RunFailed => "failed",
        EventKind::RunCancelled => "cancelled",
        _ => "outcome_unknown",
    }
}

/// Run one child delegation to terminal and deliver its single result to the
/// parent at least once. Enforces the depth limit, persists the delegation +
/// child run row BEFORE the child executes (recoverable), links + drives the
/// child via [`run_turn`], finalizes the delegation with its mapped terminal
/// status, then runs the delivery CAS: `claim` the terminal result, await the
/// caller's `deliver` (the parent-context append), and `ack` only on its success
/// — releasing the claim on failure so a later attempt or restart redelivers.
/// `deliver` is never a no-op ack: the row stays claimed until the append truly
/// lands, so the result reaches the parent exactly once and is never lost or
/// double-synthesized. A cancelled child carries no result and is never claimed.
/// Returns the child's terminal outcome.
#[allow(clippy::too_many_arguments)]
pub async fn run_child_delegation<D, Del, Fut>(
    store: &StoreHandle,
    sink: &dyn EventSink,
    conversation_id: &str,
    parent_run_id: &str,
    parent_tool_call_id: Option<String>,
    parent_depth: u32,
    ids: ChildDispatch,
    task_json: String,
    machine: AgentMachine,
    child_driver: D,
    deliver: Del,
) -> Result<TurnOutcome, ChildError>
where
    D: Driver,
    Del: FnOnce(String) -> Fut,
    Fut: Future<Output = bool>,
{
    if parent_depth >= MAX_CHILD_DEPTH {
        return Err(ChildError::DepthExceeded);
    }
    let now = now_iso();

    // 5.1: persist the delegation + child run row BEFORE the child executes, then
    // link the child (queued → running), so a crash mid-dispatch is recoverable.
    {
        let del = ids.delegation_id.clone();
        let child = ids.child_run_id.clone();
        let conv = conversation_id.to_string();
        let parent = parent_run_id.to_string();
        let ptc = parent_tool_call_id.clone();
        let tj = task_json.clone();
        let n = now.clone();
        store
            .call(move |db| {
                db.insert_delegation(&del, &parent, ptc.as_deref(), &tj, &n)?;
                db.insert_run(
                    &child,
                    &conv,
                    None,
                    None,
                    "running",
                    "preparing",
                    None,
                    None,
                    &n,
                )?;
                db.set_delegation_child(&del, &child)
            })
            .await
            .expect("persist child dispatch");
    }

    // Drive the child to terminal in the same harness as any run.
    let out = run_turn(
        store,
        sink,
        conversation_id,
        &ids.child_run_id,
        machine,
        child_driver,
    )
    .await;

    // Finalize the delegation with its mapped terminal status.
    let status = delegation_status(&out);
    let result_json = serde_json::json!({
        "child_run_id": ids.child_run_id,
        "status": status,
        "partial": out.partial,
    })
    .to_string();
    {
        let del = ids.delegation_id.clone();
        let res = if status == "cancelled" {
            None
        } else {
            Some(result_json.clone())
        };
        let n = now.clone();
        store
            .call(move |db| db.finish_delegation(&del, status, res.as_deref(), None, &n))
            .await
            .expect("finish delegation");
    }

    // 5.3: deliver at least once. Claim → parent append (`deliver`) → ack on
    // success, release on failure. A cancelled child is not deliverable, so it is
    // never claimed and its result never reaches the parent.
    if status != "cancelled" {
        let del = ids.delegation_id.clone();
        let n = now.clone();
        let claimed = store
            .call(move |db| {
                db.claim_delegation_delivery(&del, CLAIM_OWNER, &n)
                    .unwrap_or(false)
            })
            .await;
        if claimed {
            if deliver(result_json).await {
                let del = ids.delegation_id.clone();
                store
                    .call(move |db| {
                        db.ack_delegation_delivery(&del, CLAIM_OWNER)
                            .unwrap_or(false)
                    })
                    .await;
            } else {
                let del = ids.delegation_id.clone();
                store
                    .call(move |db| {
                        db.release_delegation_claim(&del, CLAIM_OWNER)
                            .unwrap_or(false)
                    })
                    .await;
            }
        }
    }

    Ok(out)
}
