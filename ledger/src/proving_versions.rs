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

//! Proves circuits of every ZKIR version, behind the current
//! `transient-crypto` types.
//!
//! | circuit              | proven by                       | verifier key       |
//! |----------------------|---------------------------------|--------------------|
//! | IR v2, minor 0 and 1 | `midnight-transient-crypto` 2   | `verifier-key[v6]` |
//! | IR v2, minor 2       | `midnight-transient-crypto` 3.1 | `verifier-key[v7]` |
//! | IR v3                | `midnight-transient-crypto` 3.1 | `verifier-key[v7]` |
//! | IR v4                | `midnight-transient-crypto` 4   | `verifier-key[v8]` |
//!
//! The circuit is read and proven by the `midnight-zkir` release of its IR
//! version. The IR v2 and v3 releases speak `midnight-transient-crypto` 3.1,
//! and the IR v2 one picks `midnight-transient-crypto` 2 for minor 0 and 1
//! itself.

use base_crypto::rng::SplittableRng;
use rand::{CryptoRng, Rng};
use serialize::{peek_tag, tagged_deserialize};
use std::io;
use transient_crypto::curve::Fr;
use transient_crypto::proofs::{
    KeyLocation, ParamsProverProvider, Proof, ProofPreimage, ProvingError, ProvingKeyMaterial,
    ProvingProvider, Resolver, Zkir,
};
use transient_crypto_3::proofs as v3;

fn fr(f: Fr) -> transient_crypto_3::curve::Fr {
    transient_crypto_3::curve::Fr::from_le_bytes(&f.as_le_bytes()).expect("same field")
}

/// Fails on a preimage carrying inner proofs, which circuits of a ZKIR
/// version before 4 cannot consume.
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
            "a preimage with inner proofs cannot be proven for a ZKIR version before 4",
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
        self.0.get_params(k).await?.write(&mut buf)?;
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

impl Resolver for Keys<'_> {
    async fn resolve_key(&self, _: KeyLocation) -> io::Result<Option<ProvingKeyMaterial>> {
        Ok(Some(self.0.clone()))
    }
}

enum Ir {
    V2(zkir_v2::IrSource),
    V3(zkir_v3::IrSource),
    V4(zkir_v4::IrSource),
}

fn load(ir_source: &[u8]) -> io::Result<Ir> {
    let tag = peek_tag(&mut io::Cursor::new(ir_source))?;
    match tag.as_str() {
        "ir-source[v2]" | "ir-source[v2-generic]" => Ok(Ir::V2(
            zkir_v2::IrSource::load_from_tagged(io::Cursor::new(ir_source))?,
        )),
        "ir-source[v3-generic]" => Ok(Ir::V3(tagged_deserialize(ir_source)?)),
        "ir-source[v4-generic]" => Ok(Ir::V4(tagged_deserialize(ir_source)?)),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Unknown ZKIR tag: '{tag}'"),
        )),
    }
}

/// The `k` the circuit of `ir_source` is proven at.
pub fn k(ir_source: &[u8]) -> io::Result<u8> {
    Ok(match load(ir_source)? {
        Ir::V2(ir) => v3::Zkir::k(&ir),
        Ir::V3(ir) => v3::Zkir::k(&ir),
        Ir::V4(ir) => Zkir::k(&ir),
    })
}

/// Runs witness generation for the circuit of `ir_source`, without proving.
pub fn check(
    preimage: &ProofPreimage,
    ir_source: &[u8],
) -> Result<Vec<Option<usize>>, ProvingError> {
    match load(ir_source)? {
        Ir::V2(ir) => v3_preimage(preimage)?.check(&ir),
        Ir::V3(ir) => v3_preimage(preimage)?.check(&ir),
        Ir::V4(ir) => preimage.check(&ir),
    }
}

/// Proves `preimage` against `keys`.
pub async fn prove(
    preimage: &ProofPreimage,
    keys: &ProvingKeyMaterial,
    rng: impl CryptoRng + SplittableRng,
    params: &impl ParamsProverProvider,
) -> Result<Proof, ProvingError> {
    let resolver = Keys(keys);
    let proof = match load(&keys.ir_source)? {
        Ir::V2(_) => {
            let provider = zkir_v2::LocalProvingProvider {
                rng,
                resolver: &resolver,
                params: &Params(params),
            };
            v3::ProvingProvider::prove(provider, &v3_preimage(preimage)?, None).await?
        }
        Ir::V3(_) => {
            v3_preimage(preimage)?
                .prove::<zkir_v3::IrSource>(rng, &Params(params), &resolver)
                .await?
                .0
        }
        Ir::V4(_) => {
            return Ok(preimage
                .prove::<zkir_v4::IrSource>(rng, params, &resolver)
                .await?
                .0);
        }
    };
    // A proof of a ZKIR version before 4 carries no accumulators.
    Ok(Proof::from_bytes(proof.0))
}

/// Proves locally, with the prover of each circuit's ZKIR version.
pub struct VersionedProvingProvider<'a, R: Rng + CryptoRng + SplittableRng, S, P> {
    pub rng: R,
    pub resolver: &'a S,
    pub params: &'a P,
}

impl<R: Rng + CryptoRng + SplittableRng, S: Resolver, P> VersionedProvingProvider<'_, R, S, P> {
    async fn keys(&self, preimage: &ProofPreimage) -> Result<ProvingKeyMaterial, anyhow::Error> {
        self.resolver
            .resolve_key(preimage.key_location.clone())
            .await?
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "could not resolve key location: {}",
                    preimage.key_location.0
                )
            })
    }
}

impl<R: Rng + CryptoRng + SplittableRng, S: Resolver, P: ParamsProverProvider> ProvingProvider
    for VersionedProvingProvider<'_, R, S, P>
{
    async fn check(&self, preimage: &ProofPreimage) -> Result<Vec<Option<usize>>, anyhow::Error> {
        check(preimage, &self.keys(preimage).await?.ir_source)
    }

    async fn prove(
        self,
        preimage: &ProofPreimage,
        overwrite_binding_input: Option<Fr>,
    ) -> Result<Proof, anyhow::Error> {
        let keys = self.keys(preimage).await?;
        let mut preimage = preimage.clone();
        if let Some(binding_input) = overwrite_binding_input {
            preimage.binding_input = binding_input;
        }
        prove(&preimage, &keys, self.rng, self.params).await
    }

    fn split(&mut self) -> Self {
        Self {
            rng: self.rng.split(),
            resolver: self.resolver,
            params: self.params,
        }
    }

    fn resolver(&self) -> &impl Resolver {
        self.resolver
    }
}
