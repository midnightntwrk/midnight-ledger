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

pub mod delta_tracking;
pub mod merkle_patricia_trie;
pub mod storage;

pub use storage_core::*;

#[cfg(feature = "state-translation")]
pub mod state_translation;

// Stress testing utilities. Needs to be pub since we call it from a bin
// target. But not meant to be consumed by library users.
#[cfg(feature = "stress-test")]
pub mod stress_test;

/// Stress tests.
#[cfg(feature = "stress-test")]
pub mod stress_tests {
    use crate::DefaultDB;
    use crate::db::DB;
    use crate::storable::Loader;
    use crate::{self as storage, Storage, arena::Sp, storage::Array};
    use serialize::Serializable;
    use storage_core::arena::*;

    fn new_arena() -> Arena<DefaultDB> {
        let storage = Storage::<DefaultDB>::new(16, DefaultDB::default());
        storage.arena
    }

    /// Test that we can allocate and drop a deeply nested `Sp` without blowing
    /// up the stack via implicit recursion.
    pub fn drop_deeply_nested_data() {
        use bin_tree::BinTree;

        let arena = new_arena();
        let mut bt = BinTree::new(0, None, None);
        let depth = 100_000;
        for i in 1..depth {
            bt = BinTree::new(i, Some(arena.alloc(bt)), None);
        }
    }

    /// Test that we can serialize a deeply nested `Sp` without blowing up the
    /// stack via recursion.
    pub fn serialize_deeply_nested_data() {
        use bin_tree::BinTree;

        let arena = new_arena();
        let mut bt = BinTree::new(0, None, None);
        let depth = 100_000;
        for i in 1..depth {
            bt = BinTree::new(i, Some(arena.alloc(bt)), None);
        }

        let mut buf = std::vec::Vec::new();
        Sp::serialize(&arena.alloc(bt), &mut buf).unwrap();
    }

    /// Similar to `tests::test_sp_nesting`, but with a more complex structure,
    /// where the `Sp` is nested inside an `Array`.
    pub fn array_nesting() {
        use super::Storable;
        #[derive(Debug, Clone, PartialOrd, Ord, PartialEq, Eq, Hash, Storable)]
        struct Nesty(Array<Nesty>);
        impl Drop for Nesty {
            fn drop(&mut self) {
                if self.0.is_empty() {
                    return;
                }
                let take = |ptr: &mut Nesty| {
                    let mut tmp = Array::new();
                    std::mem::swap(&mut tmp, &mut ptr.0);
                    tmp
                };
                let mut frontier = vec![take(self)];
                while let Some(curr) = frontier.pop() {
                    let items = curr.iter().collect::<std::vec::Vec<_>>();
                    drop(curr);
                    frontier.extend(
                        items
                            .into_iter()
                            .flat_map(Sp::into_inner)
                            .map(|mut n| take(&mut n)),
                    );
                }
            }
        }
        let mut nest = Nesty(Array::new());
        for i in 0..16_000 {
            nest = Nesty(vec![nest].into());
            if i % 100 == 0 {
                dbg!(i);
            }
        }
        drop(nest);
        // Did we survive the drop?
        println!("drop(nest) returned!");
    }

    /// See `thrash_the_cache_variations_inner` for details.
    #[cfg(feature = "sqlite")]
    pub fn thrash_the_cache_variations_sqldb(args: &[String]) {
        use crate::db::SqlDB;
        fn mk_open_db() -> impl Fn() -> SqlDB {
            let path = tempfile::NamedTempFile::new().unwrap().into_temp_path();
            move || SqlDB::exclusive_file(&path)
        }
        thrash_the_cache_variations(args, mk_open_db);
    }

    /// See `thrash_the_cache_variations_inner` for details.
    #[cfg(feature = "parity-db")]
    pub fn thrash_the_cache_variations_paritydb(args: &[String]) {
        use crate::db::ParityDb;
        fn mk_open_db() -> impl Fn() -> ParityDb {
            let path = tempfile::TempDir::new().unwrap().keep();
            move || ParityDb::open(&path)
        }
        thrash_the_cache_variations(args, mk_open_db);
    }

