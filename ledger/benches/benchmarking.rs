#![deny(warnings)]
#![allow(unused_imports)]
#![allow(unused)]
use base_crypto::rng::SplittableRng;
use base_crypto::signatures::Signature;
use base_crypto::time::Timestamp;
use coin_structure::coin::UserAddress;
use criterion::{BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
use lazy_static::lazy_static;
use midnight_ledger::dust::{DustPublicKey, InitialNonce};
use midnight_ledger::init_logger;
use midnight_ledger::prove::Resolver;
use midnight_ledger::semantics::{TransactionContext, TransactionResult};
use midnight_ledger::structure::{
    CNightGeneratesDustActionType, CNightGeneratesDustEvent, Intent, LedgerState,
    OutputInstructionUnshielded, ProofMarker, SystemTransaction, UnshieldedOffer, UtxoOutput,
    UtxoSpend,
};
use midnight_ledger::structure::{ClaimKind, Transaction};
#[cfg(feature = "proving")]
use midnight_ledger::test_utilities::well_formed_tx_builder;
use midnight_ledger::test_utilities::{TestState, test_resolver};
use midnight_ledger::verify::WellFormedStrictness;
use onchain_runtime::context::BlockContext;
use pprof::criterion::{Output, PProfProfiler};
use proptest::prelude::Arbitrary;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use serialize::Serializable;
use tempfile::tempdir;
use std::alloc::GlobalAlloc;
use std::collections::VecDeque;
use std::fmt::Debug;
use std::ops::{Deref, Div, Sub};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::sync::atomic::AtomicU64;
use std::time::Duration;
use storage::DefaultHasher;
use storage::arena::{ArenaHash, Sp, TCONSTRUCT};
use storage::backend::{OnDiskObject, StorageBackendStats};
use storage::db::{DB, DummyArbitrary, InMemoryDB, ParityDb, Update};
use storage::storage::{
    DEFAULT_CACHE_SIZE, default_storage, set_default_storage, unsafe_drop_default_storage,
};
use transient_crypto::commitment::PedersenRandomness;
use zswap::keys::SecretKeys;

#[global_allocator]
static GLOBAL_ALLOC: Allocator<std::alloc::System> = Allocator(std::alloc::System);
static CURALLOC: AtomicU64 = AtomicU64::new(0);

fn fmt_bytes(n: u64) -> String {
    const KB: u64 = 1 << 10;
    const MB: u64 = 1 << 20;
    const GB: u64 = 1 << 30;
    match n {
        0..KB => format!("{n} B"),
        KB..MB => format!("{:.2} kiB", n as f64 / KB as f64),
        MB..GB => format!("{:.2} MiB", n as f64 / MB as f64),
        GB.. => format!("{:.2} GiB", n as f64 / GB as f64),
    }
}

fn cur_alloc() -> String {
    fmt_bytes(CURALLOC.load(std::sync::atomic::Ordering::SeqCst))
}

struct Allocator<A: GlobalAlloc>(A);

unsafe impl<A: GlobalAlloc> GlobalAlloc for Allocator<A> {
    unsafe fn alloc(&self, layout: std::alloc::Layout) -> *mut u8 {
        CURALLOC
            .fetch_update(
                std::sync::atomic::Ordering::SeqCst,
                std::sync::atomic::Ordering::SeqCst,
                |x| Some(x + layout.size() as u64),
            )
            .unwrap();
        unsafe { self.0.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: std::alloc::Layout) {
        CURALLOC
            .fetch_update(
                std::sync::atomic::Ordering::SeqCst,
                std::sync::atomic::Ordering::SeqCst,
                |x| Some(x - layout.size() as u64),
            )
            .unwrap();
        unsafe { self.0.dealloc(ptr, layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: std::alloc::Layout) -> *mut u8 {
        CURALLOC
            .fetch_update(
                std::sync::atomic::Ordering::SeqCst,
                std::sync::atomic::Ordering::SeqCst,
                |x| Some(x + layout.size() as u64),
            )
            .unwrap();
        unsafe { self.0.alloc_zeroed(layout) }
    }
}

#[derive(Debug, Default, Copy, Clone)]
struct DBStats {
    inserts: usize,
    insert_data_size: usize,
    duplicate_inserts: usize,
    deletes: usize,
    delete_data_size: usize,
    false_deletes: usize,
    rc_set: usize,
    updates: usize,
    most_refs: usize,
    total_storage: usize,
}

impl Sub for DBStats {
    type Output = DBStats;
    fn sub(self, rhs: Self) -> Self::Output {
        DBStats {
            inserts: self.inserts - rhs.inserts,
            insert_data_size: self.insert_data_size - rhs.insert_data_size,
            duplicate_inserts: self.duplicate_inserts - rhs.duplicate_inserts,
            deletes: self.deletes - rhs.deletes,
            delete_data_size: self.delete_data_size - rhs.delete_data_size,
            false_deletes: self.false_deletes - rhs.false_deletes,
            rc_set: self.rc_set - rhs.rc_set,
            updates: self.updates - rhs.updates,
            most_refs: self.most_refs,
            total_storage: self.total_storage,
        }
    }
}

impl Div<usize> for DBStats {
    type Output = DBStats;
    fn div(self, rhs: usize) -> Self::Output {
        DBStats {
            inserts: self.inserts / rhs,
            insert_data_size: self.insert_data_size / rhs,
            duplicate_inserts: self.duplicate_inserts / rhs,
            deletes: self.deletes / rhs,
            delete_data_size: self.delete_data_size / rhs,
            false_deletes: self.false_deletes / rhs,
            rc_set: self.rc_set / rhs,
            updates: self.updates / rhs,
            most_refs: self.most_refs,
            total_storage: self.total_storage,
        }
    }
}

#[derive(Default, Clone)]
struct StatsDB {
    nodes: Arc<Mutex<std::collections::HashMap<ArenaHash<DefaultHasher>, OnDiskObject<DefaultHasher>>>>,
    roots: Arc<Mutex<std::collections::HashMap<ArenaHash<DefaultHasher>, u32>>>,
    refs: Arc<Mutex<std::collections::HashMap<ArenaHash<DefaultHasher>, u64>>>,
    stats: Arc<Mutex<DBStats>>,
}

impl Debug for StatsDB {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.stats.fmt(f)
    }
}

impl DummyArbitrary for StatsDB {}

#[cfg(feature = "proptest")]
/// A dummy Arbitrary impl for `InMemoryDB` to allow for deriving Arbitrary on Sp<T, D>
impl Arbitrary for StatsDB {
    type Parameters = ();
    type Strategy = proptest::strategy::Just<StatsDB>;

    fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
        proptest::strategy::Just(StatsDB::default())
    }
}

impl StatsDB {
    fn with_stats(stats: Arc<Mutex<DBStats>>) -> Self {
        StatsDB { nodes: Default::default(), roots: Default::default(), refs: Default::default(), stats }
    }
    fn lock_nodes(&self) -> std::sync::MutexGuard<'_, std::collections::HashMap<ArenaHash<DefaultHasher>, OnDiskObject<DefaultHasher>>> {
        self.nodes.lock().expect("db lock poisoned")
    }

    fn lock_roots(&self) -> std::sync::MutexGuard<'_, std::collections::HashMap<ArenaHash<DefaultHasher>, u32>> {
        self.roots.lock().expect("db lock poisoned")
    }

    fn lock_refs(&self) -> std::sync::MutexGuard<'_, std::collections::HashMap<ArenaHash<DefaultHasher>, u64>> {
        self.refs.lock().expect("db lock poisoned")
    }

    fn lock_stats(&self) -> std::sync::MutexGuard<'_, DBStats> {
        self.stats.lock().expect("db lock poisoned")
    }
}

