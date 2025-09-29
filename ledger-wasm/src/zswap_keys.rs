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
use serialize::Deserializable;
use serialize::Serializable;
use serialize::tagged_serialize;
use std::cell::RefCell;
use std::ops::Deref;
use std::rc::Rc;
use wasm_bindgen::JsError;
use wasm_bindgen::prelude::*;
use zeroize::{Zeroize, ZeroizeOnDrop};

/// Wrapper of this shape allows to hold the exact same types as exposed in the WASM bindings
/// This allows to limit number of copies of the keys, without creating ones that might be out of Rust's control
#[derive(ZeroizeOnDrop)]
struct SecretKeysWrapper {
    coin_secret_key: CoinSecretKey,
    encryption_secret_key: EncryptionSecretKey,
}
impl SecretKeysWrapper {
    pub fn wrap(keys: zswap::keys::SecretKeys) -> Self {
        SecretKeysWrapper {
            coin_secret_key: CoinSecretKey::wrap(keys.coin_secret_key),
            encryption_secret_key: EncryptionSecretKey::wrap(keys.encryption_secret_key),
        }
    }

    pub fn clear(&mut self) {
        self.coin_secret_key.clear();
        self.encryption_secret_key.clear();
    }
}

impl TryInto<zswap::keys::SecretKeys> for &SecretKeysWrapper {
    type Error = JsError;
    fn try_into(self) -> Result<zswap::keys::SecretKeys, Self::Error> {
        Ok(zswap::keys::SecretKeys {
            coin_secret_key: self.coin_secret_key.try_unwrap()?,
            encryption_secret_key: self.encryption_secret_key.try_unwrap()?,
        })
    }
}

impl Zeroize for SecretKeysWrapper {
    fn zeroize(&mut self) {
        let _ = self.clear();
    }
}

#[wasm_bindgen(getter_with_clone)]
pub struct ZswapSecretKeys(Option<SecretKeysWrapper>);

impl ZswapSecretKeys {
    pub fn wrap(keys: zswap::keys::SecretKeys) -> Self {
        ZswapSecretKeys(Some(SecretKeysWrapper::wrap(keys)))
    }
}

impl TryInto<zswap::keys::SecretKeys> for &ZswapSecretKeys {
    type Error = JsError;
    fn try_into(self) -> Result<zswap::keys::SecretKeys, Self::Error> {
        self.0
            .as_ref()
            .and_then(|wrapper| wrapper.try_into().ok())
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
        self.0.as_mut().map(|wrapper| wrapper.clear());
        self.0 = None;
    }

    #[wasm_bindgen(getter, js_name = "coinPublicKey")]
    pub fn coin_public_key(&self) -> Result<String, JsError> {
        let value = self
            .0
            .as_ref()
            .map(|wrapper| &wrapper.coin_secret_key)
            .ok_or(JsError::new("Secret keys were cleared"))?;
        value.public_key()
    }

    #[wasm_bindgen(getter, js_name = "encryptionPublicKey")]
    pub fn encryption_public_key(&self) -> Result<String, JsError> {
        let value = self
            .0
            .as_ref()
            .map(|wrapper| &wrapper.encryption_secret_key)
            .ok_or(JsError::new("Secret keys were cleared"))?;
        value.public_key()
    }

    #[wasm_bindgen(getter, js_name = "encryptionSecretKey")]
    pub fn encryption_secret_key(&self) -> Result<EncryptionSecretKey, JsError> {
        return self
            .0
            .as_ref()
            .map(|wrapper| wrapper.encryption_secret_key.clone())
            .ok_or(JsError::new("Secret keys were cleared"));
    }

    #[wasm_bindgen(getter, js_name = "coinSecretKey")]
    pub fn coin_secret_key(&self) -> Result<CoinSecretKey, JsError> {
        self.0
            .as_ref()
            .map(|wrapper| wrapper.coin_secret_key.clone())
            .ok_or(JsError::new("Secret keys were cleared"))
    }
}

#[wasm_bindgen]
pub struct CoinSecretKey(pub(crate) Rc<RefCell<Option<coin::SecretKey>>>);

const CSK_CLEAR_MSG: &str = "Coin secret key was cleared";

impl CoinSecretKey {
    pub fn wrap(key: coin::SecretKey) -> Self {
        CoinSecretKey(Rc::new(RefCell::new(Some(key))))
    }

    pub fn try_unwrap(&self) -> Result<coin::SecretKey, JsError> {
        self.0
            .borrow()
            .as_ref()
            .cloned()
            .ok_or(JsError::new(CSK_CLEAR_MSG))
    }
}

impl Zeroize for CoinSecretKey {
    fn zeroize(&mut self) {
        self.clear();
    }
}

