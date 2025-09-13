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

use crate::conversions::*;
use crate::dust::Event;
use crate::zswap_keys::ZswapSecretKeys;
use crate::zswap_wasm::{ZswapInput, ZswapOffer, ZswapOfferTypes, ZswapOutput, ZswapTransient};
use base_crypto::time::Timestamp;
use coin_structure::{
    coin::{
        Commitment, Info as CoinInfo, PublicKey as CoinPublicKey,
        QualifiedInfo as QualifiedCoinInfo,
    },
    contract::ContractAddress as Address,
};
use js_sys::{Array, Date, JsString, Map, Set, Uint8Array};
use ledger::semantics::ZswapLocalStateExt;
use onchain_runtime_wasm::from_value_ser;
use rand::Rng;
use rand::rngs::OsRng;
use serialize::tagged_serialize;
use storage::{db::InMemoryDB, storage::Map as SMap};
use transient_crypto::merkle_tree;
use wasm_bindgen::prelude::*;

#[wasm_bindgen(js_name = "createCoinInfo")]
pub fn create_coin_info(type_: &str, value: JsValue) -> Result<JsValue, JsError> {
    shielded_coininfo_to_value(&CoinInfo {
        type_: from_hex_ser(type_)?,
        value: from_value(value)?,
        nonce: OsRng.r#gen(),
    })
}

#[wasm_bindgen]
pub struct MerkleTreeCollapsedUpdate(pub(crate) merkle_tree::MerkleTreeCollapsedUpdate);

impl AsRef<merkle_tree::MerkleTreeCollapsedUpdate> for MerkleTreeCollapsedUpdate {
    fn as_ref(&self) -> &merkle_tree::MerkleTreeCollapsedUpdate {
        &self.0
    }
}

#[wasm_bindgen]
impl MerkleTreeCollapsedUpdate {
    #[wasm_bindgen(constructor)]
    pub fn new(
        state: &ZswapChainState,
        start: u64,
        end: u64,
    ) -> Result<MerkleTreeCollapsedUpdate, JsError> {
        Ok(MerkleTreeCollapsedUpdate(
            merkle_tree::MerkleTreeCollapsedUpdate::new(&state.0.coin_coms, start, end)?,
        ))
    }

    pub fn serialize(&self) -> Result<Uint8Array, JsError> {
        let mut res = Vec::new();
        tagged_serialize(&self.0, &mut res)?;
        Ok(Uint8Array::from(&res[..]))
    }

    pub fn deserialize(raw: Uint8Array) -> Result<MerkleTreeCollapsedUpdate, JsError> {
        Ok(MerkleTreeCollapsedUpdate(from_value_ser(
            raw,
            "MerkleTreeCollapsedUpdate",
        )?))
    }

    #[wasm_bindgen(js_name = "toString")]
    pub fn to_string(&self, compact: Option<bool>) -> String {
        if compact.unwrap_or(false) {
            format!("{:?}", &self.0)
        } else {
            format!("{:#?}", &self.0)
        }
    }
}

#[wasm_bindgen]
pub struct ZswapLocalState(pub(crate) zswap::local::State<InMemoryDB>);

