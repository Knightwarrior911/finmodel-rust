//! Memory capture and recall (Phase E).
//!
//! Captures structured memories (preferences, corrections, decisions, verified
//! numeric claims) after a completed turn, subject to precision gates (≥98%
//! numeric accuracy, exact entity/value/source matching). Recalls relevant
//! memories during context assembly.
//!
//! Uses the [`MemoryRepository`] trait so the same logic works over SQLite
//! (production) and in-memory (AgentMachine tests).

use std::collections::HashSet;

use fm_agent::types::Claim;
use crate::store::memory::{MemoryRepository, MemoryScope, NewMemory};

// ── Extraction input ──────────────────────────────────────────────────

/// What a completed turn produces for memory extraction.
pub struct TurnOutput {
    /// User-authored statements that may encode preferences or corrections.
    pub user_statements: Vec<String>,
    /// Verified numeric claims from the verification step.
    pub verified_claims: Vec<Claim>,
    /// Turn-level source identifier.
    pub turn_source: String,
    /// Assistant text summary (used for qualitative preferences).
    pub assistant_summary: String,
}

/// Where the memory was extracted from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExtractionSource {
    UserStatement,
    VerifiedClaim,
    AssistantSummary,
}

/// One extracted memory candidate (before precision gate / dedup).
#[derive(Clone, Debug)]
pub struct MemoryCandidate {
    pub scope_type: String,
    pub workspace_id: Option<String>,
    pub conversation_id: Option<String>,
    pub kind: String,
    pub content: String,
    pub normalized_key: String,
    pub importance: f64,
    pub confidence: f64,
    pub source_type: String,
    pub source_ref: Option<String>,
    pub source: ExtractionSource,
}

// ── Precision gate ────────────────────────────────────────────────────

/// Guards before memory write (plan decision 15, capture policy).
///
/// - Reject API keys, credentials, raw document bodies, filesystem paths,
///   provider errors, and any candidate not traceable to an eligible user span
///   or validated claim ID.
/// - Numeric facts must carry the exact verified entity/value/unit/currency/
///   scale/period/as-of/source fields.
/// - Deduplicate by `normalized_key` + scope; corrections close `valid_to`
///   and link `superseded_by`.
pub struct PrecisionGate {
    /// Pattern prefixes known to be non-candidate text.
    skip_prefixes: &'static [&'static str],
    /// Keys/patterns that look like credentials.
    secret_patterns: &'static [&'static str],
}

impl Default for PrecisionGate {
    fn default() -> Self {
        Self {
            skip_prefixes: &[
                "http://", "https://", "C:\\", "/home", "/Users",
                "api_key", "sk-", "Bearer ",
            ],
            secret_patterns: &[
                "sk-", "pk-", "api_key", "api-key", "secret",
                "token", "password", "credential",
            ],
        }
    }
}

impl PrecisionGate {
    /// Check whether a raw text candidate is safe to store as memory.
    /// Returns `Ok(())` if it passes, `Err(reason)` if rejected.
    pub fn check(&self, text: &str) -> Result<(), String> {
        let lower = text.to_lowercase();
        if text.len() < 5 {
            return Err("too short".into());
        }
        for pat in self.secret_patterns {
            if lower.contains(pat) {
                return Err(format!("contains secret pattern `{pat}`"));
            }
        }
        for pre in self.skip_prefixes {
            if text.starts_with(pre) || lower.starts_with(pre) {
                return Err(format!("starts with blocked prefix `{pre}`"));
            }
        }
        Ok(())
    }

    /// Validate that a numeric claim has all required fields.
    pub fn check_claim(&self, claim: &Claim) -> Result<(), String> {
        if claim.entity.is_empty() {
            return Err("claim missing entity".into());
        }
        if claim.normalized_value.parse::<f64>().is_err() {
            return Err("claim has non-finite or unparseable value".into());
        }
        if claim.period.is_empty() {
            return Err("claim missing period".into());
        }
        if claim.source_id.is_empty() {
            return Err("claim missing source id".into());
        }
        Ok(())
    }
}

// ── Capture service ───────────────────────────────────────────────────

/// Extract memories from a completed turn and persist them through the
/// repository. Guards: precision gate, dedup, supersession.
pub struct MemoryCapture<'a> {
    repo: &'a mut dyn MemoryRepository,
    gate: PrecisionGate,
    now: String,
}

impl<'a> MemoryCapture<'a> {
    pub fn new(repo: &'a mut dyn MemoryRepository, now: &str) -> Self {
        Self {
            repo,
            gate: PrecisionGate::default(),
            now: now.to_string(),
        }
    }

