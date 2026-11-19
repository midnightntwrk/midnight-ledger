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

//! Proves v2 ZKIR circuits through the legacy (zk-stdlib v1) pipeline of
//! `midnight-zkir` 2.x, which speaks the old `transient-crypto` types, behind
//! the current [`ProvingProvider`] interface.
//!
//! Temporary: remove once the built-in circuits are regenerated for a current
//! ZKIR version.

use base_crypto::rng::SplittableRng;
use rand::{CryptoRng, Rng};
use std::borrow::Cow;
use std::io;
use transient_crypto::curve::Fr;
use transient_crypto::proofs::{
    ParamsProverProvider, Proof, ProofPreimage, ProvingProvider, Resolver,
};
use transient_crypto_2::proofs as old;
use zkir_v2::IrSource;

/// A [`ProvingProvider`] for v2 ZKIR circuits with legacy (v1) keys.
pub struct LegacyProvingProvider<'a, R: Rng + CryptoRng + SplittableRng, S, P> {
    /// The randomness to use for proving
    pub rng: R,
    /// The resolver to use to fetch keys
    pub resolver: &'a S,
    /// The parameters provider to use
    pub params: &'a P,
}

struct OldResolver<'a, S>(&'a S);

impl<S: Resolver> old::Resolver for OldResolver<'_, S> {
    async fn resolve_key(
        &self,
        key: old::KeyLocation,
    ) -> io::Result<Option<old::ProvingKeyMaterial>> {
        Ok(self
            .0
            .resolve_key(transient_crypto::proofs::KeyLocation(key.0))
            .await?
            .map(|m| old::ProvingKeyMaterial {
                prover_key: m.prover_key,
                verifier_key: m.verifier_key,
                ir_source: m.ir_source,
            }))
    }
}

struct OldParams<'a, P>(&'a P);

impl<P: ParamsProverProvider> old::ParamsProverProvider for OldParams<'_, P> {
    async fn get_params(&self, k: u8) -> io::Result<old::ParamsProver> {
        let current = self.0.get_params(k).await?;
        let mut buf = Vec::new();
        midnight_proofs::poly::kzg::params::ParamsKZG::write_custom(
            current.as_ref(),
            &mut buf,
            midnight_proofs::utils::SerdeFormat::RawBytesUnchecked,
        )?;
        old::ParamsProver::read(&buf[..])
    }
}

fn old_fr(f: Fr) -> transient_crypto_2::curve::Fr {
    transient_crypto_2::curve::Fr::from_le_bytes(&f.as_le_bytes()).expect("Fr round-trip")
}

fn old_preimage(p: &ProofPreimage) -> io::Result<old::ProofPreimage> {
    if !p.inner_proofs.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "a preimage with inner proofs cannot be proven by the legacy pipeline",
        ));
    }
    let cvt = |v: &[Fr]| v.iter().copied().map(old_fr).collect();
    Ok(old::ProofPreimage {
        inputs: cvt(&p.inputs),
        private_transcript: cvt(&p.private_transcript),
        public_transcript_inputs: cvt(&p.public_transcript_inputs),
        public_transcript_outputs: cvt(&p.public_transcript_outputs),
        binding_input: old_fr(p.binding_input),
        communications_commitment: p
            .communications_commitment
            .map(|(a, b)| (old_fr(a), old_fr(b))),
        key_location: old::KeyLocation(Cow::Owned(p.key_location.0.to_string())),
    })
}

/// Runs witness generation for `ir` without proving, via the legacy pipeline.
pub fn check(ir: &IrSource, preimage: &ProofPreimage) -> Result<Vec<Option<usize>>, anyhow::Error> {
    old::Zkir::check(ir, &old_preimage(preimage)?)
}

/// The `k` the legacy pipeline proves `ir` at.
pub fn k(ir: &IrSource) -> u8 {
    old::Zkir::k(ir)
}

impl<R: Rng + CryptoRng + SplittableRng, S: Resolver, P: ParamsProverProvider> ProvingProvider
    for LegacyProvingProvider<'_, R, S, P>
{
    async fn check(&self, preimage: &ProofPreimage) -> Result<Vec<Option<usize>>, anyhow::Error> {
        let data = self
            .resolver
            .resolve_key(preimage.key_location.clone())
            .await?
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "attempted to check proof for '{}' without circuit data!",
                    preimage.key_location.0
                )
            })?;
        check(
            &IrSource::load_from_tagged(io::Cursor::new(&data.ir_source[..]))?,
            preimage,
        )
    }

    async fn prove(
        self,
        preimage: &ProofPreimage,
        overwrite_binding_input: Option<Fr>,
    ) -> Result<Proof, anyhow::Error> {
        let mut preimage = old_preimage(preimage)?;
        if let Some(binding_input) = overwrite_binding_input {
            preimage.binding_input = old_fr(binding_input);
        }
        let (proof, _) = preimage
            .prove::<IrSource>(
                self.rng,
                &OldParams(self.params),
                &OldResolver(self.resolver),
            )
            .await?;
        Ok(Proof::from_bytes(proof.0))
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
