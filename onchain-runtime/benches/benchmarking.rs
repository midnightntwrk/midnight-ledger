#![deny(warnings)]

//! Micro benchmarks for the VM instructions.
//!
//! These are used by `midnight-generate-cost-model` to learn the cost model of
//! the VM instructions. The cost model is used to compute the gas fees of
//! transactions in the VM. See
//! [`midnight-onchain-vm::cost_model::CostModel`]. The benchmark data is saved
//! by criterion under `target/criterion/<opcode>/<params>` for each opcode and
//! parameter combination.
//!
//! The VM opcode semantics are defined by
//! `midnight-onchain-vm::vm::run_program_internal` and documented in
//! <https://github.com/midnightntwrk/midnight-architecture/blob/main/apis-and-common-types/onchain-runtime/README.md#programs>. At
//! the time of writing, the VM code itself is the best source of documentation.
//!
//! Running the full benchmark suite takes a very long time (~4 hours in
//! `--quick` mode, ~36 hours in default mode). To speed things up during
//! development, you have a few options:
//!
//! - criterion's `--profile-time 1` option: run each benchmark for only 1
//!   second, and don't actually produce a report. This doesn't produce output
//!   suitable for consumption by `midnight-generate-cost-model`.
//!
//! - enable "fast mode", which greatly reduces the space of parameters being
//!   benched ("parameters" here means e.g. the sizes of maps benched for
//!   opcodes that manipulate maps), export `MIDNIGHT_VM_COST_MODEL_FAST=1`. This
//!   alone will still produce output that `midnight-generate-cost-model` can
//!   consume, but the learned cost models will mostly be useless.
//!
//! - run only a specific tests, using criterion filter patterns. These are like
//!   `cargo test` filter patterns, except there is full regex support. For
//!   example, you can use the filter `-- '^idx/' to run only the `idx`
//!   benchmarks, or '^/idx' to run benchmarks for all `idx*` variants.
//!
//! - criterion's `--quick` option: only run until "statistical significance" of
//!   results is achieved. This is about 10x faster than the default, but not fast.
//!
//! Example combinations:
//!
//! - for smoke testing where you just want to check that the benchmarks run
//!   without crashing, without any concern for statistically valid time
//!   measurements, you can combine fast mode and `--profile-time`:
//!
//!   ```text
//!   MIDNIGHT_VM_COST_MODEL_FAST=1 cargo bench -p midnight-onchain-runtime --bench benchmarking -- --profile-time 1
//!   ```
//!
//! - to run benchmarks for a single opcode, `member`, in order generate
//!   benchmark data to test learning its cost model via
//!   `midnight-generate-cost-model`, you can combine filter patterns and
//!   `--quick` mode:
//!
//!   ```text
//!   cargo bench -p midnight-onchain-runtime --bench benchmarking -- '^member/$' --quick
//!   ```

use base_crypto::fab::AlignedValue;
use criterion::{Criterion, black_box, criterion_group, criterion_main};
use itertools::Itertools;
use midnight_onchain_runtime::ops::*;
use midnight_onchain_runtime::state::*;
use midnight_onchain_runtime::test_utilities::{run_program, run_program_step_limited};
use midnight_onchain_runtime::vm_value::{ValueStrength, VmValue};
use onchain_runtime_state::{state, stval};
use onchain_vm::result_mode::ResultModeVerify;
use onchain_vm::vmval;
use onchain_vm::{key, op};
use rand::Rng;
use rand::rngs::ThreadRng;
use rand::seq::SliceRandom;
use serde_json::json;
use serialize::Serializable;
use storage::arena::Sp;
use storage::db::InMemoryDB;
use storage::storage::{Array, HashMap};
use transient_crypto::merkle_tree::MerkleTree;

// We need to use `ResultModeGather` to bench `popeq` and `popeqc` with eq
// checking. The other ops are agnostic to the result mode, as far as this file
// type checking.
type ResultMode = ResultModeVerify;

/// Bench one opcode by running it on the given stack.
fn bench_one_op(
    group: &mut criterion::BenchmarkGroup<criterion::measurement::WallTime>,
    stack: &[VmValue],
    op: Op<ResultMode, InMemoryDB>,
    json: serde_json::Value,
) {
    let ops = vec![op];
    group.bench_function(json.to_string(), |b| {
        b.iter(|| run_program(black_box(stack), black_box(&ops)).unwrap())
    });
}

/// Bench only the first op in `ops`, on the given `stack`.
///
/// The point is to allow benching branch ops without also executing the branch target.
fn bench_first_op(
    group: &mut criterion::BenchmarkGroup<criterion::measurement::WallTime>,
    stack: &[VmValue],
    ops: Vec<Op<ResultMode, InMemoryDB>>,
    json: serde_json::Value,
) {
    assert!(!ops.is_empty());
    let step_limit = Some(1);
    group.bench_function(json.to_string(), |b| {
        b.iter(|| run_program_step_limited(black_box(stack), black_box(&ops), step_limit).unwrap())
    });
}

