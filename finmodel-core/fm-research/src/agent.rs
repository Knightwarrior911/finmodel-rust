//! M&A research orchestrator (Phase 9) — the natural-language deal-research
//! agent ported from `src/research/agent.py` + `router.py` + `deal_synthesis.py`.
//!
//! Pure analytics (query routing, target/acquirer parsing, regex deal
//! synthesis, the sufficiency stop-condition) are unit-tested; the live
//! `run_deal_research` loop drives the Phase-8 web facade (Roam MCP or the HTTP
//! fallback) and is `#[ignore]`-tested (network + subprocess).

use std::collections::HashMap;

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::scoring::has_deal_content;
use crate::web;

/// The kind of IB research question (mirrors Python `QueryType`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum QueryType {
    EarningsAnalysis,
    SynergyRealization,
    BeneficialOwnership,
    DebtMaturitySchedule,
    RegulatoryApprovalStatus,
    EarningsEstimateConsensus,
    TransactionTerms,
    GeneralCompanyIntelligence,
}

/// Trigger phrases per query type — ported verbatim from `router.TRIGGER_MAP`.
/// Order is preserved (max-score ties resolve to the earliest type).
const TRIGGER_MAP: &[(QueryType, &[&str])] = &[
    (
        QueryType::EarningsAnalysis,
        &[
            "q1 20", "q2 20", "q3 20", "q4 20", "fy20", "financial results",
            "earnings", "quarterly results", "bank results", "earnings release",
        ],
    ),
    (
        QueryType::SynergyRealization,
        &[
            "synerg", "integration", "run-rate", "cost saving", "dis-synerg",
            "realized vs expected", "synergy target",
        ],
    ),
    (
        QueryType::BeneficialOwnership,
        &[
            "owns", "ownership", "shareholder", "stake", "beneficial", "holder",
            "investor >5%", "major shareholder", "promoter", "who owns",
        ],
    ),
    (
        QueryType::DebtMaturitySchedule,
        &[
            "debt maturity", "debt schedule", "borrowing", "leverage", "covenant",
            "credit facility", "notes outstanding", "term loan", "revolver",
            "debt structure",
        ],
    ),
    (
        QueryType::RegulatoryApprovalStatus,
        &[
            "regulatory approval", "antitrust", "merger control", "cci", "doj",
            "cleared", "condition", "regulatory condition",
        ],
    ),
    (
        QueryType::EarningsEstimateConsensus,
        &[
            "estimate", "consensus", "earnings forecast", "revenue forecast",
            "eps forecast", "analyst estimate", "sell-side",
        ],
    ),
    (
        QueryType::TransactionTerms,
        &[
            "deal terms", "acquisition terms", "purchase price", "consideration",
            "earnout", "valuation multiple", "m&a deal", "transaction details",
            "deal announced", "acquisition", "acquired", "acquir", "merger",
            "takeover", "buyout", "majority stake", "controlling stake",
            "strategic partnership", "private equity deal", "pe deal", "sold to",
            "bought by", "deal analysis", "transaction analysis", "deal close",
        ],
    ),
    (
        QueryType::GeneralCompanyIntelligence,
        &[
            "about", "profile", "business description", "competitors",
            "market position", "product portfolio", "customer base",
            "company overview", "what does", "who is",
        ],
    ),
];

/// Detect the query type by trigger-phrase scoring (Python `detect_query_type`).
pub fn detect_query_type(user_query: &str) -> QueryType {
    let q = user_query.to_lowercase();
    let mut best: Option<(QueryType, usize)> = None;
    for (qtype, triggers) in TRIGGER_MAP {
        let score = triggers.iter().filter(|t| q.contains(**t)).count();
        if score > 0 && best.map_or(true, |(_, b)| score > b) {
            best = Some((*qtype, score));
        }
    }
    best.map(|(t, _)| t)
        .unwrap_or(QueryType::GeneralCompanyIntelligence)
}

