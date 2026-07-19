//! Collector adapter (Phase 2.3) — map `fm_fetch` search/read outputs into the
//! collector's [`Candidate`] / [`ReadOutcome`] types, preserving backend, error,
//! and final-URL provenance instead of silently downgrading.
//!
//! Pure and offline-testable: it consumes already-fetched `fm_fetch` values
//! (`WebHit`, `FetchedPage`, `PageStatus`) — the network I/O stays in the
//! app/driver, which calls these mappers on each result. A fetch *error*
//! (reqwest `Err`) becomes a `Failed` record via [`read_outcome_failed`], never a
//! fabricated read.

use fm_fetch::{FetchedPage, PageStatus, WebHit};

use crate::collect::{Candidate, ReadOutcome};
use crate::research::{SourceBackend, SourceKind, SourceRecord, SourceStatus};

/// Classify a URL's evidentiary tier from its host. Regulators rank highest,
/// then company investor-relations pages, then independent newswires, then
/// generic secondary sources. Conservative: unknown hosts are `Secondary`.
pub fn classify_source_kind(url: &str) -> SourceKind {
    let host = url::Url::parse(url.trim())
        .ok()
        .and_then(|u| u.host_str().map(|h| h.to_ascii_lowercase()));
    let Some(host) = host else {
        return SourceKind::Secondary;
    };

    // US, EU, UK, and the major Asian/Commonwealth disclosure venues — a
    // non-US issuer's filings live on its exchange/regulator archive, not
    // EDGAR (HKEX news, Japan's EDINET/TDnet, London RNS, Euronext, SEDAR+…).
    const REGULATORS: &[&str] = &[
        "sec.gov",
        "sec.report",
        "europa.eu",
        "gov.uk",
        "fca.org.uk",
        "hkexnews.hk",
        "jpx.co.jp",
        "edinet-fsa.go.jp",
        "release.tdnet.info",
        "londonstockexchange.com",
        "euronext.com",
        "bundesanzeiger.de",
        "amf-france.org",
        "borsaitaliana.it",
        "sedarplus.ca",
        "asx.com.au",
        "sgx.com",
        "nseindia.com",
        "bseindia.com",
    ];
    const NEWSWIRES: &[&str] = &[
        "reuters.com",
        "bloomberg.com",
        "apnews.com",
        "ft.com",
        "wsj.com",
        "cnbc.com",
    ];
    // Paid press-release distribution: the text is WRITTEN BY THE COMPANY
    // (earnings releases, deal announcements) — issuer-primary evidence, not
    // independent journalism. Ranks above newswires, below the company's own
    // site.
    const PR_DISTRIBUTORS: &[&str] = &[
        "businesswire.com",
        "prnewswire.com",
        "globenewswire.com",
        "newsfilecorp.com",
        "accesswire.com",
    ];
    if REGULATORS
        .iter()
        .any(|d| host == *d || host.ends_with(&format!(".{d}")))
    {
        return SourceKind::Regulatory;
    }
    // Company-authored content: IR/press/newsroom subdomains, or IR/press
    // sections of the corporate site. The company's own words are the first
    // source of truth after the regulator.
    const COMPANY_SUBDOMAINS: &[&str] = &["investor.", "ir.", "press.", "news.", "media."];
    const COMPANY_PATHS: &[&str] = &[
        "/investor-relations",
        "/investors",
        "/investor",
        "/press-release",
        "/press-releases",
        "/press",
        "/newsroom",
        "/news-release",
        "/news-releases",
        "/media-center",
        "/ir/",
    ];
    let path = url::Url::parse(url.trim())
        .map(|u| u.path().to_ascii_lowercase())
        .unwrap_or_default();
    if COMPANY_SUBDOMAINS.iter().any(|p| host.starts_with(p))
        || host.contains(".investor.")
        || COMPANY_PATHS.iter().any(|p| path.contains(p))
    {
        return SourceKind::Company;
    }
    if PR_DISTRIBUTORS
        .iter()
        .any(|d| host == *d || host.ends_with(&format!(".{d}")))
    {
        return SourceKind::Primary;
    }
    // Earnings-call transcripts: the text is management speaking, regardless
    // of which site carries it — issuer-primary evidence, same tier as the
    // company's PR-wire releases.
    if path.contains("transcript") {
        return SourceKind::Primary;
    }
    if NEWSWIRES
        .iter()
        .any(|d| host == *d || host.ends_with(&format!(".{d}")))
    {
        return SourceKind::Newswire;
    }
    SourceKind::Secondary
}

