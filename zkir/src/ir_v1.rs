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

//! V1 (zk-stdlib v1) proving and verification pipeline.

use std::borrow::Cow;
use std::io;

type OldIrSource = zkir_old::IrSource;

/// Adapter: current `Resolver` → v1 `Resolver`.
pub struct V1Resolver<'a, S: transient_crypto::proofs::Resolver>(pub &'a S);

impl<S: transient_crypto::proofs::Resolver> transient_crypto_old::proofs::Resolver
    for V1Resolver<'_, S>
{
    async fn resolve_key(
        &self,
        key: transient_crypto_old::proofs::KeyLocation,
    ) -> io::Result<Option<transient_crypto_old::proofs::ProvingKeyMaterial>> {
        let current_key = transient_crypto::proofs::KeyLocation(key.0);
        let result = self.0.resolve_key(current_key).await?;
        Ok(
            result.map(|m| transient_crypto_old::proofs::ProvingKeyMaterial {
                prover_key: m.prover_key,
                verifier_key: m.verifier_key,
                ir_source: m.ir_source,
            }),
        )
    }
}

/// Adapter: current `ParamsProverProvider` → v1 `ParamsProverProvider`.
pub struct V1Params<'a, P: transient_crypto::proofs::ParamsProverProvider>(pub &'a P);

impl<P: transient_crypto::proofs::ParamsProverProvider>
    transient_crypto_old::proofs::ParamsProverProvider for V1Params<'_, P>
{
    async fn get_params(&self, k: u8) -> io::Result<transient_crypto_old::proofs::ParamsProver> {
        let current = self.0.get_params(k).await?;
        let mut buf = Vec::new();
        midnight_proofs::poly::kzg::params::ParamsKZG::write_custom(
            current.as_ref(),
            &mut buf,
            midnight_proofs::utils::SerdeFormat::RawBytesUnchecked,
        )?;
        transient_crypto_old::proofs::ParamsProver::read(&buf[..])
    }
}

/// Converts a current `ProofPreimage` into a v1 `ProofPreimage`.
pub fn preimage_to_v1(
    p: &transient_crypto::proofs::ProofPreimage,
) -> transient_crypto_old::proofs::ProofPreimage {
    let cvt = |f: transient_crypto::curve::Fr| -> transient_crypto_old::curve::Fr {
        transient_crypto_old::curve::Fr::from_le_bytes(&f.as_le_bytes()).expect("Fr round-trip")
    };
    transient_crypto_old::proofs::ProofPreimage {
        inputs: p.inputs.iter().copied().map(cvt).collect(),
        private_transcript: p.private_transcript.iter().copied().map(cvt).collect(),
        public_transcript_inputs: p
            .public_transcript_inputs
            .iter()
            .copied()
            .map(cvt)
            .collect(),
        public_transcript_outputs: p
            .public_transcript_outputs
            .iter()
            .copied()
            .map(cvt)
            .collect(),
        binding_input: cvt(p.binding_input),
        communications_commitment: p.communications_commitment.map(|(a, b)| (cvt(a), cvt(b))),
        key_location: transient_crypto_old::proofs::KeyLocation(Cow::Owned(
            p.key_location.0.to_string(),
        )),
    }
}

/// Verifies a proof using the v1 (zk-stdlib v1) pipeline.
pub fn v1_verify(
    vk: &transient_crypto::proofs::VerifierKey,
    proof: &transient_crypto::proofs::Proof,
    pis: impl Iterator<Item = transient_crypto::curve::Fr>,
) -> Result<(), transient_crypto::proofs::VerifyingError> {
    let raw = vk.original_bytes();
    let vk_buf = {
        let mut buf = Vec::new();
        serialize::Serializable::serialize(&raw, &mut buf)
            .map_err(|e| anyhow::anyhow!("vk raw serialize: {e}"))?;
        buf
    };
    let old_vk: transient_crypto_old::proofs::VerifierKey =
        serialize::Deserializable::deserialize(&mut &vk_buf[..], 0)
            .map_err(|e| anyhow::anyhow!("vk deserialize as v1: {e}"))?;

    // Convert proof.
    let old_proof = transient_crypto_old::proofs::Proof(proof.0.clone());

    // Convert PIs.
    let old_pis = pis.map(|f| {
        transient_crypto_old::curve::Fr::from_le_bytes(&f.as_le_bytes()).expect("Fr round-trip")
    });

    old_vk
        .verify(
            &transient_crypto_old::proofs::PARAMS_VERIFIER,
            &old_proof,
            old_pis,
        )
        .map_err(|e| anyhow::anyhow!("v1 verification failed: {e}"))
}

/// Mock-verifies a v1 proof (calibrated cost simulation).
#[cfg(feature = "mock-verify")]
pub fn v1_mock_verify(
    vk: &transient_crypto::proofs::VerifierKey,
    pis: impl Iterator<Item = transient_crypto::curve::Fr>,
) -> Result<(), transient_crypto::proofs::VerifyingError> {
    let raw = vk.original_bytes();
    let vk_buf = {
        let mut buf = Vec::new();
        serialize::Serializable::serialize(&raw, &mut buf)
            .map_err(|e| anyhow::anyhow!("vk raw serialize: {e}"))?;
        buf
    };
    let old_vk: transient_crypto_old::proofs::VerifierKey =
        serialize::Deserializable::deserialize(&mut &vk_buf[..], 0)
            .map_err(|e| anyhow::anyhow!("vk deserialize as v1: {e}"))?;
    let old_pis = pis.map(|f| {
        transient_crypto_old::curve::Fr::from_le_bytes(&f.as_le_bytes()).expect("Fr round-trip")
    });
    old_vk
        .mock_verify(old_pis)
        .map_err(|e| anyhow::anyhow!("v1 mock verification failed: {e}"))
}

/// Proves using the v1 (zk-stdlib v1) pipeline.
pub async fn v1_prove(
    preimage: &transient_crypto::proofs::ProofPreimage,
    rng: impl rand::Rng + rand::CryptoRng,
    params: &impl transient_crypto::proofs::ParamsProverProvider,
    resolver: &impl transient_crypto::proofs::Resolver,
) -> Result<transient_crypto::proofs::Proof, anyhow::Error> {
    let old_preimage = preimage_to_v1(preimage);
    let (old_proof, _skips) = old_preimage
        .prove::<OldIrSource>(rng, &V1Params(params), &V1Resolver(resolver))
        .await?;
    Ok(transient_crypto::proofs::Proof(old_proof.0))
}
