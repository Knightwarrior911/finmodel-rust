//! Company/topic news headlines via the public Google News RSS endpoint.
//!
//! Only the direct-fetch tier is ported from the Python `news.py` — the
//! anti-bot / browser tiers route through the Roam MCP browser (Phase 8). RSS
//! is a clean, key-free feed: `title`, `link`, `source`, `pubDate` per item.

use quick_xml::events::Event;
use quick_xml::Reader;
use serde::{Deserialize, Serialize};

use crate::market::FetchError;

const GNEWS_RSS: &str = "https://news.google.com/rss/search?q=";

/// One news headline.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Headline {
    pub title: String,
    pub source: String,
    pub url: String,
    pub published: Option<String>,
}

fn urlencode(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

/// Fetch up to `limit` headlines for `query` from Google News RSS.
pub fn fetch_headlines(query: &str, limit: usize) -> Result<Vec<Headline>, FetchError> {
    let url = format!("{GNEWS_RSS}{}&hl=en-US&gl=US&ceid=US:en", urlencode(query));
    let client = reqwest::blocking::Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) finmodel/0.1")
        .build()
        .map_err(|e| FetchError::Network(e.to_string()))?;
    let body = client
        .get(&url)
        .send()
        .map_err(|e| FetchError::Network(e.to_string()))?
        .error_for_status()
        .map_err(|e| FetchError::Network(e.to_string()))?
        .text()
        .map_err(|e| FetchError::Parse(e.to_string()))?;
    Ok(parse_rss(&body, limit))
}

/// Parse Google News RSS `<item>`s into headlines (pure — unit-testable).
/// Stops after `limit` items.
pub fn parse_rss(xml: &str, limit: usize) -> Vec<Headline> {
    let mut reader = Reader::from_str(xml);
    reader.trim_text(true);
    let mut buf = Vec::new();
    let mut out: Vec<Headline> = Vec::new();
    let mut in_item = false;
    let mut tag = String::new();
    let mut cur = Headline::default();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let local = String::from_utf8_lossy(e.name().as_ref()).to_string();
                if local == "item" {
                    in_item = true;
                    cur = Headline::default();
                } else if in_item {
                    tag = local;
                }
            }
            Ok(Event::Text(e)) if in_item => {
                let txt = e.unescape().unwrap_or_default().to_string();
                match tag.as_str() {
                    "title" => cur.title.push_str(&txt),
                    "link" => cur.url.push_str(&txt),
                    "source" => cur.source.push_str(&txt),
                    "pubDate" => match &mut cur.published {
                        Some(p) => p.push_str(&txt),
                        None => cur.published = Some(txt),
                    },
                    _ => {}
                }
            }
            Ok(Event::CData(e)) if in_item => {
                let txt = String::from_utf8_lossy(e.as_ref()).to_string();
                match tag.as_str() {
                    "title" => cur.title.push_str(&txt),
                    "link" => cur.url.push_str(&txt),
                    "source" => cur.source.push_str(&txt),
                    _ => {}
                }
            }
            Ok(Event::End(e)) => {
                let local = String::from_utf8_lossy(e.name().as_ref()).to_string();
                if local == "item" {
                    in_item = false;
                    if !cur.url.is_empty() || !cur.title.is_empty() {
                        out.push(std::mem::take(&mut cur));
                    }
                    if out.len() >= limit {
                        break;
                    }
                } else {
                    tag.clear();
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = r#"<?xml version="1.0"?>
<rss version="2.0"><channel>
  <title>Google News</title>
  <item>
    <title>Acme acquires Widget Co in $2B deal - Reuters</title>
    <link>https://www.reuters.com/markets/acme-widget-deal</link>
    <pubDate>Mon, 14 Jul 2026 09:00:00 GMT</pubDate>
    <source url="https://www.reuters.com">Reuters</source>
  </item>
  <item>
    <title>Widget Co shareholders approve merger &amp; payout - Bloomberg</title>
    <link>https://www.bloomberg.com/news/widget-merger</link>
    <pubDate>Mon, 14 Jul 2026 10:30:00 GMT</pubDate>
    <source url="https://www.bloomberg.com">Bloomberg</source>
  </item>
</channel></rss>"#;

    #[test]
    fn parses_rss_items() {
        let hs = parse_rss(FIXTURE, 10);
        assert_eq!(hs.len(), 2);
        assert_eq!(hs[0].source, "Reuters");
        assert_eq!(hs[0].url, "https://www.reuters.com/markets/acme-widget-deal");
        assert!(hs[0].title.starts_with("Acme acquires"));
        assert_eq!(hs[0].published.as_deref(), Some("Mon, 14 Jul 2026 09:00:00 GMT"));
        // XML entity decoded.
        assert!(hs[1].title.contains("merger & payout"));
    }

    #[test]
    fn respects_limit() {
        assert_eq!(parse_rss(FIXTURE, 1).len(), 1);
    }

    #[test]
    fn empty_on_garbage() {
        assert!(parse_rss("not xml at all", 5).is_empty());
    }
}