/// Map a search hit into a pre-read [`Candidate`], classifying its kind from the
/// URL and tagging the originating backend.
pub fn candidate_from_web_hit(hit: &WebHit, backend: SourceBackend) -> Candidate {
    Candidate {
        url: hit.url.clone(),
        title: hit.title.clone(),
        kind: classify_source_kind(&hit.url),
        backend,
        snippet: if hit.snippet.trim().is_empty() {
            None
        } else {
            Some(hit.snippet.clone())
        },
    }
}

/// Map an EDGAR [`Filing`](fm_fetch::edgar::Filing) into a pre-read [`Candidate`].
/// Filings are always `Regulatory` (highest evidentiary tier); the title carries
/// form/date/period metadata and the snippet the accession number, so the source
/// is self-describing before its body is fetched.
pub fn candidate_from_filing(f: &fm_fetch::edgar::Filing) -> Candidate {
    Candidate {
        url: f.url.clone(),
        title: format!(
            "{} filed {} (period {})",
            f.form_type, f.filing_date, f.fiscal_period_end
        ),
        kind: SourceKind::Regulatory,
        backend: SourceBackend::BasicHttp,
        snippet: Some(format!(
            "Form {} · filed {} · accession {}",
            f.form_type, f.filing_date, f.accession_number
        )),
    }
}

/// Choose the filing excerpt most relevant to `question`: split the body into
/// EDGAR items and concatenate them best-first (question-keyword hits, plus a
/// large boost when the question names the item), capped at `max` chars.
/// Splitting drops cover-page/TOC boilerplate so synthesis quotes real item
/// text; an unsplittable body falls back to its capped head.
pub fn select_filing_excerpt(text: &str, question: &str, max: usize) -> String {
    let items = fm_fetch::split_filing_items(text);
    if items.is_empty() {
        return text.chars().take(max).collect();
    }
    let q = question.to_lowercase();
    let qwords: Vec<&str> = q.split_whitespace().filter(|w| w.len() >= 4).collect();
    let mut scored: Vec<(usize, &str, &str)> = items
        .iter()
        .map(|(id, body)| {
            let bl = body.to_lowercase();
            let mut s = qwords.iter().filter(|w| bl.contains(**w)).count();
            if q.contains(&format!("item {}", id.to_lowercase())) {
                s += 10;
            }
            (s, id.as_str(), body.as_str())
        })
        .collect();
    // Stable sort keeps filing order among equal scores.
    scored.sort_by(|a, b| b.0.cmp(&a.0));
    let mut out = String::new();
    for (_s, id, body) in scored {
        out.push_str(&format!("Item {id}: {}\n\n", body.trim()));
        if out.chars().count() >= max {
            break;
        }
    }
    out.chars().take(max).collect()
}

/// True if `url` is an EDGAR *Archives* filing-document URL (`www.sec.gov/Archives/edgar/...`).
/// Such sources are read with the EDGAR client (`fetch_filing_doc`); everything
/// else uses the generic page reader. Lets one ledger mix filings and web pages.
pub fn is_edgar_archive_url(url: &str) -> bool {
    url::Url::parse(url.trim())
        .ok()
        .and_then(|u| {
            u.host_str()
                .map(|h| h.to_ascii_lowercase())
                .map(|h| (h, u.path().to_ascii_lowercase()))
        })
        .map(|(host, path)| {
            (host == "sec.gov" || host.ends_with(".sec.gov")) && path.contains("/archives/edgar/")
        })
        .unwrap_or(false)
}

/// Build a pre-read (`Read`) synthetic [`SourceRecord`] whose excerpt is supplied
/// directly — used for structured data (a market quote, XBRL facts) that is not a
/// fetched page. The driver's read stage leaves an already-`Read` record with an
/// excerpt untouched, so the citation/quote rules apply to it unchanged.
pub fn synthetic_source(
    id: impl Into<String>,
    url: impl Into<String>,
    title: impl Into<String>,
    excerpt: impl Into<String>,
    kind: SourceKind,
    retrieved_at: impl Into<String>,
) -> SourceRecord {
    let url = url.into();
    let domain = crate::collect::domain_of(&url).unwrap_or_default();
    let canonical_url = crate::collect::canonicalize_url(&url).unwrap_or_else(|| url.clone());
    SourceRecord {
        id: id.into(),
        requested_url: url.clone(),
        final_url: Some(url),
        canonical_url,
        title: title.into(),
        domain,
        retrieved_at: retrieved_at.into(),
        status: SourceStatus::Read,
        kind,
        backend: SourceBackend::BasicHttp,
        snippet: None,
        excerpt: Some(excerpt.into()),
        error_code: None,
    }
}

