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

//! Semantic property tests for the ledger transaction model, exercising the
//! validate -> apply -> UTXO-mutation path and byte round-trips.
//!
//! Coverage (all active): `apply` conserves unshielded token supply for pure
//! transfers; a validated transfer applies and cannot be replayed; an expired
//! intent is rejected at apply (TTL); and `ContractCall`/`DustActions` structures
//! survive a tagged serialize/deserialize round-trip. These target *semantic*
//! invariants -- the class that would have caught GHSA-vhp6-px6f-jv94, which
//! survived the byte-level serialization barrier.

use base_crypto::hash::HashOutput;
use base_crypto::time::{Duration, Timestamp};
use coin_structure::coin::{UnshieldedTokenType, UserAddress};
use midnight_ledger::structure::{
    Intent, ProofPreimageMarker, Transaction, UnshieldedOffer, UtxoOutput, UtxoSpend,
};
use rand::{Rng, SeedableRng, rngs::StdRng};
use storage::arena::Sp;
use storage::db::InMemoryDB;
use storage::storage::HashMap;
use transient_crypto::commitment::PedersenRandomness;

type Db = InMemoryDB;


/// **`apply` conserves unshielded token supply for pure transfers.**
///
/// This is the first property to exercise the full validate -> apply -> UTXO-set
/// mutation path (nothing else fuzzes `apply`). Unlike NIGHT, non-NIGHT unshielded
/// tokens have no built-in conservation invariant — the very gap that let
/// GHSA-vhp6 mint tokens — so this external oracle is the durable regression guard:
/// a transaction that only *moves* an existing UTXO must leave each token's total
/// supply unchanged.
///
/// Each case seeds one UTXO of a random non-NIGHT token, then generates a random
/// single-input transfer spending it into outputs that sum to the same amount,
/// signs it, and applies it. On success, the token's total supply must be
/// unchanged. Any future bug that mints or burns unshielded value on the transfer
/// path (a new GHSA-vhp6-style divergence) breaks this.
#[tokio::test]
async fn prop_apply_conserves_unshielded_supply() {
    use midnight_ledger::semantics::TransactionResult;
    use base_crypto::schnorr::Signature;
    use midnight_ledger::test_utilities::TestState;
    use midnight_ledger::verify::WellFormedStrictness;

    fn supply(state: &TestState<Db>, token: UnshieldedTokenType) -> u128 {
        state
            .ledger
            .utxo
            .utxos
            .iter()
            .map(|kv| (*kv.0).clone())
            .filter(|u| u.type_ == token)
            .map(|u| u.value)
            .sum()
    }

    // Split `total` into `k` positive parts (each >= 1) summing to `total`.
    fn split_value(rng: &mut StdRng, total: u128, k: usize) -> Vec<u128> {
        let k = k.min(total as usize).max(1);
        let mut parts = vec![1u128; k];
        let mut rem = total - k as u128;
        while rem > 0 {
            let i = rng.gen_range(0..k);
            let add = rng.gen_range(1..=rem);
            parts[i] += add;
            rem -= add;
        }
        parts
    }

    let mut rng = StdRng::seed_from_u64(0xC0FFEE);
    let mut failures: Vec<String> = Vec::new();
    let mut exercised = 0u32;
    // Track the richest generated shape actually applied, to prove breadth.
    let mut max_inputs = 0usize;
    let mut multi_token_applied = false;

    for case in 0..24u64 {
        let mut state = TestState::<Db>::new(&mut rng);

        // Seed several UTXOs across a few distinct non-NIGHT tokens.
        let n_tokens = rng.gen_range(1..=3usize);
        let mut tokens: Vec<UnshieldedTokenType> = Vec::new();
        for _ in 0..n_tokens {
            let token = UnshieldedTokenType(HashOutput(rng.r#gen()));
            if token == coin_structure::coin::NIGHT {
                continue; // NIGHT goes through a different (proving) seeding path
            }
            // 1..=2 separate UTXOs of this token, to reach multi-input spends.
            for _ in 0..rng.gen_range(1..=2usize) {
                let amount: u128 = rng.gen_range(2..1_000_000u128);
                state.rewards_unshielded(&mut rng, token, amount).await;
            }
            tokens.push(token);
        }
        if tokens.is_empty() {
            continue;
        }

        // Snapshot supply of every seeded token before applying.
        let before: Vec<(UnshieldedTokenType, u128)> =
            tokens.iter().map(|t| (*t, supply(&state, *t))).collect();

        // Collect the seeded UTXOs (owned by the night key) and spend a random
        // non-empty subset — a partial spend leaves the rest untouched.
        let mut all_utxos: Vec<_> = state
            .ledger
            .utxo
            .utxos
            .iter()
            .map(|kv| (*kv.0).clone())
            .filter(|u| tokens.contains(&u.type_))
            .collect();
        all_utxos.sort_by_key(|u| (u.value, u.output_no));
        let keep = rng.gen_range(1..=all_utxos.len());
        let chosen: Vec<_> = all_utxos.into_iter().take(keep).collect();

        // Inputs: one spend per chosen UTXO.
        let mut inputs: Vec<UtxoSpend> = chosen
            .iter()
            .map(|u| UtxoSpend {
                value: u.value,
                owner: state.night_key.verifying_key(),
                type_: u.type_,
                intent_hash: u.intent_hash,
                output_no: u.output_no,
            })
            .collect();
        inputs.sort();

        // Outputs: for each spent token, re-emit its full input value split
        // across a random number of outputs to fresh recipients (a pure move).
        let mut outputs: Vec<UtxoOutput> = Vec::new();
        for token in &tokens {
            let token_in: u128 = chosen
                .iter()
                .filter(|u| u.type_ == *token)
                .map(|u| u.value)
                .sum();
            if token_in == 0 {
                continue;
            }
            let k = rng.gen_range(1..=3usize);
            for value in split_value(&mut rng, token_in, k) {
                let recipient = UserAddress::from(
                    base_crypto::schnorr::SigningKey::sample(&mut rng)
                        .verifying_key(),
                );
                outputs.push(UtxoOutput {
                    value,
                    owner: recipient,
                    type_: *token,
                });
            }
        }
        outputs.sort(); // well_formed requires sorted outputs

        let n_inputs = inputs.len();
        let spent_tokens: std::collections::BTreeSet<_> =
            chosen.iter().map(|u| u.type_).collect();

        let offer = UnshieldedOffer::<Signature, Db> {
            inputs: inputs.into(),
            outputs: outputs.into(),
            signatures: vec![].into(),
        };
        let intent: Intent<Signature, ProofPreimageMarker, PedersenRandomness, Db> =
            Intent::new(&mut rng, None, Some(offer), vec![], vec![], vec![], None, state.time);
        let segment = 1u16;
        let intent = intent
            .sign(&mut rng, segment, &[], &[state.night_key.clone()], &[])
            .expect("input owner matches the signing key");
        let tx = Transaction::from_intents("local-test", HashMap::new().insert(segment, intent));

        let mut strictness = WellFormedStrictness::default();
        strictness.enforce_balancing = false; // avoid the orthogonal Dust-fee path

        match state.apply(&tx, strictness) {
            Ok(TransactionResult::Success(_)) => {
                exercised += 1;
                max_inputs = max_inputs.max(n_inputs);
                if spent_tokens.len() > 1 {
                    multi_token_applied = true;
                }
                // Conservation: EVERY seeded token's total supply is unchanged —
                // spent tokens are re-emitted in full, unspent tokens are untouched.
                for (token, before_supply) in &before {
                    let after = supply(&state, *token);
                    if *before_supply != after {
                        failures.push(format!(
                            "case {case}: apply changed total supply {before_supply} -> {after} \
                             for token {token:?} on a pure transfer ({n_inputs} inputs)"
                        ));
                    }
                }
            }
            // Not applied (rejected / partial): nothing to assert about conservation.
            other => {
                if case == 0 {
                    // Surface the shape once for debugging generator quality.
                    eprintln!("case {case}: transfer not applied: {other:?}");
                }
            }
        }
    }

    eprintln!(
        "conservation guard: {exercised} applied; max inputs in one tx = {max_inputs}; \
         multi-token transfer applied = {multi_token_applied}"
    );

    assert!(
        exercised > 0,
        "generator never produced an applicable transfer — property was vacuous"
    );
    assert!(
        failures.is_empty(),
        "apply failed to conserve unshielded supply in {} case(s):\n{}",
        failures.len(),
        failures.join("\n"),
    );
}

/// **`ContractCall` (with real transcripts) round-trips through
/// serialization.**
///
/// Exercises the transcript-generator wiring: the sampled `ContractCall`
/// carries real `Transcript`s, so this covers the
/// contract-action + transcript surface. The oracle is invariant-preservation:
/// tagged serialize/deserialize is the identity on the object graph — including
/// the optional transcripts, their `Effects` maps and (optional) version.
#[test]
fn prop_contract_call_serialization_roundtrips() {
    use midnight_ledger::structure::ContractCall;
    use serialize::{tagged_deserialize, tagged_serialize};

    let mut rng = StdRng::seed_from_u64(0xCA11);
    let mut failures: Vec<String> = Vec::new();
    let mut with_transcript = 0u32;

    for case in 0..128u64 {
        let call: ContractCall<(), Db> = rng.r#gen();
        if call.guaranteed_transcript.is_some() || call.fallible_transcript.is_some() {
            with_transcript += 1;
        }

        let mut bytes = Vec::new();
        tagged_serialize(&call, &mut bytes).expect("serialize");
        let back: ContractCall<(), Db> = tagged_deserialize(&mut &bytes[..]).expect("deserialize");

        if call != back {
            failures.push(format!(
                "case {case}: contract call changed across serialization roundtrip"
            ));
        }
    }

    assert!(
        with_transcript > 0,
        "generator never attached a transcript — the transcript wiring is not exercised"
    );
    assert!(
        failures.is_empty(),
        "contract-call roundtrip failed in {} case(s):\n{}",
        failures.len(),
        failures.join("\n"),
    );
}

/// **`DustActions` round-trips through serialization and preserves its
/// spend/registration multiset.**
///
/// Covers the dust action surface. The oracle is invariant-preservation: after a
/// tagged serialize/deserialize cycle the value is unchanged (so every spend
/// nullifier, registration key and `ctime` is preserved) and the spend/
/// registration counts match — no dust action is silently dropped or duplicated
/// at the serialization boundary (the failure mode of the sparse-`Array` class).
#[test]
fn prop_dust_actions_serialization_roundtrips() {
    use midnight_ledger::dust::{
        DustActions, DustCommitment, DustNullifier, DustPublicKey, DustRegistration, DustSpend,
    };
    use serialize::{tagged_deserialize, tagged_serialize};
    use transient_crypto::curve::Fr;

    let mut rng = StdRng::seed_from_u64(0xD057);
    let mut failures: Vec<String> = Vec::new();
    let mut nonempty = 0u32;

    for case in 0..128u64 {
        let spends: Vec<DustSpend<(), Db>> = (0..rng.gen_range(0..=3))
            .map(|_| DustSpend {
                v_fee: rng.r#gen(),
                old_nullifier: DustNullifier(rng.r#gen::<Fr>()),
                new_commitment: DustCommitment(rng.r#gen::<Fr>()),
                proof: (),
            })
            .collect();
        let registrations: Vec<DustRegistration<(), Db>> = (0..rng.gen_range(0..=2))
            .map(|_| DustRegistration {
                night_key: base_crypto::schnorr::SigningKey::sample(&mut rng).verifying_key(),
                dust_address: if rng.r#gen::<bool>() {
                    Some(Sp::new(DustPublicKey(rng.r#gen::<Fr>())))
                } else {
                    None
                },
                allow_fee_payment: rng.r#gen(),
                signature: None,
            })
            .collect();
        if !spends.is_empty() || !registrations.is_empty() {
            nonempty += 1;
        }
        let (n_spends, n_regs) = (spends.len(), registrations.len());
        let ctime = Timestamp::from_secs(rng.gen_range(0..1_000_000u64));
        let actions = DustActions::<(), (), Db> {
            spends: spends.into(),
            registrations: registrations.into(),
            ctime,
        };

        let mut bytes = Vec::new();
        tagged_serialize(&actions, &mut bytes).expect("serialize");
        let back: DustActions<(), (), Db> =
            tagged_deserialize(&mut &bytes[..]).expect("deserialize");

        if actions != back {
            failures.push(format!(
                "case {case}: dust actions changed across serialization roundtrip"
            ));
        }
        if back.spends.len() != n_spends || back.registrations.len() != n_regs {
            failures.push(format!(
                "case {case}: dust action counts changed ({n_spends} spends / {n_regs} regs \
                 -> {} / {})",
                back.spends.len(),
                back.registrations.len(),
            ));
        }
    }

    assert!(
        nonempty > 0,
        "generator only ever produced empty dust actions — property was near-vacuous"
    );
    assert!(
        failures.is_empty(),
        "dust-action roundtrip failed in {} case(s):\n{}",
        failures.len(),
        failures.join("\n"),
    );
}

/// **validate == apply, and replay protection.**
///
/// Two semantic guards over the full validate -> apply path:
/// 1. *validate == apply*: a transfer that passes `well_formed` also applies to
///    `Success`, and the resulting UTXO-set delta matches the transaction's
///    structure exactly (`after == before - inputs + outputs`). `well_formed`
///    never claims a transfer is valid that `apply` then rejects.
/// 2. *replay protection*: re-applying the identical transaction does not
///    succeed a second time (the inputs are already consumed).
#[tokio::test]
async fn prop_validated_transfer_applies_and_no_replay() {
    use midnight_ledger::semantics::TransactionResult;
    use base_crypto::schnorr::Signature;
    use midnight_ledger::test_utilities::TestState;
    use midnight_ledger::verify::WellFormedStrictness;

    let mut rng = StdRng::seed_from_u64(0x5EED_10AD);
    let mut failures: Vec<String> = Vec::new();
    let mut exercised = 0u32;

    for case in 0..12u64 {
        let mut state = TestState::<Db>::new(&mut rng);
        let token = UnshieldedTokenType(HashOutput(rng.r#gen()));
        if token == coin_structure::coin::NIGHT {
            continue;
        }
        let amount: u128 = rng.gen_range(4..1_000_000u128);
        state.rewards_unshielded(&mut rng, token, amount).await;

        let Some(utxo) = state
            .ledger
            .utxo
            .utxos
            .iter()
            .map(|kv| (*kv.0).clone())
            .find(|u| u.type_ == token)
        else {
            continue;
        };

        // Full spend split into two outputs (net UTXO delta = +1).
        let split = rng.gen_range(1..utxo.value);
        let recipient = UserAddress::from(
            base_crypto::schnorr::SigningKey::sample(&mut rng).verifying_key(),
        );
        let mut outputs = vec![
            UtxoOutput { value: split, owner: recipient, type_: token },
            UtxoOutput { value: utxo.value - split, owner: recipient, type_: token },
        ];
        outputs.sort();
        let n_outputs = outputs.len();

        let offer = UnshieldedOffer::<Signature, Db> {
            inputs: vec![UtxoSpend {
                value: utxo.value,
                owner: state.night_key.verifying_key(),
                type_: token,
                intent_hash: utxo.intent_hash,
                output_no: utxo.output_no,
            }]
            .into(),
            outputs: outputs.into(),
            signatures: vec![].into(),
        };
        let segment = 1u16;
        let intent: Intent<Signature, ProofPreimageMarker, PedersenRandomness, Db> =
            Intent::new(&mut rng, None, Some(offer), vec![], vec![], vec![], None, state.time);
        let intent = intent
            .sign(&mut rng, segment, &[], &[state.night_key.clone()], &[])
            .expect("input owner matches the signing key");
        let tx = Transaction::from_intents("local-test", HashMap::new().insert(segment, intent));

        let mut strictness = WellFormedStrictness::default();
        strictness.enforce_balancing = false;

        // (1) validate: well_formed must accept before we apply.
        if tx.well_formed(&state.ledger, strictness, state.time).is_err() {
            failures.push(format!("case {case}: well_formed rejected a constructed transfer"));
            continue;
        }

        let before = state.ledger.utxo.utxos.iter().count();
        match state.apply(&tx, strictness) {
            Ok(TransactionResult::Success(_)) => {
                exercised += 1;
                // validate == apply: the UTXO-set delta matches the tx structure.
                let after = state.ledger.utxo.utxos.iter().count();
                if after != before - 1 + n_outputs {
                    failures.push(format!(
                        "case {case}: UTXO delta {before} -> {after} does not match \
                         1 input / {n_outputs} outputs"
                    ));
                }
                // (2) replay: the identical tx must not succeed again.
                match state.apply(&tx, strictness) {
                    Ok(TransactionResult::Success(_)) => failures.push(format!(
                        "case {case}: replay of an already-applied transaction succeeded"
                    )),
                    _ => {}
                }
            }
            other => failures.push(format!(
                "case {case}: a well_formed transfer failed to apply: {other:?}"
            )),
        }
    }

    assert!(exercised > 0, "no transfer was applied — property was vacuous");
    assert!(
        failures.is_empty(),
        "validate==apply / replay guard failed in {} case(s):\n{}",
        failures.len(),
        failures.join("\n"),
    );
}

/// **an expired intent is rejected at apply.**
///
/// TTL enforcement: an intent whose `ttl` is strictly before the block time must
/// be refused. We seed and build an otherwise-valid transfer, then advance the
/// ledger clock past the intent's TTL; `apply` (via `well_formed`'s
/// `ttl_check_weak`) must reject it rather than mutate state.
#[tokio::test]
async fn prop_apply_rejects_expired_intent() {
    use midnight_ledger::semantics::TransactionResult;
    use base_crypto::schnorr::Signature;
    use midnight_ledger::test_utilities::TestState;
    use midnight_ledger::verify::WellFormedStrictness;

    let mut rng = StdRng::seed_from_u64(0x7715);
    let mut failures: Vec<String> = Vec::new();
    let mut exercised = 0u32;

    for case in 0..12u64 {
        let mut state = TestState::<Db>::new(&mut rng);
        let token = UnshieldedTokenType(HashOutput(rng.r#gen()));
        if token == coin_structure::coin::NIGHT {
            continue;
        }
        let amount: u128 = rng.gen_range(2..1_000_000u128);
        // Seed at time 0.
        state.rewards_unshielded(&mut rng, token, amount).await;

        let Some(utxo) = state
            .ledger
            .utxo
            .utxos
            .iter()
            .map(|kv| (*kv.0).clone())
            .find(|u| u.type_ == token)
        else {
            continue;
        };

        // Build a transfer whose intent TTL is time 0...
        let ttl = Timestamp::from_secs(0);
        let recipient = UserAddress::from(
            base_crypto::schnorr::SigningKey::sample(&mut rng).verifying_key(),
        );
        let offer = UnshieldedOffer::<Signature, Db> {
            inputs: vec![UtxoSpend {
                value: utxo.value,
                owner: state.night_key.verifying_key(),
                type_: token,
                intent_hash: utxo.intent_hash,
                output_no: utxo.output_no,
            }]
            .into(),
            outputs: vec![UtxoOutput { value: utxo.value, owner: recipient, type_: token }].into(),
            signatures: vec![].into(),
        };
        let segment = 1u16;
        let intent: Intent<Signature, ProofPreimageMarker, PedersenRandomness, Db> =
            Intent::new(&mut rng, None, Some(offer), vec![], vec![], vec![], None, ttl);
        let intent = intent
            .sign(&mut rng, segment, &[], &[state.night_key.clone()], &[])
            .expect("input owner matches the signing key");
        let tx = Transaction::from_intents("local-test", HashMap::new().insert(segment, intent));

        // ...then advance the block clock strictly past the TTL.
        state.time = Timestamp::from_secs(0) + Duration::from_secs(rng.gen_range(1i128..100_000i128));

        let mut strictness = WellFormedStrictness::default();
        strictness.enforce_balancing = false;

        let supply_before = state
            .ledger
            .utxo
            .utxos
            .iter()
            .map(|kv| (*kv.0).clone())
            .filter(|u| u.type_ == token)
            .map(|u| u.value)
            .sum::<u128>();

        exercised += 1;
        match state.apply(&tx, strictness) {
            Err(_) => {}
            // A non-Success result that does not mutate supply is also acceptable
            // (rejected without effect); a Success is a TTL-enforcement failure.
            Ok(TransactionResult::Success(_)) => failures.push(format!(
                "case {case}: an expired intent (ttl < block time) was applied"
            )),
            Ok(_) => {
                let supply_after = state
                    .ledger
                    .utxo
                    .utxos
                    .iter()
                    .map(|kv| (*kv.0).clone())
                    .filter(|u| u.type_ == token)
                    .map(|u| u.value)
                    .sum::<u128>();
                if supply_after != supply_before {
                    failures.push(format!(
                        "case {case}: an expired intent mutated supply {supply_before} -> {supply_after}"
                    ));
                }
            }
        }
    }

    assert!(exercised > 0, "no expired-intent case was exercised — property was vacuous");
    assert!(
        failures.is_empty(),
        "expired-intent rejection guard failed in {} case(s):\n{}",
        failures.len(),
        failures.join("\n"),
    );
}
