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
use serialize::{Tagged, peek_tag, tagged_deserialize, tagged_serialize};
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

#[derive(Debug)]
pub(crate) enum DeriveError {
    /// The IR is well-formed, but has no supported key generation.
    Unsupported(String),
    /// The IR could not be read.
    BadInput(String),
    /// Key generation failed.
    Failed(String),
}

/// An IR whose keys the server can generate.
pub(crate) enum DerivableIr {
    V2(zkir_v2::IrSource),
    V3(zkir_v3::IrSource),
}

impl DerivableIr {
    pub(crate) fn load(ir_source: &[u8]) -> Result<Self, DeriveError> {
        let bad_input = |e: std::io::Error| DeriveError::BadInput(e.to_string());
        let tag = peek_tag(&mut Cursor::new(ir_source)).map_err(bad_input)?;
        match tag.as_str() {
            "ir-source[v2]" | "ir-source[v2-generic]" => {
                let ir = zkir_v2::IrSource::load_from_tagged(Cursor::new(ir_source))
                    .map_err(bad_input)?;
                // V0 and V1 circuits use the legacy key generation, which is
                // not offered on demand.
                if ir.version != zkir_v2::IrMinorVersion::V2 {
                    return Err(DeriveError::Unsupported(format!(
                        "no on-demand key generation for ZKIR v2 minor version {:?}",
                        ir.version
                    )));
                }
                Ok(DerivableIr::V2(ir))
            }
            "ir-source[v3-generic]" => tagged_deserialize::<zkir_v3::IrSource>(ir_source)
                .map(DerivableIr::V3)
                .map_err(bad_input),
            _ => Err(DeriveError::Unsupported(format!(
                "Unsupported ZKIR tag: '{tag}'"
            ))),
        }
    }

    pub(crate) fn k(&self) -> u8 {
        match self {
            DerivableIr::V2(ir) => ir.k(),
            DerivableIr::V3(ir) => ir.k(),
        }
    }

    /// Generates the prover and verifier keys, tagged as the `zkir` CLI
    /// writes them.
    pub(crate) async fn keygen(&self) -> Result<(Vec<u8>, Vec<u8>), DeriveError> {
        match self {
            DerivableIr::V2(ir) => keygen_tagged(ir).await,
            DerivableIr::V3(ir) => keygen_tagged(ir).await,
        }
    }
}

async fn keygen_tagged<Z: Zkir + Tagged>(ir: &Z) -> Result<(Vec<u8>, Vec<u8>), DeriveError> {
    let (prover_key, verifier_key) = ir
        .keygen(&*PUBLIC_PARAMS)
        .await
        .map_err(|e| DeriveError::Failed(e.to_string()))?;
    let mut pk = Vec::new();
    tagged_serialize(&prover_key, &mut pk).map_err(|e| DeriveError::Failed(e.to_string()))?;
    let mut vk = Vec::new();
    tagged_serialize(&verifier_key, &mut vk).map_err(|e| DeriveError::Failed(e.to_string()))?;
    Ok((pk, vk))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn read(contract: &str, file: &str) -> Vec<u8> {
        let dir = std::env::var("MIDNIGHT_LEDGER_TEST_STATIC_DIR")
            .expect("MIDNIGHT_LEDGER_TEST_STATIC_DIR should be set as env variable");
        std::fs::read(format!("{dir}/{contract}/{file}")).expect("test artifact should exist")
    }

    #[tokio::test]
    async fn derived_keys_equal_compiled_keys() {
        for (contract, circuit) in [
            ("simple-merkle-tree", "check"),
            ("simple-merkle-tree", "store"),
            ("micro-dao", "setTopic"),
            ("micro-dao", "advance"),
        ] {
            let ir = DerivableIr::load(&read(contract, &format!("zkir/{circuit}.bzkir")))
                .expect("IR should be derivable");
            let (prover_key, verifier_key) = ir.keygen().await.expect("keygen should succeed");
            assert!(
                prover_key == read(contract, &format!("keys/{circuit}.prover")),
                "derived prover key for {contract}/{circuit} differs from the compiled one"
            );
            assert!(
                verifier_key == read(contract, &format!("keys/{circuit}.verifier")),
                "derived verifier key for {contract}/{circuit} differs from the compiled one"
            );
        }
    }

    #[test]
    fn legacy_v2_ir_is_not_derivable() {
        assert!(matches!(
            DerivableIr::load(&read("fallible", "zkir/count.bzkir")),
            Err(DeriveError::Unsupported(_))
        ));
    }
}
