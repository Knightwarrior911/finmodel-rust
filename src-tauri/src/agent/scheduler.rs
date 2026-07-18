//! Tool-call scheduling: group independent read-only calls into one batch,
//! serialize dependencies and writes. The reducer decides *whether* to run a
//! batch; this module decides *how* the accepted calls are grouped so the driver
//! can execute a read batch concurrently (bounded by the run's slots) while
//! writes and dependents run in order.

use fm_agent::types::Risk;
use std::future::Future;

use crate::store::StoreHandle;

/// One accepted call awaiting scheduling.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlannedCall {
    pub tool_call_id: String,
    pub risk: Risk,
    /// Ids of calls that must complete before this one.
    pub depends_on: Vec<String>,
}

impl PlannedCall {
    pub fn read(id: &str) -> Self {
        PlannedCall {
            tool_call_id: id.into(),
            risk: Risk::ReadOnly,
            depends_on: vec![],
        }
    }
    pub fn write(id: &str, risk: Risk) -> Self {
        PlannedCall {
            tool_call_id: id.into(),
            risk,
            depends_on: vec![],
        }
    }
    pub fn after(mut self, dep: &str) -> Self {
        self.depends_on.push(dep.into());
        self
    }
}

/// Plan execution batches. Each returned batch is a set of call ids that may run
/// together; batches run in order. Independent auto-run (read-only / new-version
/// create) calls in the same dependency wave are grouped; every other call
/// (overwrite/delete/export, or anything with an unmet dependency) serializes in
/// its own batch. Order is stable (input order) for determinism.
pub fn plan_batches(calls: &[PlannedCall]) -> Vec<Vec<String>> {
    let mut scheduled: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut batches: Vec<Vec<String>> = Vec::new();
    let total = calls.len();

    while scheduled.len() < total {
        // Ready = not yet scheduled and all deps already scheduled.
        let ready: Vec<&PlannedCall> = calls
            .iter()
            .filter(|c| !scheduled.contains(&c.tool_call_id))
            .filter(|c| c.depends_on.iter().all(|d| scheduled.contains(d)))
            .collect();

        if ready.is_empty() {
            // Unsatisfiable dependency (cycle or missing dep): schedule the rest
            // individually rather than looping forever — never silently drop.
            let rest: Vec<String> = calls
                .iter()
                .filter(|c| !scheduled.contains(&c.tool_call_id))
                .map(|c| c.tool_call_id.clone())
                .collect();
            for id in rest {
                batches.push(vec![id.clone()]);
                scheduled.insert(id);
            }
            break;
        }

        // Group independent auto-run reads into one batch (in input order).
        let mut read_batch: Vec<String> = Vec::new();
        for c in &ready {
            if c.risk.auto_runs() && matches!(c.risk, Risk::ReadOnly) {
                read_batch.push(c.tool_call_id.clone());
            }
        }
        if !read_batch.is_empty() {
            for id in &read_batch {
                scheduled.insert(id.clone());
            }
            batches.push(read_batch);
        }

        // Everything else ready this wave serializes, each in its own batch.
        for c in &ready {
            if c.risk.auto_runs() && matches!(c.risk, Risk::ReadOnly) {
                continue; // already grouped
            }
            batches.push(vec![c.tool_call_id.clone()]);
            scheduled.insert(c.tool_call_id.clone());
        }
    }

    batches
}

/// The result of running the due-schedule sweep once (Task 8.3).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ScheduleSweep {
    /// Schedules whose follow-up succeeded (finalized `done`).
    pub done: Vec<String>,
    /// Schedules whose follow-up failed but will retry (returned to `pending`).
    pub retried: Vec<String>,
    /// Schedules that failed terminally (reached `max_attempts`).
    pub failed: Vec<String>,
}

