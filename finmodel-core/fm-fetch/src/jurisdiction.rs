//! Jurisdiction detection + regulator-site candidate generation for non-US
//! filings.
//!
//! Ported verbatim from `src/research/browser_pipeline.py`:
//! - `JURISDICTION_PATTERNS` (16-suffix map)
//! - `REGULATOR_SITES`
//! - `ANNUAL_KEYWORDS` (multilingual)
//! - `BLOCKED_DOMAINS`
//! - `_detect_jurisdiction()`
//!
//! The browser-discovery glue that consumed these tables is NOT ported — the
//! candidate generation here feeds the DDG discovery cascade in
//! `crate::discovery`, and a later phase adds a Roam MCP tier.

/// Jurisdiction → (country, TLDs, regulator site query, filing-name query).
///
/// Ported verbatim from `JURISDICTION_PATTERNS` in `browser_pipeline.py`.
/// Order matters: the first matching ticker suffix wins (mirrors dict-iteration
/// order in the Python source).
pub const JURISDICTION_PATTERNS: &[(&str, &str, &[&str], &str, &str)] = &[
    (
        ".DE",
        "Germany",
        &["de", "com"],
        "site:bundesanzeiger.de",
        "geschäftsbericht OR annual report OR financial statements",
    ),
    (
        ".PA",
        "France",
        &["fr", "com"],
        "",
        "rapport financier OR annual report OR financial statements",
    ),
    (
        ".AS",
        "Netherlands",
        &["nl", "com"],
        "",
        "jaarverslag OR annual report OR financial statements",
    ),
    (
        ".L",
        "UK",
        &["co.uk", "com"],
        "site:companieshouse.gov.uk",
        "annual report OR annual financial report OR financial statements",
    ),
    (
        ".SW",
        "Switzerland",
        &["ch", "com"],
        "",
        "geschäftsbericht OR annual report OR financial statements",
    ),
    (
        ".MI",
        "Italy",
        &["it", "com"],
        "",
        "bilancio OR relazione finanziaria OR annual report",
    ),
    (
        ".MC",
        "Spain",
        &["es", "com"],
        "",
        "informe anual OR cuentas anuales OR annual report",
    ),
    (
        ".NS",
        "India",
        &["co.in", "com"],
        "site:bseindia.com OR site:nseindia.com",
        "annual report OR financial results",
    ),
    (
        ".BO",
        "India",
        &["co.in", "com"],
        "site:bseindia.com OR site:nseindia.com",
        "annual report OR financial results",
    ),
    (
        ".T",
        "Japan",
        &["co.jp", "com"],
        "site:edinet-fsa.go.jp",
        "annual report OR financial statements OR yukashoken hokokusho",
    ),
    (
        ".HK",
        "Hong Kong",
        &["hk", "com"],
        "site:hkexnews.hk",
        "annual report OR announcement OR circular",
    ),
    (
        ".CO",
        "Singapore",
        &["com.sg", "com"],
        "site:sgx.com",
        "annual report",
    ),
    (".IR", "Other", &["com"], "", "annual report"),
    (
        ".AX",
        "Australia",
        &["com.au", "com"],
        "site:asx.com.au",
        "annual report OR announcement",
    ),
    (
        ".ST",
        "Sweden",
        &["com", "se"],
        "",
        "annual report OR årsredovisning OR financial statements",
    ),
    (
        ".HE",
        "Finland",
        &["com", "fi"],
        "",
        "annual report OR vuosikertomus OR financial statements",
    ),
    (
        ".OL",
        "Norway",
        &["com", "no"],
        "",
        "annual report OR årsrapport OR financial statements",
    ),
];

/// Exchange/regulator databases by ticker suffix.
///
/// These are authoritative sources — annual reports are legally required to be
/// filed here. Ported verbatim from `REGULATOR_SITES` in `browser_pipeline.py`.
pub const REGULATOR_SITES: &[(&str, &str)] = &[
    (".NS", "bseindia.com"),
    (".BO", "bseindia.com"),
    (".HK", "hkexnews.hk"),
    (".AX", "asx.com.au"),
    (".T", "edinet-fsa.go.jp"),
    (".CO", "sgx.com"),
];

/// Link-text keywords that unambiguously mean "annual report" in various
/// languages. Ported verbatim from `ANNUAL_KEYWORDS` in `browser_pipeline.py`.
pub const ANNUAL_KEYWORDS: &[&str] = &[
    "annual report",
    "annual financial report",
    "annual review",
    "integrated report",
    "report and accounts",
    "jaarverslag",
    "geschäftsbericht",
    "rapport annuel",
    "relazione annuale",
    "informe anual",
    "годовой отчет",
];

