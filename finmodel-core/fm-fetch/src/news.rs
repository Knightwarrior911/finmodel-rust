//! Company/topic news headlines via the public Google News RSS endpoint.
//!
//! Only the direct-fetch tier is ported from the Python `news.py` — the
//! anti-bot / browser tiers route through the Roam MCP browser (Phase 8). RSS
//! is a clean, key-free feed: `title`, `link`, `source`, `pubDate` per item.

use std::sync::LazyLock;
use std::time::{SystemTime, UNIX_EPOCH};

use quick_xml::Reader;
use quick_xml::events::Event;
use regex::Regex;
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

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// A recency window parsed from a natural-language news query.
#[derive(Clone, Debug)]
struct Recency {
    /// Google News `when:` operator token, e.g. `when:24h` / `when:7d`.
    when_op: String,
    /// Max item age in seconds — a client-side safety net so stale results
    /// (e.g. a years-old article Google mis-ranks) never survive a "last 24h"
    /// query even when the server ignores the `when:` filter.
    max_age_secs: i64,
    /// Byte span of the matched phrase in the original query, stripped so the
    /// search text is a clean topic and not a full sentence.
    span: std::ops::Range<usize>,
}

static RE_WINDOW: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\b(?:in |over |within |during |from )?(?:the )?(?:last|past|previous|latest|recent)\s+(\d+)\s*(hour|hr|day|week|month|year)s?\b",
    )
    .expect("valid window regex")
});

static RE_KEYWORD: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(today|yesterday|this week|past week|this month|this year)\b")
        .expect("valid keyword regex")
});

static RE_LEAD: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)^\s*(?:please\s+)?(?:can you\s+)?(?:search(?:\s+the\s+web)?(?:\s+for)?|look\s+up|find\s+me|get\s+me|show\s+me|give\s+me|tell\s+me\s+about)\s+",
    )
    .expect("valid lead regex")
});

static RE_PUBDATE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(\d{1,2})\s+([A-Za-z]{3,})\s+(\d{4})\s+(\d{1,2}):(\d{2})(?::(\d{2}))?")
        .expect("valid pubdate regex")
});

/// Detect a "last N hours/days", "today", "this week", … window in `query`.
fn parse_recency(query: &str) -> Option<Recency> {
    if let Some(c) = RE_WINDOW.captures(query) {
        let n: i64 = c[1].parse().unwrap_or(1).max(1);
        let unit = c[2].to_ascii_lowercase();
        let (when_op, secs) = match unit.as_str() {
            "hour" | "hr" => (format!("when:{}h", n.clamp(1, 72)), n * 3_600),
            "day" => (format!("when:{}d", n.clamp(1, 60)), n * 86_400),
            "week" => (format!("when:{}d", (n * 7).clamp(1, 90)), n * 7 * 86_400),
            "month" => (format!("when:{}d", (n * 30).clamp(1, 365)), n * 30 * 86_400),
            _ => (format!("when:{}d", (n * 365).clamp(1, 365)), n * 365 * 86_400),
        };
        return Some(Recency {
            when_op,
            max_age_secs: secs,
            span: c.get(0).unwrap().range(),
        });
    }
    if let Some(c) = RE_KEYWORD.captures(query) {
        let kw = c[1].to_ascii_lowercase();
        let (when_op, secs) = match kw.as_str() {
            "today" => ("when:1d".to_string(), 86_400),
            "yesterday" => ("when:2d".to_string(), 2 * 86_400),
            "this week" | "past week" => ("when:7d".to_string(), 7 * 86_400),
            "this month" => ("when:30d".to_string(), 30 * 86_400),
            _ => ("when:1y".to_string(), 365 * 86_400),
        };
        return Some(Recency {
            when_op,
            max_age_secs: secs,
            span: c.get(0).unwrap().range(),
        });
    }
    None
}

/// Strip a matched recency phrase and common leading filler so the query sent to
/// Google News is a clean topic. Falls back to the trimmed original if cleaning
/// empties it.
fn clean_query(query: &str, span: Option<std::ops::Range<usize>>) -> String {
    let mut s = query.to_string();
    if let Some(r) = span {
        if r.end <= s.len() {
            s.replace_range(r, " ");
        }
    }
    let s = RE_LEAD.replace(&s, "");
    let cleaned = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if cleaned.is_empty() {
        query.trim().to_string()
    } else {
        cleaned
    }
}

/// Days since the Unix epoch for a civil date (Howard Hinnant's algorithm).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn month_num(name: &str) -> Option<i64> {
    let lower = name.to_ascii_lowercase();
    let key: &str = &lower[..lower.len().min(3)];
    Some(match key {
        "jan" => 1,
        "feb" => 2,
        "mar" => 3,
        "apr" => 4,
        "may" => 5,
        "jun" => 6,
        "jul" => 7,
        "aug" => 8,
        "sep" => 9,
        "oct" => 10,
        "nov" => 11,
        "dec" => 12,
        _ => return None,
    })
}