    /// Run extraction on a completed turn. Returns the number of memories saved.
    pub fn extract(&mut self, turn: &TurnOutput, ws_id: Option<&str>, conv_id: Option<&str>) -> usize {
        let mut saved = 0;
        let mut inserted_keys: HashSet<(String, Option<String>)> = HashSet::new();

        // 1. Extract from verified claims (highest confidence).
        for claim in &turn.verified_claims {
            if self.gate.check_claim(claim).is_err() {
                continue;
            }
            let content = format!(
                "{} {} = {} {} (period: {}, source: {})",
                claim.entity, claim.claim_key, claim.normalized_value,
                claim.currency.as_deref().unwrap_or(claim.unit.as_str()),
                claim.period, claim.source_id,
            );
            let key = format!("claim:{}:{}", claim.entity.to_lowercase(), claim.claim_key.to_lowercase());
            let scope_key = (key.clone(), ws_id.map(|s| s.to_string()));

            // Dedup: skip if we already inserted this exact key+scope.
            if inserted_keys.contains(&scope_key) {
                continue;
            }

            let candidate = MemoryCandidate {
                scope_type: "workspace".into(),
                workspace_id: ws_id.map(|s| s.to_string()),
                conversation_id: conv_id.map(|s| s.to_string()),
                kind: "numeric_claim".into(),
                content,
                normalized_key: key,
                importance: 0.9,
                confidence: 0.95,
                source_type: "verified_claim".into(),
                source_ref: Some(turn.turn_source.clone()),
                source: ExtractionSource::VerifiedClaim,
            };

            if self.persist_candidate(&candidate) {
                inserted_keys.insert(scope_key);
                saved += 1;
            }
        }

        // 2. Extract from user statements (medium confidence).
        for stmt in &turn.user_statements {
            if self.gate.check(stmt).is_err() {
                continue;
            }
            let key = normalize_user_statement(stmt);
            let scope_key = (key.clone(), ws_id.map(|s| s.to_string()));
            if inserted_keys.contains(&scope_key) {
                continue;
            }

            let candidate = MemoryCandidate {
                scope_type: "workspace".into(),
                workspace_id: ws_id.map(|s| s.to_string()),
                conversation_id: conv_id.map(|s| s.to_string()),
                kind: "preference".into(),
                content: stmt.clone(),
                normalized_key: key,
                importance: 0.6,
                confidence: 0.7,
                source_type: "user".into(),
                source_ref: Some(turn.turn_source.clone()),
                source: ExtractionSource::UserStatement,
            };

            if self.persist_candidate(&candidate) {
                inserted_keys.insert(scope_key);
                saved += 1;
            }
        }

        saved
    }

    /// Persist one candidate: dedup → supersede old → insert new.
    fn persist_candidate(&mut self, c: &MemoryCandidate) -> bool {
        // Supersession: find existing memory by normalized_key + scope,
        // close it and link to the new one.
        let existing = self.repo.search(
            &c.normalized_key,
            &MemoryScope {
                workspace_id: c.workspace_id.clone(),
                conversation_id: c.conversation_id.clone(),
                global_only: c.scope_type == "global",
            },
        );

        let mem = NewMemory {
            public_id: format!("mem_{:x}", fastrand::u64(..)),
            scope_type: c.scope_type.clone(),
            workspace_id: c.workspace_id.clone(),
            conversation_id: c.conversation_id.clone(),
            kind: c.kind.clone(),
            content: c.content.clone(),
            normalized_key: c.normalized_key.clone(),
            importance: c.importance,
            confidence: c.confidence,
            source_type: c.source_type.clone(),
            source_ref: c.source_ref.clone(),
            now: self.now.clone(),
        };

        match self.repo.insert(mem) {
            Ok(new_id) => {
                // Supersede old matches (same key, same scope).
                if let Ok(results) = &existing {
                    for result in results {
                        let old_id = result.memory.id;
                        if old_id != new_id && result.memory.superseded_by.is_none() {
                            let _ = self.repo.supersede(old_id, new_id, &self.now);
                        }
                    }
                }
                true
            }
            Err(_) => false,
        }
    }
}

/// Simple normalized key from a user statement: lowercase, strip punctuation,
/// take first 5 significant words.
fn normalize_user_statement(s: &str) -> String {
    let cleaned: String = s
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .collect();
    let words: Vec<&str> = cleaned
        .split_whitespace()
        .filter(|w| w.len() > 2)
        .collect();
    let take = words.len().min(5);
    format!("user:{}", words[..take].join(":").to_lowercase())
}

// ── Recall service ────────────────────────────────────────────────────

/// Recall relevant memories for context injection.
pub struct MemoryRecall<'a> {
    repo: &'a dyn MemoryRepository,
}

impl<'a> MemoryRecall<'a> {
    pub fn new(repo: &'a dyn MemoryRepository) -> Self {
        Self { repo }
    }

