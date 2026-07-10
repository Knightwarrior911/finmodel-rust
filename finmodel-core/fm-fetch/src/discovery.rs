//! Non-US annual report PDF discovery via DDG search + IR page scraping.
//!
//! Ported from `src/fetcher.py`:
//! - `_find_annual_report_pdf_url()` — DDG search cascade
//! - `_url_matches_company()` — domain validation
//! - `_company_domain_tokens()` — tokenizer

use scraper::{Html, Selector};
use url::Url;

/// Domains to skip when evaluating search results.
const SKIP_DOMAINS: &[&str] = &[
    "duckduckgo.com", "google.com", "bing.com",
    "youtube.com", "facebook.com", "twitter.com",
];

/// Errors from PDF discovery operations.
#[derive(Debug, thiserror::Error)]
pub enum DiscoveryError {
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("No PDF URL found for {company} ({ticker})")]
    NotFound { company: String, ticker: String },
}

/// Extract lowercase tokens from company name for domain matching.
/// Ported from `_company_domain_tokens()` in `src/fetcher.py`.
fn company_domain_tokens(company_name: &str) -> Vec<String> {
    let mut tokens: Vec<String> = Vec::new();
    for token in company_name.split(&[' ', '-', '.', '/', ','][..]) {
        let t = token.trim().to_lowercase();
        if t.len() >= 3 && !t.is_empty() {
            tokens.push(t);
        }
    }
    tokens
}

/// Check if a URL's domain matches any company name token.
/// Ported from `_url_matches_company()` in `src/fetcher.py`.
fn url_matches_company(url_str: &str, company_tokens: &[String]) -> bool {
    let parsed = match Url::parse(url_str) {
        Ok(u) => u,
        Err(_) => return false,
    };
    let domain = parsed.host_str().unwrap_or("").to_lowercase();
    company_tokens.iter().any(|tok| domain.contains(tok))
}

/// Check if a URL should be skipped (search engine domain, social media, etc.).
fn is_skippable(url_str: &str) -> bool {
    let parsed = match Url::parse(url_str) {
        Ok(u) => u,
        Err(_) => return true,
    };
    let domain = parsed.host_str().unwrap_or("");
    SKIP_DOMAINS.iter().any(|s| domain.contains(s))
}

/// DuckDuckGo HTML search — POST to the non-JSON endpoint.
fn ddg_search(query: &str) -> Result<String, reqwest::Error> {
    let client = reqwest::blocking::Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
        .timeout(std::time::Duration::from_secs(15))
        .build()?;
    let resp = client
        .post("https://html.duckduckgo.com/html/")
        .form(&[("q", query), ("kl", "us-en")])
        .send()?
        .error_for_status()?;
    resp.text()
}

