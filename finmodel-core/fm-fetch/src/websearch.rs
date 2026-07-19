//! Plain-HTTP web search + page fetch — the non-MCP fallback for the search
//! facade. Engine chain: DuckDuckGo HTML → Bing HTML → Mojeek — a research
//! product cannot go blind because one engine throttles (DDG answers rate
//! limits with an HTTP 202 "anomaly" challenge page that parses to zero
//! hits; treating that as success silently blinded every research run).
//! A page fetch + minimal tag-strip good enough for non-protected pages
//! (protected pages need the Roam MCP path).

use std::sync::LazyLock;
use std::time::Duration;

use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};

use crate::cache::{SEARCH_CACHE, search_key};
use crate::market::FetchError;
use crate::retry::{RetryClass, classify_status as retry_class, with_retries};

/// A ranked web search result.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct WebHit {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

/// Shared blocking client (one TLS/cookie pool) for BasicHttp search + page fetch.
static WEB_CLIENT: LazyLock<reqwest::blocking::Client> = LazyLock::new(|| {
    use reqwest::header::{
        ACCEPT, ACCEPT_LANGUAGE, HeaderMap, HeaderValue, UPGRADE_INSECURE_REQUESTS,
    };
    let mut headers = HeaderMap::new();
    headers.insert(
        ACCEPT,
        HeaderValue::from_static(
            "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,*/*;q=0.8",
        ),
    );
    headers.insert(ACCEPT_LANGUAGE, HeaderValue::from_static("en-US,en;q=0.9"));
    headers.insert(UPGRADE_INSECURE_REQUESTS, HeaderValue::from_static("1"));
    reqwest::blocking::Client::builder()
        .user_agent(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
             (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36",
        )
        .default_headers(headers)
        .cookie_store(true)
        .timeout(Duration::from_secs(20))
        .build()
        .unwrap_or_else(|_| reqwest::blocking::Client::new())
});

fn client() -> &'static reqwest::blocking::Client {
    &WEB_CLIENT
}

/// Multi-engine web search → up to `limit` structured hits. Tries DDG,
/// then Bing HTML, then Mojeek; the first engine returning hits wins. Only a
/// non-empty result is cached, so a throttled engine never poisons the cache.
pub fn web_search(query: &str, limit: usize) -> Result<Vec<WebHit>, FetchError> {
    let key = search_key("basic", query);
    if let Some(cached) = SEARCH_CACHE.get(&key)
        && let Ok(mut hits) = serde_json::from_str::<Vec<WebHit>>(&cached)
        && !hits.is_empty()
    {
        hits.truncate(limit);
        return Ok(hits);
    }
    let mut last_err: Option<FetchError> = None;
    let engines: [(&str, fn(&str) -> Result<Vec<WebHit>, FetchError>); 3] = [
        ("ddg", search_ddg),
        ("bing", search_bing),
        ("mojeek", search_mojeek),
    ];
    for (_name, engine) in engines {
        match engine(query) {
            Ok(hits) if !hits.is_empty() => {
                if let Ok(blob) = serde_json::to_string(&hits) {
                    SEARCH_CACHE.insert(key, blob);
                }
                let mut hits = hits;
                hits.truncate(limit);
                return Ok(hits);
            }
            Ok(_) => {} // empty page (blocked/challenge) — fall through
            Err(e) => last_err = Some(e),
        }
    }
    match last_err {
        // Every engine errored — surface the last error.
        Some(e) => Err(e),
        // Engines answered but with zero hits (obscure query) — honest empty.
        None => Ok(Vec::new()),
    }
}

/// One engine request with the shared retry/backoff policy. `is_challenge`
/// lets an engine mark "2xx but actually a block page" (DDG's 202).
fn engine_get(
    build: impl Fn() -> reqwest::blocking::RequestBuilder,
    is_challenge: impl Fn(u16) -> bool,
) -> Result<String, FetchError> {
    with_retries(|| match build().send() {
        Ok(resp) => {
            let status = resp.status();
            let code = status.as_u16();
            if is_challenge(code) {
                drop(resp);
                return Err((
                    true,
                    None,
                    FetchError::Network(format!("challenge HTTP {code}")),
                ));
            }
            match retry_class(code) {
                RetryClass::Success => resp
                    .text()
                    .map_err(|e| (false, None, FetchError::Parse(e.to_string()))),
                RetryClass::Retriable => {
                    let ra = resp
                        .headers()
                        .get("retry-after")
                        .and_then(|v| v.to_str().ok())
                        .map(|s| s.to_string());
                    drop(resp);
                    Err((true, ra, FetchError::Network(format!("HTTP {status}"))))
                }
                RetryClass::Terminal => {
                    drop(resp);
                    Err((false, None, FetchError::Network(format!("HTTP {status}"))))
                }
            }
        }
        Err(e) => Err((true, None, FetchError::Network(e.to_string()))),
    })
}