/// Domains known to aggregate/republish filings — skip these when looking for
/// IR pages. Ported verbatim from `BLOCKED_DOMAINS` in `browser_pipeline.py`.
pub const BLOCKED_DOMAINS: &[&str] = &[
    // Search engines / social
    "google.com",
    "google.co",
    "about.google",
    "bing.com",
    "linkedin.com",
    "twitter.com",
    "facebook.com",
    "reddit.com",
    "youtube.com",
    // Document aggregators — NEVER the company's own filing
    "scribd.com",
    "slideshare.net",
    "issuu.com",
    "academia.edu",
    "annualreports.com",
    "annualreportservice.com",
    "reportlinker.com",
    "slideboxx.com",
    "yumpu.com",
    "docplayer.net",
    "calameo.com",
    // Financial data vendors (not IR pages)
    "seekingalpha.com",
    "yahoo.com",
    "reuters.com",
    "bloomberg.com",
    "wsj.com",
    "ft.com",
    "macrotrends.net",
    "marketwatch.com",
    "investing.com",
    "simplywallst.com",
    "wisesheets.io",
    "stockanalysis.com",
    "marketscreener.com",
    "zonebourse.com",
    "boerse.de",
    "finanzen.net",
    "spglobal.com",
    "moodys.com",
    "fitchratings.com",
    // Nordic/European document aggregators (not the company's own IR)
    "millistream.com",
    "cision.com",
    "mb.cision.com",
    "huginonline.com",
    "newswire.ca",
    "accesswire.com",
    // General encyclopedias / news
    "wikipedia.org",
    "businesswire.com",
    "prnewswire.com",
    "globenewswire.com",
];

/// Detected jurisdiction metadata for a ticker/company.
///
/// Field-for-field port of the dict returned by `_detect_jurisdiction()`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Jurisdiction {
    pub country: String,
    pub tlds: Vec<String>,
    pub regulator_site: String,
    pub local_query: String,
    pub ticker_suffix: String,
}

/// Detect jurisdiction from ticker suffix or company-name country hints.
///
/// Ported verbatim from `_detect_jurisdiction()` in `browser_pipeline.py`.
pub fn detect_jurisdiction(ticker: &str, company: &str) -> Jurisdiction {
    let ticker_upper = ticker.to_uppercase();
    let company_lower = company.to_lowercase();

    for &(suffix, country, tlds, reg_site, query) in JURISDICTION_PATTERNS {
        if ticker_upper.ends_with(suffix) {
            return Jurisdiction {
                country: country.to_string(),
                tlds: tlds.iter().map(|s| s.to_string()).collect(),
                regulator_site: reg_site.to_string(),
                local_query: query.to_string(),
                ticker_suffix: suffix.to_string(),
            };
        }
    }

    // Country name in company name fallback (ordered to mirror the Python dict).
    const COUNTRY_HINTS: &[(&str, &str)] = &[
        ("germany", "DE"),
        ("deutschland", "DE"),
        ("france", "FR"),
        ("netherlands", "NL"),
        ("italy", "IT"),
        ("spain", "ES"),
        ("españa", "ES"),
        ("switzerland", "CH"),
        ("schweiz", "CH"),
        ("japan", "JP"),
        ("india", "IN"),
        ("china", "CN"),
        ("brazil", "BR"),
        ("brasil", "BR"),
        ("australia", "AU"),
        ("canada", "CA"),
        ("singapore", "SG"),
        ("hong kong", "HK"),
        ("uk", "UK"),
        ("united kingdom", "UK"),
    ];
    for &(hint, cc) in COUNTRY_HINTS {
        if company_lower.contains(hint) {
            return Jurisdiction {
                country: cc.to_string(),
                tlds: vec!["com".to_string()],
                regulator_site: String::new(),
                local_query: "annual report".to_string(),
                ticker_suffix: String::new(),
            };
        }
    }

    Jurisdiction {
        country: "Unknown".to_string(),
        tlds: vec!["com".to_string()],
        regulator_site: String::new(),
        local_query: "annual report".to_string(),
        ticker_suffix: String::new(),
    }
}

/// Look up the authoritative regulator domain for a ticker suffix, if any.
pub fn regulator_site(ticker: &str) -> Option<&'static str> {
    let up = ticker.to_uppercase();
    REGULATOR_SITES
        .iter()
        .find(|(suffix, _)| up.ends_with(suffix))
        .map(|(_, site)| *site)
}

/// Build regulator-site candidate search queries for a company/ticker.
///
/// These are the authoritative-source queries tried FIRST in the discovery
/// cascade (mirrors `_find_via_regulator()` — a `site:{regulator}` scoped
/// search — minus the browser glue). Returns an empty vector when the ticker's
/// suffix has no indexed regulator database.
pub fn regulator_candidates(company: &str, ticker: &str, year: Option<i32>) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(reg) = regulator_site(ticker) {
        match year {
            Some(y) => {
                out.push(format!(
                    "\"{company}\" \"annual report\" {y} site:{reg} filetype:pdf"
                ));
                out.push(format!("\"{company}\" annual report {y} site:{reg}"));
            }
            None => {
                out.push(format!(
                    "\"{company}\" \"annual report\" site:{reg} filetype:pdf"
                ));
                out.push(format!("\"{company}\" annual report site:{reg}"));
            }
        }
    }
    out
}

/// True when a URL's host matches a blocked (aggregator/vendor/social) domain.
///
/// Case-insensitive substring match on the URL, mirroring the Python filter
/// (`any(b in url.lower() for b in BLOCKED_DOMAINS)`).
pub fn is_blocked_domain(url: &str) -> bool {
    let lower = url.to_lowercase();
    BLOCKED_DOMAINS.iter().any(|b| lower.contains(b))
}

