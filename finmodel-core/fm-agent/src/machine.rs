//! The pure agent reducer.
//!
//! [`AgentMachine`] owns phase transitions, budget gating, one argument-repair
//! attempt, one verification-repair attempt, cancellation, completion gating,
//! and the terminal reason. It performs no I/O and holds no clock: the Tauri
//! driver executes each emitted [`Action`] against providers/tools/store and
//! feeds typed [`Input`]s back, including clock [`Input::Tick`]s and
//! [`Input::Cancel`].
//!
//! Phase flow:
//! ```text
//! Preparing ──uses_tools&plan──▶ Planning ─▶ Executing ⇄ AwaitingApproval
//!    │                                           │
//!    └── direct (no tools) ─────────────▶ Synthesizing ─▶ Verifying? ─▶ done
//! ```
//! Terminal phases each emit exactly one matching terminal event.

use serde::{Deserialize, Serialize};

use crate::budget::{Budget, Policy};
use crate::types::{
    AgentPhase, ApprovalResponse, BudgetKind, EventKind, Risk, StopReason, ToolCallId,
};

/// A model-requested tool call after the driver has pre-classified it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCall {
    pub tool_call_id: ToolCallId,
    pub name: String,
    pub risk: Risk,
    /// True when risk + confidentiality + path containment require a persisted
    /// approval before execution.
    pub needs_approval: bool,
    /// True when the driver validated the arguments against the tool schema.
    pub args_valid: bool,
}

/// A typed result the driver feeds back into the reducer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Input {
    /// `Preparing` finished: context recalled and tool policy selected.
    Prepared {
        /// The turn will use tools (research/finance/artifact), not a direct answer.
        uses_tools: bool,
        /// A concise user-visible plan is warranted (multi-step).
        plan_needed: bool,
        /// The final answer must pass claim/schema/artifact verification.
        needs_verification: bool,
    },
    /// The user-visible plan was created/updated.
    PlanReady,
    /// A provider request returned. `calls` are the tool calls it requested (may
    /// be empty); `final_answer` is true when it produced a terminal answer with
    /// no further tool calls.
    ModelResponded {
        calls: Vec<ToolCall>,
        final_answer: bool,
        tokens: u64,
    },
    /// A scheduled tool batch finished; all results are persisted.
    ToolsCompleted { tokens: u64 },
    /// A parked approval was resolved.
    ApprovalResolved { response: ApprovalResponse },
    /// The final narrative was produced.
    Synthesized,
    /// Verification finished. `ok=false` triggers one repair, then a partial.
    Verified { ok: bool },
    /// Bounded memory extraction finished (success, timeout, or skip).
    MemoryDone,
    /// A research/artifact-class tool was accepted; escalate the run policy.
    WorkflowAccepted { policy: Policy },
    /// The user or a parent cancelled the run. Terminal and NOT resumable.
    Cancel,
    /// The user or a parent paused the run. Terminal for THIS run but resumable
    /// via a new linked run (`RunInterrupted`), distinct from `Cancel`.
    Interrupt,
    /// Clock update from the driver (ms since run start). May trip the deadline.
    Tick { elapsed_ms: u64 },
}

/// An action the reducer asks the driver to perform.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Action {
    /// Recall scoped memory, enforce confidentiality, and select workflow/tools.
    Prepare,
    /// Create or update the concise user-visible plan.
    MakePlan,
    /// Issue one provider request.
    RequestModel,
    /// Ask the model to repair exactly one malformed tool call.
    RepairToolCall { tool_call_id: ToolCallId },
    /// Execute this batch of tool calls (independent read-only calls together).
    ScheduleTools { batch: Vec<ToolCallId> },
    /// Park the run pending approval of one write/export/delete call.
    RequestApproval { tool_call_id: ToolCallId },
    /// Produce the final answer/artifact narrative from the tool/source ledger.
    Synthesize,
    /// Validate claims, output schemas, and artifact checks.
    Verify,
    /// Run bounded automatic memory extraction (separate auxiliary budget).
    ExtractMemory,
    /// Finalize the run with exactly one terminal event and stop reason.
    Emit {
        event: EventKind,
        stop: StopReason,
        /// The visible answer is partial (budget/unverified), not a clean finish.
        partial: bool,
    },
    /// Nothing to do until the next external input.
    Wait,
}