fn search_ddg(query: &str) -> Result<Vec<WebHit>, FetchError> {
    // DDG signals rate limiting with HTTP 202 + an "anomaly" challenge page —
    // a 2xx that must NOT be treated as success.
    let html = engine_get(
        || {
            client()
                .post("https://html.duckduckgo.com/html/")
                .form(&[("q", query), ("kl", "us-en")])
        },
        |code| code == 202,
    )?;
    Ok(parse_ddg_hits(&html))
}

fn search_bing(query: &str) -> Result<Vec<WebHit>, FetchError> {
    // The RSS format serves real organic results to plain HTTP clients even
    // when Bing's HTML endpoint walls them behind a JS shell (verified live
    // while every HTML engine on this network was challenge-blocked).
    let xml = engine_get(
        || {
            client()
                .get("https://www.bing.com/search")
                .query(&[("q", query), ("format", "rss")])
        },
        |_| false,
    )?;
    Ok(parse_bing_rss(&xml))
}

fn search_mojeek(query: &str) -> Result<Vec<WebHit>, FetchError> {
    let html = engine_get(
        || {
            client()
                .get("https://www.mojeek.com/search")
                .query(&[("q", query)])
        },
        |_| false,
    )?;
    Ok(parse_mojeek_hits(&html))
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
        out.push(WebHit {
            title,
            url,
            snippet,
        });
    }
    out
}

/// Parse Bing RSS results into hits (pure). Items whose link points back at
/// bing.com (the query echo) are skipped.
pub fn parse_bing_rss(xml: &str) -> Vec<WebHit> {
    fn tag(item: &str, name: &str) -> String {
        let open = format!("<{name}>");
        let close = format!("</{name}>");
        item.split(&open)
            .nth(1)
            .and_then(|rest| rest.split(&close).next())
            .map(|v| xml_unescape(v.trim()))
            .unwrap_or_default()
    }
    let mut out = Vec::new();
    for item in xml.split("<item>").skip(1) {
        let url = tag(item, "link");
        if !url.starts_with("http") || url.contains("bing.com") {
            continue;
        }
        out.push(WebHit {
            title: tag(item, "title"),
            url,
            snippet: tag(item, "description"),
        });
    }
    out
}

/// Minimal XML entity decode for RSS text nodes.
fn xml_unescape(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
}

/// Parse Mojeek organic results (`ul.results-standard li`) into hits (pure).
pub fn parse_mojeek_hits(html: &str) -> Vec<WebHit> {
    let doc = Html::parse_document(html);
    let (Ok(row), Ok(a_sel), Ok(snip)) = (
        Selector::parse("ul.results-standard li, li.result"),
        Selector::parse("h2 a, a.title"),
        Selector::parse("p.s, p.desc"),
    ) else {
        return vec![];
    };
    let mut out = Vec::new();
    for res in doc.select(&row) {
        let Some(a) = res.select(&a_sel).next() else {
            continue;
        };
        let url = a.value().attr("href").unwrap_or("").trim().to_string();
        if !url.starts_with("http") {
            continue;
        }
        out.push(WebHit {
            title: a.text().collect::<String>().trim().to_string(),
            url,
            snippet: res
                .select(&snip)
                .next()
                .map(|s| s.text().collect::<String>().trim().to_string())
                .unwrap_or_default(),
        });
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

/// Classification of a page fetch — success, bot-blocked, or too-thin content.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PageStatus {
    #[default]
    Ok,
    Blocked,
    Thin,
}

/// A fetched page: extracted title, readable text, and a status classification.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct FetchedPage {
    pub title: String,
    pub text: String,
    pub status: PageStatus,
}

/// Map an HTTP status code to a page classification. 403/429/503 are the
/// canonical bot-block / rate-limit / temporary-unavailable codes.
pub fn classify_status(code: u16) -> PageStatus {
    match code {
        403 | 429 | 503 => PageStatus::Blocked,
        _ => PageStatus::Ok,
    }
}

/// Extract a page title: `<title>` text, else first `<h1>`, else "".
fn extract_title(html: &str) -> String {
    let doc = Html::parse_document(html);
    for sel in ["title", "h1"] {
        if let Ok(s) = Selector::parse(sel)
            && let Some(el) = doc.select(&s).next()
        {
            let t = el.text().collect::<String>();
            let t = t.split_whitespace().collect::<Vec<_>>().join(" ");
            if !t.is_empty() {
                return t;
            }
        }
    }
    String::new()
}

