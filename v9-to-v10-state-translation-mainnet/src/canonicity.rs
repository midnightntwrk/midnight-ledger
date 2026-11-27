// This file is part of midnight-ledger.
// Copyright (C) 2026 Midnight Foundation
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

//! Checks a stored state node by node, running the invariant checks the wire
//! decoder runs, without decoding the whole state at once.
//!
//! The wire decoder (`IrLoader`) decodes every node eagerly and checks each
//! with `check_invariant_memoized`. This walk decodes one node at a time from
//! the arena with a loader that does the same checks but hands back lazy
//! children, queueing each child with the type its parent decoded it at. Only
//! a window of nodes is decoded at once; between windows the arena is
//! garbage-collected.

use crate::Db;
use serialize::{Deserializable, RECURSION_LIMIT, Serializable};
use std::any::TypeId;
use std::cell::RefCell;
use std::collections::HashMap;
use std::io;
use std::time::{Duration, Instant};
use storage_core::arena::{Arena, ArenaHash, ArenaKey, BackendLoader, Sp};
use storage_core::dag_memo::InvariantMemos;
use storage_core::storable::Loader;
use storage_core::{DefaultHasher, Storable};

type Key = ArenaKey<DefaultHasher>;
type Check = fn(&Walk, &Key, u32) -> io::Result<()>;

struct Walk<'a> {
    arena: &'a Arena<Db>,
    pending: RefCell<Vec<(Key, Check, u32)>>,
    /// The deepest each node has been reached at, per type it was decoded as.
    /// A node reached deeper is checked again, so the nesting limit applies
    /// to the longest path, as the wire decoder's height tracking does.
    depths: RefCell<HashMap<(ArenaHash<DefaultHasher>, TypeId), u32>>,
    memos: RefCell<InvariantMemos<Db>>,
}

struct NodeLoader<'w, 'a> {
    walk: &'w Walk<'a>,
    depth: u32,
}

impl Loader<Db> for NodeLoader<'_, '_> {
    const CHECK_INVARIANTS: bool = true;

    fn get<T: Storable<Db>>(&self, key: &Key) -> io::Result<Sp<T, Db>> {
        let depth = self.depth + 1;
        if depth > RECURSION_LIMIT {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("nesting deeper than {RECURSION_LIMIT}"),
            ));
        }
        let typed = (key.hash().clone(), TypeId::of::<T>());
        let deeper = match self.walk.depths.borrow_mut().entry(typed) {
            std::collections::hash_map::Entry::Occupied(mut e) if *e.get() < depth => {
                e.insert(depth);
                true
            }
            std::collections::hash_map::Entry::Occupied(_) => false,
            std::collections::hash_map::Entry::Vacant(e) => {
                e.insert(depth);
                true
            }
        };
        if deeper {
            self.walk
                .pending
                .borrow_mut()
                .push((key.clone(), check_node::<T>, depth));
        }
        BackendLoader::new(self.walk.arena, Some(0)).get(key)
    }

    fn alloc<T: Storable<Db>>(&self, obj: T) -> Sp<T, Db> {
        self.walk.arena.alloc(obj)
    }

    fn get_recursion_depth(&self) -> u32 {
        self.depth
    }

    fn do_check<T: Storable<Db>>(&self, obj: T) -> io::Result<T> {
        obj.check_invariant_memoized(&mut self.walk.memos.borrow_mut())?;
        Ok(obj)
    }
}

/// Decodes the node at `key` as a `T`, which runs its invariant checks, and
/// checks it re-encodes to the same bytes and children.
fn check_node<T: Storable<Db>>(walk: &Walk, key: &Key, depth: u32) -> io::Result<()> {
    let (data, children) = match key {
        ArenaKey::Direct(node) => ((*node.data).clone(), (*node.children).clone()),
        ArenaKey::Ref(hash) => {
            let obj = walk
                .arena
                .with_backend(|b| b.get(hash).cloned())
                .ok_or_else(|| {
                    io::Error::new(io::ErrorKind::NotFound, format!("node {hash:?} missing"))
                })?;
            // `OnDiskObject`'s data is private; its encoding leads with it.
            let mut encoded = Vec::new();
            obj.serialize(&mut encoded)?;
            let data: Vec<u8> = Deserializable::deserialize(&mut &encoded[..], 0)?;
            (data, obj.children)
        }
    };
    let loader = NodeLoader { walk, depth };
    let value = T::from_binary_repr(&mut &data[..], &mut children.iter().cloned(), &loader)?;
    let mut reencoded = Vec::new();
    value.to_binary_repr(&mut reencoded)?;
    if reencoded != data || value.children() != children {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "{} does not re-encode to its stored form",
                std::any::type_name::<T>()
            ),
        ));
    }
    Ok(())
}

