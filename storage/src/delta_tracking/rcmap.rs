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

//! Reference count map for tracking charged keys in write and delete costing
use crate::Storable;
use crate::arena::ArenaKey;
use crate::db::DB;
use crate::storable::Loader;
use crate::storage::Map;
use crate::{self as storage, DefaultDB};
use derive_where::derive_where;
use rand::distributions::{Distribution, Standard};
use serialize::{Deserializable, Serializable, Tagged};
#[cfg(test)]
use std::collections::HashMap;
use std::collections::HashSet as StdHashSet;
#[cfg(feature = "proptest")]
use {proptest::prelude::Arbitrary, serialize::NoStrategy, std::marker::PhantomData};

/// A wrapper around `ArenaKey` that ensures the referenced node is persisted.
///
/// When stored in the arena, `KeyRef` reports the wrapped key as its child,
/// which causes the back-end to keep the referenced node alive as long as the
/// `KeyRef`.
#[derive_where(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
struct KeyRef<D: DB> {
    key: ArenaKey<D::Hasher>,
}

impl<D: DB> KeyRef<D> {
    fn new(key: ArenaKey<D::Hasher>) -> Self {
        Self { key }
    }
}

impl<D: DB> Storable<D> for KeyRef<D> {
    fn children(&self) -> std::vec::Vec<ArenaKey<D::Hasher>> {
        vec![self.key.clone()]
    }

    fn to_binary_repr<W: std::io::Write>(&self, _writer: &mut W) -> Result<(), std::io::Error>
    where
        Self: Sized,
    {
        // All information is in the child
        Ok(())
    }

    fn from_binary_repr<R: std::io::Read>(
        _reader: &mut R,
        child_hashes: &mut impl Iterator<Item = ArenaKey<D::Hasher>>,
        _loader: &impl Loader<D>,
    ) -> Result<Self, std::io::Error>
    where
        Self: Sized,
    {
        child_hashes.next().map(KeyRef::new).ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, "KeyRef missing child key")
        })
    }
}

impl<D: DB> Serializable for KeyRef<D> {
    fn serialize(&self, writer: &mut impl std::io::Write) -> std::io::Result<()> {
        self.key.serialize(writer)
    }

    fn serialized_size(&self) -> usize {
        self.key.serialized_size()
    }
}

impl<D: DB> Deserializable for KeyRef<D> {
    fn deserialize(reader: &mut impl std::io::Read, recursive_depth: u32) -> std::io::Result<Self> {
        ArenaKey::deserialize(reader, recursive_depth).map(KeyRef::new)
    }
}

impl<D: DB> Distribution<KeyRef<D>> for Standard {
    fn sample<R: rand::Rng + ?Sized>(&self, rng: &mut R) -> KeyRef<D> {
        KeyRef::new(rng.r#gen())
    }
}

// Manual impl because we don't derive Storable
impl<D: DB> Tagged for KeyRef<D> {
    fn tag() -> std::borrow::Cow<'static, str> {
        "keyref[v1]".into()
    }
    fn tag_unique_factor() -> String {
        "keyref[v1]".into()
    }
}

/// Reference count map for tracking charged keys in write and delete costing.
///
/// Internally we use `KeyRef` to ensure that nodes for all keys in the `RcMap`
/// will be persisted as long a the `RcMap` itself is.
#[derive_where(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[derive(serde::Serialize, serde::Deserialize, Storable)]
#[serde(bound(serialize = "", deserialize = ""))]
#[storable(db = D)]
#[tag = "rcmap[v1]"]
pub struct RcMap<D: DB = DefaultDB> {
    /// Reference counts for keys with `rc >= 1`
    rc_ge_1: Map<ArenaKey<D::Hasher>, u64, D>,
    /// Keys with reference count zero, for efficient garbage collection.
    ///
    /// The `KeyRef` here creates storage overhead -- an additional dag node for
    /// each key -- but the `rc_0` map is expected to be small, so this
    /// shouldn't matter.
    rc_0: Map<ArenaKey<D::Hasher>, KeyRef<D>, D>,
}

impl<D: DB> RcMap<D> {
    /// Returns true iff the key is charged.
    pub(crate) fn contains(&self, key: &ArenaKey<D::Hasher>) -> bool {
        self.get_rc(key).is_some()
    }

    /// Get the current reference count for a key.
    /// Returns Some(n) if key is charged (n >= 0), None if key is not in `RcMap`.
    pub(crate) fn get_rc(&self, key: &ArenaKey<D::Hasher>) -> Option<u64> {
        if let Some(count) = self.rc_ge_1.get(key) {
            Some(*count)
        } else if self.rc_0.contains_key(key) {
            Some(0)
        } else {
            None // Key not charged at all
        }
    }