    #[cfg(any(feature = "parity-db", feature = "sqlite"))]
    fn thrash_the_cache_variations<D: DB, O: Fn() -> D>(
        args: &[String],
        mk_open_db: impl Fn() -> O,
    ) {
        let msg = "thrash_the_cache_variations(p: f64, include_cyclic: bool)";
        if args.len() != 2 {
            panic!("{msg}: wrong number of args");
        }
        let p = args[0]
            .parse::<f64>()
            .unwrap_or_else(|e| panic!("{msg}: couldn't parse p={}: {e}", args[0]));
        let include_cyclic = args[1]
            .parse()
            .unwrap_or_else(|e| panic!("{msg}: couldn't parse include_cyclic={}: {e}", args[1]));
        thrash_the_cache_variations_inner(p, include_cyclic, mk_open_db);
    }

    /// Run various cache thrashing combinations with fixed `p`.
    ///
    /// Here `mk_open_db` creates a fresh `open_db` function every time it's
    /// called.
    #[cfg(any(feature = "parity-db", feature = "sqlite"))]
    fn thrash_the_cache_variations_inner<D: DB, O: Fn() -> D>(
        p: f64,
        include_cyclic: bool,
        mk_open_db: impl Fn() -> O,
    ) {
        let num_lookups = 100_000;
        thrash_the_cache(1000, p, num_lookups, false, mk_open_db());
        if include_cyclic {
            thrash_the_cache(1000, p, num_lookups, true, mk_open_db());
        }
        thrash_the_cache(10_000, p, num_lookups, false, mk_open_db());
        if include_cyclic {
            thrash_the_cache(10_000, p, num_lookups, true, mk_open_db());
        }
    }

    /// Test thrashing the cache, where the cache is smaller than the number of
    /// items actively held in memory.
    ///
    /// Parameters:
    ///
    /// - `num_values`: the number of unique values to insert into the arena.
    ///
    /// - `p`: the cache size as a proportion of `num_values`.
    ///
    /// - `num_lookups`: number of values to look up in the arena by hash key.
    ///
    /// - `is_cyclic`: whether to lookup values in a cyclic pattern, or
    ///   randomly. For random lookups, the probability of a cache hit is `p`
    ///   for each lookup. For cyclic lookups, the cache hit rate is 0 if the
    ///   `p` is less than 1.0, and 1 otherwise.
    ///
    /// - `open_db`: opens a new connection to the *same* db every time it's
    ///   called.
    ///
    /// Parity-db beats SQLite here, but by how much varies a lot:
    ///
    /// - for p = 0.5, SQLite takes about 4 times as long
    /// - for p = 0.8, SQLite takes 1.5 times to 2.5 times as long, doing better for
    ///   larger `num_values`.
    #[cfg(any(feature = "parity-db", feature = "sqlite"))]
    fn thrash_the_cache<D: DB>(
        num_values: usize,
        p: f64,
        num_lookups: usize,
        is_cyclic: bool,
        open_db: impl Fn() -> D,
    ) {
        use crate::storage::Storage;
        use rand::Rng;
        use std::io::{Write, stdout};
        use std::{collections::HashMap, time::Instant};

        assert!(p > 0.0 && p <= 1.0, "Cache proportion must be in (0,1]");

        let db = open_db();
        let cache_size = (num_values as f64 * p) as usize;
        let storage = Storage::new(cache_size, db);
        let arena = storage.arena;
        let mut key_map = HashMap::new();
        let mut rng = rand::thread_rng();

        let prefix = format!(
            "thrash_the_cache(num_values={}, p={}, num_lookups={}, is_cyclic={})",
            num_values, p, num_lookups, is_cyclic
        );

        // Insert numbers and store their root keys
        let start_time = Instant::now();
        println!("{prefix} inserting data:");
        for x in 0..num_values {
            if x % (num_values / 100) == 0 {
                print!(".");
                stdout().flush().unwrap();
            }
            let mut sp = arena.alloc(x as u64);
            sp.persist();
            key_map.insert(x, sp.as_typed_key());
        }
        let elapsed = start_time.elapsed();
        println!("{:.2?}", elapsed);

        // Flush changes and create a fresh cache.
        let start_time = Instant::now();
        print!("{prefix} flushing to disk: ");
        arena.with_backend(|b| b.flush_all_changes_to_db());
        drop(arena);
        let db = open_db();
        let storage = Storage::new(cache_size, db);
        let arena = storage.arena;
        let elapsed = start_time.elapsed();
        println!("{:.2?}", elapsed);

        // Warm up the cache, i.e. fetch all values once.
        println!("{prefix} warming up the cache:");
        let start_time = Instant::now();
        for x in 0..num_values {
            if x % (num_values / 100) == 0 {
                print!(".");
                stdout().flush().unwrap();
            }
            let hash = key_map.get(&x).unwrap();
            arena.get::<u64>(hash).unwrap();
        }
        let elapsed = start_time.elapsed();
        println!("{:.2?}", elapsed);

        // Compute values to lookup.
        let xs: std::vec::Vec<_> = if is_cyclic {
            (0..num_values).cycle().take(num_lookups).collect()
        } else {
            (0..num_lookups)
                .map(|_| rng.gen_range(0..num_values))
                .collect()
        };

        // Repeatedly lookup values via their hash, num_lookups times.
        println!("{prefix} fetching data:");
        let start_time = Instant::now();
        for (i, x) in xs.into_iter().enumerate() {
            if i % (num_lookups / 100) == 0 {
                print!(".");
                stdout().flush().unwrap();
            }
            let hash = key_map.get(&x).unwrap();
            arena.get::<u64>(hash).unwrap();
        }
        let elapsed = start_time.elapsed();
        println!("{:.2?}", elapsed);
        println!();
    }

