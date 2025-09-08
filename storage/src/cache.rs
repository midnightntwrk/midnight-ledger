// This file is part of midnight-ledger.
// Copyright (C) 2025 Midnight Foundation
// SPDX-License-Identifier: Apache-2.0
// Licensed under the Apache License, Version 2.0 (the "License");
// You may not use this file except in compliance with the License.
// You may obtain a copy of the License at
// http://www.apache.org/licenses/LICENSE-2.0
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use lru::LruCache;
use std::{hash::Hash, num::NonZeroUsize};

#[derive(Debug)]
pub(crate) struct Cache<K: Eq + Hash, V> {
    cache: LruCache<K, V>,
}

impl<K: Eq + Hash + Clone, V> Cache<K, V> {
    /// Create a bounded LRU cache with capacity `capacity`.
    ///
    /// The backing storage for the cache will be lazily allocated as the cache
    /// is filled, not allocated all at once when the cache is created.
    pub(crate) fn new(capacity: usize) -> Self {
        // The `LruCache::new` constructor eagerly allocates the backing
        // storage, but the `LruCache::resize` does not reallocate the backing
        // storage, and instead just depends on the underlying
        // `HashMap::shrink_to_fit`:
        // https://docs.rs/lru/0.12.5/src/lru/lib.rs.html#1343.  The `HashMap`
        // will then reallocate as necessary, on demand, in amortized O(1) time.
        let mut cache = LruCache::new(NonZeroUsize::new(1024).unwrap());
        cache.resize(NonZeroUsize::new(capacity).expect("Capacity must be non-zero"));
        Self { cache }
    }

    pub(crate) fn promote(&mut self, key: &K) -> bool {
        self.cache.promote(key)
    }

    /// Create an unbounded cache.
    pub(crate) fn unbounded() -> Self {
        Self {
            cache: LruCache::unbounded(),
        }
    }

    /// Inserts an object into the cache, returning any displaced key-value
    /// pair, if that displaced key-value pair is still correct.
    ///
    /// When updating an existing key, nothing is returned, because the old
    /// value under that key is now incorrect. When setting a new key, the
    /// return value depends on whether the cache is full or not:
    ///
    /// - if the cache was already at capacity, then the least-recently-used
    ///   key-value pair is dropped from the cache and returned.
    ///
    /// - if the cache was not at capacity, then nothing is returned.
    pub(crate) fn set(&mut self, key: K, value: V) -> Option<(K, V)> {
        // The underlying `cache.push` returns the old key-value pair in case of
        // updating an existing key. We need to return `None` in this case,
        // since this isn't a true eviction for our purposes.
        let evicted = self.cache.push(key.clone(), value);
        match evicted {
            Some((k, _)) if k == key => None,
            _ => evicted,
        }
    }

    /// Updates an object in place, without changing its position in the cache.
    ///
    /// # Panics
    ///
    /// Panics if the key is not in the cache.
    pub(crate) fn update_in_place(&mut self, key: K, value: V) {
        assert!(self.cache.contains(&key));
        self.cache.put(key, value);
    }

    /// removes an object, returning the object
    pub(crate) fn remove(&mut self, key: &K) -> Option<(K, V)> {
        self.cache.pop_entry(key)
    }

    /// Gets object with `key`, moving it to the front of the cache.
    pub(crate) fn _get(&mut self, key: &K) -> Option<&V> {
        self.cache.get(key)
    }

    /// Like `get`, but doesn't update the cache's LRU ordering.
    pub(crate) fn peek(&self, key: &K) -> Option<&V> {
        self.cache.peek(key)
    }

    /// Like `peek`, but returns a mutable reference.
    pub(crate) fn peek_mut(&mut self, key: &K) -> Option<&mut V> {
        self.cache.peek_mut(key)
    }

    /// Clears the cache
    pub(crate) fn clear(&mut self) {
        self.cache.clear();
    }

    /// Returns an iterator over the cache key-values.
    pub(crate) fn iter(&self) -> impl Iterator<Item = (&K, &V)> {
        self.cache.iter()
    }

    /// Returns the number of elements in the cache.
    pub(crate) fn len(&self) -> usize {
        self.cache.len()
    }

    /// Returns the least-recently used key-value pair, if any.
    pub(crate) fn pop_lru(&mut self) -> Option<(K, V)> {
        self.cache.pop_lru()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::seq::SliceRandom;

    /// Test that the least-recently-used value gets dropped when the cache is
    /// full and a new value is inserted.
    #[test]
    fn lru_val_gets_dropped() {
        let mut cache = Cache::new(2);
        assert_eq!(cache.set(1, 1), None);
        assert_eq!(cache.set(2, 2), None);
        assert_eq!(cache.set(3, 3), Some((1, 1)));
        assert_eq!(cache._get(&1), None);
        assert_eq!(cache._get(&2), Some(&2));
        assert_eq!(cache._get(&3), Some(&3));
        assert_eq!(cache.set(4, 4), Some((2, 2)));
        assert_eq!(cache._get(&2), None);
        assert_eq!(cache._get(&3), Some(&3));
        assert_eq!(cache._get(&4), Some(&4));
    }

    /// The underlying cache lib we use returns old key-value pair when updating
    /// an existing key, but we must return none for our usage in
    /// `StorageBackend`.
    #[test]
    fn updating_key_returns_none() {
        let mut cache = Cache::new(2);
        cache.set(1, 1);
        assert_eq!(cache.set(1, 2), None);
    }

    const CACHE_SIZE: usize = 20000;
    /// Time limit for `iterated_get_*` tests.
    const TIME_BOUND: f64 = 0.3;

    /// Test filling cache and then getting all values.
    ///
    /// This deterministic version tests the worst case of our old O(n)
    /// implementation, where we always lookup the
    /// least-recently-used-but-present value.
    #[test]
    fn iterated_get_lru() {
        let mut cache = Cache::new(CACHE_SIZE);

        let time = std::time::Instant::now();
        for i in 0..CACHE_SIZE {
            cache.set(i, i);
        }
        for i in 0..CACHE_SIZE {
            assert_eq!(cache._get(&i), Some(&i));
        }
        let duration = time.elapsed().as_secs_f64();
        assert!(
            duration < TIME_BOUND,
            "Cache is slow! Duration: {}",
            duration
        )
    }

    /// Test filling cache and then getting all values.
    ///
    /// Like [`iterated_get_lru`], but with lookups in random order. This should
    /// be more robust to detecting bad implementations.
    #[test]
    fn iterated_get_random() {
        let mut rng = rand::thread_rng();
        let mut shuffled: std::vec::Vec<usize> = (0..CACHE_SIZE).collect();
        shuffled.shuffle(&mut rng);

        let mut cache = Cache::new(CACHE_SIZE);

        let time = std::time::Instant::now();
        for i in 0..CACHE_SIZE {
            cache.set(i, i);
        }
        for i in shuffled {
            assert_eq!(cache._get(&i), Some(&i));
        }
        let duration = time.elapsed().as_secs_f64();
        assert!(
            duration < TIME_BOUND,
            "Cache is slow! Duration: {}",
            duration
        )
    }

    /// Test that cache allocates backing storage lazily.
    ///
    /// Assumes we don't have `2^50` bytes of RAM available 🥲
    #[test]
    fn lazy_allocation() {
        let mut cache = Cache::<u32, u32>::new(1 << 50);
        cache.set(0, 0);
    }
}
