// This file is part of midnight-ledger.
// Copyright (C) Midnight Foundation
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

//! The `Arc` behind every [`Sp`](super::Sp), and the cache of weak pointers to it.
//!
//! [`SpCache`] entries are removed only by [`SpCache::gc`], which `Sp::unload` (and so
//! `Sp::drop`) calls after releasing its strong reference. An entry therefore dangles for good
//! if the last strong reference is released anywhere else, e.g. by an `Arc` that escaped an
//! `Sp`. This module makes that unrepresentable outside it: the `Arc` is private to [`Slot`],
//! and no function here returns one. The only operations that release a strong reference are
//! [`Slot::unload`] and [`Slot::take_inner`], and the `Sp` methods that call them follow up with
//! [`SpCache::gc`], keyed by the concrete type [`Slot::unload`] returns.
//!
//! Everything here is zero cost: [`Slot`] is a transparent wrapper of the `OnceLock<Arc<T>>`
//! that `Sp` held before.

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::{Arc, OnceLock, Weak};

use super::{ArenaHash, DynTypedArenaHash};
use crate::WellBehavedHasher;

/// The possibly not yet loaded data of an `Sp`, shared with other `Sp`s of the same key.
///
/// Invariant: every `Slot` holding data is owned by an `Sp`, or is about to be moved into one.
#[repr(transparent)]
#[must_use]
pub(super) struct Slot<T: ?Sized>(OnceLock<Arc<T>>);

// Zero cost: `Sp` is exactly as large as when it held the `OnceLock<Arc<T>>` directly.
const _: () = {
    use std::mem::size_of;
    assert!(size_of::<Slot<u8>>() == size_of::<OnceLock<Arc<u8>>>());
    assert!(
        size_of::<Slot<dyn Any + Send + Sync>>()
            == size_of::<OnceLock<Arc<dyn Any + Send + Sync>>>()
    );
};

impl<T: ?Sized> Slot<T> {
    /// A slot with nothing loaded.
    pub(super) const fn empty() -> Self {
        Slot(OnceLock::new())
    }

    pub(super) fn get(&self) -> Option<&T> {
        self.0.get().map(|arc| &**arc)
    }

    pub(super) fn is_empty(&self) -> bool {
        self.0.get().is_none()
    }

    /// Move the data out, leaving `self` empty. Moves the strong reference rather than
    /// releasing it.
    pub(super) fn take(&mut self) -> Self {
        Slot(std::mem::take(&mut self.0))
    }

    /// Load `self` from `other`, unless it is already loaded. Losing a race to load the same
    /// key keeps the winner's data; `other`'s is released and the winner still holds the entry.
    pub(super) fn fill(&self, other: Self) {
        if let Some(arc) = other.0.into_inner() {
            let _ = self.0.set(arc);
        }
    }

    /// Release this slot's strong reference, returning the type to pass to [`SpCache::gc`].
    ///
    /// That is the concrete type the data is cached under, which for an upcast
    /// `Slot<dyn Any>` differs from `T`, so ask the value before dropping it.
    pub(super) fn unload(&mut self) -> TypeId
    where
        T: 'static,
    {
        let type_id = self
            .0
            .get()
            .map_or_else(TypeId::of::<T>, |arc| Any::type_id(&**arc));
        let _ = self.0.take();
        type_id
    }

    /// Whether two slots share one allocation.
    #[cfg(test)]
    pub(super) fn ptr_eq(&self, other: &Self) -> bool {
        matches!((self.0.get(), other.0.get()), (Some(a), Some(b)) if Arc::ptr_eq(a, b))
    }

    /// The number of slots sharing this one's allocation, or 0 if it is empty.
    #[cfg(test)]
    pub(super) fn strong_count(&self) -> usize {
        self.0.get().map_or(0, Arc::strong_count)
    }
}

impl<T> Slot<T> {
    pub(super) fn new(value: T) -> Self {
        Slot(OnceLock::from(Arc::new(value)))
    }

    /// Release this slot's strong reference, returning the data if it was the last one. Must be
    /// followed by [`SpCache::gc`], which then sees the allocation gone.
    pub(super) fn take_inner(&mut self) -> Option<T> {
        self.0.take().and_then(Arc::into_inner)
    }
}

