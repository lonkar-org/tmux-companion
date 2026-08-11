//! In-memory TTL maps for the server's segment caches.
//!
//! Entries record the instant they were written; freshness is decided by the
//! *reader*, which is what lets the git-status TTL be a runtime flag instead of
//! a constant baked into the store.  There is no cold-start special case: an
//! entry written at `t` is fresh until `t + ttl`, including the very first one.
//!
//! Every method takes the current instant explicitly in its `*_at` form so the
//! expiry logic is unit-testable without sleeping; the convenience wrappers
//! supply `Instant::now()`.

use std::{
    collections::HashMap,
    hash::Hash,
    time::{Duration, Instant},
};

/// A `HashMap` whose values carry a write timestamp.
///
/// Expired entries are swept on insert, so the map cannot grow without bound
/// in a server that runs for weeks across many panes and repositories.
#[derive(Debug)]
pub struct TtlMap<K, V> {
    entries: HashMap<K, (V, Instant)>,
}

impl<K, V> Default for TtlMap<K, V> {
    fn default() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }
}

impl<K: Eq + Hash, V: Clone> TtlMap<K, V> {
    pub fn new() -> Self {
        Self::default()
    }

    /// The value for `key` if it was written less than `ttl` ago.
    pub fn get_at(&self, key: &K, ttl: Duration, now: Instant) -> Option<V> {
        self.entries
            .get(key)
            .filter(|(_, written)| now.duration_since(*written) < ttl)
            .map(|(v, _)| v.clone())
    }

    /// Store `value` and drop every entry already older than `ttl`.
    pub fn insert_at(&mut self, key: K, value: V, ttl: Duration, now: Instant) {
        self.entries
            .retain(|_, (_, written)| now.duration_since(*written) < ttl);
        self.entries.insert(key, (value, now));
    }

    pub fn get(&self, key: &K, ttl: Duration) -> Option<V> {
        self.get_at(key, ttl, Instant::now())
    }

    pub fn insert(&mut self, key: K, value: V, ttl: Duration) {
        self.insert_at(key, value, ttl, Instant::now());
    }

    /// Number of stored entries, fresh or not.  Exists so the tests can prove
    /// the sweep actually evicts rather than merely hiding expired entries.
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TTL: Duration = Duration::from_secs(5);