/// Parse an RFC-2822 `pubDate` (e.g. `Mon, 14 Jul 2026 09:00:00 GMT`) to Unix
/// seconds (UTC). Google News timestamps are always GMT.
fn pubdate_epoch(s: &str) -> Option<i64> {
    let c = RE_PUBDATE.captures(s)?;
    let d: i64 = c[1].parse().ok()?;
    let mon = month_num(&c[2])?;
    let y: i64 = c[3].parse().ok()?;
    let hh: i64 = c[4].parse().ok()?;
    let mm: i64 = c[5].parse().ok()?;
    let ss: i64 = c.get(6).and_then(|m| m.as_str().parse().ok()).unwrap_or(0);
    Some(days_from_civil(y, mon, d) * 86_400 + hh * 3_600 + mm * 60 + ss)
}

/// Drop items whose `pubDate` is older than `max_age_secs` before `now_secs`.
/// Undated items are kept (Google usually dates them; absence is not staleness).
fn filter_recent(items: Vec<Headline>, now_secs: i64, max_age_secs: i64) -> Vec<Headline> {
    items
        .into_iter()
        .filter(|h| match h.published.as_deref().and_then(pubdate_epoch) {
            Some(ep) => now_secs.saturating_sub(ep) <= max_age_secs,
            None => true,
        })
        .collect()
}

/// Fetch up to `limit` headlines for `query` from Google News RSS.
///
/// A natural-language recency window ("in the last 24 hours", "today", "past
/// week") is translated to Google News' `when:` operator so the feed is
/// restricted server-side, then enforced again client-side against each item's
/// `pubDate` so stale results never leak through. Leading filler ("search the
/// web for …") is stripped so the search text is a clean topic.
pub fn fetch_headlines(query: &str, limit: usize) -> Result<Vec<Headline>, FetchError> {
    let recency = parse_recency(query);
    let topic = clean_query(query, recency.as_ref().map(|r| r.span.clone()));
    let q = match &recency {
        Some(r) => format!("{topic} {}", r.when_op),
        None => topic,
    };
    let url = format!("{GNEWS_RSS}{}&hl=en-US&gl=US&ceid=US:en", urlencode(&q));
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
    match recency {
        // Over-fetch, enforce the window client-side, then truncate to `limit`.
        Some(r) => {
            let cap = limit.saturating_mul(4).max(20);
            let items = parse_rss(&body, cap);
            // 6h slack absorbs timezone/rounding at the window edge.
            let mut items = filter_recent(items, now_secs(), r.max_age_secs + 21_600);
            items.truncate(limit);
            Ok(items)
        }
        None => Ok(parse_rss(&body, limit)),
    }
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
        assert_eq!(
            hs[0].url,
            "https://www.reuters.com/markets/acme-widget-deal"
        );
        assert!(hs[0].title.starts_with("Acme acquires"));
        assert_eq!(
            hs[0].published.as_deref(),
            Some("Mon, 14 Jul 2026 09:00:00 GMT")
        );
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

    #[test]
    fn parses_last_24_hours_window() {
        let r = parse_recency("m&a announcements in the last 24 hours").unwrap();
        assert_eq!(r.when_op, "when:24h");
        assert_eq!(r.max_age_secs, 86_400);
    }

    #[test]
    fn parses_day_and_keyword_windows() {
        assert_eq!(parse_recency("news past 3 days").unwrap().when_op, "when:3d");
        assert_eq!(parse_recency("deals today").unwrap().when_op, "when:1d");
        assert_eq!(
            parse_recency("filings this week").unwrap().when_op,
            "when:7d"
        );
        assert!(parse_recency("apple earnings").is_none());
    }

    #[test]
    fn clean_query_strips_window_and_filler() {
        let src = "search the web for m&a announcement in last 24 hours";
        let span = parse_recency(src).map(|r| r.span);
        assert_eq!(clean_query(src, span), "m&a announcement");
    }

    #[test]
    fn clean_query_keeps_topic_without_command() {
        assert_eq!(clean_query("Find Inc merger", None), "Find Inc merger");
    }

    #[test]
    fn pubdate_epoch_parses_rfc2822() {
        // 2026-07-14T09:00:00Z = 1_784_019_600
        assert_eq!(
            pubdate_epoch("Mon, 14 Jul 2026 09:00:00 GMT"),
            Some(1_784_019_600)
        );
        assert_eq!(pubdate_epoch("garbage"), None);
    }

    #[test]
    fn filter_recent_drops_stale_keeps_fresh_and_undated() {
        let now = 1_784_020_000; // just after 2026-07-14 09:00Z
        let items = vec![
            Headline {
                title: "fresh".into(),
                published: Some("Mon, 14 Jul 2026 09:00:00 GMT".into()),
                ..Default::default()
            },
            Headline {
                title: "stale".into(),
                published: Some("Mon, 09 Oct 2006 07:00:00 GMT".into()),
                ..Default::default()
            },
            Headline {
                title: "undated".into(),
                published: None,
                ..Default::default()
            },
        ];
        let kept = filter_recent(items, now, 86_400);
        let titles: Vec<_> = kept.iter().map(|h| h.title.as_str()).collect();
        assert_eq!(titles, vec!["fresh", "undated"]);
    }
}
