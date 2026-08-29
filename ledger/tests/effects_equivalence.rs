// This file is part of midnight-ledger.
// Copyright (C) Midnight Foundation
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

//! Whole-transaction equivalence, on the **success** path.
//!
//! Every earlier gate compared one rule family against its own primitive, and the
//! end-to-end guest measurement only ever ran against an *empty* state, where `try_apply`
//! rejects before doing the work. Neither shows the property the transport actually claims:
//! that for a transaction the effects fully represent, applying the effects reaches the same
//! ledger as applying the transaction.
//!
//! So this funds a state, spends the UTXO it created, and applies the result both ways from
//! the same starting ledger. The comparison is `state_hash()` — the ledger's own notion of
//! "the same state", the one `TestState::apply` already uses to check determinism, and the
//! only one that cannot be fooled by an assertion that is blind to a field.

#![cfg(feature = "proving")]

use base_crypto::hash::HashOutput;
use base_crypto::time::Duration;
use coin_structure::coin::{UnshieldedTokenType, UserAddress};
use midnight_ledger::effects::TransactionEffects;
use midnight_ledger::semantics::{ErasedTransactionResult, TransactionResult};
use midnight_ledger::structure::{
    Intent, LedgerState, StandardTransaction, Transaction, UnshieldedOffer, UtxoOutput, UtxoSpend,
};
use midnight_ledger::test_utilities::TestState;
use midnight_ledger::verify::WellFormedStrictness;
use rand::{SeedableRng, rngs::StdRng};
use storage::arena::Sp;
use storage::db::InMemoryDB;
use storage::storage::HashMap;

/// A token that is not NIGHT, so funding needs no proving and no balancing.
const TY: UnshieldedTokenType = UnshieldedTokenType(HashOutput([8; 32]));

#[tokio::test]
async fn the_effects_path_and_the_full_path_reach_the_same_ledger() {
    let mut rng = StdRng::seed_from_u64(0x5eed);
    let mut state = TestState::<InMemoryDB>::new(&mut rng);

    // ── fund, so the apply below is a *success* rather than a rejection ─────────────────
    state.rewards_unshielded(&mut rng, TY, 1_000).await;

    let owner = state.night_key.verifying_key();
    let utxo = state
        .ledger
        .utxo
        .utxos
        .iter()
        .find(|u| u.0.type_ == TY && u.0.value == 1_000)
        .expect("funding must have created the UTXO this test spends")
        .0
        .clone();

    // ── a transaction the effects fully represent: one unshielded spend, no contracts ───
    let offer: UnshieldedOffer<(), InMemoryDB> = UnshieldedOffer {
        inputs: vec![UtxoSpend {
            intent_hash: utxo.intent_hash,
            output_no: utxo.output_no,
            owner: owner.clone(),
            type_: TY,
            value: 1_000,
        }]
        .into(),
        outputs: vec![UtxoOutput {
            owner: UserAddress::from(owner),
            type_: TY,
            value: 1_000,
        }]
        .into(),
        signatures: vec![].into(),
    };
    let mut intent = Intent::empty(&mut rng, state.time + Duration::from_secs(3600));
    intent.guaranteed_unshielded_offer = Some(Sp::new(offer));
    let intent = intent
        .sign(&mut rng, 1, &[state.night_key.clone()], &[], &[])
        .expect("signing the spend must succeed");
    let tx = Transaction::Standard(StandardTransaction::new(
        "local-test",
        HashMap::new().insert(1u16, intent),
        None,
        HashMap::new(),
    ));

    let mut strictness = WellFormedStrictness::default();
    strictness.enforce_balancing = false;
    let context = state.context();
    let vtx = tx
        .well_formed(&state.ledger, strictness, state.time)
        .expect("the transaction must be well formed");

    // ── the two paths, from the same starting ledger ────────────────────────────────────
    let (full, result) = state.ledger.apply(&vtx, &context);
    assert!(
        matches!(result, TransactionResult::Success(_)),
        "the full path must SUCCEED, or this compares two rejections and proves nothing: {:?}",
        ErasedTransactionResult::from(&result)
    );

    let fx = vtx.effects().expect("a standard transaction has effects");
    assert!(
        fx.guaranteed
            .intents
            .iter()
            .any(|i| !i.unshielded.spends.is_empty() && !i.unshielded.creates.is_empty()),
        "the projection must carry both a spend and a create, or the comparison below is vacuous"
    );

    let via_effects = state
        .ledger
        .apply_effects(&fx, &context)
        .expect("effects that represent the whole transaction must apply");

    assert_same_ledger(&full, &via_effects);
    assert_ne!(
        state.ledger.state_hash(),
        full.state_hash(),
        "if applying changed nothing, equal hashes would be equality of two no-ops"
    );

    // ── and the replay gate is armed on both, not just the full path ────────────────────
    assert!(
        via_effects.apply_effects(&fx, &context).is_err(),
        "applying the same effects twice must be refused by replay protection"
    );
}

/// The starting ledger must not move under either path.
///
/// `apply` and `apply_effects` both take `&self` and return a new state. A stray `Sp` mutation
/// would make the second application above start from a state the first had already advanced,
/// which reads as a pass.
#[tokio::test]
async fn neither_path_mutates_the_ledger_it_was_given() {
    let mut rng = StdRng::seed_from_u64(0x5eed);
    let mut state = TestState::<InMemoryDB>::new(&mut rng);
    state.rewards_unshielded(&mut rng, TY, 1_000).await;

    let before = state.ledger.state_hash();
    let fx = TransactionEffects {
        guaranteed: Default::default(),
        fallible: Vec::new(),
    };
    let _ = state.ledger.apply_effects(&fx, &state.context());
    assert_eq!(
        before,
        state.ledger.state_hash(),
        "apply_effects moved the state it was handed"
    );
}

