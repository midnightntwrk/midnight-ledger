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

//! Clean-room property / randomized tests for the storage containers.
//!
//! These are *semantic* property tests, derived from the contracts the code
//! states about itself, not from serialize->deserialize byte round-trips:
//!
//!  * `Map` must behave as a dictionary (model-checked against a reference
//!    `BTreeMap`) -- its own doc-comments promise `get`/`insert`/`remove`/
//!    `size`/`contains_key` semantics with specific complexities.
//!  * `Array` must behave as a dense vector (model-checked against `Vec`); its
//!    own `invariant()` asserts every stored index is `< len`, and the docs say
//!    "elements are stored at indices `0..len()`".
//!  * The trie is *content addressed*: the representation of a map must depend
//!    only on its logical contents, never on the insertion/removal history.
//!    Two maps with equal contents must serialize identically (canonical form).
//!  * A value built through the public API must survive a round trip through the
//!    *untrusted-input* loader, `Arena::deserialize_sp`, which enforces both
//!    `check_invariant` (`Loader::CHECK_INVARIANTS == true`) and the storage
//!    "normal form" check. If the API can build something the loader rejects,
//!    the API and the loader disagree on well-formedness.
//!  * `Arena::deserialize_sp` documents itself as "a boundary for user
//!    controlled input ... we need to be careful here to gracefully handle
//!    malformed (or even maliciously formed?) input" -- so it must never panic
//!    on arbitrary bytes.
//!
//! Randomness is a small self-contained SplitMix64 PRNG seeded deterministically
//! so every failure is reproducible from its seed.

use midnight_storage::arena::Sp;
use midnight_storage::storage::{Array, Map};
use midnight_storage::{DefaultDB, Storage};
use serialize::Serializable;
use std::collections::BTreeMap;

// ---------------------------------------------------------------------------
// Tiny deterministic PRNG (SplitMix64). Avoids any dependency on rand's
// distributions so the tests are fully self-contained and reproducible.
// ---------------------------------------------------------------------------
struct Rng(u64);
impl Rng {
    fn new(seed: u64) -> Self {
        Rng(seed.wrapping_add(0x9E37_79B9_7F4A_7C15))
    }
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    /// Uniform in `0..n` (n > 0).
    fn below(&mut self, n: u64) -> u64 {
        self.next_u64() % n
    }
    fn bool(&mut self) -> bool {
        self.next_u64() & 1 == 0
    }
    /// A key drawn from a deliberately mixed distribution: often small (so keys
    /// share long common prefixes and force Extension / Branch / MidBranchLeaf
    /// nodes to appear), occasionally full-range.
    fn key(&mut self, small_space: u64) -> u64 {
        if self.bool() {
            self.below(small_space)
        } else {
            self.next_u64()
        }
    }
}

fn fresh_arena() -> midnight_storage::arena::Arena<DefaultDB> {
    Storage::<DefaultDB>::new(64, DefaultDB::default()).arena
}

// ---------------------------------------------------------------------------
// Property 1: `Map` is a faithful dictionary (model-based oracle).
// ---------------------------------------------------------------------------
#[test]
fn prop_map_behaves_as_dictionary() {
    for seed in 0..48u64 {
        let mut rng = Rng::new(seed.wrapping_mul(0xD1B5_4A32_D192_ED03));
        // Vary the key space per seed so some runs are collision-heavy and
        // some are sparse.
        let small_space = 1 + (seed % 6) * 4; // 1,5,9,...
        let mut model: BTreeMap<u64, u64> = BTreeMap::new();
        let mut map: Map<u64, u64> = Map::new();
        let mut touched: Vec<u64> = Vec::new();

        for _ in 0..250 {
            match rng.below(3) {
                0 | 1 => {
                    // insert (may overwrite)
                    let k = rng.key(small_space.max(1));
                    let v = rng.next_u64();
                    map = map.insert(k, v);
                    model.insert(k, v);
                    if !touched.contains(&k) {
                        touched.push(k);
                    }
                }
                _ => {
                    // remove: bias towards removing something that exists
                    let k = if !model.is_empty() && rng.bool() {
                        *model.keys().nth((rng.below(model.len() as u64)) as usize).unwrap()
                    } else {
                        rng.key(small_space.max(1))
                    };
                    map = map.remove(&k);
                    model.remove(&k);
                    if !touched.contains(&k) {
                        touched.push(k);
                    }
                }
            }

            // Structural agreement.
            assert_eq!(
                map.size(),
                model.len(),
                "seed {seed}: size disagrees with model"
            );
            assert_eq!(
                map.is_empty(),
                model.is_empty(),
                "seed {seed}: is_empty disagrees with model"
            );

            // Point-wise agreement on every key we've ever touched, plus a
            // couple of never-touched keys (which must be absent).
            for k in touched.iter().copied().chain([u64::MAX, small_space + 12345]) {
                assert_eq!(
                    map.get(&k).copied(),
                    model.get(&k).copied(),
                    "seed {seed}: get({k}) disagrees with model"
                );
                assert_eq!(
                    map.contains_key(&k),
                    model.contains_key(&k),
                    "seed {seed}: contains_key({k}) disagrees with model"
                );
            }
        }

        // Whole-contents agreement, via the map's own iterator.
        let from_iter: BTreeMap<u64, u64> = map.iter().map(|(k, v)| (k, *v)).collect();
        assert_eq!(
            from_iter, model,
            "seed {seed}: iter() does not reproduce the logical contents"
        );
    }
}

