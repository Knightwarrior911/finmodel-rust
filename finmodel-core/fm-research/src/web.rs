//! Web-search facade (Phase 8.3): a unified search + page-read surface backed by
//! the Roam MCP browser when a client is supplied, else a plain-HTTP fallback
//! (DDG search + tag-stripped GET). Results are ranked through the Phase-5
//! priority/skip domain sets.

use serde::{Deserialize, Serialize};

use crate::scoring::PRIORITY_DOMAINS;

/// One ranked search result.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SearchHit {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

/// Search the web. With an MCP client, uses Roam's `web_search` tool; otherwise
/// the DDG HTTP fallback. Hits are re-ordered via [`rank_urls`] (priority
/// domains first, skip domains dropped).
pub fn web_search(
    query: &str,
    mcp: Option<&mut fm_mcp::McpClient>,
) -> Result<Vec<SearchHit>, String> {
    let hits: Vec<SearchHit> = match mcp {
        Some(client) => {
            let res = client
                .call_tool("web_search", serde_json::json!({ "query": query }))
                .map_err(|e| e.to_string())?;
            parse_mcp_search(&res)
        }
        None => fm_fetch::websearch::web_search(query, 25)
            .map_err(|e| e.to_string())?
            .into_iter()
            .map(|h| SearchHit { title: h.title, url: h.url, snippet: h.snippet })
            .collect(),
    };
    Ok(rank_hits(hits))
}

/// Search-engine plumbing / junk that no user wants back from a search card
/// (SERP redirects, tracking, sign-in). Content domains (wikipedia, reddit,
/// youtube, …) are deliberately KEPT here — unlike the filings-discovery
/// ranker, general search must not drop them.
const WEB_JUNK: &[&str] = &[
    "gstatic.com", "googleapis.com", "accounts.google", "maps.google",
    "translate.google", "google.com/search", "google.com/sorry",
    "google.com/webhp", "google.com/preferences", "google.com/intl",
    "bing.com/search", "bing.com/ck", "duckduckgo.com/l/", "duckduckgo.com/y.js",
];

/// Read a page as markdown/text. With an MCP client, Roam's `read_markdown`
/// (passing the optional BM25 `query` for relevant-passage extraction); else a
/// plain GET + tag-strip (protected pages need the MCP path — error says so).
pub fn read_page(
    url: &str,
    query: Option<&str>,
    mcp: Option<&mut fm_mcp::McpClient>,
) -> Result<String, String> {
    match mcp {
        Some(client) => {
            let mut args = serde_json::json!({ "url": url });
            if let Some(q) = query {
                args["query"] = serde_json::json!(q);
            }
            let res = client
                .call_tool("read_markdown", args)
                .map_err(|e| e.to_string())?;
            Ok(extract_mcp_text(&res))
        }
        None => fm_fetch::websearch::fetch_page_text(url).map_err(|e| e.to_string()),
    }
}

/// Drop only SERP chrome/junk, then stably float priority (financial/newswire)
/// domains to the front — content domains (wikipedia/reddit/…) are preserved.
fn rank_hits(hits: Vec<SearchHit>) -> Vec<SearchHit> {
    let is_junk = |u: &str| WEB_JUNK.iter().any(|d| u.contains(d));
    let is_priority = |u: &str| PRIORITY_DOMAINS.iter().any(|d| u.contains(d));
    let kept: Vec<SearchHit> = hits.into_iter().filter(|h| !is_junk(&h.url)).collect();
    let mut out: Vec<SearchHit> = kept.iter().filter(|h| is_priority(&h.url)).cloned().collect();
    out.extend(kept.into_iter().filter(|h| !is_priority(&h.url)));
    out
}

