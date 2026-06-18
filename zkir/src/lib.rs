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

#[macro_use]
extern crate tracing;

use base_crypto::rng::SplittableRng;
use rand::{CryptoRng, Rng};
use std::io::Write;
use transient_crypto::curve::Fr;
use transient_crypto::proofs::{
    ParamsProverProvider, Proof, ProofPreimage, ProvingProvider, Resolver,
};

mod ir;
pub(crate) mod ir_v1;
mod ir_vm;

pub use ir::{Instruction, IrMinorVersion, IrSource, VersionedInnerPK};
pub use ir_vm::Preprocessed;

/// The tag used by v1 (old) verifier keys.
const V1_VK_TAG: &str = "verifier-key[v6]";

/// Deserializes a `VerifierKey` from tagged bytes, accepting both the v1 tag
/// (`verifier-key[v6]`) and the v2 tag (`verifier-key[v7]`).
pub fn load_vk_from_tagged(mut reader: impl std::io::Read + std::io::Seek) -> std::io::Result<transient_crypto::proofs::VerifierKey> {
    let tag = serialize::peek_tag(&mut reader)?;
    if tag == V1_VK_TAG {
        // v1 VK: deserialize with the old tag, then convert to current type.
        let old_vk: transient_crypto_old::proofs::VerifierKey =
            serialize::tagged_deserialize(&mut reader)?;
        let mut buf = Vec::new();
        serialize::Serializable::serialize(&old_vk, &mut buf)?;
        serialize::Deserializable::deserialize(&mut &buf[..], 0)
    } else {
        serialize::tagged_deserialize(&mut reader)
    }
}

/// Verifies a proof against a statement, dispatching to the correct pipeline
/// based on the verifier key tag.
///
/// Tagged VK bytes with `verifier-key[v6]` are verified using the v1 pipeline;
/// `verifier-key[v7]` uses the v2 pipeline.
pub fn verify(
    tagged_vk: &[u8],
    proof: &Proof,
    pis: impl Iterator<Item = Fr>,
) -> Result<(), anyhow::Error> {
    let tag = serialize::peek_tag(&mut std::io::Cursor::new(tagged_vk))?;
    if tag == V1_VK_TAG {
        let old_vk: transient_crypto_old::proofs::VerifierKey =
            serialize::tagged_deserialize(&mut &tagged_vk[..])?;
        let old_proof = transient_crypto_old::proofs::Proof(proof.0.clone());
        let old_pis = pis.map(|f| transient_crypto_old::curve::Fr(
            ff::PrimeField::from_repr(ff::PrimeField::to_repr(&f.0))
                .expect("BLS12-381 Fq round-trip"),
        ));
        old_vk
            .verify(
                &transient_crypto_old::proofs::PARAMS_VERIFIER,
                &old_proof,
                old_pis,
            )
            .map_err(|e| anyhow::anyhow!("v1 verification failed: {e}"))
    } else {
        let vk: transient_crypto::proofs::VerifierKey =
            serialize::tagged_deserialize(&mut &tagged_vk[..])?;
        vk.verify(
            &transient_crypto::proofs::PARAMS_VERIFIER,
            proof,
            pis,
        )
    }
}

/// Mock-verifies a proof (calibrated cost simulation).
#[cfg(feature = "mock-verify")]
pub fn mock_verify(
    tagged_vk: &[u8],
    pis: impl Iterator<Item = Fr>,
) -> Result<(), anyhow::Error> {
    let tag = serialize::peek_tag(&mut std::io::Cursor::new(tagged_vk))?;
    if tag == V1_VK_TAG {
        let old_vk: transient_crypto_old::proofs::VerifierKey =
            serialize::tagged_deserialize(&mut &tagged_vk[..])?;
        let old_pis = pis.map(|f| transient_crypto_old::curve::Fr(
            ff::PrimeField::from_repr(ff::PrimeField::to_repr(&f.0))
                .expect("BLS12-381 Fq round-trip"),
        ));
        old_vk
            .mock_verify(old_pis)
            .map_err(|e| anyhow::anyhow!("v1 mock verification failed: {e}"))
    } else {
        let vk: transient_crypto::proofs::VerifierKey =
            serialize::tagged_deserialize(&mut &tagged_vk[..])?;
        vk.mock_verify(pis)
    }
}

