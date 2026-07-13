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

#[cfg(feature = "experimental")]
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

#[cfg(not(feature = "experimental"))]
pub(crate) fn k(request: &[u8]) -> Result<u8, String> {
    if let Ok(ir_v2) = zkir_v2::IrSource::load_from_tagged(Cursor::new(request)) {
        Ok(ir_v2.k())
    } else {
        Err("Unsupported ZKIR version".into())
    }
}

#[cfg(feature = "experimental")]
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

#[cfg(not(feature = "experimental"))]
pub(crate) fn check(ppi: Arc<ProofPreimage>, ir: &[u8]) -> Result<Vec<Option<usize>>, String> {
    if let Ok(ir_v2) = zkir_v2::IrSource::load_from_tagged(Cursor::new(ir)) {
        ppi.check(&ir_v2).map_err(|e| e.to_string())
    } else {
        Err("Unsupported ZKIR version".to_string())
    }
}

#[cfg(feature = "experimental")]
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

#[cfg(not(feature = "experimental"))]
pub(crate) async fn prove(
    ppi: Arc<ProofPreimage>,
    ir_source: &[u8],
    resolver: &Resolver,
) -> Result<(Proof, Vec<Option<usize>>), String> {
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
    let ir =
        zkir_v2::IrSource::load_from_tagged(Cursor::new(ir_source)).map_err(|e| e.to_string())?;
    let skips = ppi.check(&ir).map_err(|e| e.to_string())?;
    Ok((proof, skips))
}
