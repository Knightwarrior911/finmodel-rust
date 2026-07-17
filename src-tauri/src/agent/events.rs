//! The single IPC event envelope replacing the old special event names.
//!
//! Every agent event — durable or ephemeral — is wrapped in one
//! [`AgentEventEnvelope`]. Durable events carry a monotonic per-run `sequence`
//! and are authoritative for replay; ephemeral events (text deltas, progress)
//! carry an `event_id` but no sequence and never determine terminal state.
//!
//! The client reduces idempotently by `event_id` and, for durable events, by
//! `sequence`, so a persist-then-broadcast durable event is authoritative and a
//! missing ephemeral delta can never strand the UI.

use serde::{Deserialize, Serialize};

use fm_agent::types::{Durability, EventKind};

/// Bump when the envelope shape changes in a client-incompatible way.
pub const SCHEMA_VERSION: u32 = 1;

/// The event name used on the single global Tauri broadcast channel.
pub const CHANNEL: &str = "agent_event";

/// The typed body of an event: its kind plus a kind-specific JSON payload.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventBody {
    pub kind: EventKind,
    pub payload: serde_json::Value,
}

/// One IPC event.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentEventEnvelope {
    pub schema_version: u32,
    pub event_id: String,
    pub conversation_id: String,
    pub run_id: String,
    /// Present iff durable; strictly monotonic per run.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sequence: Option<i64>,
    pub durability: Durability,
    pub timestamp: String,
    pub event: EventBody,
}

impl AgentEventEnvelope {
    /// Build a durable envelope (requires the persisted sequence).
    pub fn durable(
        event_id: String,
        conversation_id: String,
        run_id: String,
        sequence: i64,
        kind: EventKind,
        payload: serde_json::Value,
        timestamp: String,
    ) -> Self {
        debug_assert!(kind.is_durable(), "durable() called with ephemeral kind {kind:?}");
        AgentEventEnvelope {
            schema_version: SCHEMA_VERSION,
            event_id,
            conversation_id,
            run_id,
            sequence: Some(sequence),
            durability: Durability::Durable,
            timestamp,
            event: EventBody { kind, payload },
        }
    }

    /// Build an ephemeral envelope (no sequence).
    pub fn ephemeral(
        event_id: String,
        conversation_id: String,
        run_id: String,
        kind: EventKind,
        payload: serde_json::Value,
        timestamp: String,
    ) -> Self {
        debug_assert!(!kind.is_durable(), "ephemeral() called with durable kind {kind:?}");
        AgentEventEnvelope {
            schema_version: SCHEMA_VERSION,
            event_id,
            conversation_id,
            run_id,
            sequence: None,
            durability: Durability::Ephemeral,
            timestamp,
            event: EventBody { kind, payload },
        }
    }

    pub fn is_terminal(&self) -> bool {
        self.event.kind.is_terminal()
    }
}

/// A sink for outbound envelopes. The Tauri implementation emits on the global
/// channel; tests use an in-memory collector.
pub trait EventSink: Send + Sync {
    fn emit(&self, env: &AgentEventEnvelope);
}

/// The production sink: broadcasts on the single global Tauri channel.
pub struct TauriEventSink {
    app: tauri::AppHandle,
}

impl TauriEventSink {
    pub fn new(app: tauri::AppHandle) -> Self {
        TauriEventSink { app }
    }
}

impl EventSink for TauriEventSink {
    fn emit(&self, env: &AgentEventEnvelope) {
        use tauri::Emitter;
        let _ = self.app.emit(CHANNEL, env);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durable_has_sequence_ephemeral_does_not() {
        let d = AgentEventEnvelope::durable(
            "e1".into(),
            "c1".into(),
            "r1".into(),
            1,
            EventKind::RunStarted,
            serde_json::json!({}),
            "t".into(),
        );
        assert_eq!(d.sequence, Some(1));
        assert_eq!(d.durability, Durability::Durable);

        let e = AgentEventEnvelope::ephemeral(
            "e2".into(),
            "c1".into(),
            "r1".into(),
            EventKind::AssistantTextDelta,
            serde_json::json!({"text":"hi"}),
            "t".into(),
        );
        assert_eq!(e.sequence, None);
        assert_eq!(e.durability, Durability::Ephemeral);
    }

    #[test]
    fn ephemeral_envelope_omits_sequence_in_json() {
        let e = AgentEventEnvelope::ephemeral(
            "e2".into(),
            "c1".into(),
            "r1".into(),
            EventKind::ToolProgress,
            serde_json::json!({}),
            "t".into(),
        );
        let s = serde_json::to_string(&e).unwrap();
        assert!(!s.contains("sequence"), "ephemeral must omit sequence: {s}");
    }

    #[test]
    fn terminal_detection() {
        let d = AgentEventEnvelope::durable(
            "e".into(),
            "c".into(),
            "r".into(),
            9,
            EventKind::RunCompleted,
            serde_json::json!({}),
            "t".into(),
        );
        assert!(d.is_terminal());
    }

    #[test]
    fn roundtrips() {
        let d = AgentEventEnvelope::durable(
            "e1".into(),
            "c1".into(),
            "r1".into(),
            3,
            EventKind::ToolSucceeded,
            serde_json::json!({"tool":"get_quote"}),
            "t".into(),
        );
        let s = serde_json::to_string(&d).unwrap();
        let back: AgentEventEnvelope = serde_json::from_str(&s).unwrap();
        assert_eq!(d, back);
    }
}
