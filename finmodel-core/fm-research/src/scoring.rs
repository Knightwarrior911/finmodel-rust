//! Research URL/text scoring helpers, ported verbatim from `src/research/news.py`.
//! Used by the web-search ranking (Phase 8) and the M&A agent (Phase 9).

/// Domains never worth reading (social, search chrome, encyclopedic).
const SKIP_DOMAINS: &[&str] = &[
    "gstatic.com",
    "googleapis.com",
    "youtube.com",
    "facebook.com",
    "twitter.com",
    "instagram.com",
    "maps.google",
    "translate.google",
    "accounts.google",
    "google.com/search",
    "google.com/webhp",
    "google.com/intl",
    "google.com/sorry",
    "google.com/preferences",
    "reddit.com",
    "quora.com",
    "wikipedia.org",
    "bing.com/images",
    "bing.com/videos",
    "bing.com/maps",
    "bing.com/news?",
    "bing.com/search",
    "bing.com/ck",
    "duckduckgo.com",
];

/// High-signal financial/newswire/deal sources, floated to the top.
pub const PRIORITY_DOMAINS: &[&str] = &[
    "businesswire.com",
    "prnewswire.com",
    "globenewswire.com",
    "reuters.com",
    "bloomberg.com",
    "ft.com",
    "wsj.com",
    "sec.gov",
    "aircargonews.net",
    "freightwaves.com",
    "logisticsmgmt.com",
    "supplychaindive.com",
    "joc.com",
    "pitchbook.com",
    "preqin.com",
    "mergermarket.com",
    "yahoo.com",
    "marketwatch.com",
    // Earnings-call transcript carriers (management's words, free full text).
    "fool.com",
    "investing.com",
    // Major local newswires — the strongest independent press for non-US
    // issuers (a Nikkei or Handelsblatt story is the FT of its market).
    "nikkei.com",
    "asia.nikkei.com",
    "handelsblatt.com",
    "lesechos.fr",
    "economictimes.indiatimes.com",
    "caixinglobal.com",
    "koreaherald.com",
    "globes.co.il",
    // International disclosure venues (non-US issuers file here, not EDGAR).
    "hkexnews.hk",
    "londonstockexchange.com",
    "euronext.com",
    "edinet-fsa.go.jp",
    "sedarplus.ca",
    "asx.com.au",
];

/// M&A keywords; ≥2 present ⇒ text likely covers a deal.
const DEAL_KW: &[&str] = &[
    "acquisition",
    "acquired",
    "merger",
    "stake",
    "deal",
    "transaction",
    "equity",
    "buyout",
    "partnership",
    "announced",
];

/// Filter out skip-domains, then float priority-domains to the front (stable
/// within each group). Mirrors Python `_rank_urls`.
pub fn rank_urls(urls: &[String]) -> Vec<String> {
    let filtered: Vec<&String> = urls
        .iter()
        .filter(|u| !SKIP_DOMAINS.iter().any(|d| u.contains(d)))
        .collect();
    let (priority, rest): (Vec<&String>, Vec<&String>) = filtered
        .into_iter()
        .partition(|u| PRIORITY_DOMAINS.iter().any(|d| u.contains(d)));
    priority.into_iter().chain(rest).cloned().collect()
}

/// True when ≥2 deal keywords appear in `text` (case-insensitive).
/// Mirrors Python `_has_deal_content`.
pub fn has_deal_content(text: &str) -> bool {
    let t = text.to_lowercase();
    DEAL_KW.iter().filter(|kw| t.contains(**kw)).count() >= 2
}

/// A deal summary is "sufficient" to stop the research cascade. Mirrors Python
/// `_is_sufficient` (params extracted from the summary dict).
pub fn is_sufficient(
    announced: bool,
    deal_value_disclosed: bool,
    acquirer: bool,
    strategic_rationale: bool,
    sources_count: usize,
) -> bool {
    (announced && (deal_value_disclosed || (acquirer && strategic_rationale))) || sources_count >= 3
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn rank_urls_drops_skip_and_floats_priority() {
        let urls = v(&[
            "https://reddit.com/r/x",       // skip
            "https://example.com/article",  // rest
            "https://www.reuters.com/deal", // priority
            "https://youtube.com/watch",    // skip
            "https://businesswire.com/pr",  // priority
        ]);
        let ranked = rank_urls(&urls);
        assert_eq!(
            ranked,
            v(&[
                "https://www.reuters.com/deal",
                "https://businesswire.com/pr",
                "https://example.com/article",
            ])
        );
    }

    #[test]
    fn has_deal_content_needs_two_keywords() {
        assert!(has_deal_content(
            "The acquisition and merger were announced"
        ));
        assert!(!has_deal_content("A single acquisition rumor")); // only 1 kw
        assert!(!has_deal_content("nothing relevant here"));
    }

    #[test]
    fn is_sufficient_rules() {
        assert!(is_sufficient(true, true, false, false, 0)); // date + value
        assert!(is_sufficient(true, false, true, true, 0)); // date + acquirer + rationale
        assert!(!is_sufficient(true, false, true, false, 0)); // date + acquirer only
        assert!(is_sufficient(false, false, false, false, 3)); // ≥3 sources
        assert!(!is_sufficient(false, false, false, false, 2));
    }
}