/// Implements `ProvingProvider` locally
pub struct LocalProvingProvider<
    'a,
    R: Rng + CryptoRng + SplittableRng,
    S: Resolver,
    P: ParamsProverProvider,
> {
    /// The randomness to use for proving
    pub rng: R,
    /// The resolver to use to fetch keys
    pub resolver: &'a S,
    /// The parameters provider to use
    pub params: &'a P,
}

impl<'a, R: Rng + CryptoRng + SplittableRng, S: Resolver, P: ParamsProverProvider> ProvingProvider
    for LocalProvingProvider<'a, R, S, P>
{
    async fn check(&self, preimage: &ProofPreimage) -> Result<Vec<Option<usize>>, anyhow::Error> {
        let proving_data = self
            .resolver
            .resolve_key(preimage.key_location.clone())
            .await?
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "attempted to check proof for '{}' without circuit data!",
                    preimage.key_location.0
                )
            })?;
        let ir = IrSource::load_from_tagged(std::io::Cursor::new(&proving_data.ir_source[..]))?;
        preimage.check(&ir)
    }
    async fn prove(
        self,
        preimage: &ProofPreimage,
        overwrite_binding_input: Option<Fr>,
    ) -> Result<Proof, anyhow::Error> {
        let mut preimage = preimage.clone();
        if let Some(binding_input) = overwrite_binding_input {
            preimage.binding_input = binding_input;
        }

        // Resolve to determine the IR version.
        let proving_data = self
            .resolver
            .resolve_key(preimage.key_location.clone())
            .await?
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "attempted to prove '{}' without circuit data!",
                    preimage.key_location.0
                )
            })?;
        let ir = IrSource::load_from_tagged(std::io::Cursor::new(&proving_data.ir_source[..]))?;

        match ir.version {
            IrMinorVersion::V0 | IrMinorVersion::V1 => {
                // V0/V1: use the old Zkir trait directly with pre-resolved data.
                use transient_crypto_old::proofs::Zkir as V1Zkir;
                let old_preimage = ir_v1::preimage_to_v1(&preimage);
                let v1_params = ir_v1::V1Params(self.params);
                let vk = load_vk_from_tagged(std::io::Cursor::new(&proving_data.verifier_key[..]))?;
                let pk: transient_crypto_old::proofs::ProverKey<IrSource> =
                    serialize::tagged_deserialize(&mut &proving_data.prover_key[..])?;
                let (proof, pis, _) =
                    ir.prove(self.rng, &v1_params, pk, &old_preimage).await?;
                // Self-verify
                let mut tagged_vk = Vec::new();
                write!(&mut tagged_vk, "midnight:{}:", V1_VK_TAG)?;
                serialize::Serializable::serialize(&vk, &mut tagged_vk)?;
                let proof = Proof(proof.0.clone());
                let pis: Vec<Fr> = pis.iter().map(|f| {
                    Fr(ff::PrimeField::from_repr(ff::PrimeField::to_repr(&f.0))
                        .expect("Fr round-trip"))
                }).collect();
                if let Err(e) = verify(&tagged_vk, &proof, pis.into_iter()) {
                    error!(error = ?e, "v1 self-verification failed!");
                    return Err(e);
                }
                Ok(proof)
            }
            _ => {
                // V2+: use the current pipeline.
                Ok(preimage
                    .prove::<IrSource>(self.rng, self.params, self.resolver)
                    .await?
                    .0)
            }
        }
    }
    fn split(&mut self) -> Self {
        Self {
            rng: self.rng.split(),
            resolver: self.resolver,
            params: self.params,
        }
    }
}