    /// Increment the reference count for a key.
    /// Returns `(new_rcmap, new_rc)`.
    #[must_use]
    pub(crate) fn modify_rc(&self, key: &ArenaKey<D::Hasher>, updated: u64) -> Self {
        let curr = self.rc_ge_1.get(key).copied().unwrap_or(0);
        match (curr, updated) {
            (0, 0) =>
            // Final ref count is zero, add to rc_0.
            {
                RcMap {
                    rc_ge_1: self.rc_ge_1.clone(),
                    rc_0: self.rc_0.insert(key.clone(), KeyRef::new(key.clone())),
                }
            }
            (0, 1..) =>
            // Key exists with rc = 0, move to rc_ge_1 with count n
            {
                RcMap {
                    rc_ge_1: self.rc_ge_1.insert(key.clone(), updated),
                    rc_0: self.rc_0.remove(key),
                }
            }
            (1.., 1..) =>
            // Key exists with rc_ge_1, update
            {
                RcMap {
                    rc_ge_1: self.rc_ge_1.insert(key.clone(), updated),
                    rc_0: self.rc_0.clone(),
                }
            }
            (1.., 0) =>
            // Key exists with rc_ge_1, move to rc_0
            {
                RcMap {
                    rc_ge_1: self.rc_ge_1.remove(key),
                    rc_0: self.rc_0.insert(key.clone(), KeyRef::new(key.clone())),
                }
            }
        }
    }

    /// Get all keys that are unreachable (have `rc=0`) and not in the provided set.
    /// This is used to initialize garbage collection.
    pub(crate) fn get_unreachable_keys_not_in(
        &self,
        roots: &StdHashSet<ArenaKey<D::Hasher>>,
    ) -> impl Iterator<Item = ArenaKey<D::Hasher>> {
        self.rc_0.keys().filter(|key| !roots.contains(key))
    }

    /// Remove a key from the unreachable set (used during garbage collection).
    /// Returns `Some(updated rc map)` if key was present with `rc == 0`, and
    /// `None` otherwise.
    #[must_use]
    pub(crate) fn remove_unreachable_key(&self, key: &ArenaKey<D::Hasher>) -> Option<Self> {
        if self.rc_0.contains_key(key) {
            Some(RcMap {
                rc_ge_1: self.rc_ge_1.clone(),
                rc_0: self.rc_0.remove(key),
            })
        } else {
            None
        }
    }

    /// Get all charged keys and their reference counts (for testing).
    #[cfg(test)]
    pub(crate) fn get_rcs(&self) -> HashMap<ArenaKey<D::Hasher>, u64> {
        let mut result = HashMap::new();

        // Add all keys with rc = 0
        for key in self.rc_0.keys() {
            result.insert(key.clone(), 0);
        }

        // Add all keys with rc >= 1
        for (key, count) in self.rc_ge_1.iter() {
            result.insert(key.clone(), *count);
        }

        result
    }
}

impl<D: DB> Default for RcMap<D> {
    fn default() -> Self {
        RcMap {
            rc_ge_1: Map::new(),
            rc_0: Map::new(),
        }
    }
}

#[cfg(feature = "proptest")]
impl<D: DB> Arbitrary for RcMap<D>
where
    D::Hasher: Arbitrary,
{
    type Strategy = NoStrategy<RcMap<D>>;
    type Parameters = ();
    fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
        NoStrategy(PhantomData)
    }
}