/// The pure agent reducer for one run.
#[derive(Clone, Debug)]
pub struct AgentMachine {
    phase: AgentPhase,
    budget: Budget,
    needs_verification: bool,
    /// Pending calls awaiting approval, oldest first.
    approval_queue: Vec<ToolCall>,
    /// Read-only/auto-run calls ready to execute now.
    ready_batch: Vec<ToolCallId>,
    /// At most one malformed-argument repair per run.
    arg_repair_used: bool,
    /// At most one verification repair per run.
    verify_repair_used: bool,
    /// Set when a rounds/tokens budget trip was converted into one final
    /// no-tools synthesis pass (the "wrap-up round"): the run still terminates
    /// as budget-limited/partial, but with an answer built from the evidence
    /// already gathered instead of dying mid-task.
    budget_grace: Option<BudgetKind>,
    cancel_requested: bool,
}

impl AgentMachine {
    /// Start a run in `Preparing` with the given policy.
    pub fn new(policy: Policy) -> Self {
        AgentMachine {
            phase: AgentPhase::Preparing,
            budget: Budget::new(policy),
            needs_verification: false,
            approval_queue: Vec::new(),
            ready_batch: Vec::new(),
            arg_repair_used: false,
            verify_repair_used: false,
            budget_grace: None,
            cancel_requested: false,
        }
    }

    pub fn phase(&self) -> AgentPhase {
        self.phase
    }

    pub fn budget(&self) -> Budget {
        self.budget
    }

    /// True while the run is in the budget-grace wrap-up pass: a runaway guard
    /// tripped and the reducer granted one final no-tools synthesis. The driver
    /// uses this to make a real wrap-up model call so the run ends with an
    /// answer built from the gathered evidence, never a dead end.
    pub fn in_budget_grace(&self) -> bool {
        self.budget_grace.is_some()
    }

    /// The reducer's first move: recall context and select the tool policy.
    pub fn start(&self) -> Action {
        Action::Prepare
    }

