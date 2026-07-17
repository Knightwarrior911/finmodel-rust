//! Run budgets and policy. The reducer consults [`Budget`] to decide when a run
//! must stop; the driver charges usage as model/tool rounds complete.
//!
//! Default interactive policy: 8 model/tool rounds, 32k cumulative turn tokens,
//! 120 s wall clock. A [`WorkflowSpec`](crate) may raise these for research or
//! artifact work; accepting such a tool escalates the current run to that policy
//! *from the original start time*.

use serde::{Deserialize, Serialize};

use crate::types::BudgetKind;

/// Immutable per-run limits.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Policy {
    /// Maximum combined model/tool rounds.
    pub max_rounds: u32,
    /// Cumulative token ceiling for the turn (input+output across rounds).
    pub max_tokens: u64,
    /// Overall wall-clock deadline in milliseconds from run start.
    pub deadline_ms: u64,
    /// Maximum read-only child tasks a parent may queue in this run.
    pub max_children: u32,
}

impl Policy {
    /// The default interactive policy (decision: 8 rounds / 32k tokens / 120 s).
    pub const INTERACTIVE: Policy = Policy {
        max_rounds: 8,
        max_tokens: 32_000,
        deadline_ms: 120_000,
        max_children: 0,
    };

    /// The research/artifact workflow policy (12 rounds / 30-minute deadline).
    /// Token ceiling is raised but still clamped by the driver to the selected
    /// model's real context/cost limits.
    pub const WORKFLOW: Policy = Policy {
        max_rounds: 12,
        max_tokens: 200_000,
        deadline_ms: 30 * 60_000,
        max_children: 12,
    };

    /// Escalate to `other` while preserving the stricter side of any dimension
    /// that `other` does not raise. Used when a run accepts a workflow-class
    /// tool: rounds/deadline/tokens/children grow, never shrink.
    pub fn escalate_to(self, other: Policy) -> Policy {
        Policy {
            max_rounds: self.max_rounds.max(other.max_rounds),
            max_tokens: self.max_tokens.max(other.max_tokens),
            deadline_ms: self.deadline_ms.max(other.deadline_ms),
            max_children: self.max_children.max(other.max_children),
        }
    }

    /// Clamp the token ceiling to a model's real allowance.
    pub fn clamp_tokens(mut self, model_max: u64) -> Policy {
        if model_max > 0 {
            self.max_tokens = self.max_tokens.min(model_max);
        }
        self
    }
}

impl Default for Policy {
    fn default() -> Self {
        Policy::INTERACTIVE
    }
}

/// Mutable accounting against a [`Policy`]. `elapsed_ms` is fed from the driver's
/// clock as an input, keeping the reducer free of any clock of its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Budget {
    pub policy: Policy,
    pub rounds_used: u32,
    pub tokens_used: u64,
    pub elapsed_ms: u64,
}

impl Budget {
    pub fn new(policy: Policy) -> Self {
        Budget {
            policy,
            rounds_used: 0,
            tokens_used: 0,
            elapsed_ms: 0,
        }
    }

    /// Record one completed model/tool round and its token cost.
    pub fn charge_round(&mut self, tokens: u64) {
        self.rounds_used = self.rounds_used.saturating_add(1);
        self.tokens_used = self.tokens_used.saturating_add(tokens);
    }

    /// Update the wall-clock reading (monotonic; never rewinds).
    pub fn set_elapsed(&mut self, elapsed_ms: u64) {
        self.elapsed_ms = self.elapsed_ms.max(elapsed_ms);
    }

    /// Escalate this run's policy (workflow acceptance). Usage is preserved.
    pub fn escalate(&mut self, other: Policy) {
        self.policy = self.policy.escalate_to(other);
    }

    /// Which budget dimension (if any) is exhausted *right now*. Rounds are
    /// checked before tokens before the deadline so the reported reason is
    /// deterministic.
    pub fn exhausted(&self) -> Option<BudgetKind> {
        if self.rounds_used >= self.policy.max_rounds {
            Some(BudgetKind::Rounds)
        } else if self.tokens_used >= self.policy.max_tokens {
            Some(BudgetKind::Tokens)
        } else if self.elapsed_ms >= self.policy.deadline_ms {
            Some(BudgetKind::Deadline)
        } else {
            None
        }
    }

    /// Whether another model/tool round may begin.
    pub fn can_continue(&self) -> bool {
        self.exhausted().is_none()
    }

    /// Remaining child-task slots for this run.
    pub fn remaining_children(&self, in_flight_and_done: u32) -> u32 {
        self.policy.max_children.saturating_sub(in_flight_and_done)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_interactive_budget_can_continue() {
        let b = Budget::new(Policy::INTERACTIVE);
        assert!(b.can_continue());
        assert_eq!(b.exhausted(), None);
    }

    #[test]
    fn rounds_exhaust_first() {
        let mut b = Budget::new(Policy::INTERACTIVE);
        for _ in 0..Policy::INTERACTIVE.max_rounds {
            b.charge_round(10);
        }
        assert_eq!(b.exhausted(), Some(BudgetKind::Rounds));
        assert!(!b.can_continue());
    }

    #[test]
    fn tokens_exhaust_when_rounds_remain() {
        let mut b = Budget::new(Policy::INTERACTIVE);
        b.charge_round(Policy::INTERACTIVE.max_tokens);
        // one round used, but tokens are at the ceiling
        assert_eq!(b.exhausted(), Some(BudgetKind::Tokens));
    }

    #[test]
    fn deadline_exhausts_last() {
        let mut b = Budget::new(Policy::INTERACTIVE);
        b.charge_round(10);
        b.set_elapsed(Policy::INTERACTIVE.deadline_ms + 1);
        assert_eq!(b.exhausted(), Some(BudgetKind::Deadline));
    }

    #[test]
    fn elapsed_never_rewinds() {
        let mut b = Budget::new(Policy::INTERACTIVE);
        b.set_elapsed(5_000);
        b.set_elapsed(1_000);
        assert_eq!(b.elapsed_ms, 5_000);
    }

    #[test]
    fn escalation_grows_all_dimensions() {
        let mut b = Budget::new(Policy::INTERACTIVE);
        for _ in 0..8 {
            b.charge_round(100);
        }
        assert_eq!(b.exhausted(), Some(BudgetKind::Rounds));
        b.escalate(Policy::WORKFLOW);
        // rounds ceiling rose to 12, usage (8) preserved -> can continue again
        assert!(b.can_continue());
        assert_eq!(b.policy.max_rounds, 12);
        assert_eq!(b.rounds_used, 8);
    }

    #[test]
    fn clamp_tokens_takes_the_min() {
        let p = Policy::WORKFLOW.clamp_tokens(50_000);
        assert_eq!(p.max_tokens, 50_000);
        let p2 = Policy::INTERACTIVE.clamp_tokens(1_000_000);
        assert_eq!(p2.max_tokens, 32_000);
    }

    #[test]
    fn remaining_children_saturates() {
        let b = Budget::new(Policy::WORKFLOW);
        assert_eq!(b.remaining_children(0), 12);
        assert_eq!(b.remaining_children(12), 0);
        assert_eq!(b.remaining_children(20), 0);
    }
}
