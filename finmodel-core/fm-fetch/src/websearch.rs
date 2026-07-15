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

/// Extract the main readable content as lightweight markdown (headings,
/// paragraphs, list items) with chrome (nav/header/footer/aside/script/style/
/// forms) dropped. Good enough to render a static/SSR page in the in-app reader;
/// JS-only pages still need the Roam MCP path. Falls back to flat body text when
/// structural extraction yields too little.
pub fn strip_html(html: &str) -> String {
    let doc = Html::parse_document(html);
    // Main-content root preference: <main>/<article>/[role=main], else <body>.
    let root = ["main", "article", "[role=main]", "body"]
        .iter()
        .find_map(|s| Selector::parse(s).ok().and_then(|sel| doc.select(&sel).next()));
    let root = match root {
        Some(r) => r,
        None => return String::new(),
    };
    const SKIP_ANCESTORS: &[&str] = &[
        "nav", "footer", "aside", "header", "script", "style", "noscript",
        "form", "svg", "button", "figure", "template",
    ];
    let block_sel = Selector::parse("h1,h2,h3,h4,h5,h6,p,li,blockquote,pre").unwrap();
    let mut out: Vec<String> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut total = 0usize;
    for el in root.select(&block_sel) {
        // Skip nodes inside chrome ancestors, or nested inside another emitted
        // block (keep only the outermost — avoids <li><p>… double-emit).
        const NESTED_BLOCKS: &[&str] = &["p", "li", "blockquote", "pre"];
        let mut cur = el.parent();
        let mut skip = false;
        while let Some(node) = cur {
            if let Some(e) = node.value().as_element() {
                let n = e.name();
                if SKIP_ANCESTORS.contains(&n) || NESTED_BLOCKS.contains(&n) {
                    skip = true;
                    break;
                }
            }
            cur = node.parent();
        }
        if skip {
            continue;
        }
        let txt = el.text().collect::<String>();
        let txt = txt.split_whitespace().collect::<Vec<_>>().join(" ");
        if txt.len() < 2 {
            continue;
        }
        let line = match el.value().name() {
            "h1" => format!("# {txt}"),
            "h2" => format!("## {txt}"),
            "h3" => format!("### {txt}"),
            "h4" | "h5" | "h6" => format!("#### {txt}"),
            "li" => format!("- {txt}"),
            "blockquote" => format!("> {txt}"),
            _ => txt,
        };
        if !seen.insert(line.clone()) {
            continue; // drop duplicated nav/list lines
        }
        total += line.len();
        out.push(line);
        if total > 20_000 {
            break; // cap payload size over the IPC boundary
        }
    }
    // Structural extraction was too thin (JS-only page, odd markup) → flat text.
    if out.join(" ").trim().len() < 160 {
        return flat_body_text(&doc);
    }
    out.join("\n\n")
}

/// Whitespace-collapsed body text with script/style/noscript dropped — the
/// last-resort fallback when structural extraction finds too little.
pub fn flat_body_text(doc: &Html) -> String {
    let body_sel = Selector::parse("body").unwrap();
    let skip_sel = Selector::parse("script, style, noscript").unwrap();
    let skip: std::collections::HashSet<_> =
        doc.select(&skip_sel).flat_map(|el| el.text()).collect();
    let text: String = match doc.select(&body_sel).next() {
        Some(b) => b.text().filter(|t| !skip.contains(t)).collect::<Vec<_>>().join(" "),
        None => String::new(),
    };
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed.chars().take(20_000).collect()
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
        // Small doc → structural output is under the 160-char floor → flat fallback.
        let html = "<html><head><style>x{}</style></head><body><h1>Hi</h1><script>bad()</script><p>World</p></body></html>";
        let t = strip_html(html);
        assert!(t.contains("Hi"));
        assert!(t.contains("World"));
        assert!(!t.contains("bad()"));
    }

    #[test]
    fn extracts_main_content_as_markdown() {
        let html = r#"<html><body>
          <nav><a href="/x">Home</a><a href="/y">Login</a></nav>
          <header><h1>Site Banner</h1></header>
          <main>
            <h1>NVIDIA Annual Report 2025</h1>
            <p>Revenue for fiscal 2025 grew substantially, driven by data-center demand across the full year of operations and continued platform adoption.</p>
            <h2>Segment results</h2>
            <ul><li>Data Center up sharply year over year</li><li>Gaming steady across the period</li></ul>
            <script>track()</script>
          </main>
          <footer><p>Copyright notice and cookie policy and privacy links here.</p></footer>
        </body></html>"#;
        let md = strip_html(html);
        // Main content is captured with markdown structure...
        assert!(md.contains("# NVIDIA Annual Report 2025"));
        assert!(md.contains("## Segment results"));
        assert!(md.contains("- Data Center up sharply"));
        assert!(md.contains("Revenue for fiscal 2025"));
        // ...and chrome is dropped.
        assert!(!md.contains("Login"));
        assert!(!md.contains("Site Banner"));
        assert!(!md.contains("cookie policy"));
        assert!(!md.contains("track()"));
    }
}