/// Map a fetched page into a [`ReadOutcome`], preserving the resolved final URL
/// and capping the retained excerpt. `Ok` becomes `Read` (with excerpt); a
/// blocked/thin page keeps its status and a diagnostic code — never a fake read.
pub fn read_outcome_from_page(
    page: &FetchedPage,
    final_url: Option<String>,
    retrieved_at: impl Into<String>,
    max_excerpt: usize,
) -> ReadOutcome {
    let (status, excerpt, error_code) = match page.status {
        PageStatus::Ok => {
            let text = page.text.trim();
            if text.is_empty() {
                (SourceStatus::Thin, None, Some("empty".to_string()))
            } else {
                (SourceStatus::Read, Some(cap(text, max_excerpt)), None)
            }
        }
        PageStatus::Blocked => (SourceStatus::Blocked, None, Some("blocked".to_string())),
        PageStatus::Thin => (SourceStatus::Thin, None, Some("thin".to_string())),
    };
    ReadOutcome {
        status,
        final_url,
        retrieved_at: retrieved_at.into(),
        excerpt,
        error_code,
    }
}

/// A read that could not be performed (a fetch/transport error, a rejected
/// redirect, an SSRF block). Marked `Failed` with the given diagnostic code —
/// the provenance is preserved, not silently dropped.
pub fn read_outcome_failed(
    final_url: Option<String>,
    retrieved_at: impl Into<String>,
    error_code: impl Into<String>,
) -> ReadOutcome {
    ReadOutcome {
        status: SourceStatus::Failed,
        final_url,
        retrieved_at: retrieved_at.into(),
        excerpt: None,
        error_code: Some(error_code.into()),
    }
}

