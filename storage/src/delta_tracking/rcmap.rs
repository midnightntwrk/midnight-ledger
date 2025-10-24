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
use crate::arena::{ArenaHash, ArenaKey};
use crate::db::DB;
use crate::storable::{Loader, child_from};
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
/// When stored in the arena, `ArenaKey` reports the wrapped key as its child,
/// which causes the back-end to keep the referenced node alive as long as the
/// `ArenaKey`.
#[derive_where(Clone, Debug, PartialEq, Eq)]
pub struct ChildRef<D: DB> {
    /// The referenced child
    pub child: ArenaKey<D::Hasher>,
}

impl<D: DB> ChildRef<D> {
    /// Creates a new reference
    pub fn new(child: ArenaKey<D::Hasher>) -> Self {
        Self { child }
    }
}

// NOTE: This previously used to be much simpler, just returning the reference
// as the singular child.
//
// This ceased to work with the small nodes optimisation, because of a tacit
// assumptions that this child would be present in backend storage. This is
// guarnateed if *this* object is also in backend storage, which is *only*
// guarnateed if one of its parents is in storage *or* it gets allocated as an
// `ArenaKey::Ref` (that is, is not a small node itself).
//
// The result was that small ref nodes would not get allocated, and then when
// trying to traverse the child nodes the operation would fail.
//
// To circumvent this, we make sure that the ref node is *the same* as the thing
// its referencing from a backend perspective, that way we can always traverse
// the children -- either directly (if its a small node) or through the backend
// (if its a ref).
//
// To do this, we need to hack the Storable implementation to match what its
// referencing, which we do by resolving refs with the backend.
//
// This is quite brittle, as it still relies on the referenced key actually
// being in the backend. In particular, this doesn't tolerate untrusted inputs
// well, which is the case in contract deployments. Thankfully, in this case we
// validate that the rcmap matches that computed directly from the state.
//
// Nevertheless, longer term it may make sense to change this structure to use
// Sp<dyn Any, D> instead, though we current don't have good mechanisms for
// casting or retrieving those.
impl<D: DB> Storable<D> for ChildRef<D> {
    fn children(&self) -> std::vec::Vec<ArenaKey<D::Hasher>> {
        match &self.child {
            ArenaKey::Direct(direct) => (*direct.children).clone(),
            ArenaKey::Ref(ref_) => {
                let obj = crate::storage::default_storage::<D>()
                    .arena
                    .with_backend(|backend| backend.get(&ref_).cloned())
                    .expect("Referenced object must be in storage");
                obj.children
            }
        }
    }

    fn to_binary_repr<W: std::io::Write>(&self, writer: &mut W) -> Result<(), std::io::Error>
    where
        Self: Sized,
    {
        match &self.child {
            ArenaKey::Direct(direct) => writer.write_all(&direct.data[..]),
            ArenaKey::Ref(ref_) => {
                let obj = crate::storage::default_storage::<D>()
                    .arena
                    .with_backend(|backend| backend.get(&ref_).cloned())
                    .expect("Referenced object must be in storage");
                writer.write_all(&obj.data[..])
            }
        }
    }

    fn from_binary_repr<R: std::io::Read>(
        reader: &mut R,
        children: &mut impl Iterator<Item = ArenaKey<D::Hasher>>,
        _loader: &impl Loader<D>,
    ) -> Result<Self, std::io::Error>
    where
        Self: Sized,
    {
        let mut data = Vec::new();
        reader.read_to_end(&mut data)?;
        let children = children.collect::<Vec<_>>();
        Ok(Self {
            child: child_from(&data, &children),
        })
    }
}

impl<D: DB> Serializable for ChildRef<D> {
    fn serialize(&self, writer: &mut impl std::io::Write) -> std::io::Result<()> {
        self.child.serialize(writer)
    }

    fn serialized_size(&self) -> usize {
        self.child.serialized_size()
    }
}

impl<D: DB> Deserializable for ChildRef<D> {
    fn deserialize(reader: &mut impl std::io::Read, recursive_depth: u32) -> std::io::Result<Self> {
        ArenaKey::<D::Hasher>::deserialize(reader, recursive_depth).map(ChildRef::new)
    }
}

