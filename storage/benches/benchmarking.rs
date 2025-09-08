#![cfg(feature = "bench")]

use base_crypto::fab::AlignedValue;
use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use midnight_storage::DefaultDB;
use midnight_storage::arena::{ArenaKey, Sp};
use midnight_storage::db::InMemoryDB;
use midnight_storage::delta_tracking::{RcMap, gc_rcmap, get_writes, update_rcmap};
use midnight_storage::storage::{HashMap, Map};
use onchain_state::state::StateValue;
use pprof::criterion::{Output, PProfProfiler};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng, seq::SliceRandom};
use serde_json::json;
use std::collections::HashSet as StdHashSet;
use std::hint::black_box;

pub fn sp_new(c: &mut Criterion) {
    let mut rng = StdRng::seed_from_u64(0x42);

    c.bench_function("sp_new", |b| {
        b.iter(|| Sp::<u64, InMemoryDB>::new(black_box(rng.r#gen())))
    });
}
pub fn map_insert(c: &mut Criterion) {
    let mut rng = StdRng::seed_from_u64(0x42);

    let mut group = c.benchmark_group("map_insert");
    for size in [10, 100, 1_000, 10_000] {
        let mut map: Map<u64, u64> = Map::new();
        for _ in 0..size {
            map = map.insert(rng.r#gen(), rng.r#gen());
        }

        group.bench_with_input(BenchmarkId::from_parameter(size), &map, |b, map| {
            b.iter(|| {
                black_box(map.insert(rng.r#gen(), rng.r#gen()));
            });
        });
    }
    group.finish();
}

// BenchmarkData holds all pre-computed data for a single benchmark
struct BenchmarkData {
    old_rcmap: RcMap,
    new_rcmap: RcMap,
    new_roots: StdHashSet<ArenaKey>,
    keys_added: StdHashSet<ArenaKey>,
    json: serde_json::Value,
    _old_sp: Sp<StateValue>, // Keep StateValue alive in backend
    _new_sp: Sp<StateValue>, // Keep StateValue alive in backend
}

// Generate a pool of HashMap maps of various sizes
fn generate_state_pool() -> Vec<HashMap<AlignedValue, StateValue>> {
    let mut rng = StdRng::seed_from_u64(0x42);
    // Increasing max_size here *rapidly* slows down the generation process. If
    // you need to test changes to these benchmarks, try reducing max_size here
    // first.
    let max_size = 1 << 10;
    let num_chains = 10;
    let num_maps_per_chain = 100;
    let mut pool = Vec::new();

    // Generate num_chains independent maps in each size
    for c in 0..num_chains {
        let mut map = HashMap::new();
        println!("generate_state_pool: chain {c}/{num_chains}");
        // Generate larger maps by incrementally updating smaller maps; all maps
        // in a given inner loop here are in a containment chain, but the other
        // loop creates independent chains
        for _ in 0..num_maps_per_chain {
            for _ in 0..(max_size / num_maps_per_chain) {
                let key: AlignedValue = rng.r#gen::<u64>().into();
                map = map.insert(key, StateValue::Null);
            }
            pool.push(map.clone());
        }
    }
    pool
}

// Generate state pairs with four relationship types
fn generate_state_pairs() -> Vec<(
    HashMap<AlignedValue, StateValue>,
    HashMap<AlignedValue, StateValue>,
    &'static str,
)> {
    let mut rng = StdRng::seed_from_u64(0x42);
    let pool = generate_state_pool();
    let mut pairs = Vec::new();
    let num_iters = 25;

    for p in 0..num_iters {
        println!("generate_state_pairs: {p}/{num_iters}");
        // 1. Disjoint: two probably disjoint maps from pool
        let old = pool.choose(&mut rng).unwrap().clone();
        let new = pool.choose(&mut rng).unwrap().clone();
        pairs.push((old, new, "disjoint"));

        // 2. old ⊂ new: old is subset of new
        // 3. new ⊂ old: new is subset of old
        let old_map = pool.choose(&mut rng).unwrap();
        let new_base = pool.choose(&mut rng).unwrap();

        // Insert old into new_base to create subset relationship
        let key: AlignedValue = rng.r#gen::<u64>().into();
        let combined_map = new_base.insert(key, StateValue::Map(old_map.clone()));
        pairs.push((old_map.clone(), combined_map.clone(), "old_subset"));
        pairs.push((combined_map, old_map.clone(), "new_subset"));

        // 4. Partial overlap: old and new share multiple subtrees
        let mut old = pool.choose(&mut rng).unwrap().clone();
        let mut new = pool.choose(&mut rng).unwrap().clone();
        // Create overlapping states by inserting shared subtrees into both
        for _ in 0..10 {
            let shared = pool.choose(&mut rng).unwrap();
            let overlap_key: AlignedValue = rng.r#gen::<u64>().into();
            old = old.insert(overlap_key.clone(), StateValue::Map(shared.clone()));
            new = new.insert(overlap_key, StateValue::Map(shared.clone()));
        }
        pairs.push((old, new, "partial"));
    }

    pairs
}

// Compute benchmark data from state pairs
fn compute_benchmark_data() -> Vec<BenchmarkData> {
    let mut uid = 0;
    let arena = &midnight_storage::storage::default_storage::<InMemoryDB>().arena;
    let state_pairs = generate_state_pairs();
    let mut benchmark_data = Vec::new();

    for (i, (old_map, new_map, relationship)) in state_pairs.iter().enumerate() {
        println!("compute_benchmark_data: {i}/{}", state_pairs.len());
        let old_sp = arena.alloc(StateValue::Map(old_map.clone()));
        let new_sp = arena.alloc(StateValue::Map(new_map.clone()));

        // Build rcmap from old state
        let old_root = old_sp.hash().into();
        let old_roots = StdHashSet::from([old_root]);
        println!("get_writes(old_roots)");
        let keys_for_rcmap = get_writes::<DefaultDB>(&RcMap::default(), &old_roots);
        println!("update_rcmap(old_roots)");
        let old_rcmap = update_rcmap(&RcMap::default(), &keys_for_rcmap);

        // Get new state roots
        let new_root = new_sp.hash().into();
        let new_roots = StdHashSet::from([new_root]);

        // Pre-compute keys_added and keys_removed
        println!("get_writes(new_roots)");
        let keys_added = get_writes(&old_rcmap, &new_roots);
        println!("update_rcmap(old_roots)");
        let new_rcmap = update_rcmap(&old_rcmap, &keys_added);
        println!("gc_rcmap(old_roots)");
        let (_, keys_removed) = gc_rcmap(&new_rcmap, &new_roots, usize::MAX);
        println!("json");

        let json = json!({
            "container_type": "none",
            "keys_added_size": keys_added.len(),
            "keys_removed_size": keys_removed.len(),
            "relationship": relationship,
            "uid": uid,
        });

        benchmark_data.push(BenchmarkData {
            old_rcmap,
            new_rcmap,
            new_roots,
            keys_added,
            json,
            _old_sp: old_sp,
            _new_sp: new_sp,
        });
        uid += 1;
    }

    benchmark_data
}

// Delta tracking benchmarks
pub fn delta_tracking(c: &mut Criterion) {
    let benchmark_data = compute_benchmark_data();

    // Benchmark get_writes
    let mut group = c.benchmark_group("get_writes");
    for data in &benchmark_data {
        group.bench_function(data.json.to_string(), |b| {
            b.iter(|| {
                black_box(get_writes(&data.old_rcmap, &data.new_roots));
            });
        });
    }
    group.finish();

    // Benchmark update_rcmap
    let mut group = c.benchmark_group("update_rcmap");
    for data in &benchmark_data {
        group.bench_function(data.json.to_string(), |b| {
            b.iter(|| {
                black_box(update_rcmap(&data.old_rcmap, &data.keys_added));
            });
        });
    }
    group.finish();

    // Benchmark gc_rcmap
    let mut group = c.benchmark_group("gc_rcmap");
    for data in &benchmark_data {
        group.bench_function(data.json.to_string(), |b| {
            b.iter(|| {
                black_box(gc_rcmap(&data.new_rcmap, &data.new_roots, usize::MAX));
            });
        });
    }
    group.finish();
}

criterion_group!(
    name = benchmarking;
    config = Criterion::default().with_profiler(PProfProfiler::new(100, Output::Flamegraph(None)));
    targets = sp_new, map_insert, delta_tracking
);
criterion_main!(benchmarking);
