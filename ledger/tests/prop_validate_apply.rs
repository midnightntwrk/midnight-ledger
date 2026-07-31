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

//! Clean-room VALIDATE -> APPLY agreement property for unshielded value flows.
//!
//! The invariant a ledger promises: applying a transaction the system accepted
//! as valid produces exactly the effects its validation vouched for -- no more,
//! no less. Validation alone cannot detect a check-vs-use gap; this test runs
//! BOTH phases and compares.
//!
//! Surface: unshielded UTXO transfers (integer value + Schnorr signatures, no
//! prover). Validation = `UnshieldedOffer::well_formed` + its signature closure
//! (authorization). Application = `UtxoState::apply_offer` (semantics.rs:1902),
//! which removes each spent input and inserts each declared output.
//!
//! Oracle, for every offer that validation ACCEPTS: after `apply_offer`
//!   * every input UTXO is removed (exactly the spent set, nothing else),
//!   * every declared output exists exactly once, keyed by the intent hash and
//!     its positional `output_no`,
//!   * the resulting UTXO count is `prior - inputs + outputs`, and
//!   * unshielded value is conserved: total value after == total output value
//!     == total input value (offers are constructed balanced).
//! Any accepted offer whose application deviates from this declared accounting
//! -- value created or destroyed, an input not actually spent, an output
//! silently dropped -- is the target.
//!
//! We relax only proving machinery (irrelevant here: unshielded transfers carry
//! no ZK proofs); authorization (signatures) and the value bookkeeping are
//! exercised in full.

use base_crypto::time::Timestamp;
use coin_structure::coin::UnshieldedTokenType;
use base_crypto::schnorr::Signature;
use midnight_ledger::structure::{
    ErasedIntent, LedgerState, UnshieldedOffer, Utxo, UtxoMeta, UtxoOutput, UtxoState,
};
use midnight_ledger::semantics::TransactionContext;
use onchain_runtime::context::BlockContext;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

mod common;
use common::{D, SEG, keypair, output, parent_and_data, spend};

fn validate(offer: &UnshieldedOffer<Signature, D>, parent: &ErasedIntent<D>) -> Result<(), String> {
    match offer.clone().well_formed(SEG, parent) {
        Err(e) => Err(format!("{e:?}")),
        Ok(check) => check().map_err(|e| format!("{e:?}")),
    }
}

fn context() -> TransactionContext<D> {
    TransactionContext {
        ref_state: LedgerState::new("local-test"),
        block_context: BlockContext {
            tblock: Timestamp::from_secs(500_000),
            ..BlockContext::default()
        },
        whitelist: None,
    }
}