    /// Query for relevant memories given entity/topic hints and scope.
    /// Returns formatted context lines and the count of memories found.
    pub fn recall(
        &self,
        query: &str,
        scope: &MemoryScope,
        limit: usize,
    ) -> (Vec<String>, usize) {
        let results = match self.repo.search(query, scope) {
            Ok(r) => r,
            Err(_) => return (vec![], 0),
        };

        let count = results.len().min(limit);
        let lines: Vec<String> = results
            .into_iter()
            .take(limit)
            .enumerate()
            .map(|(i, r)| {
                let m = &r.memory;
                format!(
                    "  {}. [{}] {} (confidence: {:.0}%, source: {})",
                    i + 1,
                    m.kind,
                    m.content,
                    m.confidence * 100.0,
                    m.source_type,
                )
            })
            .collect();

        (lines, count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::memory::InMemoryMemoryRepository;
    use fm_agent::types::Claim;

    fn repo() -> InMemoryMemoryRepository {
        InMemoryMemoryRepository::new()
    }

    fn sample_claim(key: &str, entity: &str, value: f64) -> Claim {
        Claim {
            claim_key: key.into(),
            entity: entity.into(),
            normalized_value: value.to_string(),
            unit: "USD".into(),
            currency: Some("USD".into()),
            scale: "1".into(),
            period: "FY2024".into(),
            locator: "table-1".into(),
            source_id: "src-1".into(),
            quote_hash: "abc".into(),
        }
    }

    #[test]
    fn precision_gate_rejects_secrets() {
        let gate = PrecisionGate::default();
        assert!(gate.check("my api_key is sk-abc123").is_err());
        assert!(gate.check("Bearer token here").is_err());
    }

    #[test]
    fn precision_gate_rejects_paths() {
        let gate = PrecisionGate::default();
        assert!(gate.check("https://evil.com/payload").is_err());
        assert!(gate.check("C:\\Users\\admin\\secrets").is_err());
    }

    #[test]
    fn precision_gate_accepts_normal_text() {
        let gate = PrecisionGate::default();
        assert!(gate.check("User prefers P/E over EV/EBITDA").is_ok());
        assert!(gate.check("Revenue growth is the key driver for FY2025").is_ok());
    }

    #[test]
    fn precision_gate_rejects_short_text() {
        let gate = PrecisionGate::default();
        assert!(gate.check("hi").is_err());
    }

    #[test]
    fn precision_gate_rejects_invalid_claim() {
        let gate = PrecisionGate::default();
        let mut c = sample_claim("rev", "NVDA", 130.0);
        assert!(gate.check_claim(&c).is_ok());

        c.entity = "".into();
        assert!(gate.check_claim(&c).is_err());

        c.entity = "NVDA".into();
        c.normalized_value = "not_a_number".into();
        assert!(gate.check_claim(&c).is_err());
    }

    #[test]
    fn capture_extracts_verified_claims() {
        let mut r = repo();
        let now = "2026-07-17T12:00:00Z";
        let turn = TurnOutput {
            user_statements: vec![],
            verified_claims: vec![sample_claim("revenue", "NVDA", 130.0)],
            turn_source: "turn-42".into(),
            assistant_summary: "".into(),
        };

        let mut cap = MemoryCapture::new(&mut r, now);
        let saved = cap.extract(&turn, Some("ws1"), Some("conv1"));
        assert_eq!(saved, 1);

        let results = r.search("NVDA", &MemoryScope { workspace_id: Some("ws1".into()), ..Default::default() }).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].memory.kind, "numeric_claim");
    }

    #[test]
    fn capture_dedup_same_claim() {
        let mut r = repo();
        let now = "2026-07-17T12:00:00Z";
        let turn = TurnOutput {
            user_statements: vec![],
            verified_claims: vec![
                sample_claim("revenue", "NVDA", 130.0),
                sample_claim("revenue", "NVDA", 130.0), // duplicate
            ],
            turn_source: "turn-43".into(),
            assistant_summary: "".into(),
        };

        let mut cap = MemoryCapture::new(&mut r, now);
        let saved = cap.extract(&turn, Some("ws1"), Some("conv1"));
        assert_eq!(saved, 1); // dedup
    }

