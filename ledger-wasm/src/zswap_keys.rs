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
pub struct ZswapSecretKeys(Option<zswap::keys::SecretKeys>);

impl ZswapSecretKeys {
    pub fn wrap(keys: zswap::keys::SecretKeys) -> Self {
        ZswapSecretKeys(Some(keys))
    }

    pub fn try_unwrap(&self) -> Result<&zswap::keys::SecretKeys, JsError> {
        self.0
            .as_ref()
            .ok_or(JsError::new("Secret keys were cleared"))
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
        Ok(ZswapSecretKeys::wrap(keys))
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
        Ok(ZswapSecretKeys::wrap(keys))
    }

    #[wasm_bindgen(js_name = "clear")]
    pub fn clear(&mut self) {
        self.0 = None;
    }

    #[wasm_bindgen(getter, js_name = "coinPublicKey")]
    pub fn coin_public_key(&self) -> Result<String, JsError> {
        to_value_hex_ser(&(self.try_unwrap()?.coin_public_key()))
    }

    #[wasm_bindgen(getter, js_name = "encryptionPublicKey")]
    pub fn encryption_public_key(&self) -> Result<String, JsError> {
        to_value_hex_ser(&(self.try_unwrap()?.enc_public_key()))
    }

    #[wasm_bindgen(getter, js_name = "encryptionSecretKey")]
    pub fn encryption_secret_key(&self) -> Result<EncryptionSecretKey, JsError> {
        Ok(EncryptionSecretKey::wrap(
            self.try_unwrap()?.encryption_secret_key,
        ))
    }

    #[wasm_bindgen(getter, js_name = "coinSecretKey")]
    pub fn coin_secret_key(&self) -> Result<CoinSecretKey, JsError> {
        Ok(CoinSecretKey::wrap(self.try_unwrap()?.coin_secret_key))
    }
}

#[wasm_bindgen]
pub struct CoinSecretKey(pub(crate) Option<coin::SecretKey>);

impl CoinSecretKey {
    pub fn wrap(key: coin::SecretKey) -> Self {
        CoinSecretKey(Some(key))
    }

    pub fn try_unwrap(&self) -> Result<&coin::SecretKey, JsError> {
        self.0
            .as_ref()
            .ok_or(JsError::new("Coin secret key was cleared"))
    }
}

#[wasm_bindgen]
impl CoinSecretKey {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Result<CoinSecretKey, JsError> {
        Err(JsError::new(
            "CoinSecretKey cannot be constructed directly through the WASM API.",
        ))
    }

    pub fn clear(&mut self) {
        self.0 = None;
    }

    #[wasm_bindgen(js_name = "yesIKnowTheSecurityImplicationsOfThis_serialize")]
    pub fn serialize(&self) -> Result<Uint8Array, JsError> {
        let mut res = Vec::new();
        tagged_serialize(self.try_unwrap()?, &mut res)?;
        Ok(Uint8Array::from(&res[..]))
    }
}

#[wasm_bindgen]
pub struct EncryptionSecretKey(pub(crate) Option<transient_crypto::encryption::SecretKey>);

impl From<transient_crypto::encryption::SecretKey> for EncryptionSecretKey {
    fn from(value: transient_crypto::encryption::SecretKey) -> Self {
        EncryptionSecretKey(Some(value))
    }
}

impl EncryptionSecretKey {
    pub fn wrap(key: transient_crypto::encryption::SecretKey) -> Self {
        EncryptionSecretKey(Some(key))
    }

    pub fn try_unwrap(&self) -> Result<&transient_crypto::encryption::SecretKey, JsError> {
        self.0
            .as_ref()
            .ok_or(JsError::new("Encryption secret key was cleared"))
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

    pub fn clear(&mut self) {
        self.0 = None;
    }

    pub fn test(&self, offer: &ZswapOffer) -> Result<bool, JsError> {
        use crate::zswap_wasm::ZswapOfferTypes::*;
        let sk_unwrapped = self.try_unwrap()?;
        Ok(match &offer.0 {
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
                    sk_unwrapped
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
                    sk_unwrapped
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
                    sk_unwrapped
                        .decrypt::<coin::Info>(&(*ciphertext).deref().clone().into())
                        .is_some()
                }),
        })
    }

    #[wasm_bindgen(js_name = "yesIKnowTheSecurityImplicationsOfThis_serialize")]
    pub fn serialize(&self) -> Result<Uint8Array, JsError> {
        let mut res = Vec::new();
        tagged_serialize(self.try_unwrap()?, &mut res)?;
        Ok(Uint8Array::from(&res[..]))
    }

    pub fn deserialize(raw: Uint8Array) -> Result<EncryptionSecretKey, JsError> {
        Ok(EncryptionSecretKey::wrap(from_value_ser(
            raw,
            "EncryptionSecretKey",
        )?))
    }
}