/// Compare two ledgers, naming the component that diverged.
///
/// `state_hash()` alone answers "different" without saying where, and the first failure this
/// file produced was a one-field divergence inside `utxo` — the created utxo's `intent_hash`.
/// Printing the component first turns that from a bisect into a read.
fn assert_same_ledger(full: &LedgerState<InMemoryDB>, fx: &LedgerState<InMemoryDB>) {
    let parts: [(&str, String, String); 7] = [
        ("utxo", format!("{:?}", full.utxo), format!("{:?}", fx.utxo)),
        (
            "replay_protection",
            format!("{:?}", full.replay_protection),
            format!("{:?}", fx.replay_protection),
        ),
        (
            "zswap",
            format!("{:?}", full.zswap),
            format!("{:?}", fx.zswap),
        ),
        ("dust", format!("{:?}", full.dust), format!("{:?}", fx.dust)),
        (
            "contract",
            format!("{:?}", full.contract),
            format!("{:?}", fx.contract),
        ),
        (
            "treasury",
            format!("{:?}", full.treasury),
            format!("{:?}", fx.treasury),
        ),
        (
            "pools",
            format!(
                "{},{},{}",
                full.locked_pool, full.reserve_pool, full.block_reward_pool
            ),
            format!(
                "{},{},{}",
                fx.locked_pool, fx.reserve_pool, fx.block_reward_pool
            ),
        ),
    ];
    for (name, a, b) in &parts {
        assert_eq!(
            a, b,
            "the effects path diverged from the full path in `{name}`"
        );
    }
    assert_eq!(
        full.state_hash(),
        fx.state_hash(),
        "the two ledgers hash differently despite every component above comparing equal"
    );
}

/// The same comparison for a **fallible** offer, where the two intent hashes genuinely differ.
///
/// ⌖ This is the half the guaranteed case cannot see. There the creating hash and the replay
/// hash coincide, both being `intent_hash(0)`, so a wrong choice between them is invisible.
/// A fallible segment applies at its own segment, so the projection must use
/// `intent_hash(segment)` there and `intent_hash(0)` in the guaranteed pass — two different
/// answers from one function, and only this test pins the second one.
#[tokio::test]
async fn a_fallible_offer_creates_under_the_segments_own_hash() {
    let mut rng = StdRng::seed_from_u64(0x5eed);
    let mut state = TestState::<InMemoryDB>::new(&mut rng);
    state.rewards_unshielded(&mut rng, TY, 1_000).await;

    let owner = state.night_key.verifying_key();
    let utxo = state
        .ledger
        .utxo
        .utxos
        .iter()
        .find(|u| u.0.type_ == TY && u.0.value == 1_000)
        .expect("funding must have created the UTXO this test spends")
        .0
        .clone();

    let offer: UnshieldedOffer<(), InMemoryDB> = UnshieldedOffer {
        inputs: vec![UtxoSpend {
            intent_hash: utxo.intent_hash,
            output_no: utxo.output_no,
            owner: owner.clone(),
            type_: TY,
            value: 1_000,
        }]
        .into(),
        outputs: vec![UtxoOutput {
            owner: UserAddress::from(owner),
            type_: TY,
            value: 1_000,
        }]
        .into(),
        signatures: vec![].into(),
    };
    let mut intent = Intent::empty(&mut rng, state.time + Duration::from_secs(3600));
    intent.fallible_unshielded_offer = Some(Sp::new(offer));
    let intent = intent
        .sign(&mut rng, 1, &[], &[state.night_key.clone()], &[])
        .expect("signing the spend must succeed");

    // ⚠︎ Both erasures, matching `semantics.rs:1109`. `erase_proofs()` alone hashes to a
    // different value, and the assertion below then fails against a hash nothing produces.
    let erased = intent.erase_proofs().erase_signatures();
    let hash_by_segment = erased.intent_hash(1);
    let hash_by_zero = erased.intent_hash(0);
    assert_ne!(
        hash_by_segment, hash_by_zero,
        "if these matched, the whole point of this test would be unobservable"
    );

    let tx = Transaction::Standard(StandardTransaction::new(
        "local-test",
        HashMap::new().insert(1u16, intent),
        None,
        HashMap::new(),
    ));
    let mut strictness = WellFormedStrictness::default();
    strictness.enforce_balancing = false;
    let context = state.context();
    let vtx = tx
        .well_formed(&state.ledger, strictness, state.time)
        .expect("the transaction must be well formed");

    let (full, result) = state.ledger.apply(&vtx, &context);
    assert!(
        matches!(result, TransactionResult::Success(_)),
        "the full path must SUCCEED, or this compares two rejections: {:?}",
        ErasedTransactionResult::from(&result)
    );

    let fx = vtx.effects().expect("a standard transaction has effects");
    let seg = fx
        .fallible
        .iter()
        .find(|s| s.segment == 1)
        .expect("the projection must carry segment 1");
    let created = &seg
        .unshielded
        .as_ref()
        .expect("segment 1 has an unshielded offer")
        .creates;
    assert_eq!(created.len(), 1, "fixture must create exactly one utxo");
    assert_eq!(
        created[0].intent_hash, hash_by_segment,
        "a fallible offer creates under its own segment's hash, not intent_hash(0)"
    );

    let via_effects = state
        .ledger
        .apply_effects(&fx, &context)
        .expect("effects that represent the whole transaction must apply");
    assert_same_ledger(&full, &via_effects);
}
