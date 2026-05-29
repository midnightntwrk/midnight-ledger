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
use std::borrow::Cow;
use transient_crypto::curve::Fr;
use transient_crypto::proofs::{
    ParamsProverProvider, Proof, ProofPreimage, ProvingProvider, Resolver,
};

mod ir;
mod ir_vm;

pub use ir::{
    Instruction, IrMinorVersion, IrSource, OldIrSourceV2, fr_from_old, fr_to_old, ir_to_old,
    preimage_from_old, preimage_to_old,
};
pub use ir_vm::Preprocessed;

// ── Adapters for routing proving through old transient-crypto ────────────

/// Wraps a new [`Resolver`] as an old one, passing key material bytes through.
struct OldResolverAdapter<'a, S: Resolver>(&'a S);

impl<S: Resolver> transient_crypto_old::proofs::Resolver for OldResolverAdapter<'_, S> {
    async fn resolve_key(
        &self,
        key: transient_crypto_old::proofs::KeyLocation,
    ) -> std::io::Result<Option<transient_crypto_old::proofs::ProvingKeyMaterial>> {
        let new_key = transient_crypto::proofs::KeyLocation(Cow::Owned(key.0.into_owned()));
        match self.0.resolve_key(new_key).await? {
            Some(m) => Ok(Some(transient_crypto_old::proofs::ProvingKeyMaterial {
                prover_key: m.prover_key,
                verifier_key: m.verifier_key,
                ir_source: m.ir_source,
            })),
            None => Ok(None),
        }
    }
}

/// Wraps a new [`ParamsProverProvider`] as an old one by serializing params
/// through the shared raw-bytes format.
struct OldParamsAdapter<'a, P: ParamsProverProvider>(&'a P);

impl<P: ParamsProverProvider> transient_crypto_old::proofs::ParamsProverProvider
    for OldParamsAdapter<'_, P>
{
    async fn get_params(
        &self,
        k: u8,
    ) -> std::io::Result<transient_crypto_old::proofs::ParamsProver> {
        use midnight_proofs::poly::commitment::Params;
        let new_params = self.0.get_params(k).await?;
        let mut buf = Vec::new();
        new_params
            .0
            .write_custom(&mut buf, midnight_proofs::utils::SerdeFormat::RawBytesUnchecked)?;
        transient_crypto_old::proofs::ParamsProver::read(&buf[..])
    }
}

/// Implements `ProvingProvider` locally, routing through the old (pinned)
/// transient-crypto pipeline.
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
        use transient_crypto_old::proofs::Resolver as OldResolver;
        let old_preimage = preimage_to_old(preimage);
        let old_resolver = OldResolverAdapter(self.resolver);
        let proving_data = OldResolver::resolve_key(&old_resolver, old_preimage.key_location.clone())
            .await?
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "attempted to check proof for '{}' without circuit data!",
                    preimage.key_location.0
                )
            })?;
        let ir = OldIrSourceV2::load_from_tagged(std::io::Cursor::new(
            &proving_data.ir_source[..],
        ))?;
        old_preimage.check(&ir).map_err(Into::into)
    }
    async fn prove(
        self,
        preimage: &ProofPreimage,
        overwrite_binding_input: Option<Fr>,
    ) -> Result<Proof, anyhow::Error> {
        let mut old_preimage = preimage_to_old(preimage);
        if let Some(binding_input) = overwrite_binding_input {
            old_preimage.binding_input = fr_to_old(binding_input);
        }
        let (old_proof, _) = old_preimage
            .prove::<OldIrSourceV2>(
                self.rng,
                &OldParamsAdapter(self.params),
                &OldResolverAdapter(self.resolver),
            )
            .await?;
        Ok(Proof(old_proof.0))
    }
    fn split(&mut self) -> Self {
        Self {
            rng: self.rng.split(),
            resolver: self.resolver,
            params: self.params,
        }
    }
}