/// Where a company likely files, from name patterns (Python `detect_listing_type`).
pub fn detect_listing_type(company_name: &str) -> &'static str {
    let lc = company_name.to_lowercase();
    const INDIA: &[&str] = &[
        "hdfc", "icici", "sbi", "kotak", "axis", "reliance", "tcs", "infosys",
        "wipro", "hcl tech", "asian paints", "berger", "tata", "mahindra",
        "bharti", "adani", "hindustan unilever", "itc", "ntpc", "ongc",
    ];
    if INDIA.iter().any(|p| lc.contains(p)) {
        return "india";
    }
    const UK: &[&str] = &[
        "hsbc", "barclays", "lloyds", "bp", "shell", "gsk", "astrazeneca",
        "unilever", "diageo", "bae systems", "rolls-royce", "vodafone",
    ];
    if UK.iter().any(|p| lc.contains(p)) {
        return "uk";
    }
    const US: &[&str] = &["Inc.", "Corp.", "Corporation", "NYSE", "NASDAQ"];
    if US.iter().any(|ind| company_name.contains(ind)) {
        return "us";
    }
    "other"
}

/// Extract `(target, acquirer)` from an M&A query (Python `_parse_ma_query`).
pub fn parse_ma_query(user_query: &str, company_hint: &str) -> (String, String) {
    let preamble = Regex::new(
        r"(?i)^(?:what\s+(?:is|are|was|were)\s+(?:the\s+)?|(?:research|find|analyze|tell\s+me\s+about|look\s+up)\s+(?:the\s+)?|(?:deal\s+(?:terms|analysis|details|value)\s+for\s+(?:the\s+)?))",
    )
    .unwrap();
    let q = preamble.replace(user_query.trim(), "").trim().to_string();
    let clean = |s: &str| s.trim().trim_end_matches([',', '.', ';', '?', '!']).to_string();

    // "X acquisition/acquired/merger by/with/of Y"
    let re1 = Regex::new(r"(?i)^(.+?)\s+(?:acquisition|acquired?|merger)\s+(?:by|with|of)\s+(.+)").unwrap();
    if let Some(c) = re1.captures(&q) {
        return (c[1].trim().to_string(), clean(&c[2]));
    }
    // "merger of/between X with/and Y"
    let re2 = Regex::new(r"(?i)^merger\s+(?:of|between)\s+(.+?)\s+(?:with|and)\s+(.+)").unwrap();
    if let Some(c) = re2.captures(&q) {
        return (c[1].trim().to_string(), clean(&c[2]));
    }
    // "Y acquires/buys/to acquire X" → (target=X, acquirer=Y)
    let re3 = Regex::new(r"(?i)^(.+?)\s+(?:acquires?|buys?|purchased?|(?:to\s+)?acquire[sd]?)\s+(.+)").unwrap();
    if let Some(c) = re3.captures(&q) {
        return (clean(&c[2]), c[1].trim().to_string());
    }
    // "X / Y deal|acquisition|merger"
    let re4 = Regex::new(r"(?i)^([^/]+?)\s*/\s*([^/]+?)(?:\s+deal|\s+acquisition|\s+merger|$)").unwrap();
    if let Some(c) = re4.captures(&q) {
        return (c[1].trim().to_string(), c[2].trim().to_string());
    }
    // Fallback: stop before first trigger keyword → that's the target.
    const TRIGGERS: &[&str] = &["acquisition", "acquir", "merger", "takeover", "buyout", "sold", "bought"];
    let words: Vec<&str> = q.split_whitespace().collect();
    for (i, w) in words.iter().enumerate() {
        let wl = w.to_lowercase();
        if TRIGGERS.iter().any(|t| wl.contains(t)) {
            let target = words[..i].join(" ").trim().to_string();
            return (if target.is_empty() { company_hint.to_string() } else { target }, String::new());
        }
    }
    (if company_hint.is_empty() { q } else { company_hint.to_string() }, String::new())
}

/// Structured deal facts extracted from multi-source article text.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DealSummary {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub announced: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub acquirer: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    /// Always present ("undisclosed" when not found), like the Python output.
    pub deal_value: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stake: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_revenue: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_ebitda: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub multiple: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected_close: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub advisors: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strategic_rationale: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sources: Option<String>,
}

