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

use base_crypto::cost_model::{CostDuration, RunningCost};
use base_crypto::fab::AlignedValue;
use coin_structure_v9::coin::TokenType;
use coin_structure_v9::contract::ContractAddress;
use onchain_state_v9::state::{
    ChargedState, ContractMaintenanceAuthority, ContractOperation, ContractState, StateValue,
};
use proptest::prelude::*;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use serialize::{Deserializable, Serializable, Tagged, tagged_deserialize, tagged_serialize};
use std::ops::Deref;
use storage_v9::arena::Sp;
use storage_v9::db::InMemoryDB;
use storage_v9::storage::{Array, HashMap};
use storage_v10::state_translation::{TranslationTable, TypedTranslationState};
use v9_to_v10_state_translation::StateTranslationTable;

use coin_structure_v10 as cs10;
use onchain_state_v10::state as os10;

type LedgerV9 = ledger_v9::structure::LedgerState<InMemoryDB>;
type LedgerV10 = ledger_v10::structure::LedgerState<InMemoryDB>;

const TEST_NETWORK_ID: &str = "test-network";
const ENTRY_POINTS: [&[u8]; 3] = [b"operationA", b"operationB", b"operationC"];

#[test]
fn contract_operations_keep_their_keys() {
    let mut rng = StdRng::seed_from_u64(0x42);
    let mut v9 = LedgerV9::new(TEST_NETWORK_ID);
    let contract = create_test_contract(&mut rng);
    let addr: ContractAddress = rng.r#gen();
    v9.contract = v9.contract.insert(addr, contract.clone());

    let v10 = translate_to_completion(v9);

    let translated = v10
        .contract
        .get(&v10_contract_addr(&addr))
        .expect("contract should exist after translation");
    assert_eq!(translated.operations.size(), ENTRY_POINTS.len());
    for ep in ENTRY_POINTS {
        let old = contract.operations.get(&ep.into()).expect("v9 operation");
        let new = translated
            .operations
            .get(&ep.into())
            .expect("v10 operation");
        assert_eq!(new.v2, old.v2);
        assert_eq!(new.v3, old.v3);
        assert!(new.v3.is_some());
        assert!(new.v4.is_none(), "v9 keys must not be promoted to v4");
        assert_eq!(
            new.ir.as_ref().map(|ir| ir.hash()),
            old.ir.as_ref().map(|ir| ir.hash())
        );
    }
}

