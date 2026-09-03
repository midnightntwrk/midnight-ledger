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

use std::sync::Arc;

use ledger::prove::Resolver;
use rand::rngs::OsRng;
#[allow(unused_imports)]
use serialize::{peek_tag, tagged_deserialize};
use std::io::Cursor;
use transient_crypto::proofs::{Proof, ProofPreimage, Zkir};
use zkir as zkir_v2;

use crate::endpoints::PUBLIC_PARAMS;

pub(crate) fn k(request: &[u8]) -> Result<u8, String> {
    let tag = peek_tag(&mut std::io::Cursor::new(request)).map_err(|e| e.to_string())?;
    match tag.as_str() {
        "ir-source[v2]" | "ir-source[v2-generic]" => {
            let ir_v2 = zkir_v2::IrSource::load_from_tagged(Cursor::new(request))
                .map_err(|e| e.to_string())?;
            Ok(ir_v2.k())
        }
        "ir-source[v3-generic]" => {
            let ir_v3 =
                tagged_deserialize::<zkir_v3::IrSource>(request).map_err(|e| e.to_string())?;
            Ok(ir_v3.k())
        }
        _ => Err(format!("Unsupported ZKIR tag: '{tag}'")),
    }
}

pub(crate) fn check(ppi: Arc<ProofPreimage>, ir: &[u8]) -> Result<Vec<Option<usize>>, String> {
    let tag = peek_tag(&mut std::io::Cursor::new(ir)).map_err(|e| e.to_string())?;
    match tag.as_str() {
        "ir-source[v2]" | "ir-source[v2-generic]" => {
            let ir_v2 =
                zkir_v2::IrSource::load_from_tagged(Cursor::new(ir)).map_err(|e| e.to_string())?;
            ppi.check(&ir_v2).map_err(|e| e.to_string())
        }
        "ir-source[v3-generic]" => {
            let ir_v3 = tagged_deserialize::<zkir_v3::IrSource>(ir).map_err(|e| e.to_string())?;
            ppi.check(&ir_v3).map_err(|e| e.to_string())
        }
        _ => Err(format!("Unsupported ZKIR tag: '{tag}'")),
    }
}

pub(crate) async fn prove(
    ppi: Arc<ProofPreimage>,
    ir_source: &[u8],
    resolver: &Resolver,
) -> Result<(Proof, Vec<Option<usize>>), String> {
    let tag = peek_tag(&mut std::io::Cursor::new(ir_source)).map_err(|e| e.to_string())?;
    match tag.as_str() {
        "ir-source[v2]" | "ir-source[v2-generic]" => {
            let ir = zkir_v2::IrSource::load_from_tagged(Cursor::new(ir_source))
                .map_err(|e| e.to_string())?;
            // Use LocalProvingProvider for v2 IRs to handle V0/V1 backward compat routing.
            use base_crypto::rng::SplittableRng;
            use transient_crypto::proofs::ProvingProvider;

            let mut provider = zkir_v2::LocalProvingProvider {
                rng: OsRng.split(),
                resolver,
                params: &*PUBLIC_PARAMS,
            };
            let proof = provider
                .split()
                .prove(&ppi, None)
                .await
                .map_err(|e| e.to_string())?;
            let skips = ppi.check(&ir).map_err(|e| e.to_string())?;
            Ok((proof, skips))
        }
        "ir-source[v3-generic]" => {
            //let ir_source = tagged_deserialize::<zkir_v3::IrSource>(ir_source).map_err(|e| e.to_string())?;
            ppi.prove::<zkir_v3::IrSource>(OsRng, &*PUBLIC_PARAMS, resolver)
                .await
                .map_err(|e| e.to_string())
        }
        _ => Err(format!("Unsupported ZKIR tag: '{tag}'")),
    }
}

/// A [`ProvingProvider`] that routes each proof preimage to the prover matching
/// the ZKIR version of its resolved key material.
///
/// `/prove-tx` proves a whole transaction, whose calls may target circuits of
/// different ZKIR versions, so the version cannot be chosen up front the way
/// [`prove`] does for a single preimage.
pub(crate) struct VersionedProvingProvider<'a> {
    pub rng: OsRng,
    pub resolver: &'a Resolver,
}

impl<'a> VersionedProvingProvider<'a> {
    async fn ir_tag(&self, preimage: &ProofPreimage) -> Result<(String, Vec<u8>), anyhow::Error> {
        use transient_crypto::proofs::Resolver as _;
        let key_material = self
            .resolver
            .resolve_key(preimage.key_location.clone())
            .await?
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "could not resolve key location: {}",
                    preimage.key_location.0
                )
            })?;
        let tag = peek_tag(&mut Cursor::new(&key_material.ir_source))?;
        Ok((tag, key_material.ir_source))
    }
}

impl<'a> transient_crypto::proofs::ProvingProvider for VersionedProvingProvider<'a> {
    async fn check(&self, preimage: &ProofPreimage) -> Result<Vec<Option<usize>>, anyhow::Error> {
        let (tag, ir_source) = self.ir_tag(preimage).await?;
        match tag.as_str() {
            "ir-source[v2]" | "ir-source[v2-generic]" => {
                let ir = zkir_v2::IrSource::load_from_tagged(Cursor::new(&ir_source[..]))?;
                preimage.check(&ir)
            }
            "ir-source[v3-generic]" => {
                let ir: zkir_v3::IrSource = tagged_deserialize(&mut &ir_source[..])?;
                preimage.check(&ir)
            }
            _ => Err(anyhow::anyhow!("unsupported ZKIR tag: '{tag}'")),
        }
    }

    async fn prove(
        self,
        preimage: &ProofPreimage,
        overwrite_binding_input: Option<transient_crypto::curve::Fr>,
    ) -> Result<Proof, anyhow::Error> {
        let (tag, _) = self.ir_tag(preimage).await?;
        match tag.as_str() {
            "ir-source[v2]" | "ir-source[v2-generic]" => {
                // LocalProvingProvider handles the V0/V1 backward compat routing.
                let provider = zkir_v2::LocalProvingProvider {
                    rng: self.rng,
                    params: self.resolver,
                    resolver: self.resolver,
                };
                provider.prove(preimage, overwrite_binding_input).await
            }
            "ir-source[v3-generic]" => {
                let mut preimage = preimage.clone();
                if let Some(binding_input) = overwrite_binding_input {
                    preimage.binding_input = binding_input;
                }
                preimage
                    .prove::<zkir_v3::IrSource>(self.rng, self.resolver, self.resolver)
                    .await
                    .map(|(proof, _)| proof)
            }
            _ => Err(anyhow::anyhow!("unsupported ZKIR tag: '{tag}'")),
        }
    }

    fn split(&mut self) -> Self {
        use base_crypto::rng::SplittableRng;
        Self {
            rng: self.rng.split(),
            resolver: self.resolver,
        }
    }

    fn resolver(&self) -> &impl transient_crypto::proofs::Resolver {
        self.resolver
    }
}
