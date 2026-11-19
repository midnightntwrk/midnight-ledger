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

use ledger::legacy_proving;
use ledger::prove::Resolver;
use rand::rngs::OsRng;
use serialize::{peek_tag, tagged_deserialize};
use transient_crypto::proofs::{Proof, ProofPreimage, ProvingKeyMaterial, Zkir};

use crate::endpoints::PUBLIC_PARAMS;

pub(crate) fn k(request: &[u8]) -> Result<u8, String> {
    let tag = peek_tag(&mut std::io::Cursor::new(request)).map_err(|e| e.to_string())?;
    match tag.as_str() {
        "ir-source[v2]" | "ir-source[v2-generic]" | "ir-source[v3-generic]" => {
            legacy_proving::k(request).map_err(|e| e.to_string())
        }
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
        "ir-source[v2]" | "ir-source[v2-generic]" | "ir-source[v3-generic]" => {
            legacy_proving::check(&ppi, ir).map_err(|e| e.to_string())
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
) -> Result<Proof, String> {
    let tag =
        peek_tag(&mut std::io::Cursor::new(&keys.ir_source[..])).map_err(|e| e.to_string())?;
    match tag.as_str() {
        "ir-source[v2]" | "ir-source[v2-generic]" | "ir-source[v3-generic]" => {
            legacy_proving::prove(&ppi, keys, OsRng, &*PUBLIC_PARAMS)
                .await
                .map_err(|e| e.to_string())
        }
        "ir-source[v4-generic]" => ppi
            .prove::<zkir_v4::IrSource>(OsRng, &*PUBLIC_PARAMS, resolver)
            .await
            .map(|(proof, _)| proof)
            .map_err(|e| e.to_string()),
        _ => Err(format!("Unsupported ZKIR tag: '{tag}'")),
    }
}
