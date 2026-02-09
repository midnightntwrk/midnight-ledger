#![deny(warnings)]
use base_crypto::rng::SplittableRng;
use base_crypto::signatures::{Signature, SigningKey};
use base_crypto::time::Timestamp;
use coin_structure::coin::{NIGHT, UserAddress};
use criterion::{BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
use lazy_static::lazy_static;
use midnight_ledger::dust::{DustPublicKey, InitialNonce};
use midnight_ledger::prove::Resolver;
use midnight_ledger::semantics::TransactionContext;
use midnight_ledger::structure::{
    CNightGeneratesDustActionType, CNightGeneratesDustEvent, Intent, LedgerState, OutputInstructionUnshielded, ProofMarker, SystemTransaction, UnshieldedOffer, Utxo, UtxoMeta, UtxoOutput, UtxoSpend, UtxoState
};
use midnight_ledger::structure::{ClaimKind, Transaction};
use midnight_ledger::test_utilities::{test_resolver, well_formed_tx_builder};
use midnight_ledger::verify::WellFormedStrictness;
use onchain_runtime::context::BlockContext;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use storage::storage::default_storage;
use storage::arena::Sp;
use storage::db::InMemoryDB;
use transient_crypto::commitment::PedersenRandomness;
use zswap::keys::SecretKeys;
use std::ops::Deref;

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

pub fn night_transfer_by_utxo_set_size(c: &mut Criterion) {
    let mut rng = StdRng::seed_from_u64(0x42);
    let mut group = c.benchmark_group("night-transfer-by-utxo-set-size");
    let sk = SigningKey::sample(&mut rng);
    let vk = sk.verifying_key();
    let addr = UserAddress::from(vk.clone());
    for log_size in 0..=20 {
        let size = 2u64.pow(log_size);
        let mut state = LedgerState::<InMemoryDB>::new("local-test");
        let utxos = (0..size).fold(state.utxo.utxos.clone(), |state, _| state.insert(Utxo { value: rng.r#gen(), owner: rng.r#gen(), type_: rng.r#gen(), intent_hash: rng.r#gen(), output_no: 0 }, UtxoMeta { ctime: rng.r#gen() }));
        let real_utxo = Utxo {
            value: 1_000_000,
            owner: addr,
            type_: NIGHT,
            intent_hash: rng.r#gen(),
            output_no: 0,
        };
        state.utxo = Sp::new(UtxoState { utxos: utxos.insert(real_utxo.clone(), UtxoMeta { ctime: rng.r#gen() }) });
        let mut state = Sp::new(state);
        let offer: UnshieldedOffer<Signature, InMemoryDB> = UnshieldedOffer {
            inputs: vec![UtxoSpend {
                value: real_utxo.value,
                owner: vk.clone(),
                type_: NIGHT,
                intent_hash: real_utxo.intent_hash,
                output_no: 0,
            }].into(),
            outputs: vec![UtxoOutput {
                value: 100,
                owner: addr,
                type_: NIGHT,
            }, UtxoOutput {
                value: real_utxo.value - 100,
                owner: addr,
                type_: NIGHT,
            }].into(),
            signatures: vec![].into(),
        };
        let intent = Intent::new(&mut rng, Some(offer), None, vec![], vec![], vec![], None, Timestamp::from_secs(10));
        let tx = Transaction::from_intents("local-test", [(1, intent)].into_iter().collect());
        let mut strictness = WellFormedStrictness::default();
        strictness.enforce_balancing = false;
        let vtx = tx.well_formed(&*state, strictness, Timestamp::from_secs(0)).unwrap();
        let context = TransactionContext {
            ref_state: state.deref().clone(),
            block_context: BlockContext::default(),
            whitelist: None,
        };
        let key = state.as_typed_key();
        state.persist();
        drop(state);
        group.bench_with_input(BenchmarkId::from_parameter(log_size), &log_size, |b, _| {
            b.iter(|| {
                default_storage().get_lazy(&key).unwrap().apply(&vtx, &context)
            });
        });
        let state = default_storage().get_lazy(&key).unwrap();
        state.unpersist();
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

criterion_group!(
    name = benchmarking;
    config = Criterion::default().sample_size(10);
    targets = transaction_validation
);
criterion_group!(system_tx, rewards, create_dust, create_and_destroy_dust, night_transfer_by_utxo_set_size);
criterion_main!(benchmarking, system_tx);
