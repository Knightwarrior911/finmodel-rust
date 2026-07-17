//! Child subagent management (Phase F).
//!
//! A parent workflow may spawn child subagents for independent read-only
//! tasks (peer comps, parallel research queries). Children are one level
//! deep and consume the same per-run/global execution slots. Orchestration
//! itself does not.

use std::collections::HashMap;
use std::time::Instant;

use fm_agent::budget::{Budget, Policy};

/// Unique identifier for a child subagent within a workflow run.
pub type SubagentId = u32;

/// State of a child subagent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SubagentStatus {
    Queued,
    Running,
    Succeeded,
    Failed(String),
    Cancelled,
}

/// A single child subagent handle.
#[derive(Clone, Debug)]
pub struct SubagentHandle {
    pub id: SubagentId,
    pub label: String,
    pub status: SubagentStatus,
    pub started_at: Option<Instant>,
    pub finished_at: Option<Instant>,
    pub result_summary: Option<String>,
}

/// Manages a set of child subagents for one parent workflow.
pub struct SubagentPool {
    pool_id: String,
    max_children: u32,
    next_id: SubagentId,
    children: HashMap<SubagentId, SubagentHandle>,
    budget: Budget,
}

impl SubagentPool {
    /// Create a new pool. `pool_id` is a stable identifier (e.g. the parent
    /// run id) for logging. `budget` caps total child runtime.
    pub fn new(pool_id: String, max_children: u32, budget: Budget) -> Self {
        Self {
            pool_id,
            max_children,
            next_id: 1,
            children: HashMap::new(),
            budget,
        }
    }

    /// Attempt to spawn a new child subagent. Returns `None` if
    /// `max_children` would be exceeded or the budget is exhausted.
    pub fn spawn(&mut self, label: String) -> Option<SubagentHandle> {
        if self.children.len() as u32 >= self.max_children {
            return None;
        }
        let id = self.next_id;
        self.next_id += 1;
        let handle = SubagentHandle {
            id,
            label,
            status: SubagentStatus::Queued,
            started_at: None,
            finished_at: None,
            result_summary: None,
        };
        self.children.insert(id, handle.clone());
        Some(handle)
    }

    /// Mark a child as started.
    pub fn start(&mut self, id: SubagentId) {
        if let Some(child) = self.children.get_mut(&id) {
            child.status = SubagentStatus::Running;
            child.started_at = Some(Instant::now());
        }
    }

    /// Mark a child as succeeded.
    pub fn succeed(&mut self, id: SubagentId, summary: String) {
        if let Some(child) = self.children.get_mut(&id) {
            child.status = SubagentStatus::Succeeded;
            child.finished_at = Some(Instant::now());
            child.result_summary = Some(summary);
        }
    }

    /// Mark a child as failed.
    pub fn fail(&mut self, id: SubagentId, error: String) {
        if let Some(child) = self.children.get_mut(&id) {
            child.status = SubagentStatus::Failed(error);
            child.finished_at = Some(Instant::now());
        }
    }

    /// Cancel a child (or remove from queue).
    pub fn cancel(&mut self, id: SubagentId) {
        if let Some(child) = self.children.get_mut(&id) {
            child.status = SubagentStatus::Cancelled;
            child.finished_at = Some(Instant::now());
        }
    }

    /// Cancel all running/queued children (cascading cancellation).
    pub fn cancel_all(&mut self) {
        let ids: Vec<SubagentId> = self.children.keys().copied().collect();
        for id in ids {
            self.cancel(id);
        }
    }

    /// Returns the count of active (running) children.
    pub fn active_count(&self) -> usize {
        self.children
            .values()
            .filter(|c| c.status == SubagentStatus::Running)
            .count()
    }

    /// Total children spawned (queued + running + finished).
    pub fn total_count(&self) -> usize {
        self.children.len()
    }

    /// Remaining spawn capacity.
    pub fn remaining_capacity(&self) -> u32 {
        self.max_children.saturating_sub(self.children.len() as u32)
    }

    /// A snapshot of all children, sorted by id.
    pub fn snapshot(&self) -> Vec<&SubagentHandle> {
        let mut v: Vec<&SubagentHandle> = self.children.values().collect();
        v.sort_by_key(|h| h.id);
        v
    }

    pub fn pool_id(&self) -> &str {
        &self.pool_id
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn pool() -> SubagentPool {
        SubagentPool::new("test".into(), 4, Budget::new(Policy::INTERACTIVE))
    }

    #[test]
    fn spawn_returns_handle_with_incrementing_ids() {
        let mut p = pool();
        let a = p.spawn("peer AAPL".into()).unwrap();
        let b = p.spawn("peer MSFT".into()).unwrap();
        assert_eq!(a.id, 1);
        assert_eq!(b.id, 2);
        assert_eq!(a.status, SubagentStatus::Queued);
        assert_eq!(p.total_count(), 2);
    }

    #[test]
    fn spawn_respects_max_children() {
        let mut p = SubagentPool::new("test".into(), 2, Budget::new(Policy::INTERACTIVE));
        assert!(p.spawn("a".into()).is_some());
        assert!(p.spawn("b".into()).is_some());
        assert!(p.spawn("c".into()).is_none()); // capped
    }

    #[test]
    fn lifecycle_queued_running_succeeded() {
        let mut p = pool();
        let h = p.spawn("test".into()).unwrap();
        assert_eq!(h.status, SubagentStatus::Queued);
        p.start(h.id);
        assert_eq!(p.active_count(), 1);
        assert!(p.children.get(&h.id).unwrap().started_at.is_some());
        p.succeed(h.id, "done".into());
        assert_eq!(p.children.get(&h.id).unwrap().status, SubagentStatus::Succeeded);
        assert_eq!(p.active_count(), 0);
    }

    #[test]
    fn fail_sets_status_and_error() {
        let mut p = pool();
        let h = p.spawn("test".into()).unwrap();
        p.start(h.id);
        p.fail(h.id, "API error".into());
        assert_eq!(
            p.children.get(&h.id).unwrap().status,
            SubagentStatus::Failed("API error".into())
        );
    }

    #[test]
    fn cancel_individual_child() {
        let mut p = pool();
        let h = p.spawn("test".into()).unwrap();
        p.start(h.id);
        p.cancel(h.id);
        assert_eq!(p.children.get(&h.id).unwrap().status, SubagentStatus::Cancelled);
    }

    #[test]
    fn cancel_all_queued_and_running_children() {
        let mut p = pool();
        p.spawn("a".into());
        p.spawn("b".into());
        p.start(1);
        p.cancel_all();
        assert_eq!(
            p.children.get(&1).unwrap().status,
            SubagentStatus::Cancelled
        );
        assert_eq!(
            p.children.get(&2).unwrap().status,
            SubagentStatus::Cancelled
        );
    }

    #[test]
    fn remaining_capacity_decreases() {
        let mut p = pool();
        assert_eq!(p.remaining_capacity(), 4);
        p.spawn("a".into());
        p.spawn("b".into());
        assert_eq!(p.remaining_capacity(), 2);
    }

    #[test]
    fn snapshot_sorted_by_id() {
        let mut p = pool();
        p.spawn("b".into()); // id 1
        p.spawn("a".into()); // id 2
        let snap = p.snapshot();
        assert_eq!(snap[0].id, 1);
        assert_eq!(snap[1].id, 2);
        assert_eq!(snap[0].label, "b");
    }

    #[test]
    fn empty_pool_active_count_zero() {
        let p = pool();
        assert_eq!(p.active_count(), 0);
        assert_eq!(p.total_count(), 0);
    }
}
