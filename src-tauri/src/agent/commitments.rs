//! Commitment extraction (Task 8.2).
//!
//! A deterministic, high-precision pass over a completed turn: only an explicit
//! follow-up promise becomes a commitment candidate. Casual/hedged future-tense
//! never creates one, and a candidate is only *proposed* — creating or changing a
//! schedule always requires user approval (never scheduled from inference). A
//! low-cost LLM extractor may later improve recall, but it never lowers this
//! precision bar.

/// An extracted follow-up commitment candidate (proposed, not yet scheduled).
#[derive(Clone, Debug, PartialEq)]
pub struct CommitmentCandidate {
    pub text: String,
    /// Coarse due semantics parsed from the text, if any.
    pub due_semantics: Option<String>,
    /// Extraction confidence in [0,1]; only ≥ 0.6 candidates are proposed.
    pub confidence: f64,
}

/// Extract at most one commitment from the user turn. `None` for casual, hedged,
/// or non-committal text.
pub fn extract_commitment(user_text: &str) -> Option<CommitmentCandidate> {
    let t = user_text.trim();
    if t.is_empty() {
        return None;
    }
    let m = t.to_lowercase();

    // Casual / hedged future-tense is never a commitment.
    const HEDGES: &[&str] = &[
        "maybe",
        "might",
        "perhaps",
        "someday",
        "some day",
        "at some point",
        "i could",
        "we could",
        "i guess",
        "possibly",
        "sometime",
    ];
    if HEDGES.iter().any(|h| m.contains(h)) {
        return None;
    }

    // Optional due anchor.
    let due = if m.contains("after the next earnings")
        || m.contains("after earnings")
        || m.contains("after the earnings")
        || m.contains("next earnings")
    {
        Some("after_next_earnings".to_string())
    } else if m.contains("next quarter") {
        Some("next_quarter".to_string())
    } else if m.contains("tomorrow") {
        Some("tomorrow".to_string())
    } else if m.contains("next week") {
        Some("next_week".to_string())
    } else {
        None
    };

    // Explicit follow-up cues.
    const CUES: &[&str] = &[
        "re-run this after",
        "rerun after",
        "re-run after",
        "recheck after",
        "remind me",
        "follow up",
        "follow-up",
        "check back",
        "revisit after",
        "run this again after",
        "notify me when",
        "let me know when",
    ];
    let has_cue = CUES.iter().any(|c| m.contains(c));
    let has_action_with_due = due.is_some()
        && (m.contains("re-run")
            || m.contains("rerun")
            || m.contains("recheck")
            || m.contains("again"));

    let confidence = match (has_cue, due.is_some()) {
        (true, true) => 0.9,
        (true, false) => 0.7,
        (false, true) if has_action_with_due => 0.6,
        _ => 0.0,
    };
    if confidence >= 0.6 {
        Some(CommitmentCandidate {
            text: t.to_string(),
            due_semantics: due,
            confidence,
        })
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_followup_becomes_one_commitment() {
        let c = extract_commitment("Re-run this after the next earnings release.").unwrap();
        assert_eq!(c.due_semantics.as_deref(), Some("after_next_earnings"));
        assert!(c.confidence >= 0.9);
    }

    #[test]
    fn recheck_after_earnings_extracts() {
        let c = extract_commitment("recheck after earnings").unwrap();
        assert_eq!(c.due_semantics.as_deref(), Some("after_next_earnings"));
    }

    #[test]
    fn remind_me_next_week_extracts() {
        let c = extract_commitment("remind me next week to look at this").unwrap();
        assert_eq!(c.due_semantics.as_deref(), Some("next_week"));
        assert!(c.confidence >= 0.9);
    }

    #[test]
    fn casual_future_tense_creates_nothing() {
        assert!(extract_commitment("maybe I'll check Tesla sometime").is_none());
        assert!(extract_commitment("we could look at comps at some point").is_none());
        assert!(extract_commitment("What were Tesla's 2025 sales?").is_none());
        assert!(extract_commitment("").is_none());
    }

    #[test]
    fn a_due_anchor_without_an_action_is_not_a_commitment() {
        // Mentioning earnings is not a promise to re-run anything.
        assert!(extract_commitment("Tesla reports earnings next quarter, interesting.").is_none());
    }
}