impl DB for StatsDB {
    type Hasher = DefaultHasher;
    fn get_node(&self, key: &storage::arena::ArenaHash<Self::Hasher>) -> Option<storage::backend::OnDiskObject<Self::Hasher>> {
        let mut node = self.lock_nodes().get(key).cloned()?;
        node.ref_count = self.lock_refs().get(key).copied().unwrap_or(0);
        Some(node)
    }
    fn set_ref_count(&self, key: ArenaHash<Self::Hasher>, count: u64) {
        self.lock_stats().rc_set += 1;
        self.lock_refs().insert(key, count);
    }
    fn get_ref_count(&self, key: &ArenaHash<Self::Hasher>) -> u64 {
        self.lock_refs().get(key).copied().unwrap_or(0)
    }
    fn get_unreachable_keys(&self) -> std::vec::Vec<storage::arena::ArenaHash<Self::Hasher>> {
        let nodes_guard = self.lock_nodes();
        let roots_guard = self.lock_roots();
        let mut unreachable = vec![];
        for (key, node) in nodes_guard.iter() {
            if node.ref_count == 0 && !roots_guard.contains_key(key) {
                unreachable.push(key.clone());
            }
        }
        unreachable
    }

    fn insert_node(&mut self, key: ArenaHash<DefaultHasher>, object: OnDiskObject<DefaultHasher>) {
        let obj_size = object.serialized_size();
        self.lock_stats().insert_data_size += obj_size;
        let old_obj = self.lock_nodes().insert(key, object.clone());
        if let Some(old_obj) = old_obj {
            self.lock_stats().total_storage += obj_size - old_obj.serialized_size();
            self.lock_stats().duplicate_inserts += 1;

            //eprintln!("duplicate insert, is equal: {} {} {}", old_obj.data == object.data, old_obj.ref_count == object.ref_count, old_obj.children == object.children);
        } else {
            self.lock_stats().total_storage += obj_size;
            self.lock_stats().inserts += 1;
        }
        let mut stats = self.lock_stats();
        stats.most_refs = stats.most_refs.max(object.children.iter().flat_map(|c| c.refs()).count());
    }

