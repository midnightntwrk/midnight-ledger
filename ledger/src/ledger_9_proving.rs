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

//! Proving against the `verifier-key[v7]` keys that contracts deployed on
//! ledger 9 hold.
//!
//! The proof is made by `midnight-transient-crypto` 3.1 and the
//! `midnight-zkir` release ledger 9 proves the circuit's IR version with.

use midnight_proofs::utils::SerdeFormat;
use rand::{CryptoRng, Rng};
use serialize::{Tagged, peek_tag, tagged_deserialize};
use std::io;
use transient_crypto::curve::Fr;
use transient_crypto::proofs::{
    ParamsProverProvider, Proof, ProofPreimage, ProvingError, ProvingKeyMaterial,
};
use transient_crypto_3::proofs as v3;

fn fr(f: Fr) -> transient_crypto_3::curve::Fr {
    transient_crypto_3::curve::Fr::from_le_bytes(&f.as_le_bytes()).expect("same field")
}

/// Fails on a preimage carrying inner proofs, which these circuits cannot
/// consume.
fn v3_preimage(p: &ProofPreimage) -> io::Result<v3::ProofPreimage> {
    let ProofPreimage {
        inputs,
        private_transcript,
        public_transcript_inputs,
        public_transcript_outputs,
        inner_proofs,
        binding_input,
        communications_commitment,
        key_location,
    } = p;
    if !inner_proofs.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "a preimage with inner proofs cannot be proven against a verifier-key[v7] key",
        ));
    }
    let frs = |fs: &[Fr]| fs.iter().copied().map(fr).collect();
    Ok(v3::ProofPreimage {
        inputs: frs(inputs),
        private_transcript: frs(private_transcript),
        public_transcript_inputs: frs(public_transcript_inputs),
        public_transcript_outputs: frs(public_transcript_outputs),
        binding_input: fr(*binding_input),
        communications_commitment: communications_commitment.map(|(a, b)| (fr(a), fr(b))),
        key_location: v3::KeyLocation(key_location.0.clone()),
    })
}

struct Params<'a, P>(&'a P);

impl<P: ParamsProverProvider> v3::ParamsProverProvider for Params<'_, P> {
    async fn get_params(&self, k: u8) -> io::Result<v3::ParamsProver> {
        let mut buf = Vec::new();
        self.0
            .get_params(k)
            .await?
            .0
            .write_custom(&mut buf, SerdeFormat::RawBytesUnchecked)?;
        v3::ParamsProver::read(&buf[..])
    }
}

/// Resolves every location to the keys it holds.
struct Keys<'a>(&'a ProvingKeyMaterial);

impl v3::Resolver for Keys<'_> {
    async fn resolve_key(&self, _: v3::KeyLocation) -> io::Result<Option<v3::ProvingKeyMaterial>> {
        Ok(Some(v3::ProvingKeyMaterial {
            prover_key: self.0.prover_key.clone(),
            verifier_key: self.0.verifier_key.clone(),
            ir_source: self.0.ir_source.clone(),
        }))
    }
}

/// A circuit, loaded by the release ledger 9 reads its IR version with.
enum Ir {
    V2(zkir_v2::IrSource),
    V3(zkir_v3::IrSource),
}

fn load(ir_source: &[u8]) -> io::Result<Ir> {
    let tag = peek_tag(&mut io::Cursor::new(ir_source))?;
    match tag.as_str() {
        "ir-source[v2]" | "ir-source[v2-generic]" => Ok(Ir::V2(
            zkir_v2::IrSource::load_from_tagged(io::Cursor::new(ir_source))?,
        )),
        "ir-source[v3-generic]" => Ok(Ir::V3(tagged_deserialize(ir_source)?)),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Unknown ZKIR tag: '{tag}'"),
        )),
    }
}

/// Whether the serialized `verifier_key` is a `verifier-key[v7]` key.
pub fn is_ledger_9_key(verifier_key: &[u8]) -> bool {
    matches!(peek_tag(&mut io::Cursor::new(verifier_key)), Ok(tag) if tag == v3::VerifierKey::tag())
}

/// The `k` the circuit of `ir_source` is proven at.
pub fn k(ir_source: &[u8]) -> io::Result<u8> {
    Ok(match load(ir_source)? {
        Ir::V2(ir) => v3::Zkir::k(&ir),
        Ir::V3(ir) => v3::Zkir::k(&ir),
    })
}

/// Runs witness generation for the circuit of `ir_source`, without proving.
pub fn check(
    preimage: &ProofPreimage,
    ir_source: &[u8],
) -> Result<Vec<Option<usize>>, ProvingError> {
    let preimage = v3_preimage(preimage)?;
    match load(ir_source)? {
        Ir::V2(ir) => preimage.check(&ir),
        Ir::V3(ir) => preimage.check(&ir),
    }
}

/// Proves `preimage` against `keys`, whose verifier key is a
/// `verifier-key[v7]` key.
///
/// The result is what [`ProofPreimage::prove`] returns. The proof carries no
/// accumulators.
pub async fn prove(
    preimage: &ProofPreimage,
    keys: &ProvingKeyMaterial,
    rng: impl Rng + CryptoRng,
    params: &impl ParamsProverProvider,
) -> Result<(Proof, Vec<Option<usize>>), ProvingError> {
    let preimage = v3_preimage(preimage)?;
    let (params, resolver) = (Params(params), Keys(keys));
    let (proof, pi_skips) = match load(&keys.ir_source)? {
        Ir::V2(_) => {
            preimage
                .prove::<zkir_v2::IrSource>(rng, &params, &resolver)
                .await?
        }
        Ir::V3(_) => {
            preimage
                .prove::<zkir_v3::IrSource>(rng, &params, &resolver)
                .await?
        }
    };
    Ok((Proof::from_bytes(proof.0), pi_skips))
}