// ---------------------------------------------------------------------------
// Property 2: `Array` is a faithful dense vector (model-based oracle),
// including the dense-index invariant it asserts about itself.
// ---------------------------------------------------------------------------
#[test]
fn prop_array_behaves_as_vec() {
    for seed in 0..48u64 {
        let mut rng = Rng::new(seed.wrapping_mul(0x2545_F491_4F6C_DD1D) ^ 0xAAAA);
        let mut model: Vec<u64> = Vec::new();
        let mut arr: Array<u64> = Array::new();

        for _ in 0..250 {
            match rng.below(4) {
                0 | 1 => {
                    // push (grow by one)
                    let v = rng.next_u64();
                    arr = arr.push(v);
                    model.push(v);
                }
                2 => {
                    // in-bounds overwrite
                    if model.is_empty() {
                        continue;
                    }
                    let idx = rng.below(model.len() as u64) as usize;
                    let v = rng.next_u64();
                    arr = arr
                        .insert(idx, v)
                        .expect("insert at an in-bounds index must return Some");
                    model[idx] = v;
                }
                _ => {
                    // out-of-bounds overwrite must be rejected (returns None),
                    // and must not change the array.
                    let idx = model.len() + (rng.below(4) as usize);
                    assert!(
                        arr.insert(idx, 0xDEAD).is_none(),
                        "seed {seed}: insert() at out-of-bounds index {idx} (len {}) must be None",
                        model.len()
                    );
                }
            }

            assert_eq!(arr.len(), model.len(), "seed {seed}: len disagrees");
            assert_eq!(arr.is_empty(), model.is_empty(), "seed {seed}: is_empty disagrees");
            // Every in-bounds index must match; the one-past-the-end index must
            // be None (dense, no holes past len).
            for i in 0..model.len() {
                assert_eq!(
                    arr.get(i).copied(),
                    Some(model[i]),
                    "seed {seed}: get({i}) disagrees with model"
                );
            }
            assert_eq!(
                arr.get(model.len()),
                None,
                "seed {seed}: get(len) must be None"
            );
        }

        // Iterator reproduces the vector exactly and in order.
        let from_iter: Vec<u64> = arr.iter_deref().copied().collect();
        assert_eq!(from_iter, model, "seed {seed}: iter_deref order/content mismatch");
    }
}