    fn delete_node(&mut self, key: &ArenaHash<DefaultHasher>) {
        let obj = self.lock_nodes().remove(key);
        match obj {
            None => {
                self.lock_stats().false_deletes += 1;
            }
            Some(obj) => {
                self.lock_stats().deletes += 1;
                self.lock_stats().delete_data_size += obj.serialized_size();
                self.lock_stats().total_storage -= obj.serialized_size();
            }
        }
    }

    fn get_root_count(&self, key: &ArenaHash<Self::Hasher>) -> u32 {
        self.lock_roots().get(key).cloned().unwrap_or(0)
    }

    fn set_root_count(&mut self, key: ArenaHash<Self::Hasher>, count: u32) {
        if count > 0 {
            self.lock_roots().insert(key, count);
        } else {
            self.lock_roots().remove(&key);
        }
        self.lock_stats().updates += 1;
    }

    fn get_roots(&self) -> std::collections::HashMap<ArenaHash<Self::Hasher>, u32> {
        self.lock_roots().clone()
    }

    fn size(&self) -> usize {
        self.lock_nodes().len()
    }

    fn batch_update<I>(&mut self, iter: I)
    where
        I: Iterator<Item = (ArenaHash<Self::Hasher>, Update<Self::Hasher>)>,
    {
        storage::db::dubious_batch_update(self, iter);
    }

    fn batch_get_nodes<I>(
        &self,
        keys: I,
    ) -> Vec<(ArenaHash<Self::Hasher>, Option<OnDiskObject<Self::Hasher>>)>
    where
        I: Iterator<Item = ArenaHash<Self::Hasher>>,
    {
        storage::db::dubious_batch_get_nodes(self, keys)
    }
}

lazy_static! {
    static ref RESOLVER: Resolver = test_resolver("");
}