/// Fetch a page → title + readable text + status. Bot-blocked responses
/// (403/429/503) return `Blocked` with empty text rather than erroring; pages
/// whose extracted text is under 200 chars return `Thin` (partial text kept).
/// Non-protected static/SSR pages return `Ok`. Protected/JS-only pages that
/// slip through as `Blocked`/`Thin` need the Roam MCP `read_markdown` path.
pub fn fetch_page(url: &str) -> Result<FetchedPage, FetchError> {
    // Page caching deferred until the fetch path returns a validated canonical
    // final URL (redirects must not cache public→private content under the
    // request URL). Shared client still pools TLS connections.
    //
    // Bounded retry (Phase 3.4) covers ONLY the initial send: connection
    // failure and 408 / 500 / 502 / 504. 403/429/503 remain the terminal
    // "blocked" outcome (bot detection — retrying the same client won't help),
    // and body decode / thin / parse stay terminal below.
    let resp = with_retries(|| match client().get(url).send() {
        Ok(resp) => {
            let code = resp.status().as_u16();
            let is_blocked = classify_status(code) == PageStatus::Blocked;
            let transient = !is_blocked && matches!(retry_class(code), RetryClass::Retriable);
            if transient {
                Err((true, None, FetchError::Network(format!("HTTP {code}"))))
            } else {
                Ok(resp)
            }
        }
        Err(e) => Err((true, None, FetchError::Network(e.to_string()))),
    })?;
    if classify_status(resp.status().as_u16()) == PageStatus::Blocked {
        return Ok(FetchedPage {
            title: String::new(),
            text: String::new(),
            status: PageStatus::Blocked,
        });
    }
    let html = resp
        .error_for_status()
        .map_err(|e| FetchError::Network(e.to_string()))?
        .text()
        .map_err(|e| FetchError::Parse(e.to_string()))?;
    let title = extract_title(&html);
    let text = strip_html(&html);
    let status = if text.trim().len() < 200 {
        PageStatus::Thin
    } else {
        PageStatus::Ok
    };
    Ok(FetchedPage {
        title,
        text,
        status,
    })
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
        .find_map(|s| {
            Selector::parse(s)
                .ok()
                .and_then(|sel| doc.select(&sel).next())
        });
    let root = match root {
        Some(r) => r,
        None => return String::new(),
    };
    const SKIP_ANCESTORS: &[&str] = &[
        "nav", "footer", "aside", "header", "script", "style", "noscript", "form", "svg", "button",
        "figure", "template",
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
        Some(b) => b
            .text()
            .filter(|t| !skip.contains(t))
            .collect::<Vec<_>>()
            .join(" "),
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

    #[test]
    fn classify_status_flags_block_codes() {
        assert_eq!(classify_status(403), PageStatus::Blocked);
        assert_eq!(classify_status(429), PageStatus::Blocked);
        assert_eq!(classify_status(503), PageStatus::Blocked);
        assert_eq!(classify_status(200), PageStatus::Ok);
        assert_eq!(classify_status(404), PageStatus::Ok);
        assert_eq!(classify_status(500), PageStatus::Ok);
    }

    #[test]
    fn extract_title_prefers_title_then_h1() {
        assert_eq!(
            extract_title(
                "<html><head><title> Hello World </title></head><body><h1>Nope</h1></body></html>"
            ),
            "Hello World"
        );
        assert_eq!(
            extract_title("<html><body><h1>Fallback H1</h1><p>x</p></body></html>"),
            "Fallback H1"
        );
        assert_eq!(
            extract_title("<html><body><p>no title</p></body></html>"),
            ""
        );
    }

    #[test]
    fn page_status_serializes_lowercase() {
        assert_eq!(serde_json::to_string(&PageStatus::Ok).unwrap(), "\"ok\"");
        assert_eq!(
            serde_json::to_string(&PageStatus::Blocked).unwrap(),
            "\"blocked\""
        );
        assert_eq!(
            serde_json::to_string(&PageStatus::Thin).unwrap(),
            "\"thin\""
        );
    }
}

#[cfg(test)]
mod engine_tests {
    use super::*;

    #[test]
    fn parse_bing_rss_extracts_and_skips_query_echo() {
        let xml = r#"<rss><channel>
          <item><title>Bing echo</title><link>http://www.bing.com:80/search?q=x</link></item>
          <item><title>Tesla IR &amp; updates</title><link>https://ir.tesla.com/press</link><description>Investor relations home.</description></item>
          <item><title>TSLA transcript</title><link>https://www.fool.com/earnings/call-transcripts/tsla</link><description>Q1 call.</description></item>
        </channel></rss>"#;
        let hits = parse_bing_rss(xml);
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].url, "https://ir.tesla.com/press");
        assert_eq!(hits[0].title, "Tesla IR & updates");
        assert_eq!(hits[1].snippet, "Q1 call.");
    }

    #[test]
    fn parse_mojeek_hits_extracts() {
        let html = r#"<ul class="results-standard">
          <li><h2><a href="https://www.sec.gov/tsla">TSLA filings</a></h2><p class="s">EDGAR filings.</p></li>
          <li><h2><a href="/ads/click">ad</a></h2></li>
        </ul>"#;
        let hits = parse_mojeek_hits(html);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].url, "https://www.sec.gov/tsla");
        assert_eq!(hits[0].snippet, "EDGAR filings.");
    }

    #[test]
    fn empty_or_challenge_pages_parse_to_zero_hits() {
        assert!(parse_bing_rss("<html><body>captcha</body></html>").is_empty());
        assert!(parse_mojeek_hits("").is_empty());
        assert!(parse_ddg_hits("<html>anomaly challenge</html>").is_empty());
    }
}