#[wasm_bindgen]
impl ZswapLocalState {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        ZswapLocalState(zswap::local::State::new())
    }

    #[wasm_bindgen(getter = firstFree, js_name = "firstFree")]
    pub fn first_free(&self) -> u64 {
        self.0.first_free
    }

    // coins: Set<QualifiedShieldedCoinInfo>
    #[wasm_bindgen(getter)]
    pub fn coins(&self) -> Result<Set, JsError> {
        let res = Set::new(&JsValue::NULL);
        for (_, coin) in self.0.coins.iter() {
            res.add(&qualified_shielded_coininfo_to_value(&coin)?);
        }
        Ok(res)
    }

    // pendingSpends: Map<Uint8Array, [QualifiedShieldedCoinInfo, Date | undefined]>
    #[wasm_bindgen(getter, js_name = "pendingSpends")]
    pub fn pending_spends(&self) -> Result<Map, JsError> {
        let res = Map::new();
        for (nul, coin) in self.0.pending_spends.iter() {
            let tuple = Array::new();
            tuple.push(&qualified_shielded_coininfo_to_value(&coin)?);
            // add date value to tuple
            // tuple.push(&seconds_to_js_date(val.ttl.to_secs())
            tuple.push(&JsValue::UNDEFINED);
            res.set(&JsString::from(to_hex_ser(&nul)?), &tuple.into());
        }
        Ok(res)
    }

    // pendingOutputs: Map<Uint8Array, [ShieldedCoinInfo, Date | undefined]>
    #[wasm_bindgen(getter, js_name = "pendingOutputs")]
    pub fn pending_outputs(&self) -> Result<Map, JsError> {
        let res = Map::new();
        for (cm, coin) in self.0.pending_outputs.iter() {
            let tuple = Array::new();
            tuple.push(&shielded_coininfo_to_value(&coin)?);
            // add date value to tuple
            // tuple.push(&seconds_to_js_date(val.ttl.to_secs())
            tuple.push(&JsValue::UNDEFINED);
            res.set(&JsString::from(to_hex_ser(&cm)?), &tuple.into());
        }
        Ok(res)
    }

    #[wasm_bindgen(js_name = "replayEvents")]
    pub fn replay_events(
        &self,
        secret_keys: &ZswapSecretKeys,
        events: Vec<Event>,
    ) -> Result<ZswapLocalState, JsError> {
        let events = events.iter().map(|event| &event.0);
        Ok(ZswapLocalState(
            self.0.replay_events(&secret_keys.try_into()?, events)?,
        ))
    }

    pub fn apply(
        &self,
        secret_keys: &ZswapSecretKeys,
        offer: &ZswapOffer,
    ) -> Result<ZswapLocalState, JsError> {
        use ZswapOfferTypes::*;
        let sk_unwrapped = secret_keys.try_into()?;
        Ok(ZswapLocalState(match &offer.0 {
            ProvenOffer(val) => self.0.apply(&sk_unwrapped, &val),
            UnprovenOffer(val) => self.0.apply(&sk_unwrapped, &val),
            ProofErasedOffer(val) => self.0.apply(&sk_unwrapped, &val),
        }))
    }

    #[wasm_bindgen(js_name = "applyCollapsedUpdate")]
    pub fn apply_collapsed_update(
        &self,
        update: &MerkleTreeCollapsedUpdate,
    ) -> Result<ZswapLocalState, JsError> {
        Ok(ZswapLocalState(
            self.0.apply_collapsed_update(&update.as_ref())?,
        ))
    }

    #[wasm_bindgen(js_name = "clearPending")]
    pub fn clear_pending(&self, _time: Date) -> ZswapLocalState {
        ZswapLocalState(self.0.clone())
    }

    // type QualifiedCoinInfo = { type: Uint8Array, nonce: Uint8Array, value: number, mt_index: number };
    // spend(secretKeys: ZswapSecretKeys, coin: QualifiedCoinInfo, segment: number, ttl?: Date): [ZswapLocalState, ZswapInput<PreProof>]
    pub fn spend(
        &self,
        secret_keys: &ZswapSecretKeys,
        coin: JsValue,
        segment: u16,
        _ttl: Option<Date>,
    ) -> Result<JsValue, JsError> {
        let coin: QualifiedCoinInfo = value_to_qualified_shielded_coininfo(coin)?;
        let (succ, inp) = self
            .0
            .spend(&mut OsRng, &secret_keys.try_into()?, &coin, segment)?;
        let succ = JsValue::from(ZswapLocalState(succ));
        let inp = JsValue::from(ZswapInput::from(inp));
        let res = Array::new();
        res.push(&succ);
        res.push(&inp);
        Ok(res.into())
    }

    #[wasm_bindgen(js_name = "spendFromOutput")]
    pub fn spend_from_output(
        &self,
        secret_keys: &ZswapSecretKeys,
        coin: JsValue,
        segment: u16,
        output: &ZswapOutput,
        _ttl: Option<Date>,
    ) -> Result<JsValue, JsError> {
        let coin: QualifiedCoinInfo = value_to_qualified_shielded_coininfo(coin)?;
        let (succ, tra) = self.0.spend_from_output(
            &mut OsRng,
            &secret_keys.try_into()?,
            &coin,
            segment,
            output.clone().try_into()?,
        )?;
        let succ = JsValue::from(ZswapLocalState(succ));
        let tra = JsValue::from(ZswapTransient::from(tra));
        let res = Array::new();
        res.push(&succ);
        res.push(&tra);
        Ok(res.into())
    }

    // type CoinInfo = { type: Uint8Array, nonce: Uint8Array, value: number };
    // watchFor(coin: CoinInfo): LocalState
    #[wasm_bindgen(js_name = "watchFor")]
    pub fn watch_for(
        &self,
        coin_public_key: String,
        coin: JsValue,
    ) -> Result<ZswapLocalState, JsError> {
        let coin_public_key: CoinPublicKey = from_hex_ser(&coin_public_key)?;
        let coin: CoinInfo = value_to_shielded_coininfo(coin)?;
        Ok(ZswapLocalState(self.0.watch_for(&coin_public_key, &coin)))
    }

    pub fn serialize(&self) -> Result<Uint8Array, JsError> {
        let mut res = Vec::new();
        tagged_serialize(&self.0, &mut res)?;
        Ok(Uint8Array::from(&res[..]))
    }

    pub fn deserialize(raw: Uint8Array) -> Result<ZswapLocalState, JsError> {
        Ok(ZswapLocalState(from_value_ser(raw, "ZswapLocalState")?))
    }

    #[wasm_bindgen(js_name = "toString")]
    pub fn to_string(&self, compact: Option<bool>) -> String {
        if compact.unwrap_or(false) {
            format!("{:?}", &self.0)
        } else {
            format!("{:#?}", &self.0)
        }
    }
}