    fn secs(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    #[test]
    fn miss_on_empty_map() {
        let m: TtlMap<String, u32> = TtlMap::new();
        assert_eq!(m.get(&"nothing".to_string(), TTL), None);
    }

    #[test]
    fn hit_within_ttl() {
        let t0 = Instant::now();
        let mut m = TtlMap::new();
        m.insert_at("k".to_string(), 7u32, TTL, t0);
        assert_eq!(m.get_at(&"k".to_string(), TTL, t0 + secs(4)), Some(7));
    }

    #[test]
    fn expires_after_ttl() {
        let t0 = Instant::now();
        let mut m = TtlMap::new();
        m.insert_at("k".to_string(), 7u32, TTL, t0);
        assert_eq!(m.get_at(&"k".to_string(), TTL, t0 + secs(6)), None);
    }

    #[test]
    fn ttl_boundary_is_exclusive() {
        // Exactly at the TTL the entry is already stale: freshness is
        // `age < ttl`, not `<=`.
        let t0 = Instant::now();
        let mut m = TtlMap::new();
        m.insert_at("k".to_string(), 1u32, TTL, t0);
        assert_eq!(m.get_at(&"k".to_string(), TTL, t0 + TTL), None);
        assert_eq!(
            m.get_at(&"k".to_string(), TTL, t0 + TTL - Duration::from_millis(1)),
            Some(1)
        );
    }

    #[test]
    fn ttl_flag_is_honoured_from_the_very_first_hit() {
        // The first write is not special-cased: a 1-second flag expires the
        // first entry after one second just as it would the hundredth.
        let t0 = Instant::now();
        let mut m = TtlMap::new();
        let flag = secs(1);
        m.insert_at("k".to_string(), 42u32, flag, t0);
        assert_eq!(
            m.get_at(&"k".to_string(), flag, t0 + Duration::from_millis(900)),
            Some(42),
            "first entry must be served from cache inside the flagged window"
        );
        assert_eq!(
            m.get_at(&"k".to_string(), flag, t0 + Duration::from_millis(1100)),
            None,
            "first entry must expire on the flagged TTL, not a built-in default"
        );
    }

    #[test]
    fn reader_supplied_ttl_overrides_a_longer_one() {
        // The same stored entry is fresh under a long TTL and stale under a
        // short one — the store holds no TTL of its own.
        let t0 = Instant::now();
        let mut m = TtlMap::new();
        m.insert_at("k".to_string(), 1u32, secs(60), t0);
        let at = t0 + secs(3);
        assert_eq!(m.get_at(&"k".to_string(), secs(10), at), Some(1));
        assert_eq!(m.get_at(&"k".to_string(), secs(2), at), None);
    }

    #[test]
    fn insert_overwrites_and_refreshes_the_timestamp() {
        let t0 = Instant::now();
        let mut m = TtlMap::new();
        m.insert_at("k".to_string(), 1u32, TTL, t0);
        m.insert_at("k".to_string(), 2u32, TTL, t0 + secs(4));
        // Written again at t0+4, so still fresh at t0+8.
        assert_eq!(m.get_at(&"k".to_string(), TTL, t0 + secs(8)), Some(2));
        assert_eq!(m.len(), 1);
    }

    #[test]
    fn insert_sweeps_expired_entries() {
        let t0 = Instant::now();
        let mut m = TtlMap::new();
        m.insert_at("old".to_string(), 1u32, TTL, t0);
        assert_eq!(m.len(), 1);
        // A write 10s later finds "old" expired and drops it.
        m.insert_at("new".to_string(), 2u32, TTL, t0 + secs(10));
        assert_eq!(m.len(), 1, "expired entry should have been swept");
        assert_eq!(m.get_at(&"new".to_string(), TTL, t0 + secs(10)), Some(2));
        assert_eq!(m.get_at(&"old".to_string(), TTL, t0 + secs(10)), None);
    }

    #[test]
    fn sweep_keeps_fresh_entries() {
        let t0 = Instant::now();
        let mut m = TtlMap::new();
        m.insert_at("a".to_string(), 1u32, TTL, t0);
        m.insert_at("b".to_string(), 2u32, TTL, t0 + secs(1));
        assert_eq!(m.len(), 2, "neither entry is stale yet");
    }

    #[test]
    fn distinct_keys_do_not_collide() {
        let t0 = Instant::now();
        let mut m = TtlMap::new();
        m.insert_at("a".to_string(), 1u32, TTL, t0);
        m.insert_at("b".to_string(), 2u32, TTL, t0);
        assert_eq!(m.get_at(&"a".to_string(), TTL, t0), Some(1));
        assert_eq!(m.get_at(&"b".to_string(), TTL, t0), Some(2));
    }

    #[test]
    fn zero_ttl_never_hits() {
        let t0 = Instant::now();
        let mut m = TtlMap::new();
        m.insert_at("k".to_string(), 1u32, Duration::ZERO, t0);
        assert_eq!(m.get_at(&"k".to_string(), Duration::ZERO, t0), None);
    }

    #[test]
    fn works_with_pathbuf_keys() {
        use std::path::PathBuf;
        let t0 = Instant::now();
        let mut m: TtlMap<PathBuf, bool> = TtlMap::new();
        let p = PathBuf::from("/tmp/repo");
        m.insert_at(p.clone(), true, TTL, t0);
        assert_eq!(m.get_at(&p, TTL, t0 + secs(1)), Some(true));
        assert_eq!(m.get_at(&PathBuf::from("/tmp/other"), TTL, t0), None);
    }

    #[test]
    fn is_empty_tracks_len() {
        let mut m: TtlMap<String, u32> = TtlMap::new();
        assert!(m.is_empty());
        m.insert("k".to_string(), 1, TTL);
        assert!(!m.is_empty());
    }
}
