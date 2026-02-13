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
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use std::alloc::GlobalAlloc;
use std::ops::Deref;
use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use std::time::Duration;
use storage::DefaultHasher;
use storage::arena::{Sp, TCONSTRUCT};
use storage::backend::StorageBackendStats;
use storage::db::{InMemoryDB, ParityDb};
use storage::storage::{
    DEFAULT_CACHE_SIZE, default_storage, set_default_storage, unsafe_drop_default_storage,
};
use transient_crypto::commitment::PedersenRandomness;
use zswap::keys::SecretKeys;

#[global_allocator]
static GLOBAL_ALLOC: Allocator<std::alloc::System> = Allocator(std::alloc::System);
static CURALLOC: AtomicU64 = AtomicU64::new(0);

fn cur_alloc() -> String {
    let n = CURALLOC.load(std::sync::atomic::Ordering::SeqCst);
    const KB: u64 = 1 << 10;
    const MB: u64 = 1 << 20;
    const GB: u64 = 1 << 30;
    match n {
        0..KB => format!("{n} B"),
        KB..MB => format!("{} kiB", n >> 10),
        MB..GB => format!("{} MiB", n >> 20),
        GB.. => format!("{} GiB", n >> 30),
    }
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
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path()).unwrap();
    storage::Storage::new(
        DEFAULT_CACHE_SIZE,
        ParityDb::<DefaultHasher>::open(&dir.path().join("test-db")),
        //InMemoryDB::default(),
    )
}

pub fn night_transfer_by_utxo_set_size(c: &mut Criterion) {
    let rt = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    let mut rng = StdRng::seed_from_u64(0x42);
    let mut group = c.benchmark_group("night-transfer-by-utxo-set-size");
    init_logger(midnight_ledger::LogLevel::Warn);
    for log_size in 10..=15 {
        set_default_storage(mk_test_db);
        let t0 = std::time::Instant::now();
        let size = 2u64.pow(log_size);
        let mut state = TestState::new(&mut rng);
        state.mode = midnight_ledger::test_utilities::TestProcessingMode::ForceConstantTime;
        rt.block_on(state.give_fee_token(&mut rng, size as usize));
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
        //dbg!(&lstate.dust);
        //dbg!(&tx);
        //dbg!(tx.application_cost(&lstate.parameters.cost_model));
        let strictness = WellFormedStrictness::default();
        let vtx = tx.well_formed(&*lstate, strictness, state.time).unwrap();
        let context = state.context().block_context;
        let key = lstate.as_typed_key();
        let t1 = std::time::Instant::now();
        println!(
            "Took {:?} to init {size} entries, with {} allocated",
            t1 - t0,
            cur_alloc()
        );
        //println!("{:?}", tx.application_cost(&lstate.parameters.cost_model));
        lstate.persist();
        drop(lstate);
        drop(state);
        default_storage::<TestDb>().with_backend(|b| {
            b.flush_all_changes_to_db();
            b.flush_cache_evictions_to_db();
            b.gc();
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
        let state = default_storage::<TestDb>().get_lazy(&key).unwrap();
        state.unpersist();
        drop(state);
        drop(key);
        drop(vtx);
        drop(tx);
        println!("After dropping all data, {} remains allocated", cur_alloc());
        unsafe_drop_default_storage::<TestDb>();
        println!(
            "After dropped DB and all caches, {} remains allocated",
            cur_alloc()
        );
    }
    group.finish();
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
criterion_group!(night, night_transfer_by_utxo_set_size);
#[cfg(feature = "proving")]
criterion_main!(benchmarking, system_tx, night);
#[cfg(not(feature = "proving"))]
criterion_main!(system_tx, night);