impl Clone for CoinSecretKey {
    fn clone(&self) -> Self {
        CoinSecretKey(Rc::clone(&self.0))
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

    pub fn public_key(&self) -> Result<String, JsError> {
        let pk = self
            .0
            .borrow()
            .as_ref()
            .ok_or(JsError::new(CSK_CLEAR_MSG))?
            .public_key();
        to_value_hex_ser(&pk)
    }

    pub fn clear(&mut self) {
        self.0.borrow_mut().take();
    }

    #[wasm_bindgen(js_name = "yesIKnowTheSecurityImplicationsOfThis_serialize")]
    pub fn serialize(&self) -> Result<Uint8Array, JsError> {
        let mut res = Vec::new();
        tagged_serialize(
            self.0
                .borrow()
                .as_ref()
                .ok_or(JsError::new(CSK_CLEAR_MSG))?,
            &mut res,
        )?;
        Ok(Uint8Array::from(&res[..]))
    }
}

#[wasm_bindgen]
#[derive(Clone)]
pub struct EncryptionSecretKey(
    pub(crate) Rc<RefCell<Option<transient_crypto::encryption::SecretKey>>>,
);

const ESK_CLEAR_MSG: &str = "Encryption secret key was cleared";

impl From<transient_crypto::encryption::SecretKey> for EncryptionSecretKey {
    fn from(value: transient_crypto::encryption::SecretKey) -> Self {
        EncryptionSecretKey(Rc::new(RefCell::new(Some(value))))
    }
}

impl Zeroize for EncryptionSecretKey {
    fn zeroize(&mut self) {
        self.clear();
    }
}

impl EncryptionSecretKey {
    pub fn wrap(key: transient_crypto::encryption::SecretKey) -> Self {
        EncryptionSecretKey(Rc::new(RefCell::new(Some(key))))
    }

    pub fn try_unwrap(&self) -> Result<transient_crypto::encryption::SecretKey, JsError> {
        self.0
            .borrow()
            .as_ref()
            .cloned()
            .ok_or(JsError::new(ESK_CLEAR_MSG))
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
        self.0.borrow_mut().take();
    }

    pub fn public_key(&self) -> Result<String, JsError> {
        let pk = self
            .0
            .borrow()
            .as_ref()
            .ok_or(JsError::new(ESK_CLEAR_MSG))?
            .public_key();
        to_value_hex_ser(&pk)
    }

    pub fn test(&self, offer: &ZswapOffer) -> Result<bool, JsError> {
        use crate::zswap_wasm::ZswapOfferTypes::*;
        let sk_wrap = self.0.borrow();
        let sk_unwrapped = sk_wrap.as_ref().ok_or(JsError::new(ESK_CLEAR_MSG))?;
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

    #[wasm_bindgen(js_name = "yesIKnowTheSecurityImplicationsOfThis_taggedSerialize")]
    pub fn tagged_serialize(&self) -> Result<Uint8Array, JsError> {
        let mut res = Vec::new();
        tagged_serialize(
            self.0
                .borrow()
                .as_ref()
                .ok_or(JsError::new(ESK_CLEAR_MSG))?,
            &mut res,
        )?;
        Ok(Uint8Array::from(&res[..]))
    }

    #[wasm_bindgen(js_name = "yesIKnowTheSecurityImplicationsOfThis_serialize")]
    pub fn serialize(&self) -> Result<Uint8Array, JsError> {
        let mut res = Vec::new();
        Serializable::serialize(
            self.0
                .borrow()
                .as_ref()
                .ok_or(JsError::new(ESK_CLEAR_MSG))?,
            &mut res,
        )?;
        Ok(Uint8Array::from(&res[..]))
    }

    #[wasm_bindgen(js_name = "taggedDeserialize")]
    pub fn tagged_deserialize(raw: Uint8Array) -> Result<EncryptionSecretKey, JsError> {
        Ok(EncryptionSecretKey::wrap(from_value_ser(
            raw,
            "EncryptionSecretKey",
        )?))
    }

    pub fn deserialize(raw: Uint8Array) -> Result<EncryptionSecretKey, JsError> {
        use std::io::{Error, ErrorKind};
        let deserialized: transient_crypto::encryption::SecretKey =
            Deserializable::deserialize(&mut raw.to_vec().as_slice(), 0).map_err(|e| {
                Error::new(
                    ErrorKind::InvalidData,
                    format!(
                        "Unable to deserialize {}. Error: {}",
                        "EncryptionSecretKey",
                        e.to_string()
                    ),
                )
            })?;
        Ok(EncryptionSecretKey::wrap(deserialized))
    }
}