/// Claim and run every schedule due at `now`, exactly once each (Task 8.3). For
/// each claimed schedule, `run(id)` executes the follow-up; success finalizes it
/// `done`, failure bumps `attempts` and either retries (back to `pending` with a
/// backed-off `retry_next_due`, which MUST be strictly after `now` so it is not
/// re-claimed in this sweep) or fails terminally after `max_attempts`. The claim
/// is transactional (8.1), so concurrent workers never double-run a schedule.
/// Store + injected `run`, so it is testable without a live actor.
pub async fn run_due_schedules<R, Fut>(
    store: &StoreHandle,
    now: &str,
    owner: &str,
    max_attempts: i64,
    retry_next_due: &str,
    mut run: R,
) -> ScheduleSweep
where
    R: FnMut(String) -> Fut,
    Fut: Future<Output = bool>,
{
    let mut sweep = ScheduleSweep::default();
    loop {
        let (n, o) = (now.to_string(), owner.to_string());
        let claimed = store
            .call(move |db| db.claim_due_schedule(&n, &o).unwrap_or(None))
            .await;
        let Some(id) = claimed else { break };
        if run(id.clone()).await {
            let (id2, n2) = (id.clone(), now.to_string());
            store
                .call(move |db| {
                    let _ = db.finish_schedule(&id2, "succeeded", &n2);
                })
                .await;
            sweep.done.push(id);
        } else {
            let (id2, rd) = (id.clone(), retry_next_due.to_string());
            let (_, terminal) = store
                .call(move |db| {
                    db.fail_schedule_attempt(&id2, max_attempts, &rd)
                        .unwrap_or((0, true))
                })
                .await;
            if terminal {
                sweep.failed.push(id);
            } else {
                sweep.retried.push(id);
            }
        }
    }
    sweep
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn independent_reads_group_into_one_batch() {
        let calls = vec![
            PlannedCall::read("a"),
            PlannedCall::read("b"),
            PlannedCall::read("c"),
        ];
        let batches = plan_batches(&calls);
        assert_eq!(batches, vec![vec!["a".to_string(), "b".into(), "c".into()]]);
    }

    #[test]
    fn read_then_write_serializes_write() {
        let calls = vec![
            PlannedCall::read("r"),
            PlannedCall::write("w", Risk::LocalOverwrite),
        ];
        let batches = plan_batches(&calls);
        assert_eq!(batches, vec![vec!["r".to_string()], vec!["w".to_string()]]);
    }

    #[test]
    fn two_writes_each_in_own_batch() {
        let calls = vec![
            PlannedCall::write("w1", Risk::Export),
            PlannedCall::write("w2", Risk::LocalDelete),
        ];
        let batches = plan_batches(&calls);
        assert_eq!(
            batches,
            vec![vec!["w1".to_string()], vec!["w2".to_string()]]
        );
    }

    #[test]
    fn dependency_forces_ordering() {
        // b depends on a; c independent read.
        let calls = vec![
            PlannedCall::read("a"),
            PlannedCall::read("b").after("a"),
            PlannedCall::read("c"),
        ];
        let batches = plan_batches(&calls);
        // Wave 1: a + c (both ready reads). Wave 2: b.
        assert_eq!(
            batches,
            vec![vec!["a".to_string(), "c".into()], vec!["b".into()]]
        );
    }

    #[test]
    fn local_create_groups_but_overwrite_serializes() {
        // LocalCreate auto-runs but is not ReadOnly -> serialized (writes go alone).
        let calls = vec![
            PlannedCall::read("r"),
            PlannedCall::write("new", Risk::LocalCreate),
        ];
        let batches = plan_batches(&calls);
        assert_eq!(
            batches,
            vec![vec!["r".to_string()], vec!["new".to_string()]]
        );
    }

    #[test]
    fn cycle_does_not_hang() {
        let calls = vec![
            PlannedCall::read("a").after("b"),
            PlannedCall::read("b").after("a"),
        ];
        let batches = plan_batches(&calls);
        // Both get scheduled (individually) rather than looping forever.
        let flat: Vec<String> = batches.into_iter().flatten().collect();
        assert_eq!(flat.len(), 2);
        assert!(flat.contains(&"a".to_string()) && flat.contains(&"b".to_string()));
    }

    #[test]
    fn all_calls_scheduled_exactly_once() {
        let calls = vec![
            PlannedCall::read("a"),
            PlannedCall::write("w", Risk::LocalOverwrite).after("a"),
            PlannedCall::read("b"),
        ];
        let batches = plan_batches(&calls);
        let flat: Vec<String> = batches.into_iter().flatten().collect();
        assert_eq!(flat.len(), 3);
    }
}
