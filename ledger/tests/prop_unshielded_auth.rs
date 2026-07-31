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

//! Clean-room property tests for the AUTHORIZATION invariant of unshielded
//! value transfers ("who is permitted to move what").
//!
//! Contract (derived from `UnshieldedOffer::well_formed` in
//! `ledger/src/verify.rs:484` and `Intent::sign` in `ledger/src/construct.rs`):
//! an `UnshieldedOffer` spends a set of `UtxoSpend` inputs, each carrying an
//! `owner: SignatureVerifyingKey`. The offer is well-formed only if
//!   * inputs are sorted and contain no duplicates,
//!   * outputs are sorted and none has value 0,
//!   * `signatures.len() == inputs.len()`, and
//!   * for every position i, `signatures[i]` is a valid signature by
//!     `inputs[i].owner` over the parent intent's `data_to_sign(segment)`.
//! The last clause is the security-critical one: a UTXO may only be spent with
//! a signature from ITS OWN owner, over the exact intent/segment being
//! authorized. If validation accepts anything weaker, an attacker can move
//! value they do not own.
//!
//! This suite generates offers both through the intended path (sign with the
//! real owner key via the same primitive `Intent::sign` uses) AND
//! structurally -- assembling `signatures` arrays the builder can never emit
//! (wrong signer, permuted signatures, wrong count, signature over a different
//! segment). The oracle is the authorization contract above: `well_formed`
//! (plus its deferred signature closure) must ACCEPT iff the contract holds.
//! No ZK prover is needed -- only Schnorr signatures -- and proof/binding
//! machinery is irrelevant to authorization, so it is simply not exercised.

use base_crypto::schnorr::{Signature, SigningKey, VerifyingKey as SignatureVerifyingKey};
use midnight_ledger::structure::UtxoSpend;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

mod common;
use common::{SEG, keypair, offer, output, parent_and_data, spend, token, verdict};