impl<T: Any + Send + Sync> Slot<T> {
    pub(super) fn upcast(&self) -> Slot<dyn Any + Send + Sync> {
        match self.0.get() {
            Some(arc) => Slot(OnceLock::from(arc.clone() as Arc<dyn Any + Send + Sync>)),
            None => Slot::empty(),
        }
    }
}

impl Slot<dyn Any + Send + Sync> {
    /// `None` if the data is loaded and not a `T`; an empty slot if nothing is loaded.
    pub(super) fn downcast<T: Any + Send + Sync>(&self) -> Option<Slot<T>> {
        match self.0.get() {
            Some(arc) => arc
                .clone()
                .downcast()
                .ok()
                .map(|arc| Slot(OnceLock::from(arc))),
            None => Some(Slot::empty()),
        }
    }
}

impl<T: ?Sized> Clone for Slot<T> {
    fn clone(&self) -> Self {
        Slot(self.0.clone())
    }
}

/// Weak pointers to the data of every loaded `Sp`, so `Sp`s of one key share one allocation.
///
/// Keyed by hash and type: the hash alone is ambiguous, as binary serializations need not be
/// disjoint across types.
#[derive(Debug)]
pub(super) struct SpCache<H: WellBehavedHasher>(
    HashMap<DynTypedArenaHash<H>, Weak<dyn Any + Sync + Send>>,
);

impl<H: WellBehavedHasher> SpCache<H> {
    pub(super) fn new() -> Self {
        SpCache(HashMap::new())
    }

    /// The cached data for `key`, if it is still alive.
    pub(super) fn get<T: Any + Send + Sync>(&self, key: &ArenaHash<H>) -> Option<Slot<T>> {
        self.0
            .get(&(key.clone(), TypeId::of::<T>()))
            .and_then(Weak::upgrade)
            // The `downcast` can't fail because we only insert `T`s under `TypeId::of::<T>()`.
            .map(|arc| Slot(OnceLock::from(arc.downcast::<T>().unwrap())))
    }

    /// Cache the data of `slot` under `key`. A no-op if `slot` is empty.
    pub(super) fn insert<T: Any + Send + Sync>(&mut self, key: ArenaHash<H>, slot: &Slot<T>) {
        if let Some(arc) = slot.0.get() {
            let arc: Arc<dyn Any + Send + Sync> = arc.clone();
            self.0
                .insert((key, TypeId::of::<T>()), Arc::downgrade(&arc));
        }
    }

    /// Remove the entry for `key` and `type_id` if its data has been freed.
    pub(super) fn gc(&mut self, key: &ArenaHash<H>, type_id: TypeId) {
        let key = (key.clone(), type_id);
        // NOTE: Here, we rely on the `Arc` reference count to perform the cleanup if and only if
        // the underlying `Arc` is no longer allocated. This relies on the `Sp`s internal `Arc`
        // not leaking, as this ensures this check will be made during each `Arc` drop, including
        // the final one.
        //
        // Previously, this used `weak.upgrade().is_none()` to determine this, but this can lead to
        // a rare race condition between threads A and B that both hold the same Sp, and are
        // deallocating it simultaneously:
        //
        // - A drops its reference
        // - A acquires the critical section lock
        // - A calls `upgrade`, obtaining an `Arc`
        // - B drops its reference
        // - B waits for the critical section lock
        // - A calls `is_none`, dropping the final `Arc` and triggering `T::drop` in the critical
        //   section
        //
        // Because T::drop can drop further `Sp`s, this can cause the same thread to re-enter the
        // critical section, and attempt to double-borrow the sp cache mutably.
        //
        // Because `strong_count` can not bring a value of `T` into the critical section, it avoids
        // this loop.
        if self
            .0
            .get(&key)
            .is_some_and(|weak| weak.strong_count() == 0)
        {
            self.0.remove(&key);
        }
    }

    #[cfg(test)]
    pub(super) fn len(&self) -> usize {
        self.0.len()
    }

    #[cfg(test)]
    pub(super) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Whether `key` has an entry for a `T`, live or dangling.
    #[cfg(test)]
    pub(super) fn contains<T: ?Sized + 'static>(&self, key: &ArenaHash<H>) -> bool {
        self.0.contains_key(&(key.clone(), TypeId::of::<T>()))
    }
}