#[test]
fn contract_balance_and_authority_preserved() {
    let mut rng = StdRng::seed_from_u64(0x42);
    let mut v9 = LedgerV9::new(TEST_NETWORK_ID);
    let mut contract = create_test_contract(&mut rng);
    contract.balance = contract.balance.insert(TokenType::Dust, 1_000_000);
    contract.maintenance_authority = ContractMaintenanceAuthority {
        committee: vec![
            onchain_state_v9::state::ContractMaintenanceVerifyingKey::Schnorr(rng.r#gen()),
        ],
        threshold: 1,
        counter: 3,
    };
    let addr: ContractAddress = rng.r#gen();
    v9.contract = v9.contract.insert(addr, contract.clone());

    let v10 = translate_to_completion(v9);

    let translated = v10
        .contract
        .get(&v10_contract_addr(&addr))
        .expect("contract should exist");
    assert_eq!(
        translated
            .balance
            .get(&cs10::coin::TokenType::Dust)
            .map(|b| *b),
        Some(1_000_000),
    );
    assert_eq!(translated.maintenance_authority.threshold, 1);
    assert_eq!(translated.maintenance_authority.counter, 3);
    assert_eq!(
        bytes(&translated.maintenance_authority),
        bytes(&contract.maintenance_authority),
    );
    assert_eq!(translated.data.state.hash(), contract.data.state.hash());
}

/// Storage 3 leaves a short extension chain in `rc_0` when two charged roots
/// inline more than 128 bytes and diverge early. Ledger 10 must not see it.
#[test]
fn short_chained_rc_0_is_canonicalised() {
    let contract = contract_with_short_chained_rc_0();
    let rc_0 = &contract.data.charged_keys.rc_0;
    assert!(
        reencode::<_, os10::ChargedState<InMemoryDB>>(&contract.data).is_err(),
        "fixture must hold a short extension chain",
    );

    let mut rng = StdRng::seed_from_u64(0x42);
    let addr: ContractAddress = rng.r#gen();
    let mut v9 = LedgerV9::new(TEST_NETWORK_ID);
    v9.contract = v9.contract.insert(addr, contract.clone());
    let v10 = translate_to_completion(v9);
    let translated = v10
        .contract
        .get(&v10_contract_addr(&addr))
        .expect("contract should exist");
    let new_rc_0 = &translated.data.charged_keys.rc_0;

    assert!(new_rc_0.mpt.is_canonical());
    reencode::<_, os10::ContractState<InMemoryDB>>(translated)
        .expect("translated contract must decode under ledger 10");
    // Canonical form is determined by contents, so the rebuilt trie must be
    // the one storage 4 builds from the same entries.
    let rebuilt = rc_0.iter().fold(
        storage_v10::storage::Map::<_, _, InMemoryDB>::new(),
        |m, (k, v)| {
            m.insert(
                k,
                storage_v10::delta_tracking::ChildRef::new(v.child.clone()),
            )
        },
    );
    assert_eq!(new_rc_0.mpt.hash(), rebuilt.mpt.hash());
    // Contract state keeps its hashes, so the charged keys stay accurate.
    assert_eq!(translated.data.state.hash(), contract.data.state.hash());
    assert_eq!(
        translated.data.charged_keys.rc_ge_1.mpt.hash(),
        contract.data.charged_keys.rc_ge_1.mpt.hash(),
    );
}

#[test]
fn incremental_translation_takes_several_runs() {
    let mut rng = StdRng::seed_from_u64(0x42);
    let mut v9 = LedgerV9::new(TEST_NETWORK_ID);
    let mut addresses = Vec::new();
    for _ in 0..50 {
        let addr: ContractAddress = rng.r#gen();
        addresses.push(addr);
        v9.contract = v9.contract.insert(addr, create_test_contract(&mut rng));
    }
    let short_chained: ContractAddress = rng.r#gen();
    v9.contract = v9
        .contract
        .insert(short_chained, contract_with_short_chained_rc_0());

    let mut tl =
        TypedTranslationState::<LedgerV9, LedgerV10, StateTranslationTable, InMemoryDB>::start(
            Sp::new(v9),
        )
        .expect("start translation");
    let mut runs = 0;
    let v10 = loop {
        runs += 1;
        assert!(runs <= 1000, "translation did not complete");
        tl = tl
            .run(CostDuration::from_picoseconds(1_000_000_000))
            .expect("translation step");
        if let Some(res) = tl.result().expect("result") {
            break res.deref().clone();
        }
    };

    assert!(runs > 1, "expected several runs, got {runs}");
    for addr in addresses.iter().chain([&short_chained]) {
        let c = v10
            .contract
            .get(&v10_contract_addr(addr))
            .expect("contract should exist");
        assert!(c.data.charged_keys.rc_0.mpt.is_canonical());
    }
}

#[test]
fn translated_state_round_trips_under_v10() {
    let mut rng = StdRng::seed_from_u64(0xabc);
    let mut params = ledger_v9::structure::INITIAL_PARAMETERS;
    params.global_ttl = base_crypto::time::Duration::from_secs(3600);
    let treasury = 1_000_000u128;
    let mut v9 = LedgerV9::with_genesis_settings(
        TEST_NETWORK_ID,
        params,
        0,
        ledger_v9::structure::MAX_SUPPLY - treasury,
        treasury,
    )
    .expect("v9 genesis state should be valid");
    let bridge = 500_000u128;
    v9.reserve_pool -= bridge;
    let bridge_addr = coin_structure_v9::coin::UserAddress::from(
        rng.r#gen::<base_crypto::signatures::VerifyingKey>(),
    );
    v9.bridge_receiving = v9.bridge_receiving.insert(bridge_addr, bridge);
    for _ in 0..3 {
        let addr: ContractAddress = rng.r#gen();
        v9.contract = v9.contract.insert(addr, create_test_contract(&mut rng));
    }
    v9.contract = v9
        .contract
        .insert(rng.r#gen(), contract_with_short_chained_rc_0());

    let v10 = translate_to_completion(v9);

    v10.check_night_balance_invariant()
        .expect("translated state must keep the NIGHT balance invariant");
    assert_eq!(
        v10.bridge_receiving
            .get(&cs10::coin::UserAddress(bridge_addr.0))
            .copied(),
        Some(bridge),
    );
    assert_eq!(
        v10.parameters.global_ttl,
        base_crypto::time::Duration::from_secs(3600)
    );
    let round_tripped: LedgerV10 =
        reencode(&v10).expect("translated state must decode under ledger 10");
    round_tripped
        .check_night_balance_invariant()
        .expect("NIGHT balance invariant after round trip");
}

#[test]
fn table_is_closed() {
    <StateTranslationTable as TranslationTable<InMemoryDB>>::assert_closure();
}

#[test]
fn table_tags_match_types() {
    use storage_v9::delta_tracking::ChildRef as ChildRefV9;
    use storage_v9::merkle_patricia_trie::{MerklePatriciaTrie as MptV9, Node as NodeV9};
    use storage_v10::delta_tracking::ChildRef as ChildRefV10;
    use storage_v10::merkle_patricia_trie::{MerklePatriciaTrie as MptV10, Node as NodeV10};

    type AnnV9 = ledger_v9::annotation::NightAnn;
    type AnnV10 = ledger_v10::annotation::NightAnn;
    type SizeV9 = storage_v9::storable::SizeAnn;
    type SizeV10 = storage_v10::storable::SizeAnn;
    type CsV9 = ContractState<InMemoryDB>;
    type CsV10 = os10::ContractState<InMemoryDB>;
    type RefV9 = ChildRefV9<InMemoryDB>;
    type RefV10 = ChildRefV10<InMemoryDB>;

    let expected = vec![
        (LedgerV9::tag(), LedgerV10::tag()),
        (CsV9::tag(), CsV10::tag()),
        (
            MptV9::<CsV9, InMemoryDB, AnnV9>::tag(),
            MptV10::<CsV10, InMemoryDB, AnnV10>::tag(),
        ),
        (
            NodeV9::<CsV9, InMemoryDB, AnnV9>::tag(),
            NodeV10::<CsV10, InMemoryDB, AnnV10>::tag(),
        ),
        (RefV9::tag(), RefV10::tag()),
        (
            MptV9::<RefV9, InMemoryDB, SizeV9>::tag(),
            MptV10::<RefV10, InMemoryDB, SizeV10>::tag(),
        ),
        (
            NodeV9::<RefV9, InMemoryDB, SizeV9>::tag(),
            NodeV10::<RefV10, InMemoryDB, SizeV10>::tag(),
        ),
    ];
    let actual: Vec<_> = <StateTranslationTable as TranslationTable<InMemoryDB>>::TABLE
        .iter()
        .map(|(id, _)| (id.0.clone(), id.1.clone()))
        .collect();
    assert_eq!(actual, expected);
}

// ---------- Evidence for recasting contract state ----------
//
// The translation recasts every trie but `rc_0`. A short chain needs a
// 255-nibble extension with an extension below it, so a key longer than 255
// nibbles. The first property shows storage 3's mutations never leave one
// otherwise, even under adversarial shared prefixes. The second shows it end
// to end for `StateValue`, whose maps are keyed by 32-byte hashes and whose
// arrays by small indices, however long the keys the contract uses.

/// A v9 `StateValue`, as the VM builds it: inserts, then removals.
#[derive(Debug, Clone)]
enum Shape {
    Null,
    Cell(Vec<u8>),
    Map(Vec<(Vec<u8>, Shape)>, Vec<prop::sample::Index>),
    Array(Vec<Shape>, Vec<(prop::sample::Index, Shape)>),
}

fn arb_bytes() -> impl Strategy<Value = Vec<u8>> {
    prop::collection::vec(any::<u8>(), 0..300)
}

fn arb_shape() -> impl Strategy<Value = Shape> {
    let leaf = prop_oneof![Just(Shape::Null), arb_bytes().prop_map(Shape::Cell)];
    leaf.prop_recursive(3, 96, 32, |inner| {
        prop_oneof![
            (
                prop::collection::vec((arb_bytes(), inner.clone()), 0..32),
                prop::collection::vec(any::<prop::sample::Index>(), 0..8),
            )
                .prop_map(|(ins, rem)| Shape::Map(ins, rem)),
            (
                prop::collection::vec(inner.clone(), 1..=16),
                prop::collection::vec((any::<prop::sample::Index>(), inner), 0..4),
            )
                .prop_map(|(init, upd)| Shape::Array(init, upd)),
        ]
    })
}

fn build(shape: &Shape) -> StateValue<InMemoryDB> {
    match shape {
        Shape::Null => StateValue::Null,
        Shape::Cell(bytes) => StateValue::Cell(Sp::new(AlignedValue::from(bytes.clone()))),
        Shape::Map(ins, rem) => {
            let map = ins.iter().fold(HashMap::new(), |m, (k, v)| {
                m.insert(AlignedValue::from(k.clone()), build(v))
            });
            let map = rem.iter().fold(map, |m, i| {
                if ins.is_empty() {
                    m
                } else {
                    m.remove(&AlignedValue::from(i.get(ins).0.clone()))
                }
            });
            StateValue::Map(map)
        }
        Shape::Array(init, upd) => {
            let values = init.iter().map(build).collect::<Vec<_>>();
            let arr = upd
                .iter()
                .fold(Array::new_from_slice(&values), |a, (i, v)| {
                    a.insert(i.index(init.len()), build(v))
                        .expect("index is in bounds")
                });
            StateValue::Array(arr)
        }
    }
}

type Op = (prop::sample::Index, Vec<u8>, bool);

/// Fixed-length keys, as `Map` keys are prefix-free; storage 3's `remove`
/// panics on a path ending at a branch.
fn arb_short_key_ops() -> impl Strategy<Value = (usize, Vec<Vec<u8>>, Vec<Op>)> {
    (
        1usize..=255,
        prop::collection::vec(prop::collection::vec(0u8..16, 0..=255), 1..4),
        prop::collection::vec(
            (
                any::<prop::sample::Index>(),
                prop::collection::vec(0u8..2, 0..8),
                any::<bool>(),
            ),
            0..48,
        ),
    )
}

proptest! {
    #[test]
    fn short_keys_never_chain((len, prefixes, ops) in arb_short_key_ops()) {
        let trie = ops.iter().fold(
            storage_v9::merkle_patricia_trie::MerklePatriciaTrie::<u8, InMemoryDB>::new(),
            |t, (i, suffix, insert)| {
                let mut path = i.get(&prefixes).clone();
                path.extend(suffix);
                path.resize(len, 0);
                if *insert { t.insert(&path, 0) } else { t.remove(&path) }
            },
        );
        let decoded = reencode::<_, storage_v10::merkle_patricia_trie::MerklePatriciaTrie<u8, InMemoryDB>>(&trie);
        prop_assert!(decoded.is_ok(), "{:?}", decoded.err());
    }

    #[test]
    fn state_value_tries_stay_canonical(shape in arb_shape()) {
        let value = build(&shape);
        let decoded = reencode::<_, os10::StateValue<InMemoryDB>>(&value);
        prop_assert!(decoded.is_ok(), "{:?}", decoded.err());
    }
}

// ---------- Helpers ----------

fn v10_contract_addr(a: &ContractAddress) -> cs10::contract::ContractAddress {
    cs10::contract::ContractAddress(a.0)
}

fn bytes<T: Serializable>(value: &T) -> Vec<u8> {
    let mut buf = Vec::new();
    value.serialize(&mut buf).expect("serialize");
    buf
}

/// Decodes `value`'s encoding as `B` through the checking wire loader, which
/// rejects non-canonical tries, and requires the bytes to be unchanged.
fn reencode<A: Serializable + Tagged, B: Serializable + Deserializable + Tagged>(
    value: &A,
) -> std::io::Result<B> {
    let mut buf = Vec::new();
    tagged_serialize(value, &mut buf)?;
    let decoded: B = tagged_deserialize(&mut &buf[..])?;
    let mut again = Vec::new();
    tagged_serialize(&decoded, &mut again)?;
    assert_eq!(A::tag(), B::tag());
    assert_eq!(buf, again, "re-encoding must not change the bytes");
    Ok(decoded)
}

fn translate_to_completion(v9: LedgerV9) -> LedgerV10 {
    TypedTranslationState::<LedgerV9, LedgerV10, StateTranslationTable, InMemoryDB>::start(Sp::new(
        v9,
    ))
    .expect("start translation")
    .run(CostDuration::from_picoseconds(1_000_000_000_000))
    .expect("translation failed")
    .result()
    .expect("result")
    .expect("translation did not complete")
    .deref()
    .clone()
}

fn create_test_contract(rng: &mut StdRng) -> ContractState<InMemoryDB> {
    let ops = ENTRY_POINTS.iter().fold(HashMap::new(), |ops, ep| {
        let mut op = ContractOperation::new(Some(rng.r#gen()), None);
        op.v2 = Some(rng.r#gen());
        ops.insert((*ep).into(), op)
    });
    ContractState::new(
        StateValue::Cell(Sp::new(AlignedValue::from(vec![7u8; 64]))),
        ops,
        Default::default(),
    )
}

/// Two charged roots, each a cell inlining 200 bytes and diverging early, put
/// two long `Direct` keys into `rc_0`. Without garbage collection both stay.
/// The bytes are non-zero because `AlignedValue` drops zero bytes.
fn contract_with_short_chained_rc_0() -> ContractState<InMemoryDB> {
    let cell = |b: u8| {
        let mut bytes = vec![0xaau8; 200];
        bytes[4] = b;
        StateValue::Cell(Sp::new(AlignedValue::from(bytes)))
    };
    let (data, _) = ChargedState::new(cell(1)).update(cell(2), |_, _| RunningCost::ZERO, |_| 0);
    let mut contract = ContractState::new(StateValue::Null, HashMap::new(), Default::default());
    contract.data = data;
    contract
}
