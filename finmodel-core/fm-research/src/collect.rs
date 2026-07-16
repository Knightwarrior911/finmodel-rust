//! Bounded source collector — pure core (Phase 2.3).
//!
//! Turns raw search candidates into the ranked, deduped, budget-capped source
//! ledger the reducer reads. The deterministic policy lives here; the thin
//! adapter that maps `fm_fetch::{WebHit,FetchedPage,PageStatus}` and `fm_mcp`
//! outputs into [`Candidate`]s (preserving backend/error/final-URL provenance)
//! is the app/driver-side I/O layer that calls [`assemble_ledger`].
//!
//! Policy: merge canonical URLs (first wins), rank regulatory/issuer/primary
//! before independent newswires before generic secondary, cap two per domain,
//! then truncate to the source budget and assign STABLE `S1…` ids — which never
//! change once reads begin. Reads later overwrite each record's status/excerpt
//! via [`apply_read`].

use std::collections::{HashMap, HashSet};

use url::Url;

use crate::research::{SourceBackend, SourceKind, SourceRecord, SourceStatus};

/// A pre-read search candidate (from a search backend), before any page fetch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub url: String,
    pub title: String,
    pub kind: SourceKind,
    pub backend: SourceBackend,
    pub snippet: Option<String>,
}

/// Canonicalize a URL for dedupe: lowercase scheme+host, drop userinfo, port
/// default is kept as-is by `url`, strip the fragment and query, and trim a
/// trailing slash. Returns `None` for an unparseable/hostless URL.
pub fn canonicalize_url(url: &str) -> Option<String> {
    let u = Url::parse(url.trim()).ok()?;
    let host = u.host_str()?.to_ascii_lowercase();
    let scheme = u.scheme().to_ascii_lowercase();
    let mut path = u.path().trim_end_matches('/').to_string();
    if path.is_empty() {
        path = "/".to_string();
    }
    match u.port() {
        Some(p) => Some(format!("{scheme}://{host}:{p}{path}")),
        None => Some(format!("{scheme}://{host}{path}")),
    }
}

/// The registrable-ish domain (host, lowercased) for the per-domain cap.
pub fn domain_of(url: &str) -> Option<String> {
    Url::parse(url.trim())
        .ok()?
        .host_str()
        .map(|h| h.to_ascii_lowercase())
}

/// Assemble the ranked, deduped, capped ledger with stable `S#` ids.
///
/// Candidates enter as `Failed`/`unread`; the read stage overwrites each
/// record's status/excerpt/final_url via [`apply_read`]. Ranking is a STABLE
/// sort by [`SourceKind::rank`] (input order preserved within a rank), so the
/// output is fully deterministic.
pub fn assemble_ledger(
    candidates: Vec<Candidate>,
    max_sources: u32,
    max_per_domain: u32,
) -> Vec<SourceRecord> {
    // 1. Dedupe by canonical URL (first occurrence wins); drop unparseable.
    let mut seen: HashSet<String> = HashSet::new();
    let mut deduped: Vec<(Candidate, String, String)> = Vec::new();
    for c in candidates {
        let Some(canon) = canonicalize_url(&c.url) else {
            continue;
        };
        if !seen.insert(canon.clone()) {
            continue;
        }
        let domain = domain_of(&c.url).unwrap_or_default();
        deduped.push((c, canon, domain));
    }

    // 2. Stable rank: regulatory/company/primary, then newswire, then secondary.
    deduped.sort_by_key(|(c, _, _)| c.kind.rank());

    // 3. Per-domain cap, then the overall source budget.
    let mut per_domain: HashMap<String, u32> = HashMap::new();
    let mut kept: Vec<(Candidate, String, String)> = Vec::new();
    for (c, canon, domain) in deduped {
        let count = per_domain.entry(domain.clone()).or_insert(0);
        if *count >= max_per_domain {
            continue;
        }
        *count += 1;
        kept.push((c, canon, domain));
        if kept.len() >= max_sources as usize {
            break;
        }
    }

    // 4. Assign stable S# ids in final order.
    kept.into_iter()
        .enumerate()
        .map(|(i, (c, canon, domain))| SourceRecord {
            id: format!("S{}", i + 1),
            requested_url: c.url,
            final_url: None,
            canonical_url: canon,
            title: c.title,
            domain,
            retrieved_at: String::new(),
            status: SourceStatus::Failed,
            kind: c.kind,
            backend: c.backend,
            snippet: c.snippet,
            excerpt: None,
            error_code: Some("unread".to_string()),
        })
        .collect()
}