    /// Advance the machine. Returns the next [`Action`] for the driver.
    pub fn next(&mut self, input: Input) -> Action {
        // Terminal is absorbing.
        if self.phase.is_terminal() {
            return Action::Wait;
        }

        // Cross-cutting inputs handled first.
        match &input {
            Input::Cancel => {
                self.cancel_requested = true;
                return self.terminate(
                    AgentPhase::Cancelled,
                    EventKind::RunCancelled,
                    StopReason::Cancelled,
                    true,
                );
            }
            Input::Interrupt => {
                // Resumable: terminal for this run, distinct from Cancel; a new
                // linked run resumes from the last safe checkpoint.
                return self.terminate(
                    AgentPhase::Interrupted,
                    EventKind::RunInterrupted,
                    StopReason::Interrupted,
                    true,
                );
            }
            Input::Tick { elapsed_ms } => {
                self.budget.set_elapsed(*elapsed_ms);
                // A parked approval does not consume the run deadline.
                if self.phase != AgentPhase::AwaitingApproval {
                    if let Some(kind) = self.budget.exhausted() {
                        return self.budget_stop(kind);
                    }
                }
                return Action::Wait;
            }
            Input::WorkflowAccepted { policy } => {
                self.budget.escalate(*policy);
                return Action::Wait;
            }
            _ => {}
        }

        match (self.phase, input) {
            // ---- Preparing ----
            (
                AgentPhase::Preparing,
                Input::Prepared {
                    uses_tools,
                    plan_needed,
                    needs_verification,
                },
            ) => {
                self.needs_verification = needs_verification;
                if uses_tools {
                    if plan_needed {
                        self.phase = AgentPhase::Planning;
                        Action::MakePlan
                    } else {
                        self.enter_executing()
                    }
                } else {
                    // Direct, non-financial answer: skip tools and verification.
                    self.phase = AgentPhase::Synthesizing;
                    Action::Synthesize
                }
            }

            // ---- Planning ----
            (AgentPhase::Planning, Input::PlanReady) => self.enter_executing(),

            // ---- Executing ----
            (
                AgentPhase::Executing,
                Input::ModelResponded {
                    calls,
                    final_answer,
                    tokens,
                },
            ) => {
                self.budget.charge_round(tokens);
                if let Some(kind) = self.budget.exhausted() {
                    return self.budget_stop(kind);
                }
                if final_answer && calls.is_empty() {
                    return self.enter_synthesizing();
                }
                self.classify_calls(calls)
            }
            (AgentPhase::Executing, Input::ToolsCompleted { tokens }) => {
                self.budget.charge_round(tokens);
                if let Some(kind) = self.budget.exhausted() {
                    return self.budget_stop(kind);
                }
                // If approvals are still queued, request the next one.
                if !self.approval_queue.is_empty() {
                    self.phase = AgentPhase::AwaitingApproval;
                    let id = self.approval_queue[0].tool_call_id.clone();
                    return Action::RequestApproval { tool_call_id: id };
                }
                // Otherwise let the model consume the results.
                Action::RequestModel
            }

            // ---- AwaitingApproval ----
            (AgentPhase::AwaitingApproval, Input::ApprovalResolved { response }) => {
                let call = self.approval_queue.remove(0);
                self.phase = AgentPhase::Executing;
                match response {
                    ApprovalResponse::ApproveOnce | ApprovalResponse::CreateNewVersion => {
                        Action::ScheduleTools {
                            batch: vec![call.tool_call_id],
                        }
                    }
                    ApprovalResponse::Deny => {
                        // Denied: do not execute. Continue with any other queued
                        // approvals, else return to the model with the denial.
                        if !self.approval_queue.is_empty() {
                            self.phase = AgentPhase::AwaitingApproval;
                            let id = self.approval_queue[0].tool_call_id.clone();
                            Action::RequestApproval { tool_call_id: id }
                        } else if !self.ready_batch.is_empty() {
                            let batch = std::mem::take(&mut self.ready_batch);
                            Action::ScheduleTools { batch }
                        } else {
                            Action::RequestModel
                        }
                    }
                }
            }

            // ---- Synthesizing ----
            (AgentPhase::Synthesizing, Input::Synthesized) => {
                if self.budget_grace.is_some() {
                    // Wrap-up round: the budget is spent — no verification pass,
                    // straight to memory capture then the budget-limited terminal.
                    self.phase = AgentPhase::Executing; // transient; extract then terminate
                    Action::ExtractMemory
                } else if self.needs_verification {
                    self.phase = AgentPhase::Verifying;
                    Action::Verify
                } else {
                    self.phase = AgentPhase::Executing; // transient; extract then complete
                    Action::ExtractMemory
                }
            }

            // ---- Verifying ----
            (AgentPhase::Verifying, Input::Verified { ok }) => {
                if ok {
                    Action::ExtractMemory
                } else if !self.verify_repair_used {
                    // One repair: re-synthesize from the ledger.
                    self.verify_repair_used = true;
                    self.phase = AgentPhase::Synthesizing;
                    Action::Synthesize
                } else {
                    // Second failure: complete with a clearly marked partial.
                    Action::ExtractMemory
                }
            }

            // ---- Memory then terminal ----
            (_, Input::MemoryDone) => {
                // A grace-synthesized run terminates as budget-limited/partial:
                // the answer exists, but the run did not finish cleanly.
                if let Some(kind) = self.budget_grace {
                    return self.terminate(
                        AgentPhase::BudgetLimited,
                        EventKind::RunBudgetLimited,
                        StopReason::Budget(kind),
                        true,
                    );
                }
                let partial = self.verify_repair_used && self.needs_verification;
                let stop = if partial {
                    StopReason::UnverifiedClaim
                } else {
                    StopReason::EndTurn
                };
                self.terminate(
                    AgentPhase::Completed,
                    EventKind::RunCompleted,
                    stop,
                    partial,
                )
            }

            // Any other pairing is a driver/reducer contract violation.
            (phase, other) => self.terminate(
                AgentPhase::Failed,
                EventKind::RunFailed,
                StopReason::Error(format!("unexpected input {other:?} in phase {phase:?}")),
                false,
            ),
        }
    }

    fn enter_executing(&mut self) -> Action {
        self.phase = AgentPhase::Executing;
        if let Some(kind) = self.budget.exhausted() {
            return self.budget_stop(kind);
        }
        Action::RequestModel
    }

    fn enter_synthesizing(&mut self) -> Action {
        self.phase = AgentPhase::Synthesizing;
        Action::Synthesize
    }

