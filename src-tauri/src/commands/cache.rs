//! Bounded TTL + LRU cache primitive (Phase 3.6).
//!
//! The reusable building block for the app's bounded state: `SessionCache`
//! (16 entries / 30-min TTL) and `ResearchRunState` (8 ledgers / 30-min TTL).
//! Pure and deterministic — `now` (unix seconds) is injected, never read from a
//! clock here — so eviction/expiry are unit-tested exactly. Entries expire by
//! age (`inserted_at`) and, above capacity, the least-recently-USED entry
//! (`last_used`, bumped on every `get`) is evicted first.

use std::collections::HashMap;
use std::hash::Hash;

struct Entry<V> {
    value: V,
    inserted_at: i64,
    last_used: i64,
}

/// A cache bounded by both a max entry count (LRU eviction) and a TTL.
pub struct BoundedCache<K: Eq + Hash + Clone, V> {
    max: usize,
    ttl_secs: i64,
    entries: HashMap<K, Entry<V>>,
}

impl<K: Eq + Hash + Clone, V> BoundedCache<K, V> {
    /// A cache holding at most `max` entries, each living `ttl_secs` seconds.
    pub fn new(max: usize, ttl_secs: i64) -> Self {
        Self {
            max,
            ttl_secs,
            entries: HashMap::new(),
        }
    }

    /// Insert (or replace) a key. Expired entries are pruned first; if inserting
    /// a NEW key would exceed `max`, the least-recently-used entry is evicted.
    pub fn insert(&mut self, key: K, value: V, now: i64) {
        self.prune(now);
        if !self.entries.contains_key(&key) && self.entries.len() >= self.max {
            self.evict_lru();
        }
        self.entries.insert(
            key,
            Entry {
                value,
                inserted_at: now,
                last_used: now,
            },
        );
    }

    /// Fetch a live (non-expired) entry, bumping its recency. Expired entries are
    /// pruned first, so a stale entry is never served.
    pub fn get(&mut self, key: &K, now: i64) -> Option<&V> {
        self.prune(now);
        match self.entries.get_mut(key) {
            Some(e) => {
                e.last_used = now;
                Some(&e.value)
            }
            None => None,
        }
    }

    /// Remove a key (e.g. after a successful finalize). Returns the value if any.
    pub fn remove(&mut self, key: &K) -> Option<V> {
        self.entries.remove(key).map(|e| e.value)
    }

    /// Drop every entry older than the TTL.
    pub fn prune(&mut self, now: i64) {
        let ttl = self.ttl_secs;
        self.entries.retain(|_, e| now - e.inserted_at <= ttl);
    }

    /// Evict the single least-recently-used entry.
    fn evict_lru(&mut self) {
        if let Some(k) = self
            .entries
            .iter()
            .min_by_key(|(_, e)| e.last_used)
            .map(|(k, _)| k.clone())
        {
            self.entries.remove(&k);
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_and_get_roundtrip() {
        let mut c: BoundedCache<String, i32> = BoundedCache::new(4, 100);
        c.insert("a".into(), 1, 0);
        assert_eq!(c.get(&"a".to_string(), 0), Some(&1));
        assert_eq!(c.get(&"missing".to_string(), 0), None);
        assert_eq!(c.len(), 1);
    }

    #[test]
    fn entries_expire_by_ttl() {
        let mut c: BoundedCache<String, i32> = BoundedCache::new(4, 100);
        c.insert("a".into(), 1, 0);
        // Just within TTL.
        assert_eq!(c.get(&"a".to_string(), 100), Some(&1));
        // Past TTL → pruned, not served.
        assert_eq!(c.get(&"a".to_string(), 101), None);
        assert!(c.is_empty());
    }

    #[test]
    fn lru_eviction_keeps_recently_used() {
        let mut c: BoundedCache<&str, i32> = BoundedCache::new(2, 1000);
        c.insert("a", 1, 0);
        c.insert("b", 2, 1);
        // Touch "a" so it is more recently used than "b".
        assert_eq!(c.get(&"a", 2), Some(&1));
        // Inserting "c" evicts the LRU ("b").
        c.insert("c", 3, 3);
        assert_eq!(c.len(), 2);
        assert_eq!(c.get(&"a", 4), Some(&1));
        assert_eq!(c.get(&"c", 4), Some(&3));
        assert_eq!(c.get(&"b", 4), None, "least-recently-used evicted");
    }

    #[test]
    fn remove_after_finalize() {
        let mut c: BoundedCache<&str, i32> = BoundedCache::new(4, 1000);
        c.insert("a", 1, 0);
        assert_eq!(c.remove(&"a"), Some(1));
        assert_eq!(c.remove(&"a"), None);
        assert!(c.is_empty());
    }

    #[test]
    fn replacing_key_does_not_evict() {
        let mut c: BoundedCache<&str, i32> = BoundedCache::new(2, 1000);
        c.insert("a", 1, 0);
        c.insert("b", 2, 0);
        // Replacing an existing key at capacity must not evict the other.
        c.insert("a", 9, 1);
        assert_eq!(c.len(), 2);
        assert_eq!(c.get(&"a", 1), Some(&9));
        assert_eq!(c.get(&"b", 1), Some(&2));
    }

    #[test]
    fn matches_session_cache_bounds() {
        // Sanity: the roadmap's SessionCache bounds instantiate cleanly.
        let mut c: BoundedCache<String, u8> = BoundedCache::new(16, 30 * 60);
        for i in 0..20u8 {
            c.insert(format!("k{i}"), i, i as i64);
        }
        assert!(c.len() <= 16, "capacity bounded to 16");
    }
}
