//! Bounded public-response caches (Phase 3.3).
//!
//! Only unauthenticated BasicHttp / EDGAR / market responses are cached —
//! never Roam/MCP/logged-in content, prompts, keys, PDFs, or LLM output.
//! Each cache is an `Arc`-friendly `Mutex` around an LRU+TTL map; locks are
//! never held across I/O. Expired entries are never served as current.

use std::collections::HashMap;
use std::hash::Hash;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

/// Maximum entries per named public cache.
pub const MAX_ENTRIES: usize = 128;

struct Entry<V> {
    value: V,
    inserted: Instant,
    last_used: Instant,
}

/// In-process LRU + TTL map. Pure relative to an injected `now` when testing;
/// production uses `Instant::now()`.
pub struct TtlLru<K: Eq + Hash + Clone, V> {
    max: usize,
    ttl: Duration,
    map: HashMap<K, Entry<V>>,
}

impl<K: Eq + Hash + Clone, V: Clone> TtlLru<K, V> {
    pub fn new(max: usize, ttl: Duration) -> Self {
        Self {
            max: max.max(1),
            ttl,
            map: HashMap::new(),
        }
    }

    pub fn get(&mut self, key: &K, now: Instant) -> Option<V> {
        self.prune(now);
        match self.map.get_mut(key) {
            Some(e) => {
                e.last_used = now;
                Some(e.value.clone())
            }
            None => None,
        }
    }

    pub fn insert(&mut self, key: K, value: V, now: Instant) {
        self.prune(now);
        if !self.map.contains_key(&key) && self.map.len() >= self.max {
            self.evict_lru();
        }
        self.map.insert(
            key,
            Entry {
                value,
                inserted: now,
                last_used: now,
            },
        );
    }

    fn prune(&mut self, now: Instant) {
        let ttl = self.ttl;
        self.map
            .retain(|_, e| now.saturating_duration_since(e.inserted) <= ttl);
    }

    fn evict_lru(&mut self) {
        if let Some(k) = self
            .map
            .iter()
            .min_by_key(|(_, e)| e.last_used)
            .map(|(k, _)| k.clone())
        {
            self.map.remove(&k);
        }
    }

    #[cfg(test)]
    #[allow(clippy::len_without_is_empty)]
    pub fn len(&self) -> usize {
        self.map.len()
    }
}

/// Thread-safe wrapper: never hold the lock across I/O.
pub struct SyncCache<K: Eq + Hash + Clone, V> {
    inner: Mutex<TtlLru<K, V>>,
}

impl<K: Eq + Hash + Clone, V: Clone> SyncCache<K, V> {
    pub fn new(max: usize, ttl: Duration) -> Self {
        Self {
            inner: Mutex::new(TtlLru::new(max, ttl)),
        }
    }

    pub fn get(&self, key: &K) -> Option<V> {
        let now = Instant::now();
        self.inner.lock().ok().and_then(|mut g| g.get(key, now))
    }

    pub fn insert(&self, key: K, value: V) {
        let now = Instant::now();
        if let Ok(mut g) = self.inner.lock() {
            g.insert(key, value, now);
        }
    }
}

// ── Named public caches (TTLs from the roadmap) ─────────────────────────────

/// Search results: backend+query → serialized hits. 10 min.
pub static SEARCH_CACHE: LazyLock<SyncCache<String, String>> =
    LazyLock::new(|| SyncCache::new(MAX_ENTRIES, Duration::from_secs(10 * 60)));

/// Page excerpts: final_url + excerpt_query → body. 30 min.
pub static PAGE_CACHE: LazyLock<SyncCache<String, String>> =
    LazyLock::new(|| SyncCache::new(MAX_ENTRIES, Duration::from_secs(30 * 60)));

/// EDGAR companyfacts / submissions by CIK/url. 15 min.
pub static EDGAR_CACHE: LazyLock<SyncCache<String, String>> =
    LazyLock::new(|| SyncCache::new(MAX_ENTRIES, Duration::from_secs(15 * 60)));

/// Market quotes by ticker. 60 s.
pub static QUOTE_CACHE: LazyLock<SyncCache<String, String>> =
    LazyLock::new(|| SyncCache::new(MAX_ENTRIES, Duration::from_secs(60)));

/// FX rates by currency pair. 15 min.
pub static FX_CACHE: LazyLock<SyncCache<String, f64>> =
    LazyLock::new(|| SyncCache::new(MAX_ENTRIES, Duration::from_secs(15 * 60)));

/// Normalize a search cache key: `basic|{lowercased trimmed query}`.
pub fn search_key(backend: &str, query: &str) -> String {
    format!(
        "{}|{}",
        backend.trim().to_ascii_lowercase(),
        query
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_ascii_lowercase()
    )
}

/// Normalize a page cache key: `url|query`.
pub fn page_key(url: &str, excerpt_query: &str) -> String {
    format!(
        "{}|{}",
        url.trim(),
        excerpt_query
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_ascii_lowercase()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expires_and_never_serves_stale() {
        let mut c: TtlLru<String, i32> = TtlLru::new(4, Duration::from_secs(10));
        let t0 = Instant::now();
        c.insert("a".into(), 1, t0);
        assert_eq!(c.get(&"a".into(), t0 + Duration::from_secs(10)), Some(1));
        assert_eq!(c.get(&"a".into(), t0 + Duration::from_secs(11)), None);
    }

    #[test]
    fn lru_evicts_coldest() {
        let mut c: TtlLru<String, i32> = TtlLru::new(2, Duration::from_secs(1000));
        let t0 = Instant::now();
        c.insert("a".into(), 1, t0);
        c.insert("b".into(), 2, t0 + Duration::from_millis(1));
        // Touch a so b is colder when we insert c.
        assert_eq!(c.get(&"a".into(), t0 + Duration::from_millis(2)), Some(1));
        c.insert("c".into(), 3, t0 + Duration::from_millis(3));
        assert_eq!(c.get(&"b".into(), t0 + Duration::from_millis(4)), None);
        assert_eq!(c.get(&"a".into(), t0 + Duration::from_millis(4)), Some(1));
        assert_eq!(c.get(&"c".into(), t0 + Duration::from_millis(4)), Some(3));
    }

    #[test]
    fn search_key_normalizes_whitespace_case() {
        assert_eq!(
            search_key("Basic", "  Tesla   FSD  "),
            search_key("basic", "tesla fsd")
        );
    }
}