impl<D: DB> Distribution<ChildRef<D>> for Standard {
    fn sample<R: rand::Rng + ?Sized>(&self, rng: &mut R) -> ChildRef<D> {
        ChildRef::new(ArenaKey::Ref(rng.r#gen()))
    }
}

// Manual impl because we don't derive Storable
impl<D: DB> Tagged for ChildRef<D> {
    fn tag() -> std::borrow::Cow<'static, str> {
        "childref[v1]".into()
    }
    fn tag_unique_factor() -> String {
        "children[v1]".into()
    }
}

/// Reference count map for tracking charged keys in write and delete costing.
///
/// Internally we use `ChildRef` to ensure that nodes for all keys in the `RcMap`
/// will be persisted as long a the `RcMap` itself is.
#[derive_where(Debug, Clone, PartialEq, Eq)]
#[derive(Storable)]
//#[derive(serde::Serialize, serde::Deserialize, Storable)]
//#[serde(bound(serialize = "", deserialize = ""))]
#[storable(db = D)]
#[tag = "rcmap[v1]"]
pub struct RcMap<D: DB = DefaultDB> {
    /// Reference counts for keys with `rc >= 1`
    #[cfg(feature = "public-internal-structure")]
    pub rc_ge_1: Map<ArenaHash<D::Hasher>, u64, D>,
    #[cfg(not(feature = "public-internal-structure"))]
    rc_ge_1: Map<ArenaHash<D::Hasher>, u64, D>,
    /// Keys with reference count zero, for efficient garbage collection.
    ///
    /// The `ChildRef` here creates storage overhead -- an additional dag node for
    /// each key -- but the `rc_0` map is expected to be small, so this
    /// shouldn't matter.
    #[cfg(feature = "public-internal-structure")]
    pub rc_0: Map<ArenaKey<D::Hasher>, ChildRef<D>, D>,
    #[cfg(not(feature = "public-internal-structure"))]
    rc_0: Map<ArenaKey<D::Hasher>, ChildRef<D>, D>,
}

impl<D: DB> RcMap<D> {
    /// Returns true iff the key is charged.
    pub(crate) fn contains(&self, key: &ArenaKey<D::Hasher>) -> bool {
        self.get_rc(key).is_some()
    }

    /// Get the current reference count for a key.
    /// Returns Some(n) if key is charged (n >= 0), None if key is not in `RcMap`.
    pub(crate) fn get_rc(&self, key: &ArenaKey<D::Hasher>) -> Option<u64> {
        if let ArenaKey::Ref(key) = key
            && let Some(count) = self.rc_ge_1.get(key)
        {
            Some(*count)
        } else if self.rc_0.contains_key(key) {
            Some(0)
        } else {
            None // Key not charged at all
        }
    }

    #[must_use]
    pub(crate) fn ins_root(&self, key: ArenaKey<D::Hasher>) -> Self {
        RcMap {
            rc_ge_1: self.rc_ge_1.clone(),
            rc_0: self.rc_0.insert(key.clone(), ChildRef::new(key.clone())),
        }
    }

    #[must_use]
    pub(crate) fn rm_root(&self, key: &ArenaKey<D::Hasher>) -> Self {
        RcMap {
            rc_ge_1: self.rc_ge_1.clone(),
            rc_0: self.rc_0.remove(key),
        }
    }

