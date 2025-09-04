// This file is part of midnight-ledger.
// Copyright (C) 2025 Midnight Foundation
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

use crate::to_value_hex_ser;
use crate::zswap_wasm::ZswapOffer;
use coin_structure::coin;
use js_sys::Uint8Array;
use onchain_runtime_wasm::from_value_ser;
use rand::SeedableRng;
use rand_chacha::ChaCha20Rng;
use serialize::tagged_serialize;
use std::ops::Deref;
use wasm_bindgen::JsError;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct ZswapSecretKeys(zswap::keys::SecretKeys);

impl ZswapSecretKeys {
    pub fn wrap(keys: zswap::keys::SecretKeys) -> Self {
        ZswapSecretKeys(keys)
    }

    pub fn unwrap(&self) -> &zswap::keys::SecretKeys {
        &self.0
    }
}

#[wasm_bindgen]
impl ZswapSecretKeys {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Result<ZswapSecretKeys, JsError> {
        Err(JsError::new(
            "SecretKeys cannot be constructed directly through the WASM API.",
        ))
    }

    #[wasm_bindgen(js_name = "fromSeed")]
    pub fn from_seed(seed: Uint8Array) -> Result<ZswapSecretKeys, JsError> {
        let bytes: [u8; 32] = seed
            .to_vec()
            .try_into()
            .map_err(|_| JsError::new("Expected 32-byte seed"))?;
        let seed_parsed = zswap::keys::Seed::from(bytes);
        let keys = zswap::keys::SecretKeys::from(seed_parsed);
        Ok(ZswapSecretKeys(keys))
    }

    #[wasm_bindgen(js_name = "fromSeedRng")]
    pub fn from_seed_rng(seed: Uint8Array) -> Result<ZswapSecretKeys, JsError> {
        let bytes = seed.to_vec();
        let mut rng = ChaCha20Rng::from_seed(
            bytes
                .try_into()
                .map_err(|_| JsError::new("Expected 32-byte seed"))?,
        );
        let keys = zswap::keys::SecretKeys::from_rng_seed(&mut rng);
        Ok(ZswapSecretKeys(keys))
    }

    #[wasm_bindgen(getter, js_name = "coinPublicKey")]
    pub fn coin_public_key(&self) -> Result<String, JsError> {
        to_value_hex_ser(&(self.0.coin_public_key()))
    }

    #[wasm_bindgen(getter, js_name = "encryptionPublicKey")]
    pub fn encryption_public_key(&self) -> Result<String, JsError> {
        to_value_hex_ser(&(self.0.enc_public_key()))
    }

    #[wasm_bindgen(getter, js_name = "encryptionSecretKey")]
    pub fn encryption_secret_key(&self) -> Result<EncryptionSecretKey, JsError> {
        Ok(EncryptionSecretKey(self.0.encryption_secret_key))
    }

    #[wasm_bindgen(getter, js_name = "coinSecretKey")]
    pub fn coin_secret_key(&self) -> Result<CoinSecretKey, JsError> {
        Ok(CoinSecretKey(self.0.coin_secret_key))
    }
}

#[wasm_bindgen]
pub struct CoinSecretKey(pub(crate) coin::SecretKey);

#[wasm_bindgen]
impl CoinSecretKey {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Result<CoinSecretKey, JsError> {
        Err(JsError::new(
            "CoinSecretKey cannot be constructed directly through the WASM API.",
        ))
    }

    #[wasm_bindgen(js_name = "yesIKnowTheSecurityImplicationsOfThis_serialize")]
    pub fn serialize(&self) -> Result<Uint8Array, JsError> {
        let mut res = Vec::new();
        tagged_serialize(&self.0, &mut res)?;
        Ok(Uint8Array::from(&res[..]))
    }
}

#[wasm_bindgen]
pub struct EncryptionSecretKey(pub(crate) transient_crypto::encryption::SecretKey);

impl From<transient_crypto::encryption::SecretKey> for EncryptionSecretKey {
    fn from(value: transient_crypto::encryption::SecretKey) -> Self {
        EncryptionSecretKey(value)
    }
}

#[wasm_bindgen]
impl EncryptionSecretKey {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Result<EncryptionSecretKey, JsError> {
        Err(JsError::new(
            "EncryptionSecretKey cannot be constructed directly through the WASM API.",
        ))
    }

    pub fn test(&self, offer: &ZswapOffer) -> bool {
        use crate::zswap_wasm::ZswapOfferTypes::*;
        match &offer.0 {
            ProvenOffer(val) => val
                .outputs
                .iter_deref()
                .filter_map(|o| o.ciphertext.as_ref())
                .chain(
                    val.transient
                        .iter_deref()
                        .filter_map(|io| io.ciphertext.as_ref()),
                )
                .any(|ciphertext| {
                    self.0
                        .decrypt::<coin::Info>(&(*ciphertext).deref().clone().into())
                        .is_some()
                }),
            UnprovenOffer(val) => val
                .outputs
                .iter_deref()
                .filter_map(|o| o.ciphertext.as_ref())
                .chain(
                    val.transient
                        .iter_deref()
                        .filter_map(|io| io.ciphertext.as_ref()),
                )
                .any(|ciphertext| {
                    self.0
                        .decrypt::<coin::Info>(&(*ciphertext).deref().clone().into())
                        .is_some()
                }),
            ProofErasedOffer(val) => val
                .outputs
                .iter_deref()
                .filter_map(|o| o.ciphertext.as_ref())
                .chain(
                    val.transient
                        .iter_deref()
                        .filter_map(|io| io.ciphertext.as_ref()),
                )
                .any(|ciphertext| {
                    self.0
                        .decrypt::<coin::Info>(&(*ciphertext).deref().clone().into())
                        .is_some()
                }),
        }
    }

    #[wasm_bindgen(js_name = "yesIKnowTheSecurityImplicationsOfThis_serialize")]
    pub fn serialize(&self) -> Result<Uint8Array, JsError> {
        let mut res = Vec::new();
        tagged_serialize(&self.0, &mut res)?;
        Ok(Uint8Array::from(&res[..]))
    }

    pub fn deserialize(raw: Uint8Array) -> Result<EncryptionSecretKey, JsError> {
        Ok(EncryptionSecretKey(from_value_ser(
            raw,
            "EncryptionSecretKey",
        )?))
    }
}