/// The outcome of reading one source (from the fetch adapter).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReadOutcome {
    pub status: SourceStatus,
    pub final_url: Option<String>,
    pub retrieved_at: String,
    pub excerpt: Option<String>,
    pub error_code: Option<String>,
}

/// Overwrite a ledger record (matched by `id`) with a read outcome, preserving
/// its stable id, canonical URL, kind, and backend provenance. Returns whether a
/// record matched.
pub fn apply_read(ledger: &mut [SourceRecord], id: &str, outcome: ReadOutcome) -> bool {
    if let Some(r) = ledger.iter_mut().find(|r| r.id == id) {
        r.status = outcome.status;
        r.final_url = outcome.final_url;
        r.retrieved_at = outcome.retrieved_at;
        r.excerpt = outcome.excerpt;
        r.error_code = outcome.error_code;
        true
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cand(url: &str, kind: SourceKind) -> Candidate {
        Candidate {
            url: url.into(),
            title: url.into(),
            kind,
            backend: SourceBackend::BasicHttp,
            snippet: None,
        }
    }

    #[test]
    fn canonicalization_dedupes_fragment_slash_and_case() {
        assert_eq!(
            canonicalize_url("https://Example.com/Path/#frag"),
            canonicalize_url("https://example.com/Path")
        );
        assert_eq!(
            canonicalize_url("https://example.com/a?utm=1"),
            canonicalize_url("https://example.com/a")
        );
        assert!(canonicalize_url("not a url").is_none());
    }

    #[test]
    fn dedupe_keeps_first_occurrence() {
        let cands = vec![
            cand("https://ex.com/a", SourceKind::Newswire),
            cand("https://ex.com/a/#x", SourceKind::Secondary), // same canonical
            cand("https://ex.com/b", SourceKind::Newswire),
        ];
        let led = assemble_ledger(cands, 10, 10);
        assert_eq!(led.len(), 2);
        assert_eq!(led[0].canonical_url, "https://ex.com/a");
    }

    #[test]
    fn ranking_puts_regulatory_first() {
        let cands = vec![
            cand("https://blog.example/post", SourceKind::Secondary),
            cand("https://sec.gov/filing", SourceKind::Regulatory),
            cand("https://reuters.com/story", SourceKind::Newswire),
        ];
        let led = assemble_ledger(cands, 10, 10);
        assert_eq!(led[0].id, "S1");
        assert_eq!(led[0].kind, SourceKind::Regulatory);
        assert_eq!(led[1].kind, SourceKind::Newswire);
        assert_eq!(led[2].kind, SourceKind::Secondary);
    }

    #[test]
    fn caps_two_per_domain() {
        let cands = vec![
            cand("https://one.com/a", SourceKind::Newswire),
            cand("https://one.com/b", SourceKind::Newswire),
            cand("https://one.com/c", SourceKind::Newswire),
            cand("https://two.com/a", SourceKind::Newswire),
        ];
        let led = assemble_ledger(cands, 10, 2);
        let one_count = led.iter().filter(|r| r.domain == "one.com").count();
        assert_eq!(one_count, 2, "domain capped at two");
        assert_eq!(led.len(), 3);
    }

    #[test]
    fn truncates_to_source_budget() {
        let cands: Vec<Candidate> = (0..8)
            .map(|i| cand(&format!("https://d{i}.com/x"), SourceKind::Newswire))
            .collect();
        let led = assemble_ledger(cands, 6, 2);
        assert_eq!(led.len(), 6);
        // Ids are stable S1..S6 in final order.
        assert_eq!(
            led.iter().map(|r| r.id.clone()).collect::<Vec<_>>(),
            vec!["S1", "S2", "S3", "S4", "S5", "S6"]
        );
    }

    #[test]
    fn candidates_enter_as_unread_then_read_overwrites() {
        let mut led = assemble_ledger(vec![cand("https://ex.com/a", SourceKind::Newswire)], 10, 10);
        assert_eq!(led[0].status, SourceStatus::Failed);
        assert_eq!(led[0].error_code.as_deref(), Some("unread"));
        let ok = apply_read(
            &mut led,
            "S1",
            ReadOutcome {
                status: SourceStatus::Read,
                final_url: Some("https://ex.com/a".into()),
                retrieved_at: "2026-01-01T00:00:00Z".into(),
                excerpt: Some("body".into()),
                error_code: None,
            },
        );
        assert!(ok);
        assert_eq!(led[0].status, SourceStatus::Read);
        assert_eq!(led[0].excerpt.as_deref(), Some("body"));
        assert_eq!(led[0].error_code, None);
        // Stable id preserved across the read.
        assert_eq!(led[0].id, "S1");
    }
}