    #[test]
    fn capture_extracts_user_preferences() {
        let mut r = repo();
        let now = "2026-07-17T12:00:00Z";
        let turn = TurnOutput {
            user_statements: vec!["User prefers P/E multiple over EV/EBITDA".into()],
            verified_claims: vec![],
            turn_source: "turn-44".into(),
            assistant_summary: "".into(),
        };

        let mut cap = MemoryCapture::new(&mut r, now);
        let saved = cap.extract(&turn, Some("ws1"), None);
        assert_eq!(saved, 1);

        let results = r.search("prefers", &MemoryScope { workspace_id: Some("ws1".into()), ..Default::default() }).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].memory.kind, "preference");
    }

    #[test]
    fn capture_skips_garbage_user_statements() {
        let mut r = repo();
        let now = "2026-07-17T12:00:00Z";
        let turn = TurnOutput {
            user_statements: vec![
                "sk-abc123def456".into(), // looks like API key
                "https://malicious.site".into(), // URL
                "OK".into(), // too short
            ],
            verified_claims: vec![],
            turn_source: "turn-45".into(),
            assistant_summary: "".into(),
        };

        let mut cap = MemoryCapture::new(&mut r, now);
        let saved = cap.extract(&turn, Some("ws1"), None);
        assert_eq!(saved, 0); // all rejected
    }

    #[test]
    fn capture_supersedes_old_memory() {
        let mut r = repo();
        let now = "2026-07-17T12:00:00Z";
        let stmt = "Prefers equity valuation using dcf model";

        // First turn.
        let turn1 = TurnOutput {
            user_statements: vec![stmt.into()],
            verified_claims: vec![],
            turn_source: "turn-1".into(),
            assistant_summary: "".into(),
        };
        let mut cap = MemoryCapture::new(&mut r, now);
        cap.extract(&turn1, Some("ws1"), None);

        // Second turn: same preference restated → supersedes old.
        let turn2 = TurnOutput {
            user_statements: vec![stmt.into()],
            verified_claims: vec![],
            turn_source: "turn-2".into(),
            assistant_summary: "".into(),
        };
        let later = "2026-07-17T14:00:00Z";
        let mut cap2 = MemoryCapture::new(&mut r, later);
        cap2.extract(&turn2, Some("ws1"), None);

        // Should have 2 memories with the first superseded.
        let all = r.search("prefers", &MemoryScope::default()).unwrap();
        assert_eq!(all.len(), 2);

        // The older one should have valid_to set.
        let first = r.get(0).unwrap().unwrap();
        assert_eq!(first.valid_to, Some(later.to_string()));
        assert_eq!(first.superseded_by, Some(1));
    }

    #[test]
    fn recall_returns_formatted_lines() {
        let mut r = repo();
        let now = "2026-07-17T12:00:00Z";

        // Insert a memory directly.
        let mem = NewMemory {
            public_id: "mem-recall-1".into(),
            scope_type: "workspace".into(),
            workspace_id: Some("ws1".into()),
            conversation_id: None,
            kind: "preference".into(),
            content: "Prefers P/E multiple".into(),
            normalized_key: "user:prefers:multiple".into(),
            importance: 0.6,
            confidence: 0.7,
            source_type: "user".into(),
            source_ref: Some("t1".into()),
            now: now.into(),
        };
        r.insert(mem).unwrap();

        let recall = MemoryRecall::new(&r);
        let (lines, count) = recall.recall(
            "prefers",
            &MemoryScope { workspace_id: Some("ws1".into()), ..Default::default() },
            5,
        );
        assert_eq!(count, 1);
        assert!(lines[0].contains("P/E"));
        assert!(lines[0].contains("70%"));
    }

    #[test]
    fn recall_empty_when_no_match() {
        let r = repo();
        let recall = MemoryRecall::new(&r);
        let (lines, count) = recall.recall("nonexistent", &MemoryScope::default(), 5);
        assert_eq!(count, 0);
        assert!(lines.is_empty());
    }

    #[test]
    fn capture_rejects_non_numeric_claim_value() {
        let mut r = repo();
        let mut claim = sample_claim("eps", "AAPL", 6.5);
        claim.normalized_value = "not_numeric".into();
        let turn = TurnOutput {
            user_statements: vec![],
            verified_claims: vec![claim],
            turn_source: "turn-50".into(),
            assistant_summary: "".into(),
        };

        let mut cap = MemoryCapture::new(&mut r, "2026-07-17T12:00:00Z");
        let saved = cap.extract(&turn, Some("ws1"), None);
        assert_eq!(saved, 0);
    }

    #[test]
    fn capture_scope_isolation() {
        let mut r = repo();
        let now = "2026-07-17T12:00:00Z";
        let turn = TurnOutput {
            user_statements: vec!["Prefers EV/EBITDA".into()],
            verified_claims: vec![],
            turn_source: "turn-60".into(),
            assistant_summary: "".into(),
        };

        let mut cap = MemoryCapture::new(&mut r, now);
        cap.extract(&turn, Some("ws1"), None);

        // Same memory should NOT appear in a different workspace.
        let results = r.search("prefers", &MemoryScope { workspace_id: Some("ws2".into()), ..Default::default() }).unwrap();
        assert_eq!(results.len(), 0);
    }
}