fn cap(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        s.chars().take(max).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_source_kinds_by_host() {
        assert_eq!(
            classify_source_kind("https://www.sec.gov/cgi-bin/x"),
            SourceKind::Regulatory
        );
        assert_eq!(
            classify_source_kind("https://investor.nvidia.com/home"),
            SourceKind::Company
        );
        assert_eq!(
            classify_source_kind("https://www.reuters.com/story"),
            SourceKind::Newswire
        );
        // Company press/newsroom pages on the main domain are issuer-primary.
        assert_eq!(
            classify_source_kind("https://www.tesla.com/press-release/q1-2026"),
            SourceKind::Company
        );
        assert_eq!(
            classify_source_kind("https://www.apple.com/newsroom/2026/07/results/"),
            SourceKind::Company
        );
        // PR distribution carries the company's own words — Primary, above
        // independent newswires.
        assert_eq!(
            classify_source_kind("https://www.businesswire.com/news/home/tsla-q1"),
            SourceKind::Primary
        );
        // International disclosure venues are regulators.
        assert_eq!(
            classify_source_kind("https://www1.hkexnews.hk/listedco/listconews/sehk/2026/0325/doc.pdf"),
            SourceKind::Regulatory
        );
        assert_eq!(
            classify_source_kind("https://www.londonstockexchange.com/news-article/SHEL/q1-results/123"),
            SourceKind::Regulatory
        );
        // Transcript carriers hold management's spoken words — Primary too.
        assert_eq!(
            classify_source_kind(
                "https://www.fool.com/earnings/call-transcripts/2026/04/23/tesla-tsla-q1-2026-earnings-call-transcript/"
            ),
            SourceKind::Primary
        );
        assert_eq!(
            classify_source_kind("https://www.investing.com/equities/tesla-motors-earnings-transcript"),
            SourceKind::Primary
        );
        assert_eq!(
            classify_source_kind("https://someblog.example/post"),
            SourceKind::Secondary
        );
        assert_eq!(classify_source_kind("not a url"), SourceKind::Secondary);
    }

    #[test]
    fn web_hit_maps_to_candidate() {
        let hit = WebHit {
            title: "NVDA 10-K".into(),
            url: "https://www.sec.gov/filing".into(),
            snippet: "annual report".into(),
        };
        let c = candidate_from_web_hit(&hit, SourceBackend::BasicHttp);
        assert_eq!(c.kind, SourceKind::Regulatory);
        assert_eq!(c.backend, SourceBackend::BasicHttp);
        assert_eq!(c.snippet.as_deref(), Some("annual report"));
    }

    #[test]
    fn ok_page_becomes_read_with_capped_excerpt() {
        let page = FetchedPage {
            title: "t".into(),
            text: "Revenue grew.".into(),
            status: PageStatus::Ok,
        };
        let o = read_outcome_from_page(&page, Some("https://x/final".into()), "t0", 1000);
        assert_eq!(o.status, SourceStatus::Read);
        assert_eq!(o.excerpt.as_deref(), Some("Revenue grew."));
        assert_eq!(o.final_url.as_deref(), Some("https://x/final"));
        assert_eq!(o.error_code, None);
    }

    #[test]
    fn empty_ok_page_is_thin_not_read() {
        let page = FetchedPage {
            title: "t".into(),
            text: "   ".into(),
            status: PageStatus::Ok,
        };
        let o = read_outcome_from_page(&page, None, "t0", 1000);
        assert_eq!(o.status, SourceStatus::Thin);
        assert_eq!(o.excerpt, None);
    }

    #[test]
    fn blocked_and_thin_preserve_status() {
        let blocked = FetchedPage {
            title: String::new(),
            text: String::new(),
            status: PageStatus::Blocked,
        };
        assert_eq!(
            read_outcome_from_page(&blocked, None, "t", 100).status,
            SourceStatus::Blocked
        );
        let thin = FetchedPage {
            title: String::new(),
            text: "x".into(),
            status: PageStatus::Thin,
        };
        assert_eq!(
            read_outcome_from_page(&thin, None, "t", 100).status,
            SourceStatus::Thin
        );
    }

    #[test]
    fn fetch_error_becomes_failed_with_code() {
        let o = read_outcome_failed(Some("https://x".into()), "t", "ssrf_blocked");
        assert_eq!(o.status, SourceStatus::Failed);
        assert_eq!(o.error_code.as_deref(), Some("ssrf_blocked"));
    }

    #[test]
    fn excerpt_is_capped() {
        let page = FetchedPage {
            title: "t".into(),
            text: "x".repeat(5000),
            status: PageStatus::Ok,
        };
        let o = read_outcome_from_page(&page, None, "t", 2000);
        assert_eq!(o.excerpt.unwrap().chars().count(), 2000);
    }

    #[test]
    fn filing_becomes_regulatory_candidate_with_metadata() {
        let f = fm_fetch::edgar::Filing {
            form_type: "10-K".into(),
            filing_date: "2026-02-21".into(),
            fiscal_period_end: "2026-01-26".into(),
            url: "https://www.sec.gov/Archives/edgar/data/1045810/000104581026000123/nvda-10k.htm"
                .into(),
            cik: "0001045810".into(),
            accession_number: "0001045810-26-000123".into(),
        };
        let c = candidate_from_filing(&f);
        assert_eq!(c.kind, SourceKind::Regulatory);
        assert_eq!(c.backend, SourceBackend::BasicHttp);
        assert!(c.title.contains("10-K") && c.title.contains("2026-02-21"));
        assert!(c.snippet.unwrap().contains("0001045810-26-000123"));
        assert!(c.url.contains("sec.gov"));
    }

    #[test]
    fn select_filing_excerpt_prefers_question_relevant_items() {
        let filing = "Item 1. Business\nWe make GPUs and networking gear.\nItem 1A. Risk Factors\nCompetition from custom AI chips and export restrictions to China are key risks.\nItem 7. MD&A\nData center revenue grew significantly year over year.\n";
        let excerpt = select_filing_excerpt(filing, "What are the main risk factors?", 4000);
        let risk = excerpt
            .find("Competition from custom AI chips")
            .expect("risk item present");
        let biz = excerpt.find("We make GPUs").expect("business item present");
        assert!(
            risk < biz,
            "the risk-factors item should rank before business"
        );
    }

    #[test]
    fn select_filing_excerpt_falls_back_to_capped_head_when_unsplittable() {
        let plain = "x".repeat(9000);
        assert_eq!(select_filing_excerpt(&plain, "q", 100).chars().count(), 100);
    }

    #[test]
    fn edgar_archive_urls_are_detected_web_urls_are_not() {
        assert!(is_edgar_archive_url(
            "https://www.sec.gov/Archives/edgar/data/1045810/000104581026000021/nvda-20260125.htm"
        ));
        // Non-archive sec.gov (submissions API) and generic web are not filing docs.
        assert!(!is_edgar_archive_url(
            "https://data.sec.gov/submissions/CIK0001045810.json"
        ));
        assert!(!is_edgar_archive_url("https://www.cnbc.com/nvidia"));
        assert!(!is_edgar_archive_url("not a url"));
    }

    #[test]
    fn synthetic_source_is_pre_read_with_excerpt_and_derived_domain() {
        let s = synthetic_source(
            "S7",
            "https://finance.yahoo.com/quote/NVDA",
            "Market quote",
            "NVDA 180.00 USD as of 2026-07-16",
            SourceKind::Secondary,
            "2026-07-16",
        );
        assert_eq!(s.id, "S7");
        assert_eq!(s.status, SourceStatus::Read);
        assert_eq!(s.domain, "finance.yahoo.com");
        assert_eq!(
            s.excerpt.as_deref(),
            Some("NVDA 180.00 USD as of 2026-07-16")
        );
        assert_eq!(s.kind, SourceKind::Secondary);
    }
}