// ---------------------------------------------------------------------------
// Property 3: canonical (history-independent) representation.
//
// A content-addressed Merkle trie must serialize identically whenever the
// logical contents are equal, regardless of the order of insertions/removals
// used to reach that state. If not, two honest nodes computing the same map
// could disagree on its hash/serialization -- a determinism/consensus hazard.
// ---------------------------------------------------------------------------
#[test]
fn prop_map_representation_is_canonical() {
    for seed in 0..40u64 {
        let mut rng = Rng::new(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0x5151);
        let small_space = 1 + (seed % 8) * 3;

        // Decide the *target* logical contents.
        let target_len = rng.below(40);
        let mut target: BTreeMap<u64, u64> = BTreeMap::new();
        for _ in 0..target_len {
            let k = rng.key(small_space.max(1));
            let v = rng.next_u64();
            target.insert(k, v);
        }

        // Build A: insert target entries in one shuffled order.
        let mut entries: Vec<(u64, u64)> = target.iter().map(|(k, v)| (*k, *v)).collect();
        shuffle(&mut entries, &mut rng);
        let mut map_a: Map<u64, u64> = Map::new();
        for (k, v) in &entries {
            map_a = map_a.insert(*k, *v);
        }

        // Build B: a *different* history reaching the same contents --
        // different order, plus decoy keys that are later removed, plus
        // overwrites of the final keys with junk before setting the real value.
        shuffle(&mut entries, &mut rng);
        let mut map_b: Map<u64, u64> = Map::new();
        let mut decoys: Vec<u64> = Vec::new();
        for (k, v) in &entries {
            // overwrite-then-correct
            map_b = map_b.insert(*k, rng.next_u64());
            // sprinkle a decoy key that we'll delete later
            if rng.bool() {
                let d = rng.next_u64() | 1; // avoid clashing with small keys
                if !target.contains_key(&d) {
                    map_b = map_b.insert(d, rng.next_u64());
                    decoys.push(d);
                }
            }
            map_b = map_b.insert(*k, *v);
        }
        for d in &decoys {
            map_b = map_b.remove(d);
        }

        // Sanity: same logical contents.
        let contents_a: BTreeMap<u64, u64> = map_a.iter().map(|(k, v)| (k, *v)).collect();
        let contents_b: BTreeMap<u64, u64> = map_b.iter().map(|(k, v)| (k, *v)).collect();
        assert_eq!(contents_a, target, "seed {seed}: map_a contents wrong");
        assert_eq!(contents_b, target, "seed {seed}: map_b contents wrong");

        // The canonical-form oracle: identical serialization. `Sp::new`
        // allocates against the default storage, which is where `Map::insert`
        // built every internal node, so the whole sub-graph is reachable.
        let sp_a = Sp::new(map_a);
        let sp_b = Sp::new(map_b);
        let mut buf_a = Vec::new();
        let mut buf_b = Vec::new();
        Sp::serialize(&sp_a, &mut buf_a).expect("serialize A");
        Sp::serialize(&sp_b, &mut buf_b).expect("serialize B");
        assert_eq!(
            buf_a, buf_b,
            "seed {seed}: two maps with identical contents serialized differently \
             (non-canonical representation; contents={target:?})"
        );
    }
}

// ---------------------------------------------------------------------------
// Property 4: values built via the public API survive the untrusted-input
// loader, which enforces `check_invariant` + storage normal form.
// ---------------------------------------------------------------------------
#[test]
fn prop_api_built_map_reloads_under_checking_loader() {
    for seed in 0..40u64 {
        let mut rng = Rng::new(seed.wrapping_mul(0xC2B2_AE3D_27D4_EB4F) ^ 0x9999);
        let small_space = 1 + (seed % 10) * 2;
        let mut model: BTreeMap<u64, u64> = BTreeMap::new();
        let mut map: Map<u64, u64> = Map::new();

        for _ in 0..120 {
            if model.is_empty() || rng.below(3) != 0 {
                let k = rng.key(small_space.max(1));
                let v = rng.next_u64();
                map = map.insert(k, v);
                model.insert(k, v);
            } else {
                let k = *model.keys().nth(rng.below(model.len() as u64) as usize).unwrap();
                map = map.remove(&k);
                model.remove(&k);
            }
        }

        let sp = Sp::new(map);
        let mut buf = Vec::new();
        Sp::serialize(&sp, &mut buf).expect("serialize");

        // A *fresh* arena rebuilds the value purely from the (self-contained)
        // byte stream, exercising the invariant-checking loader from scratch.
        let arena = fresh_arena();
        let reloaded: Sp<Map<u64, u64>> = arena
            .deserialize_sp(&mut &buf[..], 0)
            .unwrap_or_else(|e| {
                panic!(
                    "seed {seed}: API-built map REJECTED by the invariant-checking \
                     loader (check_invariant or normal-form failure): {e}"
                )
            });

        let reloaded_contents: BTreeMap<u64, u64> =
            reloaded.iter().map(|(k, v)| (k, *v)).collect();
        assert_eq!(
            reloaded_contents, model,
            "seed {seed}: reloaded map lost/altered contents"
        );
    }
}

#[test]
fn prop_api_built_array_reloads_under_checking_loader() {
    for seed in 0..40u64 {
        let mut rng = Rng::new(seed.wrapping_mul(0x1656_67B1_9E37_79F9) ^ 0x4242);
        let mut model: Vec<u64> = Vec::new();
        let mut arr: Array<u64> = Array::new();
        for _ in 0..120 {
            if model.is_empty() || rng.below(3) != 0 {
                let v = rng.next_u64();
                arr = arr.push(v);
                model.push(v);
            } else {
                let idx = rng.below(model.len() as u64) as usize;
                let v = rng.next_u64();
                arr = arr.insert(idx, v).unwrap();
                model[idx] = v;
            }
        }

        let sp = Sp::new(arr);
        let mut buf = Vec::new();
        Sp::serialize(&sp, &mut buf).expect("serialize");

        let arena = fresh_arena();
        let reloaded: Sp<Array<u64>> = arena
            .deserialize_sp(&mut &buf[..], 0)
            .unwrap_or_else(|e| {
                panic!(
                    "seed {seed}: API-built array REJECTED by the invariant-checking \
                     loader (check_invariant or normal-form failure): {e}"
                )
            });

        let reloaded_contents: Vec<u64> = reloaded.iter_deref().copied().collect();
        assert_eq!(
            reloaded_contents, model,
            "seed {seed}: reloaded array lost/altered contents"
        );
    }
}

