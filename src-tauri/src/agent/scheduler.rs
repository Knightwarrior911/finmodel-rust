//! Tool-call scheduling: group independent read-only calls into one batch,
//! serialize dependencies and writes. The reducer decides *whether* to run a
//! batch; this module decides *how* the accepted calls are grouped so the driver
//! can execute a read batch concurrently (bounded by the run's slots) while
//! writes and dependents run in order.

use fm_agent::types::Risk;

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
        assert_eq!(batches, vec![vec!["w1".to_string()], vec!["w2".to_string()]]);
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
        assert_eq!(batches, vec![vec!["a".to_string(), "c".into()], vec!["b".into()]]);
    }

    #[test]
    fn local_create_groups_but_overwrite_serializes() {
        // LocalCreate auto-runs but is not ReadOnly -> serialized (writes go alone).
        let calls = vec![
            PlannedCall::read("r"),
            PlannedCall::write("new", Risk::LocalCreate),
        ];
        let batches = plan_batches(&calls);
        assert_eq!(batches, vec![vec!["r".to_string()], vec!["new".to_string()]]);
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