#[wasm_bindgen]
#[derive(Clone)]
pub struct ZswapChainState(pub(crate) zswap::ledger::State<InMemoryDB>);

impl From<zswap::ledger::State<InMemoryDB>> for ZswapChainState {
    fn from(state: zswap::ledger::State<InMemoryDB>) -> ZswapChainState {
        ZswapChainState(state)
    }
}

impl From<ZswapChainState> for zswap::ledger::State<InMemoryDB> {
    fn from(state: ZswapChainState) -> zswap::ledger::State<InMemoryDB> {
        state.0
    }
}

#[wasm_bindgen]
impl ZswapChainState {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        ZswapChainState(zswap::ledger::State::new())
    }

    #[wasm_bindgen(getter = firstFree, js_name = "firstFree")]
    pub fn first_free(&self) -> u64 {
        self.0.first_free
    }

    pub fn filter(&self, contract_address: &str) -> Result<ZswapChainState, JsError> {
        let contract_address: Address = from_hex_ser(contract_address)?;
        let mut state = zswap::ledger::State::new();
        state.coin_coms = self.0.filter(&[contract_address]);
        Ok(ZswapChainState(state))
    }

    #[wasm_bindgen(js_name = "postBlockUpdate")]
    pub fn post_block_update(&self, tblock: &Date) -> ZswapChainState {
        ZswapChainState(
            self.0
                .post_block_update(Timestamp::from_secs(js_date_to_seconds(tblock))),
        )
    }

    pub fn serialize(&self) -> Result<Uint8Array, JsError> {
        let mut res = Vec::new();
        tagged_serialize(&self.0, &mut res)?;
        Ok(Uint8Array::from(&res[..]))
    }

    pub fn deserialize(raw: Uint8Array) -> Result<ZswapChainState, JsError> {
        Ok(ZswapChainState(from_value_ser(raw, "ZswapChainState")?))
    }

    #[wasm_bindgen(js_name = "deserializeFromLedgerState")]
    pub fn deserialize_from_ledger_state(raw: Uint8Array) -> Result<ZswapChainState, JsError> {
        let st: ledger::structure::LedgerState<InMemoryDB> =
            from_value_ser(raw, "ZswapChainState")?;
        Ok(ZswapChainState((*st.zswap).clone()))
    }

    #[wasm_bindgen(js_name = "tryApply")]
    pub fn try_apply(&self, offer: &ZswapOffer, whitelist: JsValue) -> Result<JsValue, JsError> {
        use ZswapOfferTypes::*;
        let w = whitelist_from_value(whitelist)?;
        construct_apply_result(match &offer.0 {
            ProvenOffer(val) => self.0.try_apply(&val, w)?,
            UnprovenOffer(val) => self.0.try_apply(&val, w)?,
            ProofErasedOffer(val) => self.0.try_apply(&val, w)?,
        })
    }

    #[wasm_bindgen(js_name = "toString")]
    pub fn to_string(&self, compact: Option<bool>) -> String {
        if compact.unwrap_or(false) {
            format!("{:?}", &self.0)
        } else {
            format!("{:#?}", &self.0)
        }
    }
}

pub fn whitelist_from_value(whitelist: JsValue) -> Result<Option<SMap<Address, ()>>, JsError> {
    if whitelist.is_null() || whitelist.is_undefined() {
        Ok(None)
    } else {
        let js_set = whitelist
            .dyn_into::<Set>()
            .map_err(|_| JsError::new("Expected null or Set for whitelist"))?;
        let mut res = SMap::new();
        let mut err = None;
        js_set.for_each(&mut |key, _, _| match key
            .dyn_into::<JsString>()
            .and_then(|jsstr| from_hex_ser(&ToString::to_string(&jsstr)).map_err(Into::into))
        {
            Ok(key) => res = res.insert(key, ()),
            Err(e) => err = Some(e),
        });
        Ok(Some(res))
    }
}

fn construct_apply_result(
    (succ, indicies): (zswap::ledger::State<InMemoryDB>, SMap<Commitment, u64>),
) -> Result<JsValue, JsError> {
    let succ = JsValue::from(ZswapChainState(succ));
    let indicies_res = Map::new();
    for (cm, idx) in indicies.iter() {
        indicies_res.set(&JsString::from(to_hex_ser(&cm)?), &to_value(&idx)?);
    }
    let res = Array::new();
    res.push(&succ);
    res.push(&JsValue::from(indicies_res));
    Ok(res.into())
}