// ---------------------------------------------------------------------------
// Deterministic adversarial scenarios with known-correct verdicts.
// ---------------------------------------------------------------------------
#[test]
fn auth_scenarios() {
    let mut rng = StdRng::seed_from_u64(0xA1);
    let tt = token(&mut rng);
    let mut fail: Vec<String> = Vec::new();

    let expect = |name: &str, res: Result<(), String>, want_ok: bool, fail: &mut Vec<String>| {
        if res.is_ok() != want_ok {
            fail.push(format!(
                "{name}: expected {}, got {:?}",
                if want_ok { "ACCEPT" } else { "REJECT" },
                res
            ));
        }
    };

    // 1. Honest single-input offer, signed by the owner -> ACCEPT.
    {
        let (sk, vk) = keypair(&mut rng);
        let ins = vec![spend(&mut rng, &vk, 100, tt, 0)];
        let outs = vec![output(&vk, 100, tt)];
        let (parent, data) = parent_and_data(&mut rng, &ins, &outs, SEG);
        let sig = sk.sign(&mut rng, &data);
        expect("honest-single", verdict(offer(&ins, &outs, vec![sig]), SEG, &parent), true, &mut fail);
    }

    // 2. Honest two-input offer (two owners), each signs -> ACCEPT.
    {
        let (sk_a, vk_a) = keypair(&mut rng);
        let (sk_b, vk_b) = keypair(&mut rng);
        let mut ins = vec![
            spend(&mut rng, &vk_a, 10, tt, 0),
            spend(&mut rng, &vk_b, 20, tt, 1),
        ];
        ins.sort();
        let outs = vec![output(&vk_a, 30, tt)];
        let (parent, data) = parent_and_data(&mut rng, &ins, &outs, SEG);
        // Sign per input position, matching each input's owner.
        let sigs = ins
            .iter()
            .map(|i| {
                let sk = if i.owner == vk_a { &sk_a } else { &sk_b };
                sk.sign(&mut rng, &data)
            })
            .collect::<Vec<_>>();
        expect("honest-double", verdict(offer(&ins, &outs, sigs), SEG, &parent), true, &mut fail);
    }

    // 3. IMPERSONATION: victim owns the UTXO, attacker signs -> REJECT.
    {
        let (_sk_victim, vk_victim) = keypair(&mut rng);
        let (sk_attacker, _vk_attacker) = keypair(&mut rng);
        let ins = vec![spend(&mut rng, &vk_victim, 1_000_000, tt, 0)];
        let outs = vec![output(&vk_victim, 1_000_000, tt)];
        let (parent, data) = parent_and_data(&mut rng, &ins, &outs, SEG);
        let forged = sk_attacker.sign(&mut rng, &data); // valid sig, WRONG signer
        expect(
            "impersonation",
            verdict(offer(&ins, &outs, vec![forged]), SEG, &parent),
            false,
            &mut fail,
        );
    }

    // 4. PERMUTED signatures: two valid signatures, swapped between owners -> REJECT.
    {
        let (sk_a, vk_a) = keypair(&mut rng);
        let (sk_b, vk_b) = keypair(&mut rng);
        let mut ins = vec![
            spend(&mut rng, &vk_a, 10, tt, 0),
            spend(&mut rng, &vk_b, 20, tt, 1),
        ];
        ins.sort();
        let outs = vec![output(&vk_a, 30, tt)];
        let (parent, data) = parent_and_data(&mut rng, &ins, &outs, SEG);
        // Produce each owner's genuine signature, then place them in the wrong slots.
        let sig_for = |vk: &SignatureVerifyingKey, rng: &mut StdRng| {
            let sk = if *vk == vk_a { &sk_a } else { &sk_b };
            sk.sign(rng, &data)
        };
        let mut sigs: Vec<Signature> = ins.iter().map(|i| sig_for(&i.owner, &mut rng)).collect();
        sigs.swap(0, 1);
        expect("permuted-sigs", verdict(offer(&ins, &outs, sigs), SEG, &parent), false, &mut fail);
    }

    // 5. WRONG SEGMENT: owner signs data for a different segment -> REJECT.
    {
        let (sk, vk) = keypair(&mut rng);
        let ins = vec![spend(&mut rng, &vk, 100, tt, 0)];
        let outs = vec![output(&vk, 100, tt)];
        let (parent, _data_seg1) = parent_and_data(&mut rng, &ins, &outs, SEG);
        // Sign the SAME intent but for segment SEG+1.
        let data_other = parent.data_to_sign(SEG + 1);
        let sig = sk.sign(&mut rng, &data_other);
        expect(
            "wrong-segment",
            verdict(offer(&ins, &outs, vec![sig]), SEG, &parent),
            false,
            &mut fail,
        );
    }

    // 6. MISSING signature (0 sigs, 1 input) -> REJECT (length mismatch).
    {
        let (_sk, vk) = keypair(&mut rng);
        let ins = vec![spend(&mut rng, &vk, 100, tt, 0)];
        let outs = vec![output(&vk, 100, tt)];
        let (parent, _data) = parent_and_data(&mut rng, &ins, &outs, SEG);
        expect("missing-sig", verdict(offer(&ins, &outs, vec![]), SEG, &parent), false, &mut fail);
    }

    // 7. EXTRA signature (2 sigs, 1 input) -> REJECT (length mismatch).
    {
        let (sk, vk) = keypair(&mut rng);
        let ins = vec![spend(&mut rng, &vk, 100, tt, 0)];
        let outs = vec![output(&vk, 100, tt)];
        let (parent, data) = parent_and_data(&mut rng, &ins, &outs, SEG);
        let s1 = sk.sign(&mut rng, &data);
        let s2 = sk.sign(&mut rng, &data);
        expect("extra-sig", verdict(offer(&ins, &outs, vec![s1, s2]), SEG, &parent), false, &mut fail);
    }

    // 8. ZERO-VALUE output -> ACCEPT. well_formed does not reject a
    // zero-valued unshielded output; balance conservation is unaffected and the
    // resulting zero-value UTXO is bounded by the ordinary per-output storage
    // cost.
    {
        let (sk, vk) = keypair(&mut rng);
        let ins = vec![spend(&mut rng, &vk, 100, tt, 0)];
        let outs = vec![output(&vk, 0, tt)];
        let (parent, data) = parent_and_data(&mut rng, &ins, &outs, SEG);
        let sig = sk.sign(&mut rng, &data);
        expect("zero-output", verdict(offer(&ins, &outs, vec![sig]), SEG, &parent), true, &mut fail);
    }

    // 9. DUPLICATE input -> REJECT (structural, prevents double-count/double-spend).
    {
        let (sk, vk) = keypair(&mut rng);
        let s = spend(&mut rng, &vk, 100, tt, 0);
        let ins = vec![s.clone(), s.clone()];
        let outs = vec![output(&vk, 200, tt)];
        let (parent, data) = parent_and_data(&mut rng, &ins, &outs, SEG);
        let sig = sk.sign(&mut rng, &data);
        expect(
            "duplicate-input",
            verdict(offer(&ins, &outs, vec![sig.clone(), sig]), SEG, &parent),
            false,
            &mut fail,
        );
    }

    // 10. UNSORTED inputs -> REJECT (structural / non-normalized).
    {
        let (sk_a, vk_a) = keypair(&mut rng);
        let (sk_b, vk_b) = keypair(&mut rng);
        let mut ins = vec![
            spend(&mut rng, &vk_a, 10, tt, 0),
            spend(&mut rng, &vk_b, 20, tt, 1),
        ];
        ins.sort();
        ins.reverse(); // now guaranteed unsorted (len 2, distinct)
        let outs = vec![output(&vk_a, 30, tt)];
        let (parent, data) = parent_and_data(&mut rng, &ins, &outs, SEG);
        let sigs = ins
            .iter()
            .map(|i| (if i.owner == vk_a { &sk_a } else { &sk_b }).sign(&mut rng, &data))
            .collect::<Vec<_>>();
        expect("unsorted-inputs", verdict(offer(&ins, &outs, sigs), SEG, &parent), false, &mut fail);
    }

    assert!(fail.is_empty(), "authorization scenario failures:\n{}", fail.join("\n"));
}

