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

//! Shared helpers for the ledger property tests: key/utxo/offer construction and
//! the system's own validation entry point. Each integration-test binary that
//! `mod common;`s this file uses a different subset, so unused-code warnings are
//! suppressed here rather than per-item.

#![allow(dead_code)]

use base_crypto::time::Timestamp;
use coin_structure::coin::{UnshieldedTokenType, UserAddress};
use base_crypto::schnorr::{Signature, SigningKey, VerifyingKey as SignatureVerifyingKey};
use midnight_ledger::structure::{
    ErasedIntent, Intent, IntentHash, ProofPreimageMarker, UnshieldedOffer, UtxoOutput, UtxoSpend,
};
use rand::Rng;
use rand::rngs::StdRng;
use storage::DefaultDB;
use storage::storage::Array;
use transient_crypto::commitment::PedersenRandomness;

pub type D = DefaultDB;

/// The transaction segment id used throughout these tests.
pub const SEG: u16 = 1;

pub fn keypair(rng: &mut StdRng) -> (SigningKey, SignatureVerifyingKey) {
    let sk = SigningKey::sample(&mut *rng);
    let vk = sk.verifying_key();
    (sk, vk)
}

pub fn token(rng: &mut StdRng) -> UnshieldedTokenType {
    UnshieldedTokenType(rng.r#gen())
}

pub fn spend(
    rng: &mut StdRng,
    owner: &SignatureVerifyingKey,
    value: u128,
    tt: UnshieldedTokenType,
    output_no: u32,
) -> UtxoSpend {
    UtxoSpend {
        value,
        owner: owner.clone(),
        type_: tt,
        intent_hash: IntentHash(rng.r#gen()),
        output_no,
    }
}

pub fn output(owner: &SignatureVerifyingKey, value: u128, tt: UnshieldedTokenType) -> UtxoOutput {
    UtxoOutput {
        value,
        owner: UserAddress::from(owner.clone()),
        type_: tt,
    }
}

/// Build the erased parent intent that carries these inputs/outputs in its
/// guaranteed offer, and the exact byte-string that a legitimate signer would
/// sign for `segment`. `well_formed` recomputes this same string from the
/// parent, so signing it is what a correct owner signature must cover.
pub fn parent_and_data(
    rng: &mut StdRng,
    inputs: &[UtxoSpend],
    outputs: &[UtxoOutput],
    segment: u16,
) -> (ErasedIntent<D>, Vec<u8>) {
    let unsigned: UnshieldedOffer<Signature, D> = UnshieldedOffer {
        inputs: inputs.to_vec().into(),
        outputs: outputs.to_vec().into(),
        signatures: Array::new(),
    };
    let intent: Intent<Signature, ProofPreimageMarker, PedersenRandomness, D> = Intent::new(
        rng,
        Some(unsigned),
        None,
        vec![],
        vec![],
        vec![],
        None,
        Timestamp::from_secs(1_000_000),
    );
    let erased = intent.erase_proofs().erase_signatures();
    let data = erased.data_to_sign(segment);
    (erased, data)
}

pub fn offer(
    inputs: &[UtxoSpend],
    outputs: &[UtxoOutput],
    sigs: Vec<Signature>,
) -> UnshieldedOffer<Signature, D> {
    UnshieldedOffer {
        inputs: inputs.to_vec().into(),
        outputs: outputs.to_vec().into(),
        signatures: sigs.into(),
    }
}

/// Run the system's own validation: structural checks + the deferred signature
/// closure. Returns `Ok(())` iff the offer is accepted.
pub fn verdict(
    offer: UnshieldedOffer<Signature, D>,
    segment: u16,
    parent: &ErasedIntent<D>,
) -> Result<(), String> {
    match offer.well_formed(segment, parent) {
        Err(e) => Err(format!("{e:?}")),
        Ok(check) => check().map_err(|e| format!("{e:?}")),
    }
}
