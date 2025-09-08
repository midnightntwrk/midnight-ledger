//! An example showing how to simulate transactions for GC root updates in the arena.
//!
//! See [`GcRootUpdateQueue`] for the main implementation, and the various
//! `test_*` functions for example usage.
use midnight_storage::{
    DefaultDB, Storage,
    arena::test_helpers,
    arena::{Arena, ArenaKey, Sp},
    db::DB,
};
use std::{any::Any, collections::HashMap};

/// A queue for delaying gc root-count updates in the backend.
///
/// The idea is that this is similar to a transaction for gc root updates, but
/// it differs from an actual transaction in that the gc root counts in the
/// backend won't actually be updated until the queue is flushed. I.e., calls to
/// [`Self::persist`] and [`Self::unpersist`] won't have any effect on the
/// backend until [`Self::commit`] is called, and so the "transaction" is
/// implicitly rolled back if `Self` goes out of scope before `Self::commit` is
/// called.
///
/// See [`Self::get_roots`] for a wrapper around
/// [`midnight_storage::backend::StorageBackend::get_roots`] that takes into account the
/// pending updates.
#[derive(Debug)]
pub struct GcRootUpdateQueue<D: DB> {
    arena: Arena<D>,
    sps: HashMap<ArenaKey<D::Hasher>, Box<dyn Any>>,
    persist_counts: HashMap<ArenaKey<D::Hasher>, i32>,
}

impl<D: DB> GcRootUpdateQueue<D> {
    /// Create an empty queue.
    pub fn begin(arena: &Arena<D>) -> Self {
        Self {
            arena: arena.clone(),
            sps: HashMap::new(),
            persist_counts: HashMap::new(),
        }
    }

    /// Queue a call to `sp::persist`.
    ///
    /// Calling this function ensures that the data for `sp` will remain live in
    /// the backend until `Self::commit` is called, or `Self` goes out of
    /// scope. I.e., the caller doesn't need to worry about maintaining a
    /// reference to `sp` if they're otherwise done with it.
    ///
    /// # Note
    ///
    /// The `sp::persist` is not actually called at this time, so actual gc root
    /// counts in the backend will not be updated yet.
    pub fn persist<T: 'static>(&mut self, sp: &Sp<T, D>) {
        let key = sp.hash().clone();
        *self.persist_counts.entry(key.clone().into()).or_insert(0) += 1;
        let mut sp = sp.clone();
        // Don't keep descendant references in arena unnecessarily.
        sp.unload();
        self.sps.insert(key.into(), Box::new(sp) as Box<dyn Any>);
    }

    /// Queue a call to `sp::unpersist`.
    ///
    /// # Note
    ///
    /// The `sp::unpersist` is not actually called at this time, so actual gc
    /// root counts in the backend will not be updated yet.
    pub fn unpersist<T: 'static>(&mut self, sp: &Sp<T, D>) {
        let key = sp.hash().clone();
        *self.persist_counts.entry(key.into()).or_insert(0) -= 1;
    }

    /// Update gc root counts in the backend, by executing all the queued
    /// `persist` and `unpersist` calls.
    ///
    /// # Note
    ///
    /// These gc root updates will still need to be flushed to disk at some
    /// point, e.g. by calling
    /// [`midnight_storage::backend::StorageBackend::flush_all_changes_to_db`].
    pub fn commit(self) {
        for (key, count) in self.persist_counts {
            if count > 0 {
                for _ in 0..count {
                    self.arena.with_backend(|b| b.persist(&key));
                }
            } else {
                for _ in 0..count.abs() {
                    self.arena.with_backend(|b| b.unpersist(&key));
                }
            }
        }
    }

    /// Get mapping from root keys to their persist counts, taking into account
    /// any queued updates.
    ///
    /// For example, if the underlying database root count for key `k` is 2, and
    /// self has a net persist-count update for `k` is -1, then in the map
    /// returned by this function `k` will be mapped to 1 (i.e. 2 - 1).
    pub fn get_roots(&self) -> HashMap<ArenaKey<D::Hasher>, u32> {
        let mut roots = self.arena.with_backend(|b| b.get_roots());
        for (key, queue_count) in &self.persist_counts {
            let db_count = roots.entry(key.clone()).or_insert(0);
            let net_count = *db_count as i32 + *queue_count;
            assert!(net_count >= 0, "gc root count underflow");
            *db_count = net_count as u32;
            if net_count == 0 {
                roots.remove(key);
            }
        }
        roots
    }
}