pub fn rewards(c: &mut Criterion) {
    let mut rng = StdRng::seed_from_u64(0x42);
    fn mk_rewards<R: Rng>(rng: &mut R, n: usize) -> SystemTransaction {
        let mut outputs = Vec::with_capacity(n);
        for _ in 0..n {
            outputs.push(OutputInstructionUnshielded {
                amount: rng.r#gen::<u32>() as u128,
                target_address: UserAddress(rng.r#gen()),
                nonce: rng.r#gen(),
            });
        }
        SystemTransaction::DistributeNight(ClaimKind::Reward, outputs)
    }
    let mut ledger_state: LedgerState<InMemoryDB> = LedgerState::new("local-test");
    ledger_state = ledger_state
        .apply_system_tx(
            &SystemTransaction::DistributeReserve(ledger_state.reserve_pool),
            Timestamp::from_secs(0),
        )
        .unwrap()
        .0;

    let mut group = c.benchmark_group("rewards");
    for size in [100, 200, 300, 400, 500] {
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, &size| {
            let tx = mk_rewards(&mut rng, size);
            b.iter(|| {
                black_box(
                    ledger_state
                        .apply_system_tx(&tx, Timestamp::from_secs(0))
                        .unwrap(),
                )
            });
        });
    }
    group.finish();
}


#[cfg(feature = "proving")]
pub fn transaction_validation(c: &mut Criterion) {
    let mut rng = StdRng::seed_from_u64(0x42);
    let ledger_state: LedgerState<InMemoryDB> = LedgerState::new("local-test");
    let sk = SecretKeys::from_rng_seed(&mut rng);
    let tx: Transaction<(), ProofMarker, PedersenRandomness, InMemoryDB> =
        well_formed_tx_builder(rng.split(), &sk, &RESOLVER).unwrap();

    let mut strictness = WellFormedStrictness::default();
    strictness.enforce_balancing = false;

    c.bench_function("verification", |b| {
        b.iter(|| {
            black_box(&tx)
                .well_formed(&ledger_state, strictness, Timestamp::from_secs(0))
                .unwrap();
        })
    });
    let vtx = tx
        .well_formed(&ledger_state, strictness, Timestamp::from_secs(0))
        .unwrap();
    let context = TransactionContext {
        ref_state: LedgerState::new("local-test"),
        block_context: BlockContext::default(),
        whitelist: None,
    };

    c.bench_function("application", |b| {
        b.iter(|| black_box(ledger_state.apply(black_box(&vtx), black_box(&context))))
    });
}