const PRIORITY_SOURCE_KW: &[&str] =
    &["businesswire", "prnewswire", "globenewswire", "reuters", "bloomberg"];
const RATIONALE_KW: &[&str] = &[
    "platform", "scale", "growth", "strategy", "complementary", "synerg",
    "position", "leader", "expand", "global", "network", "freight", "logistics",
    "supply chain", "value creation",
];
const NAV_VERBS: &[&str] = &[
    "Read", "View", "See", "Click", "Download", "Visit", "Learn", "Get", "Sign",
    "Subscribe", "Watch", "Catch", "Join", "Follow", "More", "Related",
    "Featured", "Latest", "Recent", "Popular", "Search", "Contact", "About",
    "Press", "Industry", "Supply", "Share", "Print",
];

/// Split into sentences on `.!?` + whitespace (Python used a lookbehind; the
/// `regex` crate has none, so this is done manually).
fn split_sentences(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let chars: Vec<char> = text.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        cur.push(c);
        if matches!(c, '.' | '!' | '?') && chars.get(i + 1).is_some_and(|n| n.is_whitespace()) {
            out.push(cur.trim().to_string());
            cur.clear();
        }
    }
    if !cur.trim().is_empty() {
        out.push(cur.trim().to_string());
    }
    out
}

/// Strip trailing lowercase words / stop-words (Python `_proper_name`).
fn proper_name(s: &str) -> String {
    const STOP_AT: &[&str] =
        &["To", "For", "In", "A", "An", "The", "And", "Of", "With", "From"];
    let words: Vec<&str> = s.split_whitespace().collect();
    let kept: Vec<&str> = words
        .iter()
        .enumerate()
        .filter(|(i, w)| *i == 0 || w.chars().next().is_some_and(|c| c.is_uppercase()))
        .map(|(_, w)| *w)
        .collect();
    let mut final_words: Vec<&str> = Vec::new();
    for w in kept {
        if STOP_AT.contains(&w) && !final_words.is_empty() {
            break;
        }
        final_words.push(w);
    }
    final_words.join(" ").trim_matches([' ', ',', '.']).to_string()
}

/// First capture of the first matching pattern (case-insensitive, dot-all).
fn find_first(patterns: &[&str], text: &str) -> Option<String> {
    for p in patterns {
        let re = Regex::new(p).unwrap();
        if let Some(c) = re.captures(text) {
            if let Some(m) = c.get(1) {
                return Some(m.as_str().trim().to_string());
            }
        }
    }
    None
}