#[test]
fn validate_then_apply_matches_declared_effects() {
    let mut failures: Vec<String> = Vec::new();

    for seed in 0..80u64 {
        let mut rng = StdRng::seed_from_u64(0x5A17_0000 + seed);
        let tt = UnshieldedTokenType(rng.r#gen());

        // 1..=3 inputs, each owned by a distinct key, distinct values.
        let n_in = 1 + (seed % 3) as usize;
        let mut owners = Vec::new();
        let mut inputs = Vec::new();
        for i in 0..n_in {
            let (sk, vk) = keypair(&mut rng);
            owners.push((sk, vk.clone()));
            inputs.push(spend(&mut rng, &vk, 100 + (i as u128 + 1) * 10 + seed as u128, tt, i as u32));
        }
        inputs.sort();
        inputs.dedup(); // guarantee distinct (well_formed rejects duplicates)
        let total_in: u128 = inputs.iter().map(|i| i.value).sum();

        // Declared outputs that conserve value: split `total_in` across 1..=2
        // outputs (structural variation, incl. an output paying an input owner
        // and, on some seeds, two same-(owner,value,type) outputs).
        let (payee_sk_unused, payee_vk) = keypair(&mut rng);
        let _ = payee_sk_unused;
        let mut outputs: Vec<UtxoOutput> = if seed % 2 == 0 || total_in < 2 {
            vec![output(&payee_vk, total_in, tt)]
        } else {
            let half = total_in / 2;
            vec![
                output(&payee_vk, half, tt),
                output(&inputs[0].owner, total_in - half, tt),
            ]
        };
        // well_formed requires sorted outputs; apply enumerates them in this
        // same order, so our expected output_no assignment stays consistent.
        outputs.sort();
        // Skip degenerate zero-value outputs (well_formed rejects them).
        if outputs.iter().any(|o| o.value == 0) {
            continue;
        }

        let (parent, data) = parent_and_data(&mut rng, &inputs, &outputs, SEG);

        // Sign each input with its owner's key (authorized).
        let sigs: Vec<Signature> = inputs
            .iter()
            .map(|inp| {
                let sk = &owners.iter().find(|(_, vk)| *vk == inp.owner).unwrap().0;
                sk.sign(&mut rng, &data)
            })
            .collect();

        let offer = UnshieldedOffer {
            inputs: inputs.clone().into(),
            outputs: outputs.clone().into(),
            signatures: sigs.into(),
        };

        // VALIDATE.
        if let Err(e) = validate(&offer, &parent) {
            failures.push(format!("seed {seed}: authorized offer unexpectedly rejected: {e}"));
            continue;
        }

        // Seed the UTXO set with exactly the inputs.
        let meta = UtxoMeta {
            ctime: Timestamp::from_secs(400_000),
        };
        let mut state = UtxoState::<D>::default();
        for inp in &inputs {
            state = state.insert(Utxo::from(inp.clone()), meta.clone());
        }
        let prior_size = state.utxos.size();

        // APPLY.
        let new_state = match state.apply_offer(&offer, &parent, SEG, &context()) {
            Ok(s) => s,
            Err(e) => {
                failures.push(format!("seed {seed}: validated offer failed to apply: {e:?}"));
                continue;
            }
        };

        // ORACLE: applied effect == declared structure + value conserved.
        // (a) every input spent.
        for inp in &inputs {
            if new_state.utxos.contains_key(&Utxo::from(inp.clone())) {
                failures.push(format!("seed {seed}: input not spent on apply: {inp:?}"));
            }
        }
        // (b) every declared output present exactly, keyed as apply builds it.
        let ih = parent.intent_hash(SEG);
        for (i, o) in outputs.iter().enumerate() {
            let expected = Utxo {
                value: o.value,
                owner: o.owner,
                type_: o.type_,
                intent_hash: ih,
                output_no: i as u32,
            };
            if !new_state.utxos.contains_key(&expected) {
                failures.push(format!("seed {seed}: declared output #{i} not created: {o:?}"));
            }
        }
        // (c) count == prior - inputs + outputs.
        let expect_size = prior_size - inputs.len() + outputs.len();
        if new_state.utxos.size() != expect_size {
            failures.push(format!(
                "seed {seed}: utxo count {} != expected {} (prior {} - {} inputs + {} outputs) \
                 -- apply created/destroyed UTXOs beyond the declared structure",
                new_state.utxos.size(),
                expect_size,
                prior_size,
                inputs.len(),
                outputs.len()
            ));
        }
        // (d) value conservation: total remaining value == declared output value
        //     == input value (balanced offer).
        let total_out: u128 = outputs.iter().map(|o| o.value).sum();
        let after_value: u128 = new_state.utxos.iter().map(|kv| kv.0.value).sum();
        if total_out != total_in {
            failures.push(format!("seed {seed}: constructed offer not balanced ({total_out} != {total_in})"));
        }
        if after_value != total_in {
            failures.push(format!(
                "seed {seed}: VALUE NOT CONSERVED across apply: after={after_value}, \
                 validation accounted for {total_in}",
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "validate->apply agreement / conservation violations:\n{}",
        failures.join("\n")
    );
}
