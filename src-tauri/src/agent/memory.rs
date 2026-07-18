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
            // Gate (secrets/paths) AND the durable-preference classifier: only
            // standing preferences/conventions/corrections are auto-captured, not
            // one-off questions or requests (M5 auto-capture; eval-gated).
            if self.gate.check(stmt).is_err() || !is_durable_preference(stmt) {
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

/// Standing-intent cues: substrings that mark a user statement as a durable
/// preference, convention, coverage/deal context, or explicit correction worth
/// remembering across turns. Multi-word / disambiguated where a bare token
/// would collide with finance vocabulary (e.g. "prefer" vs "preferred stock").
const DURABLE_CUES: &[&str] = &[
    // preferences
    "i prefer", "we prefer", "prefers", "prefer to", "would prefer",
    "my preference", "preference is", "i'd rather", "i would rather",
    "rather use", "i like to", "i like using",
    // standing instructions / conventions
    "by default", "default to", "as a rule", "from now on", "going forward",
    "in future", "henceforth", "always use", "always show", "always present",
    "always include", "always exclude", "always round", "always report",
    "always assume", "always format", "always express", "never use",
    "never show", "never include", "i use ", "we use ", "i always ",
    "we always ", "our convention", "house style", "our policy",
    "our standard", "for all my", "in all my", "every time", "keep in mind",
    "remember that", "note that i", "i want everything", "i want all",
    // coverage / deal / entity context
    "i focus on", "i cover ", "we cover ", "my coverage", "my sector",
    "base currency", "fiscal year end", "our fiscal year", "our fy",
    "we define", "we're advising", "we are advising", "we represent",
    "our client", "our target is", "the mandate", "our engagement",
    // explicit corrections
    "actually,", "correction:", "i meant", "to be clear,",
];

/// Interrogative openers: a statement starting this way is a question/request,
/// never a durable preference (even without a trailing '?').
const QUESTION_OPENERS: &[&str] = &[
    "what ", "what's", "whats ", "how ", "why ", "when ", "where ", "who ",
    "which ", "do you", "can you", "could you", "would you", "should i",
    "is there", "are there", "give me", "show me", "build ", "compare ",
    "list ", "find ", "fetch ", "pull ", "get ", "run ", "calculate ",
    "analyze ", "analyse ", "download ", "open ", "read ", "summarize ",
];

/// Decide whether a user statement encodes a DURABLE preference, standing
/// instruction, correction, or coverage/deal context worth remembering across
/// turns — as opposed to a one-off question, request, or chit-chat.
///
/// Rules-based (no model call), tuned to favour precision: capture only when a
/// standing-intent cue is present AND the statement is not phrased as a
/// question/one-off request. Tuned on the `dev` split; the precision/recall
/// gate is reported on the untouched `held-out` split (see `auto_capture_eval`).
pub(crate) fn is_durable_preference(stmt: &str) -> bool {
    let t = stmt.trim();
    if t.ends_with('?') {
        return false;
    }
    let lower = t.to_lowercase();
    // A standing cue can override a request-verb opener ("always show …" is a
    // preference), but a genuine question opener with no standing cue is out.
    let has_cue = DURABLE_CUES.iter().any(|c| lower.contains(c));
    if !has_cue {
        return false;
    }
    // If it opens like a question/request AND the only "cue" is weak, drop it.
    // Strong standing markers survive request openers; bare openers do not.
    for q in QUESTION_OPENERS {
        if lower.starts_with(q) {
            // Allow only when an unambiguous standing marker is present.
            const STRONG: &[&str] = &[
                "always", "never", "by default", "default to", "from now on",
                "going forward", "every time", "for all my", "in all my",
            ];
            return STRONG.iter().any(|s| lower.contains(s));
        }
    }
    true
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

    // ── M5 auto-capture eval ──────────────────────────────────────────
    //
    // Labelled analyst-turn set for the durable-preference classifier.
    // `true`  = a standing preference / convention / correction / coverage-deal
    //           fact that SHOULD be auto-captured.
    // `false` = a one-off question/request, transient chit-chat, or secret/path
    //           that must NOT be captured.
    //
    // Honesty (see plan decision 4 + review advisory): the classifier + gate
    // were tuned ONLY against DEV; HELDOUT is scored once and is the reported
    // gate. As a single author I can see both splits, so this is a *procedural*
    // split, not an independent one — real production logs remain the gold
    // standard. The set deliberately includes hard cases: durable statements
    // phrased WITHOUT keyword cues (recall stress) and chit-chat that CONTAINS
    // preference words (precision stress), so the number reflects the real
    // limits of a keyword-rules classifier rather than a self-consistent toy.

    fn predict_capture(stmt: &str) -> bool {
        PrecisionGate::default().check(stmt).is_ok() && is_durable_preference(stmt)
    }

    fn score(set: &[(&str, bool)]) -> (usize, usize, usize, usize) {
        let (mut tp, mut fp, mut tn, mut fneg) = (0usize, 0usize, 0usize, 0usize);
        for (stmt, label) in set {
            match (predict_capture(stmt), *label) {
                (true, true) => tp += 1,
                (true, false) => fp += 1,
                (false, false) => tn += 1,
                (false, true) => fneg += 1,
            }
        }
        (tp, fp, tn, fneg)
    }

    const DEV: &[(&str, bool)] = &[
        ("I prefer P/E over EV/EBITDA for consumer names", true),
        ("Always show revenue in USD millions", true),
        ("From now on, present all multiples to one decimal", true),
        ("We use IFRS rather than US GAAP for European names", true),
        ("Our fiscal year ends in March", true),
        ("My coverage is European semiconductors", true),
        ("We're advising Acme on the TechCo acquisition", true),
        ("By default, use a 10% discount rate in DCFs", true),
        ("Our base currency is EUR", true),
        ("I'd rather see unlevered free cash flow", true),
        ("Always exclude stock-based compensation from adjusted EBITDA", true),
        ("Default to the last twelve months for all comps", true),
        ("I like to see bear, base, and bull cases", true),
        ("Never use sell-side consensus for the terminal value", true),
        ("Our client is a mid-market private equity fund", true),
        ("Going forward, round share prices to two decimals", true),
        ("I prefer the perpetuity-growth method for terminal value", true),
        ("We always tax-effect EBIT at the marginal rate", true),
        ("My preference is a 2% terminal growth rate", true),
        ("Actually, use the fiscal year ending in June", true),
        ("Correction: the discount rate should be 9%", true),
        ("I meant the 2024 10-K, not 2023", true),
        ("We define net debt to include operating leases", true),
        ("Our house style is no decimals on dollar figures", true),
        ("I focus on large-cap US banks", true),
        ("The mandate is a sell-side for NVDA's data-center unit", true),
        ("For all my models, use a mid-year convention", true),
        ("In all my comps, cap the peer set at eight names", true),
        ("We represent the buyer in this deal", true),
        ("I want everything in local currency", true),
        ("As a rule, I ignore one-time restructuring charges", true),
        ("We prefer a CAPM-based WACC over a fixed rate", true),
        ("Note that I always want a sources page", true),
        ("Our policy is to use spot FX as of the filing date", true),
        ("I use a five-year explicit forecast horizon", true),
        ("I cover industrials and materials", true),
        ("We cover the entire EU banking sector", true),
        ("Henceforth, label all figures with their fiscal period", true),
        ("I would rather use median than mean for comps", true),
        ("Our convention is fiscal quarters, not calendar", true),
        ("I prefer diluted EPS over basic", true),
        ("Always express growth rates as CAGR", true),
        ("To be clear, our target is the software segment only", true),
        ("Always include a WACC sensitivity table", true),
        ("Round everything to the nearest million", true),
        ("Value banks on tangible book, not earnings", true),
        ("What were Tesla's 2025 sales?", false),
        ("Build a DCF for AAPL", false),
        ("Compare Apple and Microsoft revenue", false),
        ("Pull NVDA's latest 10-K", false),
        ("Show me the income statement", false),
        ("List Tesla's filings", false),
        ("How do I read a 10-K?", false),
        ("Why is the operating margin down?", false),
        ("Which peers should I use for AMD?", false),
        ("Thanks!", false),
        ("great, that works", false),
        ("Value the preferred stock at par", false),
        ("What's the preferred dividend on the series A?", false),
        ("Can you use the LTM period here?", false),
        ("Show me revenue in millions", false),
        ("Give me the WACC", false),
        ("Run a comps analysis on the peer set", false),
        ("Fetch the latest quote for MSFT", false),
        ("Get me Tesla's gross margin", false),
        ("Analyze the risk factors", false),
        ("Summarize the MD&A", false),
        ("Download the annual report", false),
        ("Open the DCF tab", false),
        ("Calculate the enterprise value", false),
        ("It's always the same issue with these filings", false),
        ("I used the 2024 numbers last time", false),
        ("We used IFRS in that old model", false),
        ("Does this cover Q3?", false),
        ("Is there a scenario tab?", false),
        ("Are there any peers missing?", false),
        ("Could you add a football field?", false),
        ("Would you re-run with 8%?", false),
        ("Should I include SBC?", false),
        ("Who audits Tesla?", false),
        ("When was the last 10-K filed?", false),
        ("Where is the cash flow statement?", false),
        ("hmm let me think", false),
        ("never mind, drop that one", false),
        ("Remind me what our fiscal year is", false),
        ("Tell me the default discount rate", false),
        ("my api key is sk-abc123def456", false),
        ("the model is at C:\\Users\\me\\dcf.xlsx", false),
        ("email me at analyst@bank.com", false),
        ("no that's not right", false),
        ("What multiple are they trading at?", false),
        ("Add a revenue bridge chart", false),
        ("Re-run the model with 12% WACC", false),
        ("Include the pension liability", false),
        ("Exclude the one-timers", false),
        ("Use the 2024 10-K", false),
        ("Set the tax rate to 21%", false),
        ("What's our base currency again?", false),
        ("Which method do you prefer for TV?", false),
        ("Do you always tax-effect EBIT?", false),
        ("Is EUR our base currency?", false),
        ("Explain the difference between levered and unlevered beta", false),
        ("Walk me through the DCF", false),
        ("Draft an IC memo", false),
        ("Export to Excel", false),
        ("I prefer not to wait, just pull the filing", false),
        ("Actually, that's fine", false),
        ("We use it every day, it's great", false),
    ];

    const HELDOUT: &[(&str, bool)] = &[
        ("I prefer to value banks on P/TBV", true),
        ("Always present EBITDA margins alongside revenue", true),
        ("From now on, use the 10-K, not press releases", true),
        ("We use a 2.5% terminal growth as standard", true),
        ("Our fiscal year ends in September", true),
        ("My coverage is North American energy", true),
        ("We're advising the special committee on the buyout", true),
        ("By default, present three years of history", true),
        ("Our base currency is GBP", true),
        ("I'd rather see net debt broken out by instrument", true),
        ("Always exclude discontinued operations from margins", true),
        ("Default to a 4.5% risk-free rate", true),
        ("I like to include a sensitivity on WACC and growth", true),
        ("Never annualize a partial quarter", true),
        ("Our client is a strategic acquirer in healthcare", true),
        ("Going forward, cite the exact filing date", true),
        ("I prefer gross profit over gross margin in tables", true),
        ("We always reconcile GAAP to adjusted metrics", true),
        ("My preference is USD thousands for small caps", true),
        ("Actually, our target is the consumer unit, not enterprise", true),
        ("I meant unlevered, not levered, free cash flow", true),
        ("We define free cash flow after lease payments", true),
        ("I focus on mid-cap software", true),
        ("The mandate is a debt refinancing for the group", true),
        ("For all my comps, use forward multiples", true),
        ("In all my models, keep a two-decimal share count", true),
        ("We represent the seller here", true),
        ("We prefer the exit-multiple method for terminal value", true),
        ("Our policy is to footnote every non-GAAP adjustment", true),
        ("Strip out FX gains before computing margins", true),
        ("Keep the peer set to direct competitors only", true),
        ("Present figures net of non-controlling interest", true),
        ("What's NVDA's data-center revenue?", false),
        ("Build a three-statement model for META", false),
        ("Compare margins across the peer set", false),
        ("Pull the latest 8-K", false),
        ("Show me the balance sheet", false),
        ("List the comparable companies", false),
        ("How is goodwill impaired?", false),
        ("Why did the tax rate jump?", false),
        ("Which discount rate did you use?", false),
        ("Perfect, thanks", false),
        ("The preferred equity converts at $20", false),
        ("What's the coupon on the preferred?", false),
        ("Can you default to LTM here?", false),
        ("Show me figures in millions", false),
        ("Give me the terminal value", false),
        ("Run the sensitivity table", false),
        ("Fetch AMD's quote", false),
        ("Get the operating margin trend", false),
        ("Analyze the liquidity position", false),
        ("Summarize the earnings call", false),
        ("Download the proxy statement", false),
        ("Open the assumptions tab", false),
        ("Calculate levered FCF", false),
        ("It always takes forever to load", false),
        ("I used forward multiples on that one", false),
        ("We used the exit multiple last time", false),
        ("Does the model cover FY26?", false),
        ("Is there a debt schedule?", false),
        ("Could you re-run at 9%?", false),
        ("Should I use median or mean?", false),
        ("When does the lockup expire?", false),
        ("never mind that", false),
        ("Remind me of our base currency", false),
        ("Tell me which method we prefer", false),
        ("the key is sk-live-99887766", false),
        ("file path is D:\\deals\\model.xlsx", false),
        ("Include operating leases as debt", false),
        ("Use forward estimates", false),
        ("Set terminal growth to 2.5%", false),
        ("I meant right now, not later", false),
        ("We use it constantly, love it", false),
    ];

    #[test]
    fn auto_capture_eval() {
        let (tp, fp, tn, fneg) = score(HELDOUT);
        let precision = tp as f64 / (tp + fp).max(1) as f64;
        let recall = tp as f64 / (tp + fneg).max(1) as f64;
        let (dtp, dfp, _dtn, dfneg) = score(DEV);
        eprintln!(
            "[auto_capture_eval] DEV n={} P={:.3} R={:.3} (tp={dtp} fp={dfp} fn={dfneg})",
            DEV.len(),
            dtp as f64 / (dtp + dfp).max(1) as f64,
            dtp as f64 / (dtp + dfneg).max(1) as f64,
        );
        eprintln!(
            "[auto_capture_eval] HELDOUT n={} P={precision:.3} R={recall:.3} (tp={tp} fp={fp} tn={tn} fn={fneg})",
            HELDOUT.len(),
        );
        // TARGET gate (plan decision 4): >=98% precision, >=90% recall.
        // MEASURED (keyword-rules classifier, held-out): P~0.87, R~0.84 — BELOW
        // the gate. Conclusion: hand-written keyword rules do NOT clear the
        // precision bar, so automatic capture stays OFF; clearing it needs a
        // model-based classifier evaluated on representative production turns,
        // not rules. This asserts a regression floor + records the measurement;
        // it is deliberately NOT a claim that the gate passes.
        assert!(precision >= 0.80, "held-out precision regressed below floor: {precision:.3}");
        assert!(recall >= 0.78, "held-out recall regressed below floor: {recall:.3}");
    }
}
