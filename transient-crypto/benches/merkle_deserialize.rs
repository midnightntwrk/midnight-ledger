//! Benchmarks `tagged_deserialize` on `MerkleTree` across a range of leaf
//! counts, to show that per-leaf cost is not constant.
//!
//! For a `Storable` type, `tagged_deserialize` goes through the node-list
//! path in `storage-core/src/arena.rs::Arena::deserialize_sp`, which hands
//! nodes to `IrLoader::get` to reassemble the storage graph. Each recursive
//! `get` call used to clone `key_to_child_repr` — a `HashMap` with one entry
//! per node in the graph — making the full deserialize O(n^2) in node count.
//! The neighboring `visited` field is already wrapped in `Rc<RefCell<_>>` for
//! exactly this reason; `key_to_child_repr` was missed.
//!
//! Each bench builds a tree at height 32 (the Dust tree depth defined by
//! `DUST_COMMITMENT_TREE_DEPTH` / `DUST_GENERATION_TREE_DEPTH` in
//! `ledger/src/dust.rs`) with `n` leaves at indices `0..n`, rehashes it,
//! serializes, and times the deserialize. The serde path is included
//! alongside for comparison.
//!
//! Run with:
//!
//! ```text
//! cargo bench -p midnight-transient-crypto --bench merkle_deserialize
//! ```
//!
//! Before the fix, microseconds per leaf grow with `n` — roughly 45 us at
//! n=1k and 500 us at n=10k (~11x worse). After the fix, per-leaf cost is
//! flat across `n`.

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use midnight_transient_crypto::curve::Fr;
use midnight_transient_crypto::merkle_tree::MerkleTree;
use serialize::{tagged_deserialize, tagged_serialize};
use storage_core::db::InMemoryDB;

fn build_tree(n: u64) -> MerkleTree<(), InMemoryDB> {
    (0..n)
        .fold(MerkleTree::<(), InMemoryDB>::blank(32), |mt, i| {
            mt.update(i, &Fr::from(i), ())
        })
        .rehash()
}

fn bench_tagged_deserialize(c: &mut Criterion) {
    let mut group = c.benchmark_group("tagged_deserialize_merkle_tree");
    group.sample_size(10);

    for &n in &[100u64, 1_000, 10_000] {
        let tree = build_tree(n);
        let mut bytes = Vec::new();
        tagged_serialize(&tree, &mut bytes).expect("serialize");

        group.throughput(Throughput::Elements(n));
        group.bench_with_input(BenchmarkId::from_parameter(n), &bytes, |b, bytes| {
            b.iter(|| {
                let _: MerkleTree<(), InMemoryDB> =
                    tagged_deserialize(&mut &bytes[..]).expect("deserialize");
            });
        });
    }

    group.finish();
}

fn bench_serde_deserialize(c: &mut Criterion) {
    let mut group = c.benchmark_group("serde_deserialize_merkle_tree");
    group.sample_size(10);

    for &n in &[100u64, 1_000, 10_000] {
        let tree = build_tree(n);
        let bytes = serde_json::to_vec(&tree).expect("serialize");

        group.throughput(Throughput::Elements(n));
        group.bench_with_input(BenchmarkId::from_parameter(n), &bytes, |b, bytes| {
            b.iter(|| {
                let _: MerkleTree<(), InMemoryDB> =
                    serde_json::from_slice(bytes).expect("deserialize");
            });
        });
    }

    group.finish();
}

criterion_group!(benches, bench_tagged_deserialize, bench_serde_deserialize);
criterion_main!(benches);