/// Extract structured deal facts from source-keyed article text
/// (Python `synthesize_deal`). Priority-source text is double-weighted.
pub fn synthesize_deal(sources: &HashMap<String, String>) -> DealSummary {
    let mut parts: Vec<String> = Vec::new();
    for (k, v) in sources {
        if !v.is_empty() && !v.contains("ERROR") && v.len() > 50 {
            parts.push(v.clone());
            if PRIORITY_SOURCE_KW.iter().any(|p| k.to_lowercase().contains(p)) {
                parts.push(v.clone()); // double-weight primaries
            }
        }
    }
    let combined = parts.join(" ");
    let mut out = DealSummary { deal_value: "undisclosed".to_string(), ..Default::default() };
    if combined.trim().is_empty() {
        return out;
    }

    // Deal-context sentences (mention a deal keyword) for date/rationale.
    let deal_ctx_re = Regex::new(r"(?i)(acqui|merger|deal|transaction|purchase|stake|announced|signed|agreed|partnership|invest)").unwrap();
    let deal_sentences: Vec<String> = split_sentences(&combined)
        .into_iter()
        .filter(|s| deal_ctx_re.is_match(s))
        .collect();
    let deal_ctx = if deal_sentences.is_empty() {
        combined.clone()
    } else {
        deal_sentences.join(" ")
    };

    out.announced = find_first(
        &[
            r"(?is)(?:announced?|signed?|closed?|completed?|agreed)\s+(?:on\s+)?(\w+\s+\d{1,2},?\s*202[4-6])",
            r"(?is)((?:January|February|March|April|May|June|July|August|September|October|November|December)\s+\d{1,2},?\s*202[4-6])",
            r"(?is)(202[4-6]-\d{2}-\d{2})",
            r"(?is)(\d{1,2}\s+(?:January|February|March|April|May|June|July|August|September|October|November|December)\s+202[4-6])",
        ],
        &deal_ctx,
    );

    let acq = r"([A-Z][A-Za-z&]+(?:[ \t]+[A-Z][A-Za-z&]+){0,4})";
    let acquirer_pats = [
        format!(r"(?s)(?:acquired?|purchased?|bought)\s+by\s+{acq}"),
        format!(r"(?s)(?:sold\s+(?:a\s+)?(?:majority|controlling|minority|\d+%)?\s*(?:stake|interest|ownership)?\s*to)\s+{acq}"),
        format!(r"(?s){acq}\s+(?:has\s+)?(?:agreed\s+to\s+acquire|will\s+acquire|completed\s+(?:the\s+|its\s+)?acquisition\s+of|announced\s+(?:the\s+|its\s+)?acquisition\s+of|signed\s+(?:a\s+)?definitive\s+agreement)\s+[A-Z]"),
        format!(r"(?s){acq}\s+(?:acquires?|acquired?|purchases?|to\s+acquire)\s+[A-Z]"),
        format!(r"(?s){acq}\s+(?:Equity\s+Group|Capital|Partners|Ventures?)(?:\s+acquires?|\s+announced|\s+has)"),
        format!(r"(?s){acq}\s+(?:takes?|took)\s+(?:majority|controlling|minority)?\s*(?:stake|ownership)"),
    ];
    let acq_refs: Vec<&str> = acquirer_pats.iter().map(|s| s.as_str()).collect();
    if let Some(a) = find_first(&acq_refs, &deal_ctx) {
        let name = proper_name(&a);
        if !name.is_empty() && !NAV_VERBS.contains(&name.split_whitespace().next().unwrap_or("")) {
            out.acquirer = Some(name);
        }
    }

    if let Some(t) = find_first(
        &[r"(?is)(?:acquires?|acquired?|purchases?)\s+([A-Z][A-Za-z\s&,]+?)(?:\s+for|\s+in\s+a|\s+\(|\.|,)"],
        &combined,
    ) {
        let name = proper_name(&t);
        if name.len() >= 4 && !NAV_VERBS.contains(&name.split_whitespace().next().unwrap_or("")) {
            out.target = Some(name);
        }
    }

    if let Some(v) = find_first(
        &[
            r"(?is)valued?\s+at\s+(?:approximately\s+)?\$\s*([\d,.]+\s*(?:billion|million|bn|mn)\b)",
            r"(?is)(?:enterprise value|EV)\s+of\s+(?:approximately\s+)?\$?\s*([\d,.]+\s*(?:billion|million|bn|mn)\b)",
            r"(?is)\$([\d,.]+\s*(?:billion|million|bn|mn)\b)\s+(?:deal|transaction|acquisition|purchase)",
            r"(?is)(?:purchase price|consideration)\s+of\s+\$?\s*([\d,.]+\s*(?:billion|million|bn|mn)\b)",
            r"(?is)\$([\d,.]+[Bb]\b)",
            r"(?is)\$([\d,.]+[Mm]\b)\s+(?:deal|acquisition)",
        ],
        &combined,
    ) {
        out.deal_value = v;
    }

    out.stake = find_first(
        &[
            r"(?is)(\d{1,3}(?:\.\d+)?%)\s+(?:ownership|interest|equity stake|stake)",
            r"(?is)(?:acquire[sd]?|purchas\w+)\s+(?:a\s+)?(\d{1,3}(?:\.\d+)?%)\s+stake",
            r"(?is)(majority|controlling|minority|100%)\s+(?:ownership\s+)?(?:stake|interest)",
            r"(?is)(majority|controlling|minority)\s+(?:equity\s+)?(?:position|ownership)",
        ],
        &combined,
    );
    out.target_revenue = find_first(
        &[
            r"(?is)(?:annual\s+)?revenue[s]?\s+of\s+(?:approximately\s+|~)?\$?\s*([\d,.]+\s*(?:billion|million|bn|mn)\b)",
            r"(?is)\$\s*([\d,.]+\s*(?:billion|million|bn|mn)\b)\s+in\s+(?:annual\s+)?revenue",
            r"(?is)(?:generates?|reported?)\s+(?:annual\s+)?revenue[s]?\s+of\s+\$?\s*([\d,.]+\s*(?:billion|million|bn|mn)\b)",
        ],
        &combined,
    );
    out.target_ebitda = find_first(
        &[
            r"(?is)EBITDA\s+of\s+(?:approximately\s+)?\$?\s*([\d,.]+\s*(?:billion|million|bn|mn)\b)",
            r"(?is)\$?\s*([\d,.]+\s*(?:billion|million|bn|mn)\b)\s+(?:of\s+)?EBITDA",
            r"(?is)adjusted\s+EBITDA\s+of\s+\$?\s*([\d,.]+\s*(?:billion|million|bn|mn)\b)",
        ],
        &combined,
    );
    out.multiple = find_first(
        &[
            r"(?is)(\d+\.?\d*[xX])\s+(?:LTM\s+)?EBITDA",
            r"(?is)(\d+\.?\d*[xX])\s+(?:trailing|forward)?\s*revenue",
            r"(?is)(?:EBITDA|revenue)\s+multiple\s+of\s+(\d+\.?\d*[xX]?)",
            r"(?is)valued?\s+at\s+(\d+\.?\d*[xX])\s+(?:times|x)",
        ],
        &combined,
    );
    out.expected_close = find_first(
        &[
            r"(?is)(?:expected?\s+to\s+close|anticipated?\s+to\s+close|close\s+in)\s+((?:Q[1-4]\s+)?(?:the\s+)?(?:first|second|third|fourth)\s+(?:quarter\s+of\s+)?20\d{2}|(?:early|mid|late)\s+20\d{2}|\w+\s+20\d{2})",
            r"(?is)(?:subject\s+to|pending)\s+([^.]{10,80}(?:regulatory|approval|clearance)[^.]{0,40})",
        ],
        &combined,
    );

    let advisor_re = Regex::new(
        r"(?i)\b(Goldman Sachs|Morgan Stanley|J\.?P\.?\s*Morgan|Barclays|Lazard|Rothschild|Evercore|Centerview|Jefferies|Citi(?:group|bank)?|Deutsche Bank|UBS|Houlihan Lokey|PJT Partners|Perella Weinberg|Kirkland(?:\s*&\s*Ellis)?|Sullivan(?:\s*&\s*Cromwell)?|Weil(?:\s*,?\s*Gotshal)?|Latham(?:\s*&\s*Watkins)?|Simpson(?:\s*Thacher)?|Skadden|Davis\s*Polk|Freshfields|Cleary)\b",
    )
    .unwrap();
    let mut advisors: Vec<String> = Vec::new();
    for c in advisor_re.captures_iter(&combined) {
        let a = c[1].to_string();
        if !advisors.contains(&a) {
            advisors.push(a);
        }
    }
    if !advisors.is_empty() {
        out.advisors = Some(advisors.into_iter().take(4).collect::<Vec<_>>().join("; "));
    }

    // Strategic rationale: deal-context sentences with ≥2 rationale keywords,
    // no boilerplate, no ALL-CAPS run.
    let boilerplate = Regex::new(r"(?i)\b(LAUNCH|OFFER|SUBSCRIBE|NEWSLETTER|PODCAST|WEBCAST|SIGN\s*UP|ADVERTISEMENT|SPONSORED|COOKIE|PRIVACY|COPYRIGHT|ALL\s*RIGHTS|leading\s+provider|leading\s+source|leading\s+publication|is\s+the\s+premier|is\s+a\s+leading|is\s+the\s+leading|catch\s+up\s+on|trade\s+shows?|special\s+events?|events\s+taking)\b").unwrap();
    let caps_run = Regex::new(r"[A-Z]{3,}\s+[A-Z]{3,}").unwrap();
    let lead_caption = Regex::new(r"^\([^)]{1,30}\)\s*").unwrap();
    let mut rationale: Vec<String> = Vec::new();
    for sent in split_sentences(&deal_ctx) {
        let sent = lead_caption.replace(sent.trim(), "").to_string();
        let kw = RATIONALE_KW.iter().filter(|k| sent.to_lowercase().contains(**k)).count();
        if sent.len() > 50 && sent.len() < 300 && kw >= 2 && !boilerplate.is_match(&sent) && !caps_run.is_match(&sent) {
            rationale.push(sent);
        }
    }
    if !rationale.is_empty() {
        out.strategic_rationale = Some(rationale.into_iter().take(2).collect::<Vec<_>>().join(" "));
    }

    let mut used: Vec<String> = sources
        .iter()
        .filter(|(_, v)| !v.is_empty() && !v.contains("ERROR") && v.len() > 100)
        .map(|(k, _)| k.clone())
        .collect();
    used.sort();
    if !used.is_empty() {
        out.sources = Some(used.join(", "));
    }
    out
}

