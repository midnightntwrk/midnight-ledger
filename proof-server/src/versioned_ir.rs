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
use transient_crypto::proofs::{Proof, ProofPreimage, ProvingKeyMaterial, Zkir};
use zswap::prove::legacy;

use crate::endpoints::PUBLIC_PARAMS;

pub(crate) fn k(request: &[u8]) -> Result<u8, String> {
    let tag = peek_tag(&mut std::io::Cursor::new(request)).map_err(|e| e.to_string())?;
    match tag.as_str() {
        "ir-source[v2]" | "ir-source[v2-generic]" => {
            let ir_v2 = zkir_v2::IrSource::load_from_tagged(Cursor::new(request))
                .map_err(|e| e.to_string())?;
            Ok(legacy::k(&ir_v2))
        }
        "ir-source[v3-generic]" => ledger::ledger_9_proving::k(request).map_err(|e| e.to_string()),
        "ir-source[v4-generic]" => {
            let ir_v4 =
                tagged_deserialize::<zkir_v4::IrSource>(request).map_err(|e| e.to_string())?;
            Ok(ir_v4.k())
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
            legacy::check(&ir_v2, &ppi).map_err(|e| e.to_string())
        }
        "ir-source[v3-generic]" => {
            ledger::ledger_9_proving::check(&ppi, ir).map_err(|e| e.to_string())
        }
        "ir-source[v4-generic]" => {
            let ir_v4 = tagged_deserialize::<zkir_v4::IrSource>(ir).map_err(|e| e.to_string())?;
            ppi.check(&ir_v4).map_err(|e| e.to_string())
        }
        _ => Err(format!("Unsupported ZKIR tag: '{tag}'")),
    }
}

pub(crate) async fn prove(
    ppi: Arc<ProofPreimage>,
    keys: &ProvingKeyMaterial,
    resolver: &Resolver,
) -> Result<(Proof, Vec<Option<usize>>), String> {
    if ledger::ledger_9_proving::is_ledger_9_key(&keys.verifier_key) {
        return ledger::ledger_9_proving::prove(&ppi, keys, OsRng, &*PUBLIC_PARAMS)
            .await
            .map_err(|e| e.to_string());
    }
    let ir_source = &keys.ir_source[..];
    let tag = peek_tag(&mut std::io::Cursor::new(ir_source)).map_err(|e| e.to_string())?;
    match tag.as_str() {
        "ir-source[v2]" | "ir-source[v2-generic]" => {
            let ir = zkir_v2::IrSource::load_from_tagged(Cursor::new(ir_source))
                .map_err(|e| e.to_string())?;
            use base_crypto::rng::SplittableRng;
            use transient_crypto::proofs::ProvingProvider;

            let mut provider = legacy::LegacyProvingProvider {
                rng: OsRng.split(),
                resolver,
                params: &*PUBLIC_PARAMS,
            };
            let proof = provider
                .split()
                .prove(&ppi, None)
                .await
                .map_err(|e| e.to_string())?;
            let skips = legacy::check(&ir, &ppi).map_err(|e| e.to_string())?;
            Ok((proof, skips))
        }
        // The key check above takes every `ir-source[v3-generic]` circuit.
        "ir-source[v3-generic]" => {
            Err("ZKIR v3 is only proven against verifier-key[v7] keys".into())
        }
        "ir-source[v4-generic]" => ppi
            .prove::<zkir_v4::IrSource>(OsRng, &*PUBLIC_PARAMS, resolver)
            .await
            .map_err(|e| e.to_string()),
        _ => Err(format!("Unsupported ZKIR tag: '{tag}'")),
    }
}