pub fn create_dust(c: &mut Criterion) {
    let mut rng = StdRng::seed_from_u64(0x42);
    fn mk_dust<R: Rng>(rng: &mut R, n: usize) -> SystemTransaction {
        let mut events = Vec::with_capacity(n);
        for _ in 0..n {
            events.push(CNightGeneratesDustEvent {
                value: rng.r#gen::<u32>() as u128,
                owner: DustPublicKey(rng.r#gen()),
                time: Timestamp::from_secs(0),
                action: CNightGeneratesDustActionType::Create,
                nonce: InitialNonce(rng.r#gen()),
            });
        }
        SystemTransaction::CNightGeneratesDustUpdate { events }
    }
    let ledger_state: LedgerState<InMemoryDB> = LedgerState::new("local-test");

    let mut group = c.benchmark_group("create-dust");
    for size in [100, 200, 300, 400] {
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, &size| {
            let tx = mk_dust(&mut rng, size);
            b.iter(|| {
                black_box(
                    ledger_state
                        .apply_system_tx(&tx, Timestamp::from_secs(0))
                        .unwrap(),
                )
            });
        });
    }
    group.finish();
}

type TestDb = ParityDb;

fn mk_test_db() -> storage::Storage<TestDb> {
    //let dir = tempfile::tempdir().unwrap().keep();
    storage::Storage::new(
        10000,
        //DEFAULT_CACHE_SIZE,
        Default::default(),
        //SqlDB::default(),
        //ParityDb::<DefaultHasher>::open(&dir),
        //InMemoryDB::default(),
    )
}

fn write_times_inner<D: DB>(log_batch_size: usize, cb: impl Fn(usize, u64, Option<u128>)) {
    let rt = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    let mut rng = StdRng::seed_from_u64(0x42);
    let mut state: TestState<D> = TestState::new(&mut rng);
    let mut reached_size = 0;
    let mut saved_states = VecDeque::new();
    const CUTOFF: std::time::Duration = std::time::Duration::from_secs(1800);
    'outer: for log_size in log_batch_size..=18 {
        let t0 = std::time::Instant::now();
        let mut dt_save_to_disk = std::time::Duration::default();
        let size = 2u64.pow(log_size as u32);
        state.mode = midnight_ledger::test_utilities::TestProcessingMode::ForceConstantTime;
        let mut gcs = 0;
        for _ in (reached_size >> log_batch_size)..(size >> log_batch_size) {
            let mut events = Vec::with_capacity(1 << log_batch_size);
            for _ in 0..1 << log_batch_size {
                events.push(CNightGeneratesDustEvent {
                    value: rng.r#gen::<u32>() as u128,
                    owner: DustPublicKey(rng.r#gen()),
                    time: Timestamp::from_secs(0),
                    action: CNightGeneratesDustActionType::Create,
                    nonce: InitialNonce(rng.r#gen()),
                });
            }
            state.apply_system_tx(
                &SystemTransaction::CNightGeneratesDustUpdate { events }
            ).unwrap();
            //rt.block_on(state.give_fee_token(&mut rng, 1 << log_batch_size));
            let tpre_save = std::time::Instant::now();
            saved_states.push_back(state.swizzle_to_db());
            dt_save_to_disk += tpre_save.elapsed();
            //if saved_states.len() > 10 {
            //    let (a, b, c) = saved_states.pop_front().unwrap();
            //    default_storage::<D>().get_lazy(&a).unwrap().unpersist();
            //    default_storage::<D>().get_lazy(&b).unwrap().unpersist();
            //    default_storage::<D>().get_lazy(&c).unwrap().unpersist();
            //    gcs += default_storage::<D>().with_backend(|b| b.gc());
            //}
            //if t0.elapsed() > CUTOFF {
            //    cb(log_size, size - reached_size, None);
            //    break 'outer;
            //}
        }
        cb(log_size, size - reached_size, Some(dt_save_to_disk.as_nanos()));
        //println!("{gcs} gcs!");
        reached_size = size;
    }
}

pub fn write_times_by_utxo_set_and_batch_size(_: &mut Criterion) {
    println!("db\tbatch\tsize\ttime_micros\ttotal_size");
    let stats = Arc::new(Mutex::new(DBStats::default()));
    let mut last_stats = Arc::new(Mutex::new(DBStats::default()));
    for log_batch_size in [4] {
        let pdb_path = tempdir().unwrap();
        //set_default_storage::<StatsDB>(|| storage::Storage::new(10, StatsDB::with_stats(stats.clone())));
        //set_default_storage::<SqlDB>(|| storage::Storage::new(10, Default::default()));
        set_default_storage::<ParityDb>(|| storage::Storage::new(10, ParityDb::open(pdb_path.path())));
        //write_times_inner::<StatsDB>(log_batch_size, |log_size, delta_size, t| {
        //    let cur_stats = *stats.clone().lock().unwrap();
        //    let last_stats_arc = last_stats.clone();
        //    let mut last_stats_locked = last_stats_arc.lock().unwrap();
        //    let dstats = cur_stats - *last_stats_locked;
        //    *last_stats_locked = cur_stats;
        //    let batches = delta_size / (1 << log_batch_size);
        //    println!("stats\t{log_batch_size}\t{log_size}\t{:?}", t.map(|_| dstats / batches as usize));
        //});
        //unsafe_drop_default_storage::<StatsDB>();
        //write_times_inner::<SqlDB>(log_batch_size, |log_size, _, time| println!("sqlite\t{log_batch_size}\t{log_size}\t{time:?}"));
        write_times_inner::<ParityDb>(log_batch_size, |log_size, delta_size, time| {
            let mut frontier = vec![pdb_path.path().to_owned()];
            let mut total_size = 0;
            while let Some(dir) = frontier.pop() {
                for entry in std::fs::read_dir(&dir).unwrap() {
                    let entry = entry.unwrap();
                    if entry.file_type().unwrap().is_dir() {
                        frontier.push(entry.path());
                    } else {
                        total_size += entry.metadata().unwrap().len();
                    }
                }
            }
            println!("parity\t{log_batch_size}\t{log_size}\t{:?}\t{}", time.map(|t| t / 1000 / (delta_size / (1 << log_batch_size)) as u128), fmt_bytes(total_size));
        });
        //unsafe_drop_default_storage::<SqlDB>();
        unsafe_drop_default_storage::<ParityDb>();
    }
}

pub fn night_transfer_by_utxo_set_size(c: &mut Criterion) {
    let rt = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    let mut rng = StdRng::seed_from_u64(0x42);
    let mut group = c.benchmark_group("night-transfer-by-utxo-set-size");
    init_logger(midnight_ledger::LogLevel::Warn);
    set_default_storage(mk_test_db);
    let mut state = TestState::new(&mut rng);
    let mut reached_size = 0;
    for log_size in 10.. {
        let t0 = std::time::Instant::now();
        let mut dt_save_to_disk = std::time::Duration::default();
        let size = 2u64.pow(log_size);
        state.mode = midnight_ledger::test_utilities::TestProcessingMode::ForceConstantTime;
        for _ in (reached_size >> 10)..(size >> 10) {
            rt.block_on(state.give_fee_token(&mut rng, 1 << 10));
            let tpre_save = std::time::Instant::now();
            state.swizzle_to_db();
            dt_save_to_disk += tpre_save.elapsed();
        }
        reached_size = size;
        let mut lstate = Sp::new(state.ledger.clone());
        let utxo = (**state.utxos.iter().next().unwrap()).clone();
        let offer: UnshieldedOffer<Signature, TestDb> = UnshieldedOffer {
            inputs: vec![UtxoSpend {
                value: utxo.value,
                owner: state.night_key.verifying_key(),
                type_: utxo.type_,
                intent_hash: utxo.intent_hash,
                output_no: utxo.output_no,
            }]
            .into(),
            outputs: vec![
                UtxoOutput {
                    value: 100,
                    owner: UserAddress::from(state.night_key.verifying_key()),
                    type_: utxo.type_,
                },
                UtxoOutput {
                    value: utxo.value - 100,
                    owner: UserAddress::from(state.night_key.verifying_key()),
                    type_: utxo.type_,
                },
            ]
            .into(),
            signatures: vec![].into(),
        };
        let intent = Intent::new(
            &mut rng,
            Some(offer),
            None,
            vec![],
            vec![],
            vec![],
            None,
            state.time,
        );
        let tx = Transaction::from_intents("local-test", [(1, intent)].into_iter().collect());
        let tx = rt
            .block_on(state.balance_tx(rng.split(), tx, &test_resolver("benchmarks")))
            .unwrap();
        let strictness = WellFormedStrictness::default();
        let vtx = tx.well_formed(&*lstate, strictness, state.time).unwrap();
        let context = state.context().block_context;
        let key = lstate.as_typed_key();
        println!(
            "Took {:?} (of which {:?} flushing to disk) to init {size} entries, with {} allocated",
            t0.elapsed(),
            dt_save_to_disk,
            cur_alloc()
        );
        lstate.persist();
        drop(lstate);
        state.swizzle_to_db();
        default_storage::<TestDb>().with_backend(|b| {
            b.flush_all_changes_to_db();
            b.flush_cache_evictions_to_db();
            //b.gc();
        });
        let mut runs = 0;
        let mut total_construct_time =
            std::collections::HashMap::<&'static str, (usize, std::time::Duration)>::new();
        group.bench_with_input(BenchmarkId::from_parameter(log_size), &log_size, |b, _| {
            b.iter_custom(|i| {
                let mut dt = Default::default();
                for _ in 0..i {
                    let pre_cache = default_storage::<TestDb>().with_backend(|b| b.get_stats());
                    let tconstruct0 = TCONSTRUCT.lock().unwrap().clone().unwrap_or_default();
                    let t0 = std::time::Instant::now();
                    let state = black_box(default_storage().get_lazy(&key)).unwrap();
                    let context = TransactionContext {
                        ref_state: state.deref().clone(),
                        block_context: context.clone(),
                        whitelist: None,
                    };
                    let (_, res) = black_box(state.apply(black_box(&vtx), &context));
                    dt += t0.elapsed();
                    let tconstruct1 = TCONSTRUCT.lock().unwrap().clone().unwrap_or_default();
                    for (k, v) in tconstruct1 {
                        total_construct_time.entry(k).or_default().0 +=
                            v.0 - tconstruct0.get(k).copied().unwrap_or_default().0;
                        total_construct_time.entry(k).or_default().1 +=
                            v.1 - tconstruct0.get(k).copied().unwrap_or_default().1;
                    }
                    let post_cache = default_storage::<TestDb>().with_backend(|b| b.get_stats());
                    runs += 1;
                    assert!(matches!(res, TransactionResult::Success(..)));
                }
                dt
            });
        });
        let mut total_construct_time = total_construct_time
            .into_iter()
            .map(|(k, (n, d))| (k, (n / runs as usize, d / runs)))
            .collect::<Vec<_>>();
        total_construct_time.sort_by_key(|v| v.1.1);
        println!(
            "Construct time: {:?}",
            total_construct_time
                .iter()
                .map(|(_, (_, d))| d)
                .sum::<Duration>()
        );
        let lstate = default_storage::<TestDb>().get_lazy(&key).unwrap();
        lstate.unpersist();
        drop(lstate);
        drop(key);
        drop(vtx);
        drop(tx);
        println!("After dropping all data, {} remains allocated", cur_alloc());
    }
    group.finish();
    unsafe_drop_default_storage::<TestDb>();
}

pub fn create_and_destroy_dust(c: &mut Criterion) {
    let mut rng = StdRng::seed_from_u64(0x42);
    fn mk_tx<R: Rng>(rng: &mut R, n: usize) -> SystemTransaction {
        let mut events = Vec::with_capacity(n);
        let mut owners = Vec::with_capacity(n);
        for _ in 0..n {
            if owners.is_empty() || rng.gen_bool(0.5) {
                let key = DustPublicKey(rng.r#gen());
                let amt = rng.r#gen::<u32>() as u128;
                owners.push((key, amt));
                events.push(CNightGeneratesDustEvent {
                    value: amt,
                    owner: key,
                    time: Timestamp::from_secs(0),
                    action: CNightGeneratesDustActionType::Create,
                    nonce: InitialNonce(rng.r#gen()),
                });
            } else {
                let key: DustPublicKey;
                let amt: u128;
                loop {
                    let idx = rng.gen_range(0..owners.len());
                    let (k, max_amt) = owners[idx];
                    if max_amt == 0 {
                        continue;
                    }
                    key = k;
                    amt = rng.gen_range(0..max_amt);
                    owners[idx] = (key, max_amt - amt);
                    break;
                }
                events.push(CNightGeneratesDustEvent {
                    value: amt,
                    owner: key,
                    time: Timestamp::from_secs(0),
                    action: CNightGeneratesDustActionType::Destroy,
                    nonce: InitialNonce(rng.r#gen()),
                })
            }
        }
        SystemTransaction::CNightGeneratesDustUpdate { events }
    }
    let ledger_state: LedgerState<InMemoryDB> = LedgerState::new("local-test");
    let mut group = c.benchmark_group("create-and-destroy-dust");
    for size in [100, 200, 300, 400] {
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, &size| {
            let tx = mk_tx(&mut rng, size);
            b.iter(|| {
                black_box(
                    ledger_state
                        .apply_system_tx(&tx, Timestamp::from_secs(0))
                        .unwrap(),
                )
            });
        });
    }
    group.finish();
}

#[cfg(feature = "proving")]
criterion_group!(
    name = benchmarking;
    config = Criterion::default().sample_size(10);
    targets = transaction_validation
);
criterion_group!(system_tx, rewards, create_dust, create_and_destroy_dust);
criterion_group!(
    name = night;
    config = Criterion::default().with_profiler(PProfProfiler::new(100, Output::Flamegraph(None)));
    targets = write_times_by_utxo_set_and_batch_size
    //targets = night_transfer_by_utxo_set_size
);
#[cfg(feature = "proving")]
criterion_main!(benchmarking, system_tx, night);
#[cfg(not(feature = "proving"))]
criterion_main!(system_tx, night);