/// Deal-cascade stop condition (Python `is_sufficient`).
pub fn is_sufficient(s: &DealSummary) -> bool {
    let has_date = s.announced.is_some();
    let has_value = s.deal_value != "undisclosed";
    let has_acquirer = s.acquirer.is_some();
    let has_rationale = s.strategic_rationale.is_some();
    let n_sources = s.sources.as_ref().map_or(0, |v| v.split(',').count());
    (has_date && (has_value || (has_acquirer && has_rationale))) || n_sources >= 3
}

/// The result of a deal-research run.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct DealResearch {
    pub query_type: Option<QueryType>,
    pub target: String,
    pub acquirer: String,
    pub summary: DealSummary,
    pub sources_read: Vec<String>,
    pub sufficient: bool,
}

/// IB-style deal search queries from the parsed entities.
fn build_deal_queries(target: &str, acquirer: &str) -> Vec<String> {
    let mut qs = Vec::new();
    if !acquirer.is_empty() {
        qs.push(format!("{acquirer} acquires {target} deal terms value"));
        qs.push(format!("{target} acquired by {acquirer} transaction announced"));
    }
    qs.push(format!("{target} acquisition announced deal value rationale"));
    qs.push(format!("{target} merger agreement purchase price advisors"));
    qs
}

fn domain_of(url: &str) -> String {
    url.split("://")
        .nth(1)
        .unwrap_or(url)
        .split('/')
        .next()
        .unwrap_or(url)
        .trim_start_matches("www.")
        .to_string()
}