    /// Split model tool calls into: one repair (if a malformed call remains and
    /// no repair was spent), approval-gated calls (parked), and a ready batch.
    fn classify_calls(&mut self, calls: Vec<ToolCall>) -> Action {
        // Malformed calls: at most one repair per run, then they are dropped.
        if let Some(bad) = calls.iter().find(|c| !c.args_valid) {
            if !self.arg_repair_used {
                self.arg_repair_used = true;
                return Action::RepairToolCall {
                    tool_call_id: bad.tool_call_id.clone(),
                };
            }
        }
        let valid: Vec<ToolCall> = calls.into_iter().filter(|c| c.args_valid).collect();
        let (needs_approval, auto): (Vec<ToolCall>, Vec<ToolCall>) = valid
            .into_iter()
            .partition(|c| c.needs_approval || !c.risk.auto_runs());

        self.approval_queue = needs_approval;
        self.ready_batch = auto.iter().map(|c| c.tool_call_id.clone()).collect();

        if !self.ready_batch.is_empty() {
            // Execute auto-run calls first; approvals are requested when the
            // batch completes.
            let batch = std::mem::take(&mut self.ready_batch);
            Action::ScheduleTools { batch }
        } else if !self.approval_queue.is_empty() {
            self.phase = AgentPhase::AwaitingApproval;
            let id = self.approval_queue[0].tool_call_id.clone();
            Action::RequestApproval { tool_call_id: id }
        } else {
            // No executable calls (all dropped after repair): ask the model again.
            Action::RequestModel
        }
    }

    fn budget_stop(&mut self, kind: BudgetKind) -> Action {
        // Rounds/tokens exhaustion earns one wrap-up synthesis from the
        // evidence already gathered — an answer, not a dead end. The deadline
        // is wall-clock-real and always hard-stops, and grace fires once.
        if kind != BudgetKind::Deadline && self.budget_grace.is_none() {
            self.budget_grace = Some(kind);
            self.phase = AgentPhase::Synthesizing;
            return Action::Synthesize;
        }
        self.terminate(
            AgentPhase::BudgetLimited,
            EventKind::RunBudgetLimited,
            StopReason::Budget(kind),
            true,
        )
    }

