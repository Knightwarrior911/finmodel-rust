//! The unified single-owner agent loop (Phase B+).
//!
//! A lazily spawned conversation actor owns one conversation's transcript,
//! active run, pending approval, streaming accumulator, and cancellation tree,
//! driving the pure [`fm_agent::AgentMachine`] reducer. The driver performs
//! provider/tool/store I/O (Phase C). Durable transitions persist before
//! broadcast; ephemeral deltas stream directly.
//!
//! Modules:
//! - [`events`] — the single IPC event envelope + sink.
//! - [`context`] — selected-branch context assembly and rolling compaction.
//! - [`registry`] — active-run authority and bounded concurrency.

pub mod actor;
pub mod context;
pub mod events;
pub mod registry;
pub mod security;
pub mod scheduler;
pub mod tools;
pub mod fallback;
pub mod workflows;
pub mod subagents;
pub mod memory;
pub mod provider;