    /// See `load_large_tree_inner` for details.
    ///
    /// Example time with height = 20:
    ///
    /// ```text
    /// $ cargo run --all-features --release --bin stress -p midnight-storage -- arena::stress_tests::load_large_tree_sqldb 20
    /// load_large_tree: 0.40/0.40: init
    /// load_large_tree: 20.43/20.84: create tree
    /// load_large_tree: 12.20/33.03: persist tree to disk
    /// load_large_tree: 2.10/35.13: drop
    /// load_large_tree: 0.69/35.81: init
    /// load_large_tree: 27.89/63.70: lazy load and traverse tree, no prefetch
    /// load_large_tree: 2.47/66.17: drop
    /// load_large_tree: 0.00/66.18: init
    /// load_large_tree: 8.40/74.57: lazy load and traverse tree, with prefetch
    /// load_large_tree: 2.46/77.03: drop
    /// load_large_tree: 0.00/77.04: init
    /// load_large_tree: 26.19/103.23: eager load and traverse tree, no prefetch
    /// load_large_tree: 2.47/105.69: drop
    /// load_large_tree: 0.00/105.70: init
    /// load_large_tree: 8.01/113.71: eager load and traverse tree, with prefetch
    /// load_large_tree: 2.48/116.19: drop
    ///   97.10s user 18.82s system 99% cpu 1:56.56 total
    /// ```
    ///
    /// Note that tree creation takes 20 s here, vs 4 s for parity-db, even tho
    /// naively, creation should have no interaction with the db: turns out the
    /// hidden db interaction on creation happens in `StorageBackend::cache`,
    /// which checks the db to see if the to-be-cached key is already in the db
    /// or not, which is used for ref-counting. If I comment that check out,
    /// which is irrelevant for this test, then creation time drops to 3.5 s, larger than an 80 %
    /// improvement.
    #[cfg(feature = "sqlite")]
    pub fn load_large_tree_sqldb(args: &[String]) {
        use crate::db::SqlDB;
        let path = tempfile::NamedTempFile::new().unwrap().into_temp_path();
        let open_db = || SqlDB::<crate::DefaultHasher>::exclusive_file(&path);
        load_large_tree(args, open_db);
    }