/// Examples are required to have `main` functions, but this one doesn't do
/// anything.
fn main() {
    println!("See tests for example usage");
}

/// Test all operations of `GcRootUpdateQueue`.
///
/// Specifically, test that the queue
///
/// - maintains a reference to an `Sp` for each key for which `persist` or
///   `unpersist` is called, to ensure the data for that key doesn't get
///   removed from the backend before calling `commit`.
///
/// - has no effect before `commit` is called.
///
/// - calls `persist` and/or `unpersist` the approapriate number of times
///   when `commit` is called.
pub fn test_gc_root_update_queue_delayed_effect() {
    let storage = Storage::new(16, DefaultDB::default());
    let arena = &storage.arena;
    let mut queue = GcRootUpdateQueue::begin(arena);

    // Allocate and persist some `Sp`s, so that we have some non-zero root
    // counts to work with.
    let sp1 = arena.alloc(13u32);
    let sp2 = arena.alloc(42u32);
    sp1.persist();
    sp2.persist();
    drop(sp1);
    drop(sp2);

    // Allocate some `Sp`s, and queue some `persist` and `unpersist` calls,
    // with net effect:
    //
    // - sp1: +1
    // - sp2: -1
    // - sp3: +2
    let sp1 = arena.alloc(13u32);
    let sp2 = arena.alloc(42u32);
    let sp3 = arena.alloc(69u32);
    queue.persist(&sp1);
    queue.unpersist(&sp2);
    queue.persist(&sp3);
    queue.unpersist(&sp1);
    queue.persist(&sp2);
    queue.persist(&sp3);
    queue.persist(&sp1);
    queue.unpersist(&sp2);

    // Save the keys and drop the `Sp`s, to ensure that the
    // `GcRootUpdateQueue` correctly keeps around references to the `Sp`s.
    let k1 = sp1.hash().clone();
    let k2 = sp2.hash().clone();
    let k3 = sp3.hash().clone();
    drop(sp1);
    drop(sp2);
    drop(sp3);

    // Check that the gc root counts haven't been updated yet.
    assert_eq!(test_helpers::get_root_count(arena, &k1.clone().into()), 1);
    assert_eq!(test_helpers::get_root_count(arena, &k2.clone().into()), 1);
    assert_eq!(test_helpers::get_root_count(arena, &k3.clone().into()), 0);

    // Check that `GcRootUpdateQueue::get_roots` correctly takes the
    // uncommitted root updates into account.
    assert_eq!(queue.get_roots(), {
        let mut roots = HashMap::new();
        roots.insert(k1.clone().into(), 2);
        roots.insert(k3.clone().into(), 2);
        roots
    });

    // Commit the gc root updates.
    queue.commit();

    // Check that the gc root counts have been updated correctly.
    assert_eq!(test_helpers::get_root_count(arena, &k1.into()), 2);
    assert_eq!(test_helpers::get_root_count(arena, &k2.into()), 0);
    assert_eq!(test_helpers::get_root_count(arena, &k3.into()), 2);
}

/// Check that queuing a gc-root update doesn't keep child `Sp`s cached in
/// the arena.
pub fn test_gc_root_update_queue_no_leak() {
    let storage = Storage::new(16, DefaultDB::default());
    let arena = &storage.arena;
    let mut queue = GcRootUpdateQueue::begin(arena);

    let sp_child = arena.alloc(420u32);
    let key_child = sp_child.hash().clone();
    let sp_parent = arena.alloc(Some(sp_child.clone()));
    let key_parent = sp_parent.hash().clone();
    queue.persist(&sp_parent);
    drop(sp_child);
    drop(sp_parent);
    assert!(test_helpers::read_sp_cache::<_, u32>(arena, &key_child.into()).is_none());
    assert!(test_helpers::read_sp_cache::<_, Option<Sp<u32>>>(arena, &key_parent.into()).is_none());
}

#[cfg(test)]
mod arena_transactions {
    use super::*;

    #[test]
    fn gc_root_update_queue_delayed_effect() {
        test_gc_root_update_queue_delayed_effect()
    }

    #[test]
    fn gc_root_update_queue_no_leak() {
        test_gc_root_update_queue_no_leak()
    }
}
