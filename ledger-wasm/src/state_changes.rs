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

// Shared utilities for converting ZswapStateChanges and DustStateChanges to WASM types
// This module provides common functionality that can be used by both
// Zswap and Dust implementations

use crate::conversions::*;
use js_sys::Array;
use ledger::dust::DustStateChanges as LedgerDustStateChanges;
use ledger::zswap::ZswapStateChanges as LedgerZswapStateChanges;
use wasm_bindgen::prelude::*;

/// WASM wrapper for ZswapStateChanges (used by Zswap)
#[wasm_bindgen]
#[derive(Clone)]
pub struct ZswapStateChanges {
    pub(crate) inner: LedgerZswapStateChanges,
}

#[wasm_bindgen]
impl ZswapStateChanges {
    #[wasm_bindgen(getter)]
    pub fn source(&self) -> Result<String, JsError> {
        to_hex_ser(&self.inner.source)
    }

    #[wasm_bindgen(getter, js_name = "receivedCoins")]
    pub fn received_coins(&self) -> Result<Array, JsError> {
        let coins = Array::new();
        for coin in &self.inner.received_coins {
            coins.push(&qualified_shielded_coininfo_to_value(coin)?);
        }
        Ok(coins)
    }

    #[wasm_bindgen(getter, js_name = "spentCoins")]
    pub fn spent_coins(&self) -> Result<Array, JsError> {
        let coins = Array::new();
        for coin in &self.inner.spent_coins {
            coins.push(&qualified_shielded_coininfo_to_value(coin)?);
        }
        Ok(coins)
    }
}

impl From<LedgerZswapStateChanges> for ZswapStateChanges {
    fn from(inner: LedgerZswapStateChanges) -> Self {
        ZswapStateChanges { inner }
    }
}

// WASM wrapper for DustStateChanges (used by Dust)
#[wasm_bindgen]
#[derive(Clone)]
pub struct DustStateChanges {
    pub(crate) inner: LedgerDustStateChanges,
}

#[wasm_bindgen]
impl DustStateChanges {
    #[wasm_bindgen(getter)]
    pub fn source(&self) -> Result<String, JsError> {
        to_hex_ser(&self.inner.source)
    }

    #[wasm_bindgen(getter, js_name = "receivedUtxos")]
    pub fn received_utxos(&self) -> Result<Array, JsError> {
        let utxos = Array::new();
        for utxo in &self.inner.received_utxos {
            utxos.push(&qdo_to_value(utxo)?);
        }
        Ok(utxos)
    }

    #[wasm_bindgen(getter, js_name = "spentUtxos")]
    pub fn spent_utxos(&self) -> Result<Array, JsError> {
        let utxos = Array::new();
        for utxo in &self.inner.spent_utxos {
            utxos.push(&qdo_to_value(utxo)?);
        }
        Ok(utxos)
    }
}

impl From<LedgerDustStateChanges> for DustStateChanges {
    fn from(inner: LedgerDustStateChanges) -> Self {
        DustStateChanges { inner }
    }
}