/// Find the annual report PDF URL for a company.
///
/// Ported from `_find_annual_report_pdf_url()` in `src/fetcher.py`.
/// Strategy:
/// 1. DDG search for `"{company} annual report {year} filetype:pdf"`
/// 2. First pass: direct PDF links matching company domain
/// 3. Second pass: fetch top IR page result, scan for PDF links
pub fn find_annual_report_pdf_url(
    company_name: &str,
    ticker: &str,
    year: Option<i32>,
) -> Result<String, DiscoveryError> {
    let year = year.unwrap_or_else(|| {
        // Default: most recently completed fiscal year (assumes current date)
        2025
    });

    let company_tokens = company_domain_tokens(company_name);
    let mut fallback_pdf: Option<String> = None;

    let queries = [
        format!("{company_name} annual report {year} filetype:pdf"),
        format!("{company_name} {ticker} annual report {year} PDF"),
        format!("{company_name} annual report {year} PDF investor relations"),
    ];

    for query in &queries {
        let html = match ddg_search(query) {
            Ok(h) => h,
            Err(_) => continue,
        };

        // Parse HTML with scraper
        let doc = Html::parse_fragment(&html);
        let link_sel = Selector::parse("a.result__a").ok();

        let links: Vec<String> = if let Some(sel) = &link_sel {
            doc.select(sel)
                .filter_map(|el| el.value().attr("href"))
                .filter(|h| h.starts_with("http"))
                .map(|h| h.to_string())
                .collect()
        } else {
            continue;
        };

        // First pass: direct PDF links
        let direct_pdfs: Vec<&str> = links.iter()
            .filter(|h| h.to_lowercase().ends_with(".pdf"))
            .filter(|h| !is_skippable(h))
            .map(|s| s.as_str())
            .collect();

        for &href in &direct_pdfs {
            if url_matches_company(href, &company_tokens) {
                return Ok(href.to_string());
            }
        }
        if fallback_pdf.is_none() && !direct_pdfs.is_empty() {
            fallback_pdf = direct_pdfs.first().map(|s| s.to_string());
        }

        // Second pass: fetch IR pages and scan for PDFs
        for href in &links {
            if is_skippable(href) {
                continue;
            }
            if !url_matches_company(href, &company_tokens) {
                continue;
            }
            // Fetch the IR page
            let ir_html = match reqwest::blocking::Client::builder()
                .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
                .timeout(std::time::Duration::from_secs(10))
                .build()
                .and_then(|c| c.get(href).send()?.error_for_status()?.text())
            {
                Ok(t) => t,
                Err(_) => continue,
            };

            let ir_doc = Html::parse_document(&ir_html);
            let all_a = Selector::parse("a").ok();
            if let Some(a_sel) = all_a {
                for link_el in ir_doc.select(&a_sel) {
                    let lhref = match link_el.value().attr("href") {
                        Some(h) => h,
                        None => continue,
                    };
                    // Resolve relative URLs
                    let resolved = if lhref.starts_with("http") {
                        lhref.to_string()
                    } else {
                        match Url::parse(href).ok().and_then(|u| u.join(lhref).ok()) {
                            Some(u) => u.to_string(),
                            None => continue,
                        }
                    };

                    let ltext = link_el.text().collect::<String>().to_lowercase();
                    let lower_href = resolved.to_lowercase();

                    if lower_href.ends_with(".pdf")
                        && (ltext.contains("annual")
                            || ltext.contains("report")
                            || ltext.contains("20-f")
                            || ltext.contains("results"))
                    {
                        return Ok(resolved);
                    }
                }
            }
        }
    }

    // Return best fallback or error
    fallback_pdf.ok_or_else(|| DiscoveryError::NotFound {
        company: company_name.to_string(),
        ticker: ticker.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_company_domain_tokens() {
        let tokens = company_domain_tokens("Atlas Copco AB");
        assert!(tokens.contains(&"atlas".to_string()));
        assert!(tokens.contains(&"copco".to_string()));
        assert!(!tokens.contains(&"ab".to_string())); // too short
    }

    #[test]
    fn test_company_domain_tokens_with_hyphen() {
        let tokens = company_domain_tokens("NOVO-Nordisk A/S");
        assert!(tokens.contains(&"novo".to_string()));
        assert!(tokens.contains(&"nordisk".to_string()));
    }

    #[test]
    fn test_url_matches_company_positive() {
        let tokens = company_domain_tokens("Sandvik AB");
        assert!(url_matches_company("https://www.sandvik.com/report.pdf", &tokens));
        assert!(url_matches_company("https://home.sandvik/en/investors", &tokens));
    }

    #[test]
    fn test_url_matches_company_negative() {
        let tokens = company_domain_tokens("Sandvik AB");
        assert!(!url_matches_company("https://www.siemens.com/report.pdf", &tokens));
        assert!(!url_matches_company("https://example.com/sandvik_fake.pdf", &tokens));
    }

    #[test]
    fn test_is_skippable_positive() {
        assert!(is_skippable("https://www.duckduckgo.com/result"));
        assert!(is_skippable("https://google.com/search"));
    }

    #[test]
    fn test_is_skippable_negative() {
        assert!(!is_skippable("https://www.sandvik.com/report.pdf"));
    }

    #[test]
    #[ignore]
    fn test_ddg_search_returns_html() {
        let html = ddg_search("test query").expect("DDG search should return HTML");
        assert!(!html.is_empty());
        assert!(html.contains("html") || html.contains("HTML") || html.contains("results"));
    }
}