    /// Increment the reference count for a key.
    /// Returns `(new_rcmap, new_rc)`.
    #[must_use]
    pub(crate) fn modify_rc(&self, key: &ArenaHash<D::Hasher>, updated: u64) -> Self {
        let curr = self.rc_ge_1.get(key).copied().unwrap_or(0);
        match (curr, updated) {
            (0, 0) =>
            // Final ref count is zero, add to rc_0.
            {
                RcMap {
                    rc_ge_1: self.rc_ge_1.clone(),
                    rc_0: self.rc_0.insert(
                        ArenaKey::Ref(key.clone()),
                        ChildRef::new(ArenaKey::Ref(key.clone())),
                    ),
                }
            }
            (0, 1..) =>
            // Key exists with rc = 0, move to rc_ge_1 with count n
            {
                RcMap {
                    rc_ge_1: self.rc_ge_1.insert(key.clone(), updated),
                    rc_0: self.rc_0.remove(&ArenaKey::Ref(key.clone())),
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
                    rc_0: self.rc_0.insert(
                        ArenaKey::Ref(key.clone()),
                        ChildRef::new(ArenaKey::Ref(key.clone())),
                    ),
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
            result.insert(ArenaKey::Ref(key.clone()), *count);
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
    use crate::storable::SMALL_OBJECT_LIMIT;

    // Test Storable serialization of vector of ChildRef, to be sure the manual
    // Storable impl makes sense.
    #[test]
    fn keyref_round_trip_storable() {
        // Create a dummy value to get an arena key
        let val = Sp::<_, InMemoryDB>::new([0u8; 1024]);
        let key = val.as_child();
        let keyref = ChildRef::<InMemoryDB>::new(key);

        let _ = Sp::new(keyref.clone());
        // Create a vector with 3 of the same ChildRef
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
        let deserialized: Vec<Sp<ChildRef<InMemoryDB>, InMemoryDB>> =
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
            match current {
                ArenaKey::Direct(d) => to_visit.extend(d.children.iter().cloned()),
                ArenaKey::Ref(ref r) => {
                    arena.with_backend(|backend| {
                        let disk_obj = backend.get(r).expect("Key should exist in backend");
                        to_visit.extend(disk_obj.children.clone());
                    });
                }
            }
        }
        visited
    }

    // Test that keys in rc_0 are descendants of RcMap via ChildRef storage.
    #[test]
    fn rc_0_keys_are_descendants() {
        let val = Sp::<_, InMemoryDB>::new([42u8; SMALL_OBJECT_LIMIT]);
        let key = val.root.clone();

        // Create RcMap with key in rc_0
        let rcmap = RcMap::<InMemoryDB>::default().modify_rc(&key, 0);
        assert!(rcmap.rc_0.contains_key(&ArenaKey::Ref(key.clone())));

        let descendants = get_rcmap_descendants(&rcmap);
        assert!(
            descendants.contains(&val.as_child()),
            "Key in rc_0 must be a descendant of RcMap"
        );
    }

    // Comprehensive test of RcMap basic operations
    #[test]
    fn rcmap_operations() {
        // Create test keys using simple u8 values
        let val1 = Sp::<_, InMemoryDB>::new([1u8; 1024]);
        let key1 = val1.as_child();
        let ArenaKey::Ref(hash1) = key1.clone() else {
            panic!("testing refs");
        };
        let val2 = Sp::<_, InMemoryDB>::new([2u8; 1024]);
        let key2 = val2.as_child();
        let ArenaKey::Ref(hash2) = key2.clone() else {
            panic!("testing refs");
        };
        let val3 = Sp::<_, InMemoryDB>::new([3u8; 1024]);
        let key3 = val3.as_child();
        let ArenaKey::Ref(hash3) = key3.clone() else {
            panic!("testing refs");
        };

        let rcmap = RcMap::<InMemoryDB>::default().ins_root(key1.clone());

        // Test initialize_key sets rc=0
        assert_eq!(rcmap.get_rc(&key1), Some(0), "get_rc should return 0");
        assert!(rcmap.rc_0.contains_key(&key1), "key1 should be in rc_0 map");
        assert!(
            !rcmap.rc_ge_1.contains_key(&hash1),
            "key1 should not be in rc_ge_1 map"
        );

        // Test increment_rc from 0 to 1 moves to rc_ge_1
        let rcmap = rcmap.modify_rc(&hash1, 1);
        assert_eq!(rcmap.get_rc(&key1), Some(1), "get_rc should return 1");
        assert!(
            !rcmap.rc_0.contains_key(&key1),
            "key1 should not be in rc_0 map"
        );
        assert!(
            rcmap.rc_ge_1.contains_key(&hash1),
            "key1 should be in rc_ge_1 map"
        );

        // Test increment_rc multiple times
        let rcmap = rcmap.modify_rc(&hash1, 2);
        let rcmap = rcmap.modify_rc(&hash1, 3);
        assert_eq!(rcmap.get_rc(&key1), Some(3), "get_rc should return 3");
        assert!(
            rcmap.rc_ge_1.contains_key(&hash1),
            "key1 should remain in rc_ge_1 map"
        );

        // Test decrement_rc multiple times
        let rcmap = rcmap.modify_rc(&hash1, 2);
        let rcmap = rcmap.modify_rc(&hash1, 1);
        assert!(
            rcmap.rc_ge_1.contains_key(&hash1),
            "key1 should still be in rc_ge_1 map"
        );

        // Test decrement_rc from 1 to 0 moves back to rc_0
        let rcmap = rcmap.modify_rc(&hash1, 0);
        assert_eq!(rcmap.get_rc(&key1), Some(0), "get_rc should return 0");
        assert!(
            rcmap.rc_0.contains_key(&key1),
            "key1 should be back in rc_0 map"
        );
        assert!(
            !rcmap.rc_ge_1.contains_key(&hash1),
            "key1 should not be in rc_ge_1 map"
        );

        // Test get_rc on nonexistent key returns None
        assert_eq!(
            rcmap.get_rc(&key2),
            None,
            "get_rc on nonexistent key should return None"
        );

        // Test multiple keys
        let rcmap = rcmap.modify_rc(&hash2, 1);
        let rcmap = rcmap.modify_rc(&hash3, 2);

        // Verify all keys have correct reference counts
        assert_eq!(rcmap.get_rc(&key1), Some(0));
        assert_eq!(rcmap.get_rc(&key2), Some(1));
        assert_eq!(rcmap.get_rc(&key3), Some(2));

        // Verify correct map placement
        assert!(rcmap.rc_0.contains_key(&key1));
        assert!(rcmap.rc_ge_1.contains_key(&hash2));
        assert!(rcmap.rc_ge_1.contains_key(&hash3));

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