/// Checks every node reachable from `root`, decoded as `T`, `window` nodes at
/// a time.
pub fn check_stored<T: Storable<Db>>(
    arena: &Arena<Db>,
    root: Key,
    window: usize,
    gc_budget: Duration,
) -> io::Result<u64> {
    let walk = Walk {
        arena,
        depths: RefCell::new(HashMap::from([(
            (root.hash().clone(), TypeId::of::<T>()),
            1,
        )])),
        pending: RefCell::new(vec![(root, check_node::<T> as Check, 1)]),
        memos: RefCell::new(InvariantMemos::new()),
    };
    let started = Instant::now();
    let mut checked = 0u64;
    loop {
        for _ in 0..window {
            let Some((key, check, depth)) = walk.pending.borrow_mut().pop() else {
                break;
            };
            check(&walk, &key, depth)?;
            checked += 1;
        }
        // Memos hold decoded results; sharing is lost across windows, not
        // correctness.
        *walk.memos.borrow_mut() = InvariantMemos::new();
        let culled = arena.with_backend(|b| b.gc(gc_budget));
        // SAFETY: `malloc_trim` only returns free heap memory to the OS.
        #[cfg(all(target_os = "linux", target_env = "gnu"))]
        unsafe {
            libc::malloc_trim(0)
        };
        log::info!(
            "canonicity: {checked} nodes checked in {:.0}s, {} pending, {culled} culled; rss {} MiB",
            started.elapsed().as_secs_f64(),
            walk.pending.borrow().len(),
            crate::rss_mib(),
        );
        if walk.pending.borrow().is_empty() {
            return Ok(checked);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::check_stored;
    use crate::Db;
    use base_crypto::cost_model::RunningCost;
    use base_crypto::fab::AlignedValue;
    use onchain_state_v9::state::{ChargedState as ChargedV9, StateValue};
    use onchain_state_v10::state::ChargedState as ChargedV10;
    use std::time::Duration;
    use storage_core::arena::Sp;
    use storage_core::storage::{Storage, default_storage, set_default_storage};

    /// Checks a v9 `ChargedState` holding `cells` as a v10 one.
    fn check_as_v10(cells: &[u8]) -> std::io::Result<u64> {
        let cell = |b: u8| {
            let mut bytes = vec![0xaau8; 200];
            bytes[4] = b;
            StateValue::Cell(Sp::new(AlignedValue::from(bytes)))
        };
        let mut data = ChargedV9::<Db>::new(cell(cells[0]));
        for &b in &cells[1..] {
            data = data.update(cell(b), |_, _| RunningCost::ZERO, |_| 0).0;
        }
        let mut stored = Sp::new(data);
        stored.persist();
        let arena = &default_storage::<Db>().arena;
        arena.with_backend(|b| b.flush_all_changes_to_db());
        check_stored::<ChargedV10<Db>>(arena, stored.as_child(), 100, Duration::ZERO)
    }

    // Storage 3 leaves a short extension chain in `rc_0` when two charged
    // roots inline 200 bytes each and diverge early (the translation crate's
    // `short_chained_rc_0_is_canonicalised` fixture); ledger 10's decoder
    // rejects it, so the walk must too. One root makes no chain.
    #[test]
    fn rejects_what_the_decoder_rejects() {
        let dir = tempfile::tempdir().expect("temp dir");
        set_default_storage(|| Storage::new(1000, Db::open(dir.path())))
            .expect("default storage unset");
        check_as_v10(&[1]).expect("one root is canonical");
        let err = check_as_v10(&[1, 2]).expect_err("a short extension chain must be rejected");
        assert!(
            err.to_string()
                .contains("when having another Node::Extension child"),
            "rejected for another reason: {err}",
        );
    }
}