    /// See `load_large_tree_inner` for details.
    ///
    /// Example time with height = 20:
    ///
    /// ```text
    /// $ cargo run --all-features --release --bin stress -p midnight-storage -- arena::stress_tests::load_large_tree_paritydb 20
    /// load_large_tree: 0.06/0.06: init
    /// load_large_tree: 3.83/3.89: create tree
    /// load_large_tree: 6.02/9.90: persist tree to disk
    /// load_large_tree: 19.16/29.06: drop
    /// load_large_tree: 0.92/29.99: init
    /// load_large_tree: 7.83/37.81: lazy load and traverse tree, no prefetch
    /// load_large_tree: 2.50/40.31: drop
    /// load_large_tree: 0.02/40.33: init
    /// load_large_tree: 8.31/48.64: lazy load and traverse tree, with prefetch
    /// load_large_tree: 2.52/51.16: drop
    /// load_large_tree: 0.02/51.18: init
    /// load_large_tree: 7.03/58.21: eager load and traverse tree, no prefetch
    /// load_large_tree: 2.58/60.79: drop
    /// load_large_tree: 0.02/60.81: init
    /// load_large_tree: 7.30/68.11: eager load and traverse tree, with prefetch
    /// load_large_tree: 2.47/70.59: drop
    ///   66.03s user 1.54s system 94% cpu 1:11.23 total
    /// ```
    ///
    /// Note that the biggest time delta is in the first `drop`. I think this is
    /// because parity-db does many operations asynchronously, returning to the
    /// caller immediately after work is passed off to a background thread),
    /// which then needs to be finished before the db can be dropped.
    #[cfg(feature = "parity-db")]
    pub fn load_large_tree_paritydb(args: &[String]) {
        use crate::db::ParityDb;
        let path = tempfile::TempDir::new().unwrap().keep();
        let open_db = || ParityDb::<crate::DefaultHasher>::open(&path);
        load_large_tree(args, open_db);
    }

    #[cfg(any(feature = "parity-db", feature = "sqlite"))]
    fn load_large_tree<D: DB>(args: &[String], open_db: impl Fn() -> D) {
        let msg = "load_large_tree(height: usize)";
        if args.len() != 1 {
            panic!("{msg}: wrong number of args");
        }
        let height = args[0]
            .parse()
            .unwrap_or_else(|e| panic!("{msg}: couldn't parse height={}: {e}", args[0]));
        load_large_tree_inner(height, open_db);
    }

    /// Create and persist a large tree, then load it various ways and traverse
    /// it.
    ///
    /// Here `open_db` must open a new connection to the *same* db every
    /// time. The test flushes to the db, and then reopens it.
    ///
    /// The tree will have `2^height - 1` nodes.
    #[cfg(any(feature = "parity-db", feature = "sqlite"))]
    fn load_large_tree_inner<D: DB>(height: usize, open_db: impl Fn() -> D) {
        use crate::storage::Storage;
        use bin_tree::*;

        let cache_size = 1 << height;
        // Value sum of tree: 1 + 2 + 3 + 4 + ... + 2^height - 1
        let sum = (1 << (height - 1)) * ((1 << height) - 1);
        let mut timer = crate::test::Timer::new("load_large_tree");

        // Build and persist tree.
        //
        // Compute key in a block, to ensure everything else gets dropped.
        let key = {
            let db = open_db();
            let storage = Storage::new(cache_size, db);
            let arena = storage.arena;
            timer.delta("init");

            let mut bt = counting_tree(&arena, height);
            timer.delta("create tree");

            bt.persist();
            arena.with_backend(|b| b.flush_all_changes_to_db());
            timer.delta("persist tree to disk");

            bt.as_typed_key()
        };
        timer.delta("drop");

        // Lazy load and traverse tree, no prefetch.
        {
            let db = open_db();
            let storage = Storage::new(cache_size, db);
            let arena = storage.arena;
            timer.delta("init");

            let bt = arena.get_lazy::<BinTree<D>>(&key).unwrap();
            assert_eq!(bt.sum(), sum);
            timer.delta("lazy load and traverse tree, no prefetch");
        }
        timer.delta("drop");

        // Lazy load and traverse tree, with prefetch.
        {
            let db = open_db();
            let storage = Storage::new(cache_size, db);
            let arena = storage.arena;
            timer.delta("init");

            let max_depth = Some(height);
            let truncate = false;
            arena.with_backend(|b| {
                key.key
                    .refs()
                    .iter()
                    .for_each(|hash| b.pre_fetch(hash, max_depth, truncate))
            });
            let bt = arena.get_lazy::<BinTree<D>>(&key).unwrap();
            assert_eq!(bt.sum(), sum);
            timer.delta("lazy load and traverse tree, with prefetch");
        }
        timer.delta("drop");

        // Eager load and traverse tree, no prefetch.
        {
            let db = open_db();
            let storage = Storage::new(cache_size, db);
            let arena = storage.arena;
            timer.delta("init");

            let bt = arena.get::<BinTree<D>>(&key).unwrap();
            assert_eq!(bt.sum(), sum);
            timer.delta("eager load and traverse tree, no prefetch");
        }
        timer.delta("drop");

        // Eager load and traverse tree, with prefetch.
        {
            let db = open_db();
            let storage = Storage::new(cache_size, db);
            let arena = storage.arena;
            timer.delta("init");

            let max_depth = Some(height);
            let truncate = false;
            arena.with_backend(|b| {
                key.key
                    .refs()
                    .iter()
                    .for_each(|hash| b.pre_fetch(hash, max_depth, truncate))
            });
            let bt = arena.get::<BinTree<D>>(&key).unwrap();
            assert_eq!(bt.sum(), sum);
            timer.delta("eager load and traverse tree, with prefetch");
        }
        timer.delta("drop");
    }