    fn terminate(
        &mut self,
        phase: AgentPhase,
        event: EventKind,
        stop: StopReason,
        partial: bool,
    ) -> Action {
        self.phase = phase;
        Action::Emit {
            event,
            stop,
            partial,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Risk;

    fn ro_call(id: &str) -> ToolCall {
        ToolCall {
            tool_call_id: id.into(),
            name: "get_quote".into(),
            risk: Risk::ReadOnly,
            needs_approval: false,
            args_valid: true,
        }
    }

    #[test]
    fn direct_answer_skips_tools_and_verification() {
        let mut m = AgentMachine::new(Policy::INTERACTIVE);
        assert_eq!(m.start(), Action::Prepare);
        let a = m.next(Input::Prepared {
            uses_tools: false,
            plan_needed: false,
            needs_verification: false,
        });
        assert_eq!(a, Action::Synthesize);
        assert_eq!(m.phase(), AgentPhase::Synthesizing);
        assert_eq!(m.next(Input::Synthesized), Action::ExtractMemory);
        let end = m.next(Input::MemoryDone);
        assert_eq!(
            end,
            Action::Emit {
                event: EventKind::RunCompleted,
                stop: StopReason::EndTurn,
                partial: false
            }
        );
        assert_eq!(m.phase(), AgentPhase::Completed);
    }

    #[test]
    fn tool_turn_runs_batch_then_synthesizes_then_verifies() {
        let mut m = AgentMachine::new(Policy::INTERACTIVE);
        m.next(Input::Prepared {
            uses_tools: true,
            plan_needed: false,
            needs_verification: true,
        });
        assert_eq!(m.phase(), AgentPhase::Executing);
        // Model asks for two independent read-only calls.
        let a = m.next(Input::ModelResponded {
            calls: vec![ro_call("a"), ro_call("b")],
            final_answer: false,
            tokens: 100,
        });
        assert_eq!(
            a,
            Action::ScheduleTools {
                batch: vec!["a".into(), "b".into()]
            }
        );
        // Tools done -> model consumes results.
        assert_eq!(
            m.next(Input::ToolsCompleted { tokens: 50 }),
            Action::RequestModel
        );
        // Model produces final answer.
        assert_eq!(
            m.next(Input::ModelResponded {
                calls: vec![],
                final_answer: true,
                tokens: 80
            }),
            Action::Synthesize
        );
        assert_eq!(m.next(Input::Synthesized), Action::Verify);
        assert_eq!(m.next(Input::Verified { ok: true }), Action::ExtractMemory);
        let end = m.next(Input::MemoryDone);
        assert_eq!(
            end,
            Action::Emit {
                event: EventKind::RunCompleted,
                stop: StopReason::EndTurn,
                partial: false
            }
        );
    }

    #[test]
    fn plan_needed_inserts_planning() {
        let mut m = AgentMachine::new(Policy::INTERACTIVE);
        let a = m.next(Input::Prepared {
            uses_tools: true,
            plan_needed: true,
            needs_verification: false,
        });
        assert_eq!(a, Action::MakePlan);
        assert_eq!(m.phase(), AgentPhase::Planning);
        assert_eq!(m.next(Input::PlanReady), Action::RequestModel);
        assert_eq!(m.phase(), AgentPhase::Executing);
    }

    #[test]
    fn one_arg_repair_then_drop() {
        let mut m = AgentMachine::new(Policy::INTERACTIVE);
        m.next(Input::Prepared {
            uses_tools: true,
            plan_needed: false,
            needs_verification: false,
        });
        let bad = ToolCall {
            tool_call_id: "x".into(),
            name: "build_model".into(),
            risk: Risk::LocalCreate,
            needs_approval: false,
            args_valid: false,
        };
        // First malformed call -> one repair.
        assert_eq!(
            m.next(Input::ModelResponded {
                calls: vec![bad.clone()],
                final_answer: false,
                tokens: 10
            }),
            Action::RepairToolCall {
                tool_call_id: "x".into()
            }
        );
        // Still malformed after repair -> dropped, ask model again.
        assert_eq!(
            m.next(Input::ModelResponded {
                calls: vec![bad],
                final_answer: false,
                tokens: 10
            }),
            Action::RequestModel
        );
    }

    #[test]
    fn write_call_parks_for_approval_then_executes_once() {
        let mut m = AgentMachine::new(Policy::INTERACTIVE);
        m.next(Input::Prepared {
            uses_tools: true,
            plan_needed: false,
            needs_verification: false,
        });
        let write = ToolCall {
            tool_call_id: "w".into(),
            name: "export_excel".into(),
            risk: Risk::LocalOverwrite,
            needs_approval: true,
            args_valid: true,
        };
        let a = m.next(Input::ModelResponded {
            calls: vec![write],
            final_answer: false,
            tokens: 10,
        });
        assert_eq!(
            a,
            Action::RequestApproval {
                tool_call_id: "w".into()
            }
        );
        assert_eq!(m.phase(), AgentPhase::AwaitingApproval);
        let a = m.next(Input::ApprovalResolved {
            response: ApprovalResponse::ApproveOnce,
        });
        assert_eq!(
            a,
            Action::ScheduleTools {
                batch: vec!["w".into()]
            }
        );
        assert_eq!(m.phase(), AgentPhase::Executing);
    }

    #[test]
    fn denied_write_does_not_execute() {
        let mut m = AgentMachine::new(Policy::INTERACTIVE);
        m.next(Input::Prepared {
            uses_tools: true,
            plan_needed: false,
            needs_verification: false,
        });
        let write = ToolCall {
            tool_call_id: "w".into(),
            name: "delete_artifact".into(),
            risk: Risk::LocalDelete,
            needs_approval: true,
            args_valid: true,
        };
        m.next(Input::ModelResponded {
            calls: vec![write],
            final_answer: false,
            tokens: 10,
        });
        let a = m.next(Input::ApprovalResolved {
            response: ApprovalResponse::Deny,
        });
        assert_eq!(a, Action::RequestModel);
        assert_eq!(m.phase(), AgentPhase::Executing);
    }

    #[test]
    fn read_batch_before_parked_write() {
        let mut m = AgentMachine::new(Policy::INTERACTIVE);
        m.next(Input::Prepared {
            uses_tools: true,
            plan_needed: false,
            needs_verification: false,
        });
        let write = ToolCall {
            tool_call_id: "w".into(),
            name: "export_excel".into(),
            risk: Risk::Export,
            needs_approval: true,
            args_valid: true,
        };
        // One read + one write: read batch runs first.
        let a = m.next(Input::ModelResponded {
            calls: vec![ro_call("r"), write],
            final_answer: false,
            tokens: 10,
        });
        assert_eq!(
            a,
            Action::ScheduleTools {
                batch: vec!["r".into()]
            }
        );
        // After the read batch, the parked write requests approval.
        let a = m.next(Input::ToolsCompleted { tokens: 5 });
        assert_eq!(
            a,
            Action::RequestApproval {
                tool_call_id: "w".into()
            }
        );
    }

    #[test]
    fn verification_one_repair_then_partial() {
        let mut m = AgentMachine::new(Policy::INTERACTIVE);
        m.next(Input::Prepared {
            uses_tools: true,
            plan_needed: false,
            needs_verification: true,
        });
        m.next(Input::ModelResponded {
            calls: vec![],
            final_answer: true,
            tokens: 10,
        });
        m.next(Input::Synthesized);
        // First verify fails -> re-synthesize.
        assert_eq!(m.next(Input::Verified { ok: false }), Action::Synthesize);
        assert_eq!(m.phase(), AgentPhase::Synthesizing);
        m.next(Input::Synthesized);
        // Second verify fails -> partial completion.
        assert_eq!(m.next(Input::Verified { ok: false }), Action::ExtractMemory);
        let end = m.next(Input::MemoryDone);
        assert_eq!(
            end,
            Action::Emit {
                event: EventKind::RunCompleted,
                stop: StopReason::UnverifiedClaim,
                partial: true
            }
        );
    }

    #[test]
    fn cancel_is_terminal_and_absorbing() {
        let mut m = AgentMachine::new(Policy::INTERACTIVE);
        m.next(Input::Prepared {
            uses_tools: true,
            plan_needed: false,
            needs_verification: false,
        });
        let a = m.next(Input::Cancel);
        assert_eq!(
            a,
            Action::Emit {
                event: EventKind::RunCancelled,
                stop: StopReason::Cancelled,
                partial: true
            }
        );
        assert_eq!(m.phase(), AgentPhase::Cancelled);
        // Absorbing.
        assert_eq!(m.next(Input::MemoryDone), Action::Wait);
    }

    #[test]
    fn interrupt_is_terminal_resumable_and_distinct_from_cancel() {
        let mut m = AgentMachine::new(Policy::INTERACTIVE);
        m.next(Input::Prepared {
            uses_tools: true,
            plan_needed: false,
            needs_verification: false,
        });
        let a = m.next(Input::Interrupt);
        // Interrupt terminates THIS run as RunInterrupted (resumable), NOT
        // RunCancelled: the terminal event/phase/reason are all distinct.
        assert_eq!(
            a,
            Action::Emit {
                event: EventKind::RunInterrupted,
                stop: StopReason::Interrupted,
                partial: true
            }
        );
        assert_eq!(m.phase(), AgentPhase::Interrupted);
        assert!(m.phase().is_terminal());
        // Absorbing after terminal.
        assert_eq!(m.next(Input::MemoryDone), Action::Wait);
    }

    #[test]
    fn interrupt_and_cancel_produce_different_terminals() {
        let mut c = AgentMachine::new(Policy::INTERACTIVE);
        c.next(Input::Prepared {
            uses_tools: true,
            plan_needed: false,
            needs_verification: false,
        });
        let cancel = c.next(Input::Cancel);
        let mut i = AgentMachine::new(Policy::INTERACTIVE);
        i.next(Input::Prepared {
            uses_tools: true,
            plan_needed: false,
            needs_verification: false,
        });
        let interrupt = i.next(Input::Interrupt);
        assert_ne!(cancel, interrupt);
        assert_eq!(c.phase(), AgentPhase::Cancelled);
        assert_eq!(i.phase(), AgentPhase::Interrupted);
    }

    #[test]
    fn deadline_tick_trips_budget() {
        let mut m = AgentMachine::new(Policy::INTERACTIVE);
        m.next(Input::Prepared {
            uses_tools: true,
            plan_needed: false,
            needs_verification: false,
        });
        let a = m.next(Input::Tick {
            elapsed_ms: Policy::INTERACTIVE.deadline_ms + 1,
        });
        assert_eq!(
            a,
            Action::Emit {
                event: EventKind::RunBudgetLimited,
                stop: StopReason::Budget(BudgetKind::Deadline),
                partial: true
            }
        );
    }

    #[test]
    fn approval_park_does_not_trip_deadline() {
        let mut m = AgentMachine::new(Policy::INTERACTIVE);
        m.next(Input::Prepared {
            uses_tools: true,
            plan_needed: false,
            needs_verification: false,
        });
        let write = ToolCall {
            tool_call_id: "w".into(),
            name: "export_excel".into(),
            risk: Risk::Export,
            needs_approval: true,
            args_valid: true,
        };
        m.next(Input::ModelResponded {
            calls: vec![write],
            final_answer: false,
            tokens: 10,
        });
        assert_eq!(m.phase(), AgentPhase::AwaitingApproval);
        // Deadline passes while parked: no budget stop.
        let a = m.next(Input::Tick {
            elapsed_ms: Policy::INTERACTIVE.deadline_ms + 10_000,
        });
        assert_eq!(a, Action::Wait);
        assert_eq!(m.phase(), AgentPhase::AwaitingApproval);
    }

    #[test]
    fn rounds_budget_stops_execution() {
        let mut m = AgentMachine::new(Policy::INTERACTIVE);
        m.next(Input::Prepared {
            uses_tools: true,
            plan_needed: false,
            needs_verification: false,
        });
        // Exhaust rounds via repeated model/tool cycles; the trip may land on
        // either the model round or the tool round.
        let mut last = Action::Wait;
        for _ in 0..Policy::INTERACTIVE.max_rounds + 1 {
            last = m.next(Input::ModelResponded {
                calls: vec![ro_call("a")],
                final_answer: false,
                tokens: 1,
            });
            if matches!(last, Action::Synthesize) {
                break;
            }
            last = m.next(Input::ToolsCompleted { tokens: 1 });
            if matches!(last, Action::Synthesize) {
                break;
            }
        }
        // Rounds exhaustion earns one wrap-up synthesis (an answer from the
        // gathered evidence), then the run still terminates budget-limited.
        assert_eq!(last, Action::Synthesize);
        assert_eq!(m.phase(), AgentPhase::Synthesizing);
        assert_eq!(m.next(Input::Synthesized), Action::ExtractMemory);
        assert_eq!(
            m.next(Input::MemoryDone),
            Action::Emit {
                event: EventKind::RunBudgetLimited,
                stop: StopReason::Budget(BudgetKind::Rounds),
                partial: true
            }
        );
        assert_eq!(m.phase(), AgentPhase::BudgetLimited);
    }

    #[test]
    fn deadline_still_hard_stops_and_grace_fires_once() {
        // Deadline: no grace, immediate budget terminal (wall clock is real).
        let mut m = AgentMachine::new(Policy::INTERACTIVE);
        m.next(Input::Prepared {
            uses_tools: true,
            plan_needed: false,
            needs_verification: false,
        });
        let a = m.next(Input::Tick {
            elapsed_ms: Policy::INTERACTIVE.deadline_ms + 1,
        });
        assert!(matches!(
            a,
            Action::Emit {
                event: EventKind::RunBudgetLimited,
                ..
            }
        ));
        // Grace fires at most once: a second budget trip terminates directly.
        let mut g = AgentMachine::new(Policy::INTERACTIVE);
        g.next(Input::Prepared {
            uses_tools: true,
            plan_needed: false,
            needs_verification: false,
        });
        g.budget_grace = Some(BudgetKind::Rounds);
        assert!(matches!(
            g.budget_stop(BudgetKind::Rounds),
            Action::Emit {
                event: EventKind::RunBudgetLimited,
                ..
            }
        ));
    }

    #[test]
    fn workflow_acceptance_raises_rounds_ceiling() {
        let mut m = AgentMachine::new(Policy::INTERACTIVE);
        m.next(Input::Prepared {
            uses_tools: true,
            plan_needed: false,
            needs_verification: false,
        });
        m.next(Input::WorkflowAccepted {
            policy: Policy::WORKFLOW,
        });
        assert_eq!(m.budget().policy.max_rounds, Policy::WORKFLOW.max_rounds);
    }
}
