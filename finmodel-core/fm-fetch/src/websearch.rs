//! Plain-HTTP web search + page fetch — the non-MCP fallback for the search
//! facade. DuckDuckGo HTML endpoint (same host/technique as the annual-report
//! discovery) → structured hits; a page fetch + minimal tag-strip good enough
//! for non-protected pages (protected pages need the Roam MCP path).

use std::time::Duration;

use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};

use crate::market::FetchError;

/// A ranked web search result.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct WebHit {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

fn client() -> Result<reqwest::blocking::Client, FetchError> {
    reqwest::blocking::Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| FetchError::Network(e.to_string()))
}

/// DuckDuckGo HTML search → up to `limit` structured hits.
pub fn web_search(query: &str, limit: usize) -> Result<Vec<WebHit>, FetchError> {
    let html = client()?
        .post("https://html.duckduckgo.com/html/")
        .form(&[("q", query), ("kl", "us-en")])
        .send()
        .map_err(|e| FetchError::Network(e.to_string()))?
        .error_for_status()
        .map_err(|e| FetchError::Network(e.to_string()))?
        .text()
        .map_err(|e| FetchError::Parse(e.to_string()))?;
    let mut hits = parse_ddg_hits(&html);
    hits.truncate(limit);
    Ok(hits)
}

/// Parse DDG HTML results into hits (pure — unit-testable).
pub fn parse_ddg_hits(html: &str) -> Vec<WebHit> {
    let doc = Html::parse_document(html);
    let result_sel = match Selector::parse("div.result, div.web-result") {
        Ok(s) => s,
        Err(_) => return vec![],
    };
    let a_sel = Selector::parse("a.result__a").unwrap();
    let snip_sel = Selector::parse("a.result__snippet, .result__snippet").unwrap();
    let mut out = Vec::new();
    for res in doc.select(&result_sel) {
        let a = match res.select(&a_sel).next() {
            Some(a) => a,
            None => continue,
        };
        let url = decode_ddg_href(a.value().attr("href").unwrap_or(""));
        if url.is_empty() {
            continue;
        }
        let title = a.text().collect::<String>().trim().to_string();
        let snippet = res
            .select(&snip_sel)
            .next()
            .map(|s| s.text().collect::<String>().trim().to_string())
            .unwrap_or_default();
        out.push(WebHit { title, url, snippet });
    }
    out
}

/// DDG wraps results as `//duckduckgo.com/l/?uddg=<pct-encoded-url>&…`. Decode.
fn decode_ddg_href(href: &str) -> String {
    if let Some(idx) = href.find("uddg=") {
        let enc = href[idx + 5..].split('&').next().unwrap_or("");
        return percent_decode(enc);
    }
    if href.starts_with("http") {
        href.to_string()
    } else if href.starts_with("//") {
        format!("https:{href}")
    } else {
        String::new()
    }
}

/// Minimal percent-decoder (handles `%XX` and `+`).
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
                match hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                    Some(b) => {
                        out.push(b);
                        i += 3;
                    }
                    None => {
                        out.push(b'%');
                        i += 1;
                    }
                }
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Fetch a page and reduce it to whitespace-collapsed body text (good enough for
/// non-protected pages; protected pages need the Roam MCP `read_markdown`).
pub fn fetch_page_text(url: &str) -> Result<String, FetchError> {
    let html = client()?
        .get(url)
        .send()
        .map_err(|e| FetchError::Network(e.to_string()))?
        .error_for_status()
        .map_err(|e| FetchError::Network(e.to_string()))?
        .text()
        .map_err(|e| FetchError::Parse(e.to_string()))?;
    Ok(strip_html(&html))
}

/// Body text, whitespace-collapsed, with script/style contents dropped.
pub fn strip_html(html: &str) -> String {
    let doc = Html::parse_document(html);
    let body_sel = Selector::parse("body").unwrap();
    let skip_sel = Selector::parse("script, style, noscript").unwrap();
    let skip: std::collections::HashSet<_> =
        doc.select(&skip_sel).flat_map(|el| el.text()).collect();
    let root = doc.select(&body_sel).next();
    let text: String = match root {
        Some(b) => b
            .text()
            .filter(|t| !skip.contains(t))
            .collect::<Vec<_>>()
            .join(" "),
        None => String::new(),
    };
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_ddg_redirect() {
        let href = "//duckduckgo.com/l/?uddg=https%3A%2F%2Fwww.nestle.com%2Fannual&rut=x";
        assert_eq!(decode_ddg_href(href), "https://www.nestle.com/annual");
    }

    #[test]
    fn parses_ddg_hits() {
        let html = r#"<div class="result web-result">
          <a class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fex.com%2Fa">Example A</a>
          <a class="result__snippet">A snippet here</a>
        </div>
        <div class="result web-result">
          <a class="result__a" href="https://direct.com/b">Direct B</a>
        </div>"#;
        let hits = parse_ddg_hits(html);
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].url, "https://ex.com/a");
        assert_eq!(hits[0].title, "Example A");
        assert_eq!(hits[0].snippet, "A snippet here");
        assert_eq!(hits[1].url, "https://direct.com/b");
    }

    #[test]
    fn strips_to_body_text() {
        let html = "<html><head><style>x{}</style></head><body><h1>Hi</h1><script>bad()</script><p>World</p></body></html>";
        let t = strip_html(html);
        assert!(t.contains("Hi"));
        assert!(t.contains("World"));
        assert!(!t.contains("bad()"));
    }
}