impl<D: DB> Distribution<RcMap<D>> for Standard {
    fn sample<R: rand::Rng + ?Sized>(&self, rng: &mut R) -> RcMap<D> {
        RcMap {
            rc_ge_1: rng.r#gen(),
            rc_0: rng.r#gen(),
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::arena::Sp;
    use crate::db::InMemoryDB;

    // Test Storable serialization of vector of KeyRefs, to be sure the manual
    // Storable impl makes sense.
    #[test]
    fn keyref_round_trip_storable() {
        // Create a dummy value to get an arena key
        let val = Sp::<_, InMemoryDB>::new(42u64);
        let key = val.root.clone();
        let keyref = KeyRef::<InMemoryDB>::new(key);

        // Create a vector with 3 of the same KeyRef
        let keyrefs = vec![
            Sp::new(keyref.clone()),
            Sp::new(keyref.clone()),
            Sp::new(keyref.clone()),
        ];

        // Roundtrip Storable
        let mut bytes = Vec::new();
        keyrefs.to_binary_repr(&mut bytes).unwrap();
        let mut reader = &bytes[..];
        let mut child_iter = keyrefs.children().into_iter();
        let arena = &crate::storage::default_storage().arena;
        let loader = crate::arena::BackendLoader::new(arena, None);
        let deserialized: Vec<Sp<KeyRef<InMemoryDB>, InMemoryDB>> =
            Storable::from_binary_repr(&mut reader, &mut child_iter, &loader).unwrap();

        assert_eq!(keyrefs, deserialized);
    }

    // Helper function to get all descendants of RcMap recursively
    #[cfg(test)]
    pub(crate) fn get_rcmap_descendants<D: DB>(
        rcmap: &RcMap<D>,
    ) -> std::collections::HashSet<ArenaKey<D::Hasher>> {
        let mut visited = std::collections::HashSet::new();
        let mut to_visit = rcmap.children();
        let arena = &crate::storage::default_storage::<D>().arena;
        while let Some(current) = to_visit.pop() {
            if !visited.insert(current.clone()) {
                continue;
            }
            arena.with_backend(|backend| {
                let disk_obj = backend.get(&current).expect("Key should exist in backend");
                to_visit.extend(disk_obj.children.clone());
            });
        }
        visited
    }

    // Test that keys in rc_0 are descendants of RcMap via KeyRef storage.
    #[test]
    fn rc_0_keys_are_descendants() {
        let val = Sp::<_, InMemoryDB>::new(42u64);
        let key = val.root.clone();

        // Create RcMap with key in rc_0
        let rcmap = RcMap::<InMemoryDB>::default().modify_rc(&key, 0);
        assert!(rcmap.rc_0.contains_key(&key));

        let descendants = get_rcmap_descendants(&rcmap);
        assert!(
            descendants.contains(&key),
            "Key in rc_0 must be a descendant of RcMap"
        );
    }

    // Comprehensive test of RcMap basic operations
    #[test]
    fn rcmap_operations() {
        // Create test keys using simple u8 values
        let val1 = Sp::<_, InMemoryDB>::new(1u8);
        let key1 = val1.root.clone();
        let val2 = Sp::<_, InMemoryDB>::new(2u8);
        let key2 = val2.root.clone();
        let val3 = Sp::<_, InMemoryDB>::new(3u8);
        let key3 = val3.root.clone();

        let rcmap = RcMap::<InMemoryDB>::default().modify_rc(&key1, 0);

        // Test initialize_key sets rc=0
        assert_eq!(rcmap.get_rc(&key1), Some(0), "get_rc should return 0");
        assert!(rcmap.rc_0.contains_key(&key1), "key1 should be in rc_0 map");
        assert!(
            !rcmap.rc_ge_1.contains_key(&key1),
            "key1 should not be in rc_ge_1 map"
        );

        // Test increment_rc from 0 to 1 moves to rc_ge_1
        let rcmap = rcmap.modify_rc(&key1, 1);
        assert_eq!(rcmap.get_rc(&key1), Some(1), "get_rc should return 1");
        assert!(
            !rcmap.rc_0.contains_key(&key1),
            "key1 should not be in rc_0 map"
        );
        assert!(
            rcmap.rc_ge_1.contains_key(&key1),
            "key1 should be in rc_ge_1 map"
        );

        // Test increment_rc multiple times
        let rcmap = rcmap.modify_rc(&key1, 2);
        let rcmap = rcmap.modify_rc(&key1, 3);
        assert_eq!(rcmap.get_rc(&key1), Some(3), "get_rc should return 3");
        assert!(
            rcmap.rc_ge_1.contains_key(&key1),
            "key1 should remain in rc_ge_1 map"
        );

        // Test decrement_rc multiple times
        let rcmap = rcmap.modify_rc(&key1, 2);
        let rcmap = rcmap.modify_rc(&key1, 1);
        assert!(
            rcmap.rc_ge_1.contains_key(&key1),
            "key1 should still be in rc_ge_1 map"
        );

        // Test decrement_rc from 1 to 0 moves back to rc_0
        let rcmap = rcmap.modify_rc(&key1, 0);
        assert_eq!(rcmap.get_rc(&key1), Some(0), "get_rc should return 0");
        assert!(
            rcmap.rc_0.contains_key(&key1),
            "key1 should be back in rc_0 map"
        );
        assert!(
            !rcmap.rc_ge_1.contains_key(&key1),
            "key1 should not be in rc_ge_1 map"
        );

        // Test get_rc on nonexistent key returns None
        assert_eq!(
            rcmap.get_rc(&key2),
            None,
            "get_rc on nonexistent key should return None"
        );

        // Test multiple keys
        let rcmap = rcmap.modify_rc(&key2, 1);
        let rcmap = rcmap.modify_rc(&key3, 2);

        // Verify all keys have correct reference counts
        assert_eq!(rcmap.get_rc(&key1), Some(0));
        assert_eq!(rcmap.get_rc(&key2), Some(1));
        assert_eq!(rcmap.get_rc(&key3), Some(2));

        // Verify correct map placement
        assert!(rcmap.rc_0.contains_key(&key1));
        assert!(rcmap.rc_ge_1.contains_key(&key2));
        assert!(rcmap.rc_ge_1.contains_key(&key3));

        // Test remove_unreachable_key functionality
        // Remove key1 (rc=0) should succeed
        let rcmap_new = rcmap.remove_unreachable_key(&key1);
        assert!(
            rcmap_new.is_some(),
            "remove_unreachable_key should succeed for rc=0 key"
        );
        let rcmap = rcmap_new.unwrap();
        assert!(!rcmap.contains(&key1), "key1 should no longer be in rcmap");
        assert_eq!(
            rcmap.get_rc(&key1),
            None,
            "get_rc should return None for removed key"
        );

        // Remove key2 (rc=1) should fail
        let rcmap_new = rcmap.remove_unreachable_key(&key2);
        assert!(
            rcmap_new.is_none(),
            "remove_unreachable_key should fail for rc>0 key"
        );

        // Remove nonexistent key should fail
        let rcmap_new = rcmap.remove_unreachable_key(&key1);
        assert!(
            rcmap_new.is_none(),
            "remove_unreachable_key should fail for nonexistent key"
        );
    }
}