/// Parse a Roam `web_search` tool result into hits. Roam's exact shape is
/// unverified, so this is defensive: accept a top-level or `content`-nested
/// `results`/`hits` array of `{title,url/link,snippet/description}`.
pub fn parse_mcp_search(result: &serde_json::Value) -> Vec<SearchHit> {
    fn arr_from(v: &serde_json::Value) -> Option<&Vec<serde_json::Value>> {
        for key in ["results", "hits", "data"] {
            if let Some(a) = v.get(key).and_then(|x| x.as_array()) {
                return Some(a);
            }
        }
        v.as_array()
    }
    // Roam often wraps in content[].text as a JSON string or structured data.
    let structured = arr_from(result)
        .cloned()
        .or_else(|| {
            result
                .get("content")
                .and_then(|c| c.as_array())
                .and_then(|items| items.iter().find_map(|it| it.get("data").and_then(|d| d.as_array()).cloned())
                    .or_else(|| items.iter().find_map(|it| {
                        it.get("text")
                            .and_then(|t| t.as_str())
                            .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
                            .and_then(|v| arr_from(&v).cloned())
                    })))
        })
        .unwrap_or_default();
    structured
        .iter()
        .filter_map(|h| {
            let url = h
                .get("url")
                .or_else(|| h.get("link"))
                .and_then(|u| u.as_str())?
                .to_string();
            let title = h
                .get("title")
                .and_then(|t| t.as_str())
                .unwrap_or("")
                .to_string();
            let snippet = h
                .get("snippet")
                .or_else(|| h.get("description"))
                .and_then(|s| s.as_str())
                .unwrap_or("")
                .to_string();
            Some(SearchHit { title, url, snippet })
        })
        .collect()
}

/// Extract text from a Roam `read_markdown` result (content[].text join, else
/// a `markdown`/`text` field, else the raw JSON string).
pub fn extract_mcp_text(result: &serde_json::Value) -> String {
    if let Some(items) = result.get("content").and_then(|c| c.as_array()) {
        let joined: String = items
            .iter()
            .filter_map(|it| it.get("text").and_then(|t| t.as_str()))
            .collect::<Vec<_>>()
            .join("\n");
        if !joined.is_empty() {
            return joined;
        }
    }
    for key in ["markdown", "text", "content"] {
        if let Some(s) = result.get(key).and_then(|v| v.as_str()) {
            return s.to_string();
        }
    }
    result.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranks_fallback_hits_priority_first() {
        let hits = vec![
            SearchHit { title: "a".into(), url: "https://example.com/x".into(), snippet: String::new() },
            SearchHit { title: "r".into(), url: "https://www.reuters.com/y".into(), snippet: String::new() },
            SearchHit { title: "wiki".into(), url: "https://en.wikipedia.org/z".into(), snippet: String::new() },
            SearchHit { title: "junk".into(), url: "https://duckduckgo.com/l/?uddg=x".into(), snippet: String::new() },
        ];
        let ranked = rank_hits(hits);
        // SERP junk dropped; content domains (wikipedia) KEPT — unlike the
        // filings ranker.
        assert_eq!(ranked.len(), 3);
        assert_eq!(ranked[0].url, "https://www.reuters.com/y"); // priority floated first
        assert!(ranked.iter().any(|h| h.url.contains("wikipedia.org")));
        assert!(!ranked.iter().any(|h| h.url.contains("duckduckgo.com/l/")));
    }

    #[test]
    fn parses_mcp_results_shapes() {
        let structured = serde_json::json!({ "results": [
            { "title": "T1", "url": "https://a.com", "snippet": "s1" },
            { "title": "T2", "link": "https://b.com", "description": "s2" }
        ]});
        let hits = parse_mcp_search(&structured);
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].url, "https://a.com");
        assert_eq!(hits[1].url, "https://b.com");
        assert_eq!(hits[1].snippet, "s2");
        // content[].text as embedded JSON string
        let nested = serde_json::json!({ "content": [
            { "type": "text", "text": "{\"hits\":[{\"title\":\"N\",\"url\":\"https://c.com\"}]}" }
        ]});
        let hits = parse_mcp_search(&nested);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].url, "https://c.com");
    }

    #[test]
    fn extracts_mcp_text() {
        let r = serde_json::json!({ "content": [{ "type": "text", "text": "# Page\nBody" }] });
        assert_eq!(extract_mcp_text(&r), "# Page\nBody");
        let r2 = serde_json::json!({ "markdown": "hello" });
        assert_eq!(extract_mcp_text(&r2), "hello");
    }
}