/// True when text (link text or URL) contains an unambiguous annual-report
/// keyword in any supported language.
pub fn has_annual_keyword(text: &str) -> bool {
    let lower = text.to_lowercase();
    ANNUAL_KEYWORDS.iter().any(|k| lower.contains(k))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Table snapshot: every suffix in the map resolves to its exact country
    /// + regulator + local query, verbatim from the Python source.
    #[test]
    fn detect_jurisdiction_table_snapshot() {
        let cases: &[(&str, &str, &str, &str, &[&str])] = &[
            (
                "BAS.DE",
                "Germany",
                ".DE",
                "site:bundesanzeiger.de",
                &["de", "com"],
            ),
            ("MC.PA", "France", ".PA", "", &["fr", "com"]),
            ("ASML.AS", "Netherlands", ".AS", "", &["nl", "com"]),
            (
                "ULVR.L",
                "UK",
                ".L",
                "site:companieshouse.gov.uk",
                &["co.uk", "com"],
            ),
            ("NESN.SW", "Switzerland", ".SW", "", &["ch", "com"]),
            ("ENI.MI", "Italy", ".MI", "", &["it", "com"]),
            ("SAN.MC", "Spain", ".MC", "", &["es", "com"]),
            (
                "RELIANCE.NS",
                "India",
                ".NS",
                "site:bseindia.com OR site:nseindia.com",
                &["co.in", "com"],
            ),
            (
                "TCS.BO",
                "India",
                ".BO",
                "site:bseindia.com OR site:nseindia.com",
                &["co.in", "com"],
            ),
            (
                "7203.T",
                "Japan",
                ".T",
                "site:edinet-fsa.go.jp",
                &["co.jp", "com"],
            ),
            (
                "0700.HK",
                "Hong Kong",
                ".HK",
                "site:hkexnews.hk",
                &["hk", "com"],
            ),
            (
                "D05.CO",
                "Singapore",
                ".CO",
                "site:sgx.com",
                &["com.sg", "com"],
            ),
            ("XYZ.IR", "Other", ".IR", "", &["com"]),
            (
                "BHP.AX",
                "Australia",
                ".AX",
                "site:asx.com.au",
                &["com.au", "com"],
            ),
            ("ATCO-B.ST", "Sweden", ".ST", "", &["com", "se"]),
            ("NOKIA.HE", "Finland", ".HE", "", &["com", "fi"]),
            ("EQNR.OL", "Norway", ".OL", "", &["com", "no"]),
        ];
        for &(ticker, country, suffix, reg, tlds) in cases {
            let j = detect_jurisdiction(ticker, "");
            assert_eq!(j.country, country, "country for {ticker}");
            assert_eq!(j.ticker_suffix, suffix, "suffix for {ticker}");
            assert_eq!(j.regulator_site, reg, "regulator for {ticker}");
            assert_eq!(
                j.tlds,
                tlds.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
                "tlds for {ticker}"
            );
        }
    }

    #[test]
    fn detect_jurisdiction_company_name_fallback() {
        // No known suffix → country-name hint in the company string wins.
        let j = detect_jurisdiction("XYZ", "Some Company Germany AG");
        assert_eq!(j.country, "DE");
        assert_eq!(j.ticker_suffix, "");
        assert_eq!(j.local_query, "annual report");

        let j = detect_jurisdiction("ABC", "Grupo España SA");
        assert_eq!(j.country, "ES");
    }

    #[test]
    fn detect_jurisdiction_unknown_default() {
        let j = detect_jurisdiction("AAPL", "Apple Inc");
        assert_eq!(j.country, "Unknown");
        assert_eq!(j.tlds, vec!["com".to_string()]);
        assert_eq!(j.regulator_site, "");
        assert_eq!(j.local_query, "annual report");
        assert_eq!(j.ticker_suffix, "");
    }

    #[test]
    fn regulator_candidates_scoped_to_site() {
        let c = regulator_candidates("Reliance Industries", "RELIANCE.NS", Some(2024));
        assert!(!c.is_empty(), "NS ticker has a regulator");
        assert!(c[0].contains("site:bseindia.com"));
        assert!(c[0].contains("2024"));
        // Non-regulator jurisdiction (Sweden) → no authoritative candidates.
        assert!(regulator_candidates("Sandvik AB", "SAND.ST", Some(2024)).is_empty());
    }

    #[test]
    fn blocked_and_annual_keyword_filters() {
        assert!(is_blocked_domain(
            "https://www.scribd.com/doc/123/annual.pdf"
        ));
        assert!(is_blocked_domain("https://mb.cision.com/Main/x.pdf"));
        assert!(!is_blocked_domain("https://www.sandvik.com/ir/report.pdf"));

        assert!(has_annual_keyword("2024 Annual Report (PDF)"));
        assert!(has_annual_keyword("Geschäftsbericht 2024"));
        assert!(!has_annual_keyword("Sustainability highlights"));
    }
}