// ---------------------------------------------------------------------------
// Randomized authorization fuzz: structural validity held FIXED (sorted,
// distinct, non-zero, correct signature count); ONLY the per-position signer
// assignment varies. Acceptance must hold iff every input is signed by its own
// owner over the correct data. This isolates the authorization predicate.
// ---------------------------------------------------------------------------
#[test]
fn auth_signer_assignment_fuzz() {
    let mut failures: Vec<String> = Vec::new();

    for seed in 0..300u64 {
        let mut rng = StdRng::seed_from_u64(0xF00D_0000 + seed);
        let tt = token(&mut rng);
        let n = 1 + (seed % 4) as usize; // 1..=4 inputs

        // Distinct owners and inputs.
        let keys: Vec<(SigningKey, SignatureVerifyingKey)> =
            (0..n).map(|_| keypair(&mut rng)).collect();
        // A spare key never associated with any input (an outsider/attacker).
        let (outsider_sk, _outsider_vk) = keypair(&mut rng);

        let mut ins: Vec<UtxoSpend> = keys
            .iter()
            .enumerate()
            .map(|(i, (_, vk))| spend(&mut rng, vk, 10 + i as u128, tt, i as u32))
            .collect();
        ins.sort();
        // Re-associate keys to the sorted input owners for correct signing.
        let owner_key = |vk: &SignatureVerifyingKey| -> &SigningKey {
            &keys.iter().find(|(_, k)| k == vk).unwrap().0
        };

        let outs = vec![output(&keys[0].1, 999, tt)];
        let (parent, data) = parent_and_data(&mut rng, &ins, &outs, SEG);
        let wrong_data = parent.data_to_sign(SEG + 7);

        // Per-position signing choice: 0 = correct owner+data (authorized),
        // 1 = outsider key (wrong signer), 2 = owner but wrong data.
        let mut all_authorized = true;
        let sigs: Vec<Signature> = ins
            .iter()
            .map(|inp| match rng.gen_range(0u8..3) {
                0 => owner_key(&inp.owner).sign(&mut rng, &data),
                1 => {
                    all_authorized = false;
                    outsider_sk.sign(&mut rng, &data)
                }
                _ => {
                    all_authorized = false;
                    owner_key(&inp.owner).sign(&mut rng, &wrong_data)
                }
            })
            .collect();

        let res = verdict(offer(&ins, &outs, sigs), SEG, &parent);
        if res.is_ok() != all_authorized {
            failures.push(format!(
                "seed {seed}: n={n} expected {} got {:?}",
                if all_authorized { "ACCEPT" } else { "REJECT" },
                res
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "authorization fuzz found validation disagreeing with the owner-signs contract \
         (accept must imply every input authorized by its owner):\n{}",
        failures.join("\n")
    );
}
