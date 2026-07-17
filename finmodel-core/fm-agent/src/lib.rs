//! `fm-agent` — the pure agent-loop reducer for the finmodel virtual analyst.
//!
//! Following `fm-research`'s reducer/driver split, this crate is runtime-agnostic
//! and has no async, I/O, or clock of its own. [`machine::AgentMachine`] emits
//! typed [`machine::Action`]s and consumes typed [`machine::Input`]s; the Tauri
//! driver (`src-tauri/src/agent`) performs all provider/tool/store I/O and feeds
//! results back, including clock ticks and cancellation.
//!
//! Modules:
//! - [`types`] — identifiers, phases, terminal reasons, message parts, tool risk,
//!   the tool-result envelope, and the numeric claim record.
//! - [`budget`] — per-run policy and usage accounting.
//! - [`ids`] — pure UUID-v4 formatting.
//! - [`machine`] — the phase reducer.

pub mod budget;
pub mod ids;
pub mod machine;
pub mod workflows;
pub mod types;

pub use budget::{Budget, Policy};
pub use machine::{Action, AgentMachine, Input, ToolCall};
pub use types::{
    AgentPhase, ApprovalResponse, ArtifactRef, BudgetKind, Claim, Confidentiality, Durability,
    EventKind, PartKind, Risk, SourceRef, StopReason, ToolResultEnvelope, Trust,
};