/// Like `bench_one_op`, but doesn't crash if the vm crashes, and instead just
/// indicates in the json if the vm crashed or not.
///
/// For the purposes of learning cost models from the benchmark stats, we want
/// to avoid cheap crashes of otherwise expensive ops: this would result in
/// trying to fit a linear model to a non-linear function of the form
///
/// ```text
/// crashed * f_crash(...) + (1 - crashed) * f_didnt_crash(...)
/// ```
///
/// where `crashed = 1 if crashed else 0` and `f_crash(...)` and
/// `f_didnt_crash(...)` are themselves linear functions, and that ain't gonna
/// work real good ...
fn bench_one_op_that_may_crash(
    group: &mut criterion::BenchmarkGroup<criterion::measurement::WallTime>,
    stack: &[VmValue],
    op: Op<ResultMode, InMemoryDB>,
    json: serde_json::Value,
) {
    let ops = vec![op];
    let crashed = run_program(stack, &ops).is_err();
    let mut json = json;
    json["crashed"] = json!(crashed as usize);
    group.bench_function(json.to_string(), |b| {
        b.iter(|| {
            let _ = run_program(black_box(stack), black_box(&ops));
        });
    });
}

/// Create an AlignedValue with serialized size approximately `size`.
///
/// Note the "approximately": the serialized size of the return value will be
/// larger than `size`, because the alignment and size of the cell are encoded
/// in the serialization.
fn gen_av(size: usize) -> AlignedValue {
    let mut rng = rand::thread_rng();
    let v = (0..size).map(|_| rng.r#gen::<u8>()).collect::<Vec<_>>();
    let av: AlignedValue = v.into();
    let actual_size = av.serialized_size();
    // Corner case: if the generated av is too large as a cell, then make it
    // smaller. This is ok because the benchmarks use the actual size of the
    // cell, not the size requested in the call to `gen_av`.
    if actual_size > state::CELL_BOUND {
        gen_av(size - (actual_size - state::CELL_BOUND))
    } else {
        av
    }
}

type StateValueMap = HashMap<AlignedValue, StateValue>;

/// Generate random maps of (approximately) the given sizes. Assumes sizes are
/// increasing.
fn gen_maps(sizes: &[usize]) -> Vec<StateValueMap> {
    let mut rng = rand::thread_rng();
    let mut maps = vec![];
    let mut map: StateValueMap = HashMap::new();
    let mut cur_size = 0;
    for size in sizes {
        let delta = *size - cur_size;
        // Randomly grow `map` by `delta` entries, all with value `null`. The returned
        // map may be a little smaller than intended, in case of random key collisions.
        //
        // We assume it's OK for entries to point to null, since non-iterated VM
        // operations that act on maps don't scrutinize entry contents.
        //
        // We incrementally grow instead of starting from scratch to save some
        // time, since creating these maps takes ~10s.
        for _ in 0..delta {
            map = map.insert(rng.r#gen(), stval!(null));
        }
        maps.push(map.clone());
        cur_size = *size;
    }
    maps
}

type StateValueMerkleTree = MerkleTree<()>;

/// Generate bmts of the given heights, empty and full.
///
/// For bmts, the log size is just the height, indep of fullness, so we have the
/// separate concern of how full the tree actually is. However, looking at the implementation of `MerkleTree`, it seems insertion cost shouldn't depend much on fullness, and we can verify this by benching ...
///
/// The valid log sizes / heights are 1..32.
///
/// Note: these bmts aren't random. We could get random bmts by randomly
/// choosing some subset of paths to define values at, but looking at the
/// code it seems empty and full are the extreme states: an empty tree is a
/// `Stub`, that needs to be expanded, whereas a full tree has no stubs, and
/// everything in between has an intermediate number of stubs. Performance
/// should only vary in terms of how many stubs are on a path, and in terms of
/// how expensive it is to hash a new input, which is orthogonal to building the
/// bmts here.
fn gen_bmts(log_sizes: &[usize]) -> Vec<StateValueMerkleTree> {
    let mut bmts = vec![];
    for height in log_sizes {
        if *height < 1 || *height > 32 {
            continue;
        }
        let mut bmt = MerkleTree::blank(*height as u8);
        bmts.push(bmt.clone());
        for path in 0u64..(1 << height) {
            // Also use the path as the value to hash.
            bmt = bmt.update(path, &path, ());
        }
        bmt = bmt.rehash();
        bmts.push(bmt.clone());
    }
    bmts
}

/// Generate up to `max_keys` many random bmt keys of the given bit sizes.
///
/// If `max_keys` is larger than then number of `k` bit strings, then for size
/// `k` we just return all possible keys.
fn gen_bmt_keys(
    log_sizes: &[usize],
    max_keys: usize,
) -> std::collections::HashMap<usize, Vec<usize>> {
    let mut rng = rand::thread_rng();
    let mut bmt_keys = std::collections::HashMap::new();
    for &log_size in log_sizes {
        let mask = (1 << log_size) - 1;
        let mut keys = vec![];
        if max_keys >= 1 << log_size {
            keys.extend(0..(1 << log_size));
            bmt_keys.insert(log_size, keys);
        } else {
            for _ in 0..max_keys {
                keys.push(rng.r#gen::<usize>() & mask);
            }
            bmt_keys.insert(log_size, keys);
        }
    }
    bmt_keys
}

type StateValueArray = Array<StateValue>;

/// Generate arrays of the given sizes, all filled with nulls.
///
/// There isn't any randomness we can apply to arrays, except their content, but
/// that shouldn't factor into timing for lookings, and insertions can measure
/// the actually inserted values if needed.
fn gen_arrays(sizes: &[usize]) -> Vec<StateValueArray> {
    let mut arrays = vec![];
    for size in sizes {
        // Create an array of `size` many nulls.
        let vs = vec![stval!(null); *size];
        let array = vs.into();
        arrays.push(array);
    }
    arrays
}

/// Generate `num` many random aligned values, of roughly evenly spaced size.
///
/// The implementation is heuristic, and works ok for the current underlying
/// `Distribution<AlignedValue>` impl. If we want to generate larger random
/// aligned values, an easy way would be to use `AlignedValue::concat` to build
/// up from smaller random values.
fn choose_aligned_values(num: usize, map: &StateValueMap) -> Vec<AlignedValue> {
    let avs: Vec<_> = map
        .keys()
        .sorted_by_key(|av| <AlignedValue as Serializable>::serialized_size(av))
        .collect();
    let size = avs.len();
    let half_size = size / 2;
    let mut results = vec![];
    // Here ((num+1)/2) = ceil(num/2).
    for i in 0..((num + 1) / 2) {
        // Even steps thru the first half of the range
        results.push(avs[(size / num) * i].clone())
    }
    for i in 0..(num / 2) {
        // Exponential steps in the second half of the range.
        //
        // The generated values seem to cluster in the first half of the range,
        // so by biasing towards the end of the range we pick up the larger
        // values.
        results.push(avs[size - half_size / (1 << (2 * i))].clone())
    }
    results
}

fn mk_vm_val(s: StateValue<InMemoryDB>) -> VmValue {
    // Here `Weak` means "cached", and `Strong` means "not cached". The cached
    // variant ops (e.g. `remc`) require some of their args to be cached.
    VmValue::new(ValueStrength::Weak, s)
}

/// Helper to run op-specific benchmark closures with various arg combinations.
struct BenchWithArgs {
    rng: ThreadRng,
    /// If true, make benchmark as fast as possible. This is used for testing
    /// only, and is not expected to produce meaningful timing stats.
    fast: bool,
    uid: usize,
    /// Maps of sizes in even linear steps
    lin_step_maps: Vec<StateValueMap>,
    /// Maps of sizes in even log steps, 1,2,4,8,...
    log_step_maps: Vec<StateValueMap>,
    /// Keys used in the maps in `maps`.
    log_step_map_keys: Vec<AlignedValue>,
    bmts: Vec<StateValueMerkleTree>,
    /// Map from tree height to random keys for that height.
    bmt_keys: std::collections::HashMap<usize, Vec<usize>>,
    arrays: Vec<StateValueArray>,
    usizes: Vec<usize>,
}

impl BenchWithArgs {
    fn new() -> Self {
        let fast = std::env::var("MIDNIGHT_VM_COST_MODEL_FAST").map_or(false, |v| {
            if v == "1" {
                true
            } else {
                panic!("Unexpected value '{v}' for MIDNIGHT_VM_COST_MODEL_FAST")
            }
        });
        let mut p1: usize = 19;
        let mut p2: usize = 12;
        if fast {
            println!(
                "MIDNIGHT_VM_COST_MODEL_FAST detected! Minimizing parameter space to speed up benchmarking, results will not be useful for actual cost model learning ..."
            );
            p1 = 1;
            p2 = 1;
        }
        println!("Generating BenchWithArgs ...");
        let rng = rand::thread_rng();
        let uid: usize = 0;
        let log_sizes: Vec<usize> = (0..p1).collect();
        let num_aligned_values: usize = p2;
        // Take p1 even steps up to size 2^p1
        println!("... generating lin-step maps ...");
        let lin_step_map_sizes = (0..p1).map(|i| (i * (1 << p1)) / p1).collect::<Vec<_>>();
        let lin_step_maps = gen_maps(&lin_step_map_sizes);
        println!("... generating log-step maps ...");
        let log_step_map_sizes: Vec<usize> = log_sizes.iter().map(|&i| 1 << i).collect();
        let log_step_maps = gen_maps(&log_step_map_sizes);
        println!("... maps done ...");
        let log_step_map_keys =
            choose_aligned_values(num_aligned_values, &log_step_maps[log_step_maps.len() - 1]);
        println!("... map keys done ...");
        let bmt_sizes: Vec<usize> = (1..p1.max(2)).step_by(2).collect();
        // Generating the bmts here takes a few minutes in non "fast" mode.
        let bmts = gen_bmts(&bmt_sizes);
        println!("... bmts done ...");
        let num_bmt_keys = p2;
        let bmt_keys = gen_bmt_keys(&bmt_sizes, num_bmt_keys);
        println!("... bmt keys done ...");
        let arrays = gen_arrays(&log_sizes);
        println!("... arrays done ...");
        let usizes = (0..p1).map(|x| x * 100).collect();
        println!("Done!");
        Self {
            rng,
            fast,
            uid,
            lin_step_maps,
            log_step_maps,
            log_step_map_keys,
            bmts,
            bmt_keys,
            arrays,
            usizes,
        }
    }

    fn next_uid(&mut self) -> usize {
        let uid = self.uid;
        self.uid += 1;
        uid
    }

    /// Iterate over `(map, key, value)` combinations, considering both the case
    /// where `key` in `map` and `key` not in `map`.
    fn with_map_and_key_and_value<B>(&mut self, mut bench: B)
    where
        B: FnMut(VmValue, VmValue, VmValue, serde_json::Value),
    {
        for map in self.log_step_maps.clone() {
            for av in self.log_step_map_keys.clone() {
                let key = mk_vm_val(StateValue::Cell(Sp::new(av.clone())));
                let key_size = key.serialized_size_as_cell();
                for present in [0, 1] {
                    let map = if present == 0 {
                        map.remove(&av)
                    } else {
                        map.insert(av.clone(), stval!(null))
                    };
                    let container = mk_vm_val(StateValue::Map(map));
                    let container_log_size = container.log_size();
                    let json_params = json!({
                        "container_type": "map",
                        "key_size": key_size,
                        "container_log_size": container_log_size,
                        "key_present": present,
                        "uid": self.next_uid(),
                    });
                    let value = mk_vm_val(self.rng.r#gen());
                    bench(container, key.clone(), value, json_params);
                }
            }
        }
    }

    fn with_map_and_key<B>(&mut self, mut bench: B)
    where
        B: FnMut(VmValue, VmValue, serde_json::Value),
    {
        self.with_map_and_key_and_value(|map, key, _value, json| bench(map, key, json))
    }

    /// Similar to `with_map_and_key_and_value`, except we don't guarantee that
    /// `key` not in `bmt` is covered.
    fn with_bmt_and_key_and_value<B>(&mut self, mut bench: B)
    where
        B: FnMut(VmValue, VmValue, VmValue, serde_json::Value),
    {
        for bmt in self.bmts.clone() {
            for raw_key in self.bmt_keys[&(bmt.height() as usize)].clone() {
                let raw_key = raw_key as u64;
                let key = mk_vm_val(StateValue::Cell(Sp::new(raw_key.into())));
                let key_size = key.serialized_size_as_cell();
                for present in [0, 1] {
                    // We can't usefully remove a key from a bmt, so we just
                    // force existence in the `present == 1` case.
                    let bmt = if present == 0 {
                        bmt.clone()
                    } else {
                        bmt.update(raw_key, &raw_key, ())
                    };
                    let container = mk_vm_val(StateValue::BoundedMerkleTree(bmt));
                    let container_log_size = container.log_size();
                    let json_params = json!({
                        "container_type": "bmt",
                        "key_size": key_size,
                        "container_log_size": container_log_size,
                        "key_present": present,
                        "uid": self.next_uid(),
                    });
                    // Generate a cell that can surely be `try_into()`d into
                    // `HashOutput`: the `ins` opcode assumes its argument is a
                    // cell, and that it can be converted into a `HashOutput`.
                    let value =
                        mk_vm_val(StateValue::Cell(Sp::new(self.rng.r#gen::<u64>().into())));
                    bench(container, key.clone(), value, json_params);
                }
            }
        }
    }

    fn with_bmt_and_key<B>(&mut self, mut bench: B)
    where
        B: FnMut(VmValue, VmValue, serde_json::Value),
    {
        self.with_bmt_and_key_and_value(|map, key, _value, json| bench(map, key, json))
    }

    /// Similar to `with_map_and_key_and_value`, except we only cover `key` in
    /// `array` case.
    fn with_array_and_key_and_value<B>(&mut self, mut bench: B)
    where
        B: FnMut(VmValue, VmValue, VmValue, serde_json::Value),
    {
        for arr in self.arrays.clone() {
            // For arrays, all keys for size (`0..arr.len()`) are always
            // defined, and we can't remove them. So only choices are which keys
            // to provide, and what values to provide for those keys.
            for raw_key in 0..(arr.len() as u64) {
                let key = mk_vm_val(StateValue::Cell(Sp::new(raw_key.into())));
                let key_size = key.serialized_size_as_cell();
                let container = mk_vm_val(StateValue::Array(arr.clone()));
                let container_log_size = container.log_size();
                let json_params = json!({
                    "container_type": "array",
                    "key_size": key_size,
                    "container_log_size": container_log_size,
                    "key_present": 1,
                    "uid": self.next_uid(),
                });
                let value = mk_vm_val(self.rng.r#gen());
                bench(container, key, value, json_params);
            }
        }
    }

    fn with_array_and_key<B>(&mut self, mut bench: B)
    where
        B: FnMut(VmValue, VmValue, serde_json::Value),
    {
        self.with_array_and_key_and_value(|arr, key, _value, json| bench(arr, key, json))
    }

    /// Run `bench` on a variety of length-`n` vectors of `AlignedValue`.
    ///
    /// The variety is chosen to provide good coverage of acceptable and
    /// crashing inputs for logical and arithmetic ops, e.g. by pairing bools
    /// with bools and u64s with u64s. Also provides inputs that are large,
    /// random cells, but these will crash the bin ops.
    fn with_cells<B>(&mut self, n: usize, mut bench: B)
    where
        B: FnMut(Vec<VmValue>, serde_json::Value),
    {
        let mut av_vecs: Vec<Vec<AlignedValue>> = Vec::new();
        let mut choose_rng = self.rng.clone();
        let mut choose = |collection: &[AlignedValue]| {
            std::iter::repeat_with(|| collection.choose(&mut choose_rng).unwrap().clone())
                .take(n)
                .collect::<Vec<_>>()
        };

        // Add 2 random boolean combinations.
        let bool_values = [true.into(), false.into()];

        for _ in 0..(if self.fast { 1 } else { 2 }) {
            av_vecs.push(choose(&bool_values));
        }

        // Add 4 random u64 pairs
        for _ in 0..(if self.fast { 0 } else { 4 }) {
            let vec = std::iter::repeat_with(|| self.rng.r#gen::<u64>().into())
                .take(n)
                .collect();
            av_vecs.push(vec);
        }

        // Collect 5 random AVs from bwa.avs
        let mut av_collection = Vec::new();
        for i in 0..5 {
            if !self.log_step_map_keys.is_empty() {
                av_collection
                    .push(self.log_step_map_keys[i % self.log_step_map_keys.len()].clone());
            }
        }
        // Collect 15 AVs of sizes evenly distributed across valid cell sizes
        for i in 0..15 {
            av_collection.push(gen_av((i * state::CELL_BOUND) / 15));
        }

        // Add 10 uniform random pairs from the collection
        for _ in 0..(if self.fast { 1 } else { 10 }) {
            av_vecs.push(choose(&av_collection));
        }

        // Convert all AlignedValue pairs to VmValue pairs and run benchmarks
        for (i, avs) in av_vecs.into_iter().enumerate() {
            let vals: Vec<_> = avs
                .into_iter()
                .map(|av| mk_vm_val(StateValue::Cell(Sp::new(av))))
                .collect();
            let sizes: Vec<_> = vals
                .iter()
                .map(|val| val.serialized_size_as_cell())
                .collect();
            let total_size: usize = sizes.iter().sum();
            let mut json = json!({
                "container_type": "cell",
                "total_size": total_size,
                "pair_index": i, // In case we want to debug a crash
                "uid": self.next_uid(),
            });
            if sizes.len() == 1 {
                json[format!("value_size")] = sizes[0].into();
            } else {
                for (i, size) in sizes.iter().enumerate() {
                    json[format!("value_{i}_size")] = (*size).into();
                }
            }
            bench(vals, json);
        }
    }

    fn with_null<B>(&mut self, mut bench: B)
    where
        B: FnMut(StateValue, serde_json::Value),
    {
        let value = stval!(null);
        let value_size = Serializable::serialized_size(&value);
        let json_params = json!({
            "container_type": "null",
            "value_size":value_size,
            "uid": self.next_uid(),
        });
        bench(value, json_params);
    }

    fn with_cell<B>(&mut self, mut bench: B)
    where
        B: FnMut(StateValue, serde_json::Value),
    {
        self.with_cells(1, |vals, json| {
            bench(vals[0].value.clone(), json);
        });
    }

    fn with_map<B>(&mut self, mut bench: B)
    where
        B: FnMut(StateValue, serde_json::Value),
    {
        for map in self.lin_step_maps.clone() {
            let value = StateValue::Map(map);
            let value_size = Serializable::serialized_size(&value);
            let json_params = json!({
                "container_type": "map",
                "value_size":value_size,
                "uid": self.next_uid(),
            });
            bench(value, json_params);
        }
    }

    fn with_bmt<B>(&mut self, mut bench: B)
    where
        B: FnMut(StateValue, serde_json::Value),
    {
        for bmt in self.bmts.clone() {
            let value = StateValue::BoundedMerkleTree(bmt);
            let value_size = Serializable::serialized_size(&value);
            let json_params = json!({
                "container_type": "bmt",
                "value_size":value_size,
                "uid": self.next_uid(),
            });
            bench(value, json_params);
        }
    }

    fn with_array<B>(&mut self, mut bench: B)
    where
        B: FnMut(StateValue, serde_json::Value),
    {
        for arr in self.arrays.clone() {
            let value = StateValue::Array(arr);
            let value_size = Serializable::serialized_size(&value);
            let json_params = json!({
                "container_type": "array",
                "value_size":value_size,
                "uid": self.next_uid(),
            });
            bench(value, json_params);
        }
    }

    /// Run a benchmark with a generic usize argument.
    fn with_usize<B>(&mut self, mut bench: B)
    where
        B: FnMut(usize, serde_json::Value),
    {
        for arg in self.usizes.clone() {
            let json_params = json!({
                "container_type": "none",
                "arg": arg,
                "uid": self.next_uid(),
            });
            bench(arg, json_params);
        }
    }

    /// Run a benchmark with a cell value and random immediate u32 value.
    ///
    /// Reuses the existing `with_cell_pair` logic but discards the second cell value
    /// and generates random immediate values instead.
    fn with_cell_and_immediate<B>(&mut self, mut bench: B)
    where
        B: FnMut(VmValue, u32, serde_json::Value),
    {
        let mut rng = self.rng.clone();
        self.with_cells(1, |vals, json| {
            // Use the same constraint as in ops.rs: modulo 0x1FFFFF
            let immediate = rng.r#gen::<u32>() % 0x1FFFFF;
            bench(vals[0].clone(), immediate, json);
        });
    }
}

/// Benchmarks for each VM op.
pub fn vm_op_benchmarks(c: &mut Criterion) {
    let mut bwa = BenchWithArgs::new();

    // Make group and print name, useful for tracking progress when running
    // benchmarking with a filter, where it sets and then skips most groups.
    fn mk_group<'a>(
        c: &'a mut Criterion,
        name: &str,
    ) -> criterion::BenchmarkGroup<'a, criterion::measurement::WallTime> {
        println!("Creating benchmark group: {}", name);
        c.benchmark_group(name)
    }

    let mut group = mk_group(c, "noop");
    bwa.with_usize(|arg, json| {
        let stack = [];
        let op = op!(noop(arg as u32));
        bench_one_op(&mut group, &stack, op, json);
    });
    group.finish();

    let mut group = mk_group(c, "branch");
    bwa.with_usize(|arg, mut json| {
        for b in [true, false] {
            json["took_branch"] = json!(b as usize);
            let stack = [mk_vm_val(StateValue::Cell(Sp::new(b.into())))];
            let mut ops = vec![op!(noop(0)); arg];
            ops.insert(0, op!(branch(arg as u32)));
            bench_first_op(&mut group, &stack, ops, json.clone());
        }
    });
    group.finish();

    let mut group = mk_group(c, "jmp");
    bwa.with_usize(|arg, json| {
        let stack = [];
        let mut ops = vec![op!(noop(0)); arg];
        ops.insert(0, op!(jmp(arg as u32)));
        bench_first_op(&mut group, &stack, ops, json);
    });
    group.finish();

    let mut group = mk_group(c, "ckpt");
    let json = serde_json::json!({
        "container_type": "none",
    });
    let stack = [];
    let op = op![ckpt];
    bench_one_op(&mut group, &stack, op, json);
    group.finish();

    let mut group = mk_group(c, "lt");
    bwa.with_cells(2, |stack, json| {
        bench_one_op_that_may_crash(&mut group, &stack, op![lt], json);
    });
    group.finish();

    let mut group = mk_group(c, "eq");
    bwa.with_cells(2, |stack, json| {
        bench_one_op_that_may_crash(&mut group, &stack, op![eq], json);
    });
    group.finish();

    // For the `type` op the vm just matches the StateValue constructor, so no
    // reason to test random inputs.
    let mut group = mk_group(c, "type");
    let containers = [
        (vmval!((0u64)), "cell"),
        (vmval!(null), "null"),
        (vmval!({}), "map"),
        (vmval!([]), "array"),
        (vmval!({MT(0) {}}), "bmt"),
    ];
    for (con, name) in containers {
        let stack = [con];
        let op = op![type];
        let json = serde_json::json!({
            "container_type": name
        });
        bench_one_op(&mut group, &stack, op.clone(), json);
    }
    group.finish();

    // The `size` op is expected to be O(1), so not clear we should be scaling
    // it here. However, at one point `map.size()` was Theta(n), so better to be
    // paranoid.
    let mut group = mk_group(c, "size");
    let mut bench = |value, json| {
        let stack = [mk_vm_val(value)];
        let op = op![size];
        bench_one_op(&mut group, &stack, op, json);
    };
    bwa.with_map(&mut bench);
    bwa.with_bmt(&mut bench);
    bwa.with_array(&mut bench);
    group.finish();

    let mut group = mk_group(c, "new");
    // The argument to `new` is a u8 tag with lower 4 bits determining container
    // type.
    let container_types = [
        (0u8, "cell"),
        (1u8, "null"),
        (2u8, "map"),
        // The upper 4 bits (here 0) are used to determine the array size.
        (3u8, "array"),
        // The upper 4 bits (here 0) are used to determine the bmt size.
        (4u8, "bmt"),
    ];
    for (tag, name) in container_types.iter() {
        let stack = [vmval!((*tag))];
        let op = op![new];
        let json = serde_json::json!({
            "container_type": name
        });
        bench_one_op(&mut group, &stack, op, json);
    }
    group.finish();

    let mut group = mk_group(c, "and");
    bwa.with_cells(2, |stack, json| {
        bench_one_op_that_may_crash(&mut group, &stack, op![and], json);
    });
    group.finish();

    let mut group = mk_group(c, "or");
    bwa.with_cells(2, |stack, json| {
        bench_one_op_that_may_crash(&mut group, &stack, op![or], json);
    });
    group.finish();

    let mut group = mk_group(c, "neg");
    bwa.with_cells(1, |stack, json| {
        bench_one_op_that_may_crash(&mut group, &stack, op![neg], json);
    });
    group.finish();

    let mut group = mk_group(c, "log");
    let mut bench = |value, json| {
        let con = mk_vm_val(value);
        let stack = [con];
        let op = op![log];
        bench_one_op(&mut group, &stack, op, json);
    };
    bwa.with_null(&mut bench);
    bwa.with_cell(&mut bench);
    bwa.with_map(&mut bench);
    bwa.with_bmt(&mut bench);
    bwa.with_array(&mut bench);
    group.finish();

    let mut group = mk_group(c, "root");
    let mut bench = |value, json| {
        let stack = [mk_vm_val(value)];
        let op = op![root];
        bench_one_op(&mut group, &stack, op, json);
    };
    bwa.with_bmt(&mut bench);
    group.finish();

    let mut group = mk_group(c, "pop");
    let mut bench = |value, json| {
        let stack = [mk_vm_val(value)];
        let op = op![pop];
        bench_one_op(&mut group, &stack, op, json);
    };
    bwa.with_null(&mut bench);
    bwa.with_cell(&mut bench);
    bwa.with_map(&mut bench);
    bwa.with_bmt(&mut bench);
    bwa.with_array(&mut bench);
    group.finish();

    let bench_pop_eq =
        |group: &mut criterion::BenchmarkGroup<criterion::measurement::WallTime>,
         bwa: &mut BenchWithArgs,
         make_op: fn(AlignedValue) -> Op<ResultMode, InMemoryDB>| {
            bwa.with_cell(|value, mut json| {
                let cell_val = mk_vm_val(value);
                let stack = [cell_val.clone()];
                let av = (*cell_val.as_cell().unwrap()).clone();
                let results = [
                    // Matching result
                    av.clone(),
                    // Non-matching result: cell vs cell concatenated with
                    // itself. Well, could match if the cell is empty ...
                    AlignedValue::concat(&[av.clone(), av.clone()]),
                ];
                for result in results {
                    json["eq"] = serde_json::json!((av == result) as usize);
                    let op = make_op(result.clone());
                    bench_one_op_that_may_crash(group, &stack, op, json.clone());
                }
            });
        };

    let mut group = mk_group(c, "popeq");
    bench_pop_eq(&mut group, &mut bwa, |result| op![popeq(result)]);
    group.finish();

    let mut group = mk_group(c, "popeqc");
    bench_pop_eq(&mut group, &mut bwa, |result| op![popeqc(result)]);
    group.finish();

    let mut group = mk_group(c, "addi");
    bwa.with_cell_and_immediate(|cell_val, immediate, json| {
        let stack = vec![cell_val];
        let op = op![addi(immediate)];
        bench_one_op_that_may_crash(&mut group, &stack, op, json);
    });
    group.finish();

    let mut group = mk_group(c, "subi");
    bwa.with_cell_and_immediate(|cell_val, immediate, json| {
        let stack = vec![cell_val];
        let op = op![subi(immediate)];
        bench_one_op_that_may_crash(&mut group, &stack, op, json);
    });
    group.finish();

    let mut group = mk_group(c, "push");
    let mut bench = |value, json| {
        let stack = vec![];
        let op = Op::Push {
            storage: false,
            value,
        };
        bench_one_op(&mut group, &stack, op, json);
    };
    bwa.with_null(&mut bench);
    bwa.with_cell(&mut bench);
    bwa.with_map(&mut bench);
    bwa.with_bmt(&mut bench);
    bwa.with_array(&mut bench);
    group.finish();

    let mut group = mk_group(c, "pushs");
    let mut bench = |value, json| {
        let stack = vec![];
        let op = Op::Push {
            storage: true,
            value,
        };
        bench_one_op(&mut group, &stack, op, json);
    };
    bwa.with_null(&mut bench);
    bwa.with_cell(&mut bench);
    bwa.with_map(&mut bench);
    bwa.with_bmt(&mut bench);
    bwa.with_array(&mut bench);
    group.finish();

    let mut group = mk_group(c, "add");
    bwa.with_cells(2, |stack, json| {
        bench_one_op_that_may_crash(&mut group, &stack, op![add], json);
    });
    group.finish();

    let mut group = mk_group(c, "sub");
    bwa.with_cells(2, |stack, json| {
        bench_one_op_that_may_crash(&mut group, &stack, op![sub], json);
    });
    group.finish();

    // Cost is linear in input size (not log input size like for containers), so
    // we test the range linearly.
    let mut group = mk_group(c, "concat");
    bwa.with_cells(2, |stack, json| {
        let total_size = json["total_size"].as_u64().unwrap() as u32;
        let op = op![concat(total_size)];
        // May crash with `CellBoundExceeded` if arg or result cell is too large
        // (currently 32kiB, defined by `onchain-state::state::CELL_SIZE`).
        bench_one_op_that_may_crash(&mut group, &stack, op, json);
    });
    group.finish();

    let mut group = mk_group(c, "concatc");
    bwa.with_cells(2, |stack, json| {
        let total_size = json["total_size"].as_u64().unwrap() as u32;
        let op = op![concatc(total_size)];
        bench_one_op_that_may_crash(&mut group, &stack, op, json);
    });
    group.finish();

    let mut group = mk_group(c, "member");
    bwa.with_map_and_key(|container, key, json| {
        let stack = [container, key];
        let op = op![member];
        bench_one_op(&mut group, &stack, op, json);
    });
    group.finish();

    let configs: [(&str, fn() -> Op<ResultMode>); 2] =
        [("rem", || op![rem]), ("remc", || op![remc])];
    for (name, mk_op) in configs {
        let mut group = mk_group(c, name);
        let mut bench = |container, key, json| {
            let stack = [container, key];
            let op = mk_op();
            bench_one_op(&mut group, &stack, op, json);
        };
        bwa.with_map_and_key(&mut bench);
        bwa.with_bmt_and_key(bench);
        group.finish();
    }

    // Cover the full range of u8 values, the arg type of dup and swap.
    let arg_values: Vec<u8> = (0..=4).map(|i| 1 << (2 * i)).collect();

    let bench_dup_or_swap =
        |group: &mut criterion::BenchmarkGroup<criterion::measurement::WallTime>,
         bwa: &mut BenchWithArgs,
         make_op: fn(u8) -> Op<ResultMode, InMemoryDB>| {
            let mut arg_index = 0;
            let mut bench = |value: StateValue, mut json: serde_json::Value| {
                let arg = arg_values[arg_index % arg_values.len()];
                arg_index += 1;
                json["arg"] = serde_json::json!(arg);
                // Use n+2 as stack size: sufficient for both  dup and swap
                let stack_size = (arg as usize) + 2;
                let vm_value = mk_vm_val(value.clone());
                let stack: Vec<VmValue> = (0..stack_size).map(|_| vm_value.clone()).collect();
                let op = make_op(arg);
                bench_one_op(group, &stack, op, json);
            };
            bwa.with_null(&mut bench);
            bwa.with_cell(&mut bench);
            bwa.with_map(&mut bench);
            bwa.with_bmt(&mut bench);
            bwa.with_array(&mut bench);
        };

    let mut group = mk_group(c, "dup");
    bench_dup_or_swap(&mut group, &mut bwa, |arg| op![dup(arg)]);
    group.finish();

    let mut group = mk_group(c, "swap");
    bench_dup_or_swap(&mut group, &mut bwa, |arg| op![swap(arg)]);
    group.finish();

    // We only check a single iteration of `idx` -- i.e. a length 1 index --
    // because the `idx*` opcodes loop over the input and do the same thing in
    // each iteration, and so testing a single iteration should be
    // representative, and eliminate another dimension of benchmark space
    // blowup. If we wanted to test multiple iterations, we'd need to combine
    // the per-iteration parameters into per-call parameters, by summing all of
    // the per-iteration parameters.
    let configs: [(
        &str,
        fn() -> Op<ResultMode>,
        fn(AlignedValue) -> Op<ResultMode>,
    ); 4] = [
        ("idx", || op![idx[stack]], |key| op![idx[key]]),
        ("idxc", || op![idxc[stack]], |key| op![idxc[key]]),
        ("idxp", || op![idxp[stack]], |key| op![idxp[key]]),
        ("idxpc", || op![idxpc[stack]], |key| op![idxpc[key]]),
    ];
    for (name, mk_stack_op, mk_key_op) in configs {
        let mut group = mk_group(c, name);
        let mut bench_stack_key = |container, key, mut json: serde_json::Value| {
            // Use `stack` arg to `idx` to take the key from the stack. This is
            // a keyword, not a reference to the `stack` variable defined above!
            json["arg_from_stack"] = json!(1);
            let stack = [container, key];
            let op = mk_stack_op();
            bench_one_op(&mut group, &stack, op, json);
        };
        bwa.with_map_and_key(&mut bench_stack_key);
        bwa.with_bmt_and_key(&mut bench_stack_key);
        bwa.with_array_and_key(bench_stack_key);
        let mut bench_arg_key = |container, key: VmValue, mut json: serde_json::Value| {
            // Pass the key as an arg the idx op.
            json["arg_from_stack"] = json!(0);
            let stack = [container];
            let raw_key = (*key.as_cell().unwrap()).clone();
            let op = mk_key_op(raw_key);
            bench_one_op(&mut group, &stack, op, json);
        };
        bwa.with_map_and_key(&mut bench_arg_key);
        bwa.with_bmt_and_key(&mut bench_arg_key);
        bwa.with_array_and_key(&mut bench_arg_key);
        group.finish();
    }

    // Like for `idx*`, we only test a single iteration.
    let configs: [(&str, fn() -> Op<ResultMode>); 2] =
        [("ins", || op![ins 1]), ("insc", || op![insc 1])];
    for (name, mk_op) in configs {
        let mut group = mk_group(c, name);
        let mut bench = |container, key, value, json| {
            let stack = [container, key, value];
            // Insert value at key.
            let op = mk_op();
            bench_one_op(&mut group, &stack, op, json);
        };
        bwa.with_map_and_key_and_value(&mut bench);
        bwa.with_bmt_and_key_and_value(&mut bench);
        bwa.with_array_and_key_and_value(&mut bench);
        group.finish();
    }
}

criterion_group!(benchmarking, vm_op_benchmarks);
criterion_main!(benchmarking);