    /// Performance when reading and writing random data into a map and flushing
    /// it in a tight loop
    pub fn read_write_map_loop<D: DB>(args: &[String]) {
        let msg = "read_write_map_loop(num_operations: usize, flush_interval: usize)";
        if args.len() != 2 {
            panic!("{msg}: wrong number of args");
        }
        let num_operations = args[0]
            .parse()
            .unwrap_or_else(|e| panic!("{msg}: couldn't parse num_operations={}: {e}", args[0]));
        let flush_interval = args[1]
            .parse()
            .unwrap_or_else(|e| panic!("{msg}: couldn't parse flush_interval={}: {e}", args[1]));
        read_write_map_loop_inner::<D>(num_operations, flush_interval);
    }

    /// Performance when reading and writing random data into a map and flushing
    /// it in a tight loop.
    ///
    /// Parameters:
    ///
    /// - `num_operations`: total number of reads and writes to perform.
    ///
    /// - `flush_interval`: how many operations to do between each flush.
    ///
    /// # Summary of performance of `SqlDB` for various SQLite configurations
    ///
    /// For the 1000000 operation, 1000 flush interval `read_write_map_loop`
    /// stress test, we see the following total db flush times:
    ///
    /// - original settings: 2860 s
    ///
    /// - with synchronous = 0, but original journal mode: 466 s
    ///
    /// - with synchronous = 0, and WAL journal: 442 s
    ///
    /// I.e. speedup factor is 2860/442 ~ 6.5 times.
    ///
    /// The time spent on in-memory storage::Map updates in this stress test
    /// don't depend on the db settings (of course), and are about 175 s, so with
    /// the DB optimizations we have a ratio of ~ 2.5 times for in-memory updates vs
    /// disk writes for map inserts, which seems pretty good from the point of
    /// view of db traffic, but may indicate there's room to improve the
    /// implementation of the in-memory part.
    ///
    /// Of the db flush time, it seems about `7%` is devoted to preparing the data
    /// to be flushed, and the other `93%` is the time our `db::sql::SqlDB` takes to
    /// do the actual flushing.
    fn read_write_map_loop_inner<D: DB>(num_operations: usize, flush_interval: usize) {
        use crate::storage::{Map, Storage, WrappedDB, set_default_storage};
        use rand::{Rng, seq::SliceRandom as _};
        use serde_json::json;
        use std::io::{Write, stdout};
        use std::time::Instant;

        // Create a unique tag for our WrappedDB
        struct Tag;
        type DB<D> = WrappedDB<D, Tag>;

        let storage = set_default_storage(Storage::<DB<D>>::default).unwrap();

        let mut rng = rand::thread_rng();
        let mut map = Map::<u128, u128, DB<D>>::new();
        let mut keys = vec![];
        let mut total_write_time = std::time::Duration::new(0, 0);
        let mut total_read_time = std::time::Duration::new(0, 0);
        let mut total_flush_time = std::time::Duration::new(0, 0);
        let mut reads = 0;
        let mut writes = 0;
        let mut flushes = 0;
        let prefix = format!(
            "read_write_map_loop(num_operations={num_operations}, flush_interval={flush_interval})"
        );

        let mut time_series: std::vec::Vec<serde_json::Value> = vec![];

        println!("{prefix} running: ");
        let start = Instant::now();
        for i in 1..=num_operations {
            // Print progress
            if i % (num_operations / 100) == 0 {
                print!(".");
                stdout().flush().unwrap();
            }

            // Alternate between reading and writing.
            if i % 2 == 0 {
                // Choose a random key to read from the keys inserted so far.
                let key = keys.choose(&mut rng).unwrap();
                let read_start = Instant::now();
                let _ = map.get(key);
                total_read_time += read_start.elapsed();
                reads += 1;
            } else {
                // Generate a random key to write.
                let key = rng.r#gen::<u128>();
                keys.push(key);
                let value = rng.r#gen::<u128>();
                let write_start = Instant::now();
                map = map.insert(key, value);
                total_write_time += write_start.elapsed();
                writes += 1;
            }

            // Periodic flush
            if i % flush_interval == 0 {
                // debug
                let cache_size = storage.arena.with_backend(|b| b.get_write_cache_len());
                let cache_bytes = storage
                    .arena
                    .with_backend(|b| b.get_write_cache_obj_bytes());

                let flush_start = Instant::now();
                storage
                    .arena
                    .with_backend(|backend| backend.flush_all_changes_to_db());
                let flush_time = flush_start.elapsed();
                total_flush_time += flush_time;
                flushes += 1;

                // debug
                let map_size = map.size();
                let map_size_ratio = flush_time / (map_size as u32);
                let cache_size_ratio = flush_time / (cache_size as u32);
                let cache_bytes_ratio = flush_time / (cache_bytes as u32);
                println!(
                    "ft: {:0.2?}; ms: {}; ft/ms: {:0.2?}; cs: {}; ft/cs: {:0.2?}; cb: {}; ft/cb: {:0.2?}; cb/cs: {}",
                    flush_time,
                    map_size,
                    map_size_ratio,
                    cache_size,
                    cache_size_ratio,
                    cache_bytes,
                    cache_bytes_ratio,
                    cache_bytes / cache_size,
                );
                time_series.push(json!({
                    "i": i,
                    "flush_time": flush_time.as_secs_f32(),
                    "map_size": map_size,
                    "cache_size": cache_size,
                    "cache_bytes": cache_bytes,
                }));
            }
        }
        println!();

        // Print statistics
        let total_time = start.elapsed();
        println!("{prefix} results:");
        println!("- total time: {:.2?}", total_time);
        println!("- operations performed: {num_operations}");
        println!(
            "  - reads:  {} (avg {:.2?} per op)",
            reads,
            total_read_time / reads as u32
        );
        println!(
            "  - writes: {} (avg {:.2?} per op)",
            writes,
            total_write_time / writes as u32
        );
        println!(
            "  - flushes: {} (avg {:.2?} per flush)",
            flushes,
            total_flush_time / std::cmp::max(flushes, 1) as u32
        );
        println!(
            "  - ops/second: {:.0}",
            num_operations as f64 / total_time.as_secs_f64()
        );

        // Save statistics in json file.
        let write_json = || -> std::io::Result<()> {
            let now = std::time::UNIX_EPOCH.elapsed().unwrap().as_secs();
            std::fs::create_dir_all("tmp")?;
            let file_path =
                format!("tmp/read_write_map_loop.{num_operations}_{flush_interval}.{now:?}.json");
            let mut file = std::fs::File::create(&file_path)?;
            let header = json!({
                "total_time": total_time.as_secs_f32(),
                "num_operations": num_operations,
                "flush_interval": flush_interval,
                "reads": reads,
                "total_read_time": total_read_time.as_secs_f32(),
                "writes": writes,
                "total_write_time": total_write_time.as_secs_f32(),
                "flushes": flushes,
                "total_flush_time": total_flush_time.as_secs_f32(),
            });
            let json = json!({
                "header": header,
                "data": time_series,
            });
            writeln!(file, "{}", serde_json::to_string_pretty(&json)?)?;
            let canon_path = std::path::Path::new(&file_path).canonicalize()?;
            println!("- JSON stats: {}", canon_path.display());
            Ok(())
        };
        write_json().unwrap();
    }
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "stress-test")]
    #[test]
    fn array_nesting() {
        crate::stress_test::runner::StressTest::new()
            .with_max_memory(1 << 30)
            .run("arena::stress_tests::array_nesting");
    }

    #[cfg(feature = "stress-test")]
    #[test]
    // Remove this "should_panic" once implicit recursion in Sp drop is fixed.
    #[should_panic = "has overflowed its stack"]
    fn drop_deeply_nested_data() {
        crate::stress_test::runner::StressTest::new()
            // Must capture, so we can match the output with `should_panic`.
            .with_nocapture(false)
            .run("arena::stress_tests::drop_deeply_nested_data");
    }

    #[cfg(feature = "stress-test")]
    #[test]
    // Remove this "should_panic" once implicit recursion in Sp drop is fixed.
    #[should_panic = "has overflowed its stack"]
    fn serialize_deeply_nested_data() {
        crate::stress_test::runner::StressTest::new()
            // Must capture, so we can match the output with `should_panic`.
            .with_nocapture(false)
            .run("arena::stress_tests::serialize_deeply_nested_data");
    }

    #[cfg(all(feature = "stress-test", feature = "sqlite"))]
    #[test]
    fn thrash_the_cache_sqldb() {
        thrash_the_cache("sqldb");
    }
    #[cfg(all(feature = "stress-test", feature = "parity-db"))]
    #[test]
    fn thrash_the_cache_paritydb() {
        thrash_the_cache("paritydb");
    }
    #[cfg(all(
        feature = "stress-test",
        any(feature = "sqlite", feature = "parity-db")
    ))]
    /// Here `db_name` should be `paritydb` or `sqldb`.
    fn thrash_the_cache(db_name: &str) {
        let test_name = &format!("arena::stress_tests::thrash_the_cache_variations_{db_name}");
        let time_limit = 10 * 60;
        // Thrash the cache with p=0.1.
        //
        // Here we should see a small variation between cyclic and non-cyclic,
        // since the cache is very small.
        crate::stress_test::runner::StressTest::new()
            .with_max_runtime(time_limit)
            .run_with_args(test_name, &["0.1", "true"]);
        // Thrash the cache with p=0.5.
        //
        // Don't include cyclic, since it will be the same as in the last test.
        crate::stress_test::runner::StressTest::new()
            .with_max_runtime(time_limit)
            .run_with_args(test_name, &["0.5", "false"]);
        // Thrash the cache with p=0.8.
        //
        // Don't include cyclic, since it will be the same as in the last test.
        crate::stress_test::runner::StressTest::new()
            .with_max_runtime(time_limit)
            .run_with_args(test_name, &["0.8", "false"]);
        // Thrash the cache with p=1.0.
        //
        // With cache as large as the data, cyclic vs non-cyclic should be
        // irrelevant, and this should be the fastest.
        crate::stress_test::runner::StressTest::new()
            .with_max_runtime(time_limit)
            .run_with_args(test_name, &["1.0", "true"]);
    }

    #[cfg(all(feature = "stress-test", feature = "sqlite"))]
    #[test]
    fn load_large_tree_sqldb() {
        crate::stress_test::runner::StressTest::new()
            .with_max_runtime(5 * 60)
            .with_max_memory(2 << 30)
            .run_with_args("arena::stress_tests::load_large_tree_sqldb", &["15"]);
    }

    #[cfg(all(feature = "stress-test", feature = "parity-db"))]
    #[test]
    fn load_large_tree_paritydb() {
        crate::stress_test::runner::StressTest::new()
            .with_max_runtime(5 * 60)
            .with_max_memory(2 << 30)
            .run_with_args("arena::stress_tests::load_large_tree_paritydb", &["15"]);
    }

    #[cfg(all(feature = "stress-test", feature = "sqlite"))]
    #[test]
    fn read_write_map_loop_sqldb() {
        crate::stress_test::runner::StressTest::new()
            .with_max_runtime(60)
            .run_with_args(
                "arena::stress_tests::read_write_map_loop_sqldb",
                &["10000", "1000"],
            );
    }
    #[cfg(all(feature = "stress-test", feature = "parity-db"))]
    #[test]
    fn read_write_map_loop_paritydb() {
        crate::stress_test::runner::StressTest::new()
            .with_max_runtime(60)
            .run_with_args(
                "arena::stress_tests::read_write_map_loop_paritydb",
                &["10000", "1000"],
            );
    }
}
