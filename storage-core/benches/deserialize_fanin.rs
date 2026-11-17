//! Benchmarks `Arena::deserialize_sp` on a high-fan-in graph: many distinct
//! nodes that all reference one shared child.
//!
//! This is the axis the sibling `transient-crypto/benches/merkle_deserialize`
//! bench cannot see. That one builds a *tree* (every node has one parent), so no
//! node is referenced more than once. `deserialize_sp` used to recompute a
//! referenced child's hash *once per reference to it*, in two separate loops --
//! O(references x child size). A tree never exercises it; a shared child does.
//!
//! Here `n` distinct leaf holders sit under a shallow balanced 16-ary tree, and
//! every one of them points at the same shared leaf of `n * LEAF_STRIDE` bytes.
//! The payload stays roughly linear in `n`, so any work that scales with
//! references-times-referenced-size is O(n^2) in `n` -- the byte-bounded worst
//! case, where an attacker splits a fixed payload budget between one big shared
//! object and many references to it.
//!
//! `deserialize_sp` had TWO such O(n^2) costs on this shape, both now fixed:
//!   1. the two-loop hash recomputation in `deserialize_sp` itself -- fixed by
//!      memoizing each node's hash by position;
//!   2. `get_from_cache` deriving a cached node's child repr via `as_child`, which
//!      re-serializes and re-hashes the whole node -- O(node size) on every one of
//!      its references. Fixed by using `Ref(key)` directly: the deserializer only
//!      reaches that path for a child it was handed as `Ref(key)`.
//!
//! With both fixes, a B-byte node referenced R times costs O(R + B), not O(R x B),
//! so this bench's throughput is flat in `n`. It is the regression guard against
//! either quadratic coming back. (A fixed-size shared node already looked linear in
//! R; the quadratic needed the node size to grow with the payload, as here.)
//!
//! (A tree, not a chain, to keep the fan-in while staying well under the
//! deserialize recursion limit.)
//!
//! Run with:
//!
//! ```text
//! cargo bench -p midnight-storage-core --bench deserialize_fanin
//! ```

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use midnight_storage_core as storage;
use midnight_storage_core::DefaultDB;
use midnight_storage_core::Storable;
use midnight_storage_core::arena::{ArenaKey, Sp};
use midnight_storage_core::db::DB;
use midnight_storage_core::storable::Loader;
use midnight_storage_core::storage::default_storage;
use serialize::{Deserializable, Serializable, Tagged, tagged_deserialize, tagged_serialize};

/// The shared leaf is `n * LEAF_STRIDE` bytes, so references (`n`) times
/// referenced-size (`n * LEAF_STRIDE`) is quadratic in `n` -- the shape the fix
/// collapses to linear.
const LEAF_STRIDE: usize = 64;

#[derive(Clone, Serializable, Storable)]
#[storable(base)]
#[tag = "bench-fanin-leaf"]
struct Leaf {
    data: Vec<u8>,
}

/// A node is either a leaf holder (points at the shared leaf) or an inner node
/// (groups up to 16 children). `id` keeps otherwise-identical nodes distinct, so
/// the leaf holders do not dedup into one and the fan-in is preserved.
#[derive(Storable, Clone)]
#[tag = "bench-fanin-node"]
struct Node {
    id: u64,
    kids: Vec<Sp<Node>>,
    shared: Option<Sp<Leaf>>,
}

/// `n` distinct leaf holders, all referencing one shared leaf, under a balanced
/// 16-ary tree. Depth is `log16(n)`, so deserialize recursion stays shallow.
fn build_tree(n: u64) -> Node {
    let arena = &default_storage::<DefaultDB>().arena;
    let leaf_bytes = n as usize * LEAF_STRIDE;
    let shared = arena.alloc(Leaf {
        data: vec![0xAB; leaf_bytes],
    });

    let mut next_id = 0u64..;
    let mut level: Vec<Sp<Node>> = (0..n)
        .map(|_| {
            arena.alloc(Node {
                id: next_id.next().unwrap(),
                kids: Vec::new(),
                shared: Some(shared.clone()),
            })
        })
        .collect();

    while level.len() > 1 {
        level = level
            .chunks(16)
            .map(|chunk| {
                arena.alloc(Node {
                    id: next_id.next().unwrap(),
                    kids: chunk.to_vec(),
                    shared: None,
                })
            })
            .collect();
    }
    (*level.pop().expect("n >= 1")).clone()
}

fn bench_deserialize_fanin(c: &mut Criterion) {
    let mut group = c.benchmark_group("deserialize_fanin");
    group.sample_size(10);

    for &n in &[256u64, 1_000, 4_000] {
        let head = build_tree(n);
        let mut bytes = Vec::new();
        tagged_serialize(&head, &mut bytes).expect("serialize");

        group.throughput(Throughput::Elements(n));
        group.bench_with_input(BenchmarkId::from_parameter(n), &bytes, |b, bytes| {
            b.iter(|| {
                let _: Node = tagged_deserialize(&mut &bytes[..]).expect("deserialize");
            });
        });
    }

    group.finish();
}

criterion_group!(benches, bench_deserialize_fanin);
criterion_main!(benches);