// ---------------------------------------------------------------------------
// Property 5: the untrusted-input deserialization boundary must never panic
// on arbitrary / mutated bytes (its own doc comment promises graceful
// handling of "malformed (or even maliciously formed?) input").
// ---------------------------------------------------------------------------
#[test]
fn prop_deserialize_sp_never_panics_on_garbage() {
    use std::panic::{AssertUnwindSafe, catch_unwind};

    // Silence the noise of intentional panics during this test.
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));

    // A valid serialization to use as a mutation base, so some inputs reach
    // deep into the decoder rather than being rejected at the first byte.
    let base = {
        let mut m: Map<u64, u64> = Map::new();
        for i in 0..20u64 {
            m = m.insert(i, i.wrapping_mul(7));
        }
        let sp = Sp::new(m);
        let mut buf = Vec::new();
        Sp::serialize(&sp, &mut buf).unwrap();
        buf
    };

    let mut rng = Rng::new(0xFEED_FACE_C0FF_EE00);
    let mut failures: Vec<String> = Vec::new();

    for i in 0..4000u64 {
        // Three flavours of adversarial input.
        let input: Vec<u8> = match i % 3 {
            0 => {
                // pure random bytes, random length up to 256
                let len = rng.below(256) as usize;
                (0..len).map(|_| rng.below(256) as u8).collect()
            }
            1 => {
                // valid serialization with a handful of single-byte flips
                let mut b = base.clone();
                if !b.is_empty() {
                    let flips = 1 + rng.below(6);
                    for _ in 0..flips {
                        let pos = rng.below(b.len() as u64) as usize;
                        b[pos] = rng.below(256) as u8;
                    }
                }
                b
            }
            _ => {
                // valid serialization truncated at a random point
                let cut = rng.below(base.len().max(1) as u64) as usize;
                base[..cut].to_vec()
            }
        };

        // Fresh arena per input: a panic must not be able to corrupt shared
        // state and cascade into later iterations.
        //
        // "Did not panic" is only the floor. The stronger property: for every
        // input the loader ACCEPTS (returns Ok), the resulting value must still
        // satisfy the map's full semantic invariants -- otherwise malformed
        // bytes have produced a "valid" value that flows downstream. So inside
        // the guarded closure we run the same size- and canonicality-oracles we
        // use on the honest path, and surface any violation.
        let arena = fresh_arena();
        let res: Result<Option<String>, _> = catch_unwind(AssertUnwindSafe(|| {
            match arena.deserialize_sp::<Map<u64, u64>, _>(&mut &input[..], 0) {
                Err(_) => None, // gracefully rejected -- fine
                Ok(map) => {
                    // (S) the reported size must equal the true entry count.
                    let count = map.iter().count();
                    if map.size() != count {
                        return Some(format!(
                            "accepted map with size()={} but iter() yields {count} entries",
                            map.size()
                        ));
                    }
                    // (C) the accepted encoding must be canonical: rebuilding
                    // from its own contents via the public API must serialize
                    // identically.
                    let mut rebuilt: Map<u64, u64> = Map::new();
                    for (k, v) in map.iter() {
                        rebuilt = rebuilt.insert(k, *v);
                    }
                    let mut a = Vec::new();
                    let mut b = Vec::new();
                    Sp::serialize(&map, &mut a).unwrap();
                    Sp::serialize(&Sp::new(rebuilt), &mut b).unwrap();
                    if a != b {
                        return Some("accepted a non-canonical map encoding".to_string());
                    }
                    None
                }
            }
        }));
        match res {
            Err(_) => failures.push(format!(
                "PANIC on input #{i} ({} bytes): {:02x?}",
                input.len(),
                input
            )),
            Ok(Some(reason)) => failures.push(format!(
                "ACCEPT-INVARIANT-VIOLATION on input #{i}: {reason}; bytes ({}): {:02x?}",
                input.len(),
                input
            )),
            Ok(None) => {}
        }
        if failures.len() >= 5 {
            break;
        }
    }

    std::panic::set_hook(prev);
    assert!(
        failures.is_empty(),
        "deserialize_sp PANICKED on malformed input (should return Err instead):\n{}",
        failures.join("\n")
    );
}

// Fisher-Yates using the local PRNG.
fn shuffle<T>(v: &mut [T], rng: &mut Rng) {
    let n = v.len();
    for i in (1..n).rev() {
        let j = rng.below((i + 1) as u64) as usize;
        v.swap(i, j);
    }
}
