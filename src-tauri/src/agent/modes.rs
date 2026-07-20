//! Working modes — the user's autonomy dial for a turn (composer mode chip).
//!
//! A mode changes three things and nothing else:
//! 1. the budget [`Policy`] the run starts with (Goal/Loop escalate to the
//!    WORKFLOW guard rails; everything else stays INTERACTIVE),
//! 2. whether the tool belt is filtered to read-only (Plan mode never
//!    builds or overwrites anything), and
//! 3. one system doctrine layer appended after the seed message.
//!
//! Modes NEVER weaken safety: approvals, the conversation spending limit,
//! and verification run identically in every mode.

use fm_agent::budget::Policy;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum AgentMode {
    /// Balanced default: research, tools, and answers as you chat.
    #[default]
    Analyst,
    /// Scope with read-only research, produce a numbered plan, then STOP
    /// and wait for the user's go-ahead.
    Plan,
    /// Outcome-driven autonomy: escalated budgets, minimal check-ins,
    /// verify before declaring done.
    Goal,
    /// Finish, self-critique against the ask, redo — until a pass finds
    /// nothing material.
    Loop,
    /// Red-team the answer: re-derive figures, hunt disconfirmation,
    /// grade confidence honestly.
    Skeptic,
}

impl AgentMode {
    /// Parse the wire value from the composer ("plan", "goal", …).
    /// Unknown/absent → Analyst (never an error — the mode chip is UX,
    /// not a contract the send should fail on).
    pub fn parse(s: Option<&str>) -> Self {
        match s.unwrap_or("").trim().to_ascii_lowercase().as_str() {
            "plan" => AgentMode::Plan,
            "goal" => AgentMode::Goal,
            "loop" => AgentMode::Loop,
            "skeptic" => AgentMode::Skeptic,
            _ => AgentMode::Analyst,
        }
    }

    /// The wire/provenance name.
    pub fn name(&self) -> &'static str {
        match self {
            AgentMode::Analyst => "analyst",
            AgentMode::Plan => "plan",
            AgentMode::Goal => "goal",
            AgentMode::Loop => "loop",
            AgentMode::Skeptic => "skeptic",
        }
    }

    /// Budget guard rails: outcome-driven modes get the WORKFLOW ceiling
    /// (an analyst-day of work); everything else keeps INTERACTIVE.
    pub fn policy(&self) -> Policy {
        match self {
            AgentMode::Goal | AgentMode::Loop => Policy::WORKFLOW,
            _ => Policy::INTERACTIVE,
        }
    }

    /// Plan mode is the only read-only mode: it may research but never
    /// build, overwrite, or export.
    pub fn read_only(&self) -> bool {
        matches!(self, AgentMode::Plan)
    }

    /// The doctrine layer appended to the system seed. Analyst adds none.
    pub fn system_layer(&self) -> Option<&'static str> {
        match self {
            AgentMode::Analyst => None,
            AgentMode::Plan => Some(
                "PLAN MODE. The user wants a plan before any work happens. Use only read-only research (quotes, filings, financials, web) to scope the task, then reply with a numbered plan: each step names the tool or artifact involved and what it will prove or produce, with a rough order and any decision points where you'd need the user's call. Do NOT execute the plan, build artifacts, or draft deliverables. End by asking for the go-ahead in one short line.",
            ),
            AgentMode::Goal => Some(
                "GOAL MODE. The user has given you an outcome, not a task list. Work autonomously toward it: decompose, research, compute, and build without stopping to ask unless you are genuinely blocked or an action needs approval. Before declaring the goal complete, verify it: check the deliverables against the stated outcome and say plainly what is done, what is verified, and what (if anything) remains.",
            ),
            AgentMode::Loop => Some(
                "LOOP MODE. Quality through iteration. After finishing the work, critique your own result against the user's request — are the numbers cited to deterministic tool results, are sources primary, is anything missing or weakly grounded? — then immediately do another pass fixing what the critique found. Repeat until a critique pass finds nothing material. Deliver the final version with one short note listing what each pass improved.",
            ),
            AgentMode::Skeptic => Some(
                "SKEPTIC MODE. Your job is to challenge the thesis, not to please. Independently re-derive the key figures with deterministic tools rather than accepting stated numbers; actively search for disconfirming evidence and credible opposing views; list the assumptions that would have to hold for the conclusion to survive, and which ones look fragile. End with an honest confidence grade and the single most likely way the answer is wrong.",
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_is_lenient_and_defaults_to_analyst() {
        assert_eq!(AgentMode::parse(Some("plan")), AgentMode::Plan);
        assert_eq!(AgentMode::parse(Some(" GOAL ")), AgentMode::Goal);
        assert_eq!(AgentMode::parse(Some("loop")), AgentMode::Loop);
        assert_eq!(AgentMode::parse(Some("skeptic")), AgentMode::Skeptic);
        // The chip is UX, not a contract: junk never fails a send.
        assert_eq!(AgentMode::parse(Some("warp-speed")), AgentMode::Analyst);
        assert_eq!(AgentMode::parse(None), AgentMode::Analyst);
    }

    #[test]
    fn budgets_and_safety_shape() {
        // Outcome modes escalate; conversational modes stay interactive.
        assert_eq!(AgentMode::Goal.policy(), Policy::WORKFLOW);
        assert_eq!(AgentMode::Loop.policy(), Policy::WORKFLOW);
        assert_eq!(AgentMode::Plan.policy(), Policy::INTERACTIVE);
        assert_eq!(AgentMode::Skeptic.policy(), Policy::INTERACTIVE);
        // Plan is the ONLY read-only mode.
        assert!(AgentMode::Plan.read_only());
        assert!(!AgentMode::Goal.read_only());
        assert!(!AgentMode::Loop.read_only());
        // Analyst is the silent default: no doctrine layer.
        assert!(AgentMode::Analyst.system_layer().is_none());
        for m in [AgentMode::Plan, AgentMode::Goal, AgentMode::Loop, AgentMode::Skeptic] {
            assert!(m.system_layer().is_some(), "{m:?} needs a doctrine layer");
        }
    }
}