/// Run a live M&A deal-research cascade: route the query, parse entities, then
/// search → read → synthesize until [`is_sufficient`] or the query set is
/// exhausted. Uses the Roam MCP client when supplied, else the HTTP fallback.
pub fn run_deal_research(
    user_query: &str,
    mut mcp: Option<&mut fm_mcp::McpClient>,
) -> DealResearch {
    let query_type = detect_query_type(user_query);
    let (target, acquirer) = parse_ma_query(user_query, "");
    let queries = build_deal_queries(&target, &acquirer);
    let mut sources: HashMap<String, String> = HashMap::new();
    let mut read: Vec<String> = Vec::new();
    let mut summary = DealSummary { deal_value: "undisclosed".into(), ..Default::default() };

    'outer: for q in &queries {
        let hits = match web::web_search(q, mcp.as_deref_mut()) {
            Ok(h) => h,
            Err(_) => continue,
        };
        for hit in hits.iter().take(4) {
            if read.contains(&hit.url) {
                continue;
            }
            let text = match web::read_page(&hit.url, Some(&target), mcp.as_deref_mut()) {
                Ok(t) => t,
                Err(_) => continue,
            };
            read.push(hit.url.clone());
            if !has_deal_content(&text) {
                continue;
            }
            sources.insert(domain_of(&hit.url), text);
            summary = synthesize_deal(&sources);
            if is_sufficient(&summary) {
                break 'outer;
            }
        }
    }
    let sufficient = is_sufficient(&summary);
    DealResearch { query_type: Some(query_type), target, acquirer, summary, sources_read: read, sufficient }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routes_transaction_and_earnings() {
        assert_eq!(detect_query_type("Nestlé acquisition deal terms"), QueryType::TransactionTerms);
        assert_eq!(detect_query_type("Apple Q3 2025 earnings release"), QueryType::EarningsAnalysis);
        assert_eq!(detect_query_type("who owns Tesla shares"), QueryType::BeneficialOwnership);
        assert_eq!(detect_query_type("hello there"), QueryType::GeneralCompanyIntelligence);
    }

    #[test]
    fn parses_ma_query_shapes() {
        assert_eq!(parse_ma_query("Microsoft acquires Activision", ""), ("Activision".into(), "Microsoft".into()));
        assert_eq!(parse_ma_query("Activision acquired by Microsoft", ""), ("Activision".into(), "Microsoft".into()));
        assert_eq!(parse_ma_query("research the Credit Suisse merger with UBS", ""), ("Credit Suisse".into(), "UBS".into()));
        let (t, a) = parse_ma_query("Figma / Adobe deal", "");
        assert_eq!((t.as_str(), a.as_str()), ("Figma", "Adobe"));
    }

    #[test]
    fn listing_type_from_name() {
        assert_eq!(detect_listing_type("Reliance Industries"), "india");
        assert_eq!(detect_listing_type("Barclays PLC"), "uk");
        assert_eq!(detect_listing_type("Acme Corporation"), "us");
        assert_eq!(detect_listing_type("Some GmbH"), "other");
    }

    #[test]
    fn synthesizes_deal_facts() {
        let mut src = HashMap::new();
        src.insert(
            "reuters.com".to_string(),
            "Global Freight Corp acquires Cargo Systems for $2.5 billion, the companies \
             announced on March 12, 2025. The transaction values Cargo Systems at an \
             enterprise value of $2.5 billion. The deal expands the acquirer's logistics \
             platform and network scale across global supply chain markets. Goldman Sachs \
             advised on the transaction. The acquisition is expected to close in the fourth \
             quarter of 2025."
                .to_string(),
        );
        let s = synthesize_deal(&src);
        assert!(s.announced.is_some(), "date");
        assert_eq!(s.deal_value, "2.5 billion");
        assert!(s.acquirer.is_some(), "acquirer");
        assert!(s.strategic_rationale.is_some(), "rationale");
        assert!(s.advisors.as_deref().unwrap_or("").contains("Goldman Sachs"));
        assert!(is_sufficient(&s));
    }

    #[test]
    fn insufficient_when_empty() {
        let s = synthesize_deal(&HashMap::new());
        assert_eq!(s.deal_value, "undisclosed");
        assert!(!is_sufficient(&s));
    }

    #[test]
    fn sufficiency_by_source_count() {
        let s = DealSummary {
            deal_value: "undisclosed".into(),
            sources: Some("a.com, b.com, c.com".into()),
            ..Default::default()
        };
        assert!(is_sufficient(&s));
    }

    #[test]
    fn domain_extraction() {
        assert_eq!(domain_of("https://www.reuters.com/deal/x"), "reuters.com");
        assert_eq!(domain_of("http://ex.com"), "ex.com");
    }

    #[test]
    #[ignore] // live network + Roam MCP / HTTP fallback
    fn live_deal_research() {
        let r = run_deal_research("Microsoft acquires Activision Blizzard", None);
        assert_eq!(r.acquirer, "Microsoft");
        assert!(!r.sources_read.is_empty());
    }
}
