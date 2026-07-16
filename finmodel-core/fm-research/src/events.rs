//! Deterministic progress-event contract (Phase 2.7) — pure, offline-testable.
//!
//! The async driver emits [`ResearchProgress`] events as it runs; this module is
//! the CONTRACT those events must satisfy, expressed as a validator the driver's
//! tests assert against. Invariants:
//!   * within each phase, `total` is fixed on its first event and `completed` is
//!     monotonic and never exceeds `total`,
//!   * phases occur in canonical order (Planning → Searching → Reading →
//!     Synthesizing), any prefix/subset allowed (Quick skips Planning),
//!   * exactly one terminal event (`done` | `cancelled` | `error`), and it is
//!     last.

use crate::ResearchProgress;
use crate::research::ResearchPhase;

/// Whether a phase is a terminal outcome.
pub fn is_terminal(phase: ResearchPhase) -> bool {
    matches!(
        phase,
        ResearchPhase::Done | ResearchPhase::Cancelled | ResearchPhase::Error
    )
}

/// Validate a full progress-event sequence against the deterministic contract.
pub fn check_progress_sequence(events: &[ResearchProgress]) -> Result<(), String> {
    if events.is_empty() {
        return Err("no progress events".into());
    }

    // Exactly one terminal, and it is the last event.
    let terminals: Vec<usize> = events
        .iter()
        .enumerate()
        .filter(|(_, e)| is_terminal(e.phase))
        .map(|(i, _)| i)
        .collect();
    if terminals.len() != 1 {
        return Err(format!(
            "expected exactly one terminal event, found {}",
            terminals.len()
        ));
    }
    if terminals[0] != events.len() - 1 {
        return Err("terminal event must be last".into());
    }

    // Walk contiguous per-phase runs, enforcing fixed total + monotonic completed.
    let mut order: Vec<ResearchPhase> = Vec::new();
    let mut i = 0;
    while i < events.len() {
        let phase = events[i].phase;
        if is_terminal(phase) {
            i += 1;
            continue;
        }
        if order.contains(&phase) {
            return Err(format!("phase {phase:?} recurs non-contiguously"));
        }
        order.push(phase);
        let total = events[i].total;
        let mut last_completed = 0u32;
        while i < events.len() && events[i].phase == phase {
            let e = &events[i];
            if e.total != total {
                return Err(format!("total changed within phase {phase:?}"));
            }
            if e.completed < last_completed {
                return Err(format!("completed decreased within phase {phase:?}"));
            }
            if e.completed > e.total {
                return Err(format!(
                    "completed {} exceeds total {} in {phase:?}",
                    e.completed, e.total
                ));
            }
            last_completed = e.completed;
            i += 1;
        }
    }

    check_phase_order(&order)
}

/// The non-terminal phases must be a subsequence of the canonical stage order.
fn check_phase_order(order: &[ResearchPhase]) -> Result<(), String> {
    const CANON: [ResearchPhase; 4] = [
        ResearchPhase::Planning,
        ResearchPhase::Searching,
        ResearchPhase::Reading,
        ResearchPhase::Synthesizing,
    ];
    let mut idx = 0;
    for p in order {
        while idx < CANON.len() && CANON[idx] != *p {
            idx += 1;
        }
        if idx >= CANON.len() {
            return Err(format!("phase {p:?} out of canonical order"));
        }
        idx += 1;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::research::ResearchPhase;

    fn ev(phase: ResearchPhase, completed: u32, total: u32) -> ResearchProgress {
        ResearchProgress {
            run_id: "r1".into(),
            parent_run_id: None,
            attempt: 1,
            conversation_id: "c1".into(),
            phase,
            completed,
            total,
            source_id: None,
            title: None,
            url: None,
            source_status: None,
            detail_code: None,
        }
    }

    #[test]
    fn valid_full_sequence_passes() {
        let events = vec![
            ev(ResearchPhase::Planning, 0, 1),
            ev(ResearchPhase::Planning, 1, 1),
            ev(ResearchPhase::Searching, 0, 3),
            ev(ResearchPhase::Searching, 3, 3),
            ev(ResearchPhase::Reading, 0, 2),
            ev(ResearchPhase::Reading, 2, 2),
            ev(ResearchPhase::Synthesizing, 0, 1),
            ev(ResearchPhase::Done, 1, 1),
        ];
        assert!(check_progress_sequence(&events).is_ok());
    }

    #[test]
    fn quick_path_without_planning_passes() {
        let events = vec![
            ev(ResearchPhase::Searching, 1, 1),
            ev(ResearchPhase::Reading, 1, 1),
            ev(ResearchPhase::Synthesizing, 0, 1),
            ev(ResearchPhase::Done, 1, 1),
        ];
        assert!(check_progress_sequence(&events).is_ok());
    }

    #[test]
    fn cancelled_terminal_passes() {
        let events = vec![
            ev(ResearchPhase::Searching, 0, 3),
            ev(ResearchPhase::Cancelled, 0, 0),
        ];
        assert!(check_progress_sequence(&events).is_ok());
    }

    #[test]
    fn rejects_total_change_within_phase() {
        let events = vec![
            ev(ResearchPhase::Searching, 0, 3),
            ev(ResearchPhase::Searching, 1, 4), // total changed
            ev(ResearchPhase::Done, 1, 1),
        ];
        assert!(
            check_progress_sequence(&events)
                .unwrap_err()
                .contains("total changed")
        );
    }

    #[test]
    fn rejects_non_monotonic_completed() {
        let events = vec![
            ev(ResearchPhase::Searching, 2, 3),
            ev(ResearchPhase::Searching, 1, 3), // went backwards
            ev(ResearchPhase::Done, 1, 1),
        ];
        assert!(
            check_progress_sequence(&events)
                .unwrap_err()
                .contains("decreased")
        );
    }

    #[test]
    fn rejects_completed_over_total() {
        let events = vec![
            ev(ResearchPhase::Searching, 5, 3),
            ev(ResearchPhase::Done, 1, 1),
        ];
        assert!(
            check_progress_sequence(&events)
                .unwrap_err()
                .contains("exceeds")
        );
    }

    #[test]
    fn rejects_out_of_order_phase() {
        let events = vec![
            ev(ResearchPhase::Reading, 0, 2),
            ev(ResearchPhase::Searching, 0, 3), // searching after reading
            ev(ResearchPhase::Done, 1, 1),
        ];
        assert!(
            check_progress_sequence(&events)
                .unwrap_err()
                .contains("out of canonical order")
        );
    }

    #[test]
    fn rejects_missing_or_multiple_terminals() {
        // No terminal.
        let no_term = vec![ev(ResearchPhase::Searching, 0, 3)];
        assert!(check_progress_sequence(&no_term).is_err());
        // Two terminals.
        let two = vec![
            ev(ResearchPhase::Done, 1, 1),
            ev(ResearchPhase::Error, 0, 0),
        ];
        assert!(check_progress_sequence(&two).is_err());
        // Terminal not last.
        let mid = vec![
            ev(ResearchPhase::Done, 1, 1),
            ev(ResearchPhase::Searching, 0, 3),
        ];
        assert!(check_progress_sequence(&mid).is_err());
    }
}
