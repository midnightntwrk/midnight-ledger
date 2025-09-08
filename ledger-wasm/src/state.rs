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
use crate::dust::{DustState, UtxoMeta};
use crate::onchain_runtime::{ContractState, value_to_token_type};
use crate::tx::{SystemTransaction, TransactionContext, TransactionResult, VerifiedTransaction};
use crate::zswap_state::*;
use crate::zswap_wasm::*;
use base_crypto::time::Timestamp;
use coin_structure::coin::UserAddress;
use js_sys::{Array, BigInt, Date, Function, Map, Set, Uint8Array};
use ledger::structure::{ClaimKind, OutputInstructionUnshielded};
use onchain_runtime_wasm::from_value_ser;
use onchain_runtime_wasm::state::ChargedState;
use rand::Rng;
use rand::rngs::OsRng;
use serialize::tagged_serialize;
use storage::arena::Sp;
use storage::db::InMemoryDB;
use storage::storage::HashMap;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct LedgerState(pub(crate) ledger::structure::LedgerState<InMemoryDB>);

#[wasm_bindgen]
impl LedgerState {
    #[wasm_bindgen(constructor)]
    pub fn new(network_id: String, zswap: &ZswapChainState) -> LedgerState {
        let mut res = Self::blank(network_id);
        res.0.zswap = Sp::new(zswap.clone().into());
        res
    }

    pub fn blank(network_id: String) -> LedgerState {
        LedgerState(ledger::structure::LedgerState::new(network_id))
    }

    #[wasm_bindgen(getter)]
    pub fn parameters(&self) -> LedgerParameters {
        (*self.0.parameters).clone().into()
    }

    #[wasm_bindgen(setter = parameters)]
    pub fn set_parameters(&mut self, params: &LedgerParameters) {
        self.0.parameters = Sp::new(params.clone().into());
    }

    #[wasm_bindgen(js_name = "postBlockUpdate")]
    pub fn post_block_update(
        &self,
        tblock: &Date,
        fullness: JsValue,
    ) -> Result<LedgerState, JsError> {
        let fullness = if fullness.is_null() || fullness.is_undefined() {
            self.0.parameters.limits.block_limits * 0.5
        } else {
            from_value(fullness)?
        };
        Ok(LedgerState(self.0.post_block_update(
            Timestamp::from_secs(js_date_to_seconds(tblock)),
            fullness,
        )?))
    }

    #[wasm_bindgen(js_name = treasuryBalance)]
    pub fn treasury_balance(&self, token_type: JsValue) -> Result<BigInt, JsError> {
        let token_type = value_to_token_type(token_type)?;
        Ok(self
            .0
            .treasury
            .get(&token_type)
            .copied()
            .unwrap_or(0)
            .into())
    }

    #[wasm_bindgen(js_name = bridgeReceiving)]
    pub fn bridge_receiving(&self, recipient: &str) -> Result<BigInt, JsError> {
        let recipient: UserAddress = from_hex_ser(recipient)?;
        Ok(self
            .0
            .bridge_receiving
            .get(&recipient)
            .copied()
            .unwrap_or(0)
            .into())
    }

    #[wasm_bindgen(getter = lockedPool)]
    pub fn locked_pool(&self) -> BigInt {
        self.0.locked_pool.into()
    }

    #[wasm_bindgen(getter = reservePool)]
    pub fn reserve_pool(&self) -> BigInt {
        self.0.reserve_pool.into()
    }

    #[wasm_bindgen(js_name = unclaimedBlockRewards)]
    pub fn unclaimed_block_rewards(&self, recipient: &str) -> Result<BigInt, JsError> {
        let recipient: UserAddress = from_hex_ser(recipient)?;
        Ok(self
            .0
            .unclaimed_block_rewards
            .get(&recipient)
            .copied()
            .unwrap_or(0)
            .into())
    }

    #[wasm_bindgen(getter = blockRewardPool)]
    pub fn block_reward_pool(&self) -> BigInt {
        self.0.block_reward_pool.into()
    }

    #[wasm_bindgen(getter)]
    pub fn zswap(&self) -> ZswapChainState {
        (*self.0.zswap).clone().into()
    }

    #[wasm_bindgen(getter)]
    pub fn utxo(&self) -> UtxoState {
        (*self.0.utxo).clone().into()
    }

    #[wasm_bindgen(getter)]
    pub fn dust(&self) -> DustState {
        DustState((*self.0.dust).clone())
    }

    pub fn apply(
        &self,
        transaction: &VerifiedTransaction,
        context: &TransactionContext,
    ) -> JsValue {
        let (next_state, result) = self.0.apply(&transaction.0, &context.0);
        let res = Array::new();
        res.push(&JsValue::from(LedgerState(next_state)));
        res.push(&JsValue::from(TransactionResult(result)));
        res.into()
    }

    #[wasm_bindgen(js_name = "applySystemTx")]
    pub fn apply_system_tx(&self, tx: &SystemTransaction, tblock: &Date) -> Result<Array, JsError> {
        let (state, events) = self.0.apply_system_tx(
            tx.as_ref(),
            Timestamp::from_secs(js_date_to_seconds(tblock)),
        )?;
        let events: Vec<_> = events.into_iter().map(Event::from).collect();

        let tuple = Array::new();
        tuple.push(&JsValue::from(LedgerState(state)));
        tuple.push(&JsValue::from(events));

        Ok(tuple)
    }

    pub fn index(&self, address: &str) -> Result<Option<ContractState>, JsError> {
        Ok(self.0.index(from_hex_ser(address)?).map(Into::into))
    }

    #[wasm_bindgen(js_name = "updateIndex")]
    pub fn update_index(
        &self,
        address: &str,
        state: &ChargedState,
        balances_map: Map,
    ) -> Result<LedgerState, JsError> {
        let mut balances = HashMap::new();
        for key in balances_map.keys() {
            let key = key.unwrap();
            let value = balances_map.get(&key);
            let token_type = value_to_token_type(key)?;
            balances = balances.insert(token_type, from_value(value)?);
        }
        let mut new_state = self.0.clone();
        new_state = new_state.update_index(from_hex_ser(address)?, state.clone().into(), balances);
        Ok(LedgerState(new_state))
    }

    pub fn serialize(&self) -> Result<Uint8Array, JsError> {
        let mut res = Vec::new();
        tagged_serialize(&self.0, &mut res)?;
        Ok(Uint8Array::from(&res[..]))
    }

    pub fn deserialize(raw: Uint8Array) -> Result<LedgerState, JsError> {
        Ok(LedgerState(from_value_ser(raw, "LedgerState")?))
    }

    #[wasm_bindgen(js_name = "toString")]
    pub fn to_string(&self, compact: Option<bool>) -> String {
        if compact.unwrap_or(false) {
            format!("{:?}", &self.0)
        } else {
            format!("{:#?}", &self.0)
        }
    }

    #[wasm_bindgen(js_name = "testingDistributeNight")]
    pub fn distribute_night(
        &self,
        user_address: &str,
        amount: BigInt,
        tblock: &Date,
    ) -> Result<LedgerState, JsError> {
        let address: UserAddress = from_hex_ser(user_address)?;
        let amount = u128::try_from(amount).map_err(|_| JsError::new("amount is out of range"))?;
        let sys_tx_distribute = ledger::structure::SystemTransaction::DistributeReserve(amount);
        let time = Timestamp::from_secs(js_date_to_seconds(tblock));
        let (ledger, _) = self.0.apply_system_tx(&sys_tx_distribute, time)?;

        let sys_tx_rewards = ledger::structure::SystemTransaction::DistributeNight(
            ClaimKind::Reward,
            vec![OutputInstructionUnshielded {
                amount: amount,
                target_address: address,
                nonce: OsRng.r#gen(),
            }],
        );
        let (ledger, _) = ledger.apply_system_tx(&sys_tx_rewards, time)?;

        Ok(LedgerState(ledger))
    }
}

#[wasm_bindgen]
#[derive(Clone)]
pub struct UtxoState(pub(crate) ledger::structure::UtxoState<InMemoryDB>);

impl From<ledger::structure::UtxoState<InMemoryDB>> for UtxoState {
    fn from(state: ledger::structure::UtxoState<InMemoryDB>) -> UtxoState {
        UtxoState(state)
    }
}

impl From<UtxoState> for ledger::structure::UtxoState<InMemoryDB> {
    fn from(state: UtxoState) -> ledger::structure::UtxoState<InMemoryDB> {
        state.0
    }
}

#[wasm_bindgen]
impl UtxoState {
    // Map<Utxo, UtxoMeta>
    pub fn new(utxo_map: Map) -> Result<Self, JsError> {
        let mut storage_utxos = HashMap::new();
        for key in utxo_map.keys() {
            let key = key.unwrap();
            let value = utxo_map.get(&key);
            let utxo = value_to_utxo(key).map_err(|_| JsError::new("unable to decode UTXO"))?;
            let meta = value_to_utxo_meta(value)
                .map_err(|_| JsError::new("unable to decode UTXO Meta"))?;

            storage_utxos = storage_utxos.insert(utxo, meta);
        }
        Ok(UtxoState(ledger::structure::UtxoState::<InMemoryDB> {
            utxos: storage_utxos,
        }))
    }

    #[wasm_bindgen(js_name = "lookupMeta")]
    pub fn lookup_meta(&self, utxo: JsValue) -> Result<Option<UtxoMeta>, JsError> {
        let utxo = value_to_utxo(utxo)?;
        let meta = self
            .0
            .utxos
            .get(&utxo)
            .map(|utxo| UtxoMeta((*utxo).clone()));
        Ok(meta)
    }

    #[wasm_bindgen(getter)]
    // Set<Utxo>
    pub fn utxos(&self) -> Result<Set, JsError> {
        let res = Set::new(&JsValue::NULL);
        for utxo in self.0.utxos.iter() {
            let utxo = &*utxo.0;
            res.add(&utxo_to_value(&utxo)?);
        }
        Ok(res)
    }

    // Set<Utxo>
    pub fn filter(&self, user_address: &str) -> Result<Set, JsError> {
        let address: UserAddress = from_hex_ser(user_address)?;
        let res = Set::new(&JsValue::NULL);
        for utxo in self.0.utxos.iter() {
            let utxo = &*utxo.0;
            if utxo.owner == address {
                res.add(&utxo_to_value(&utxo)?);
            }
        }
        Ok(res)
    }

    // delta(prior: UtxoState<D>, filterBy?: (utxo: Utxo) => boolean): [Set<Utxo>, Set<Utxo>]
    pub fn delta(&self, prior: &Self, filter_by: Option<Function>) -> Result<Array, JsError> {
        let this_minus_prior = Set::new(&JsValue::NULL);
        let prior_minus_this = Set::new(&JsValue::NULL);

        for utxo in self.0.utxos.iter() {
            let utxo = &*utxo.0;
            let is_member = prior.0.utxos.contains_key(&utxo);
            if !is_member {
                let js_value = utxo_to_value(&utxo)?;
                let mut accepted = true;

                if let Some(filter_by) = filter_by.clone() {
                    let resp = filter_by
                        .call1(&JsValue::NULL, &js_value)
                        .map_err(|_| JsError::new("callback error"))?;

                    let r = resp
                        .as_bool()
                        .ok_or(JsError::new("non boolean received from the callback"))?;
                    accepted = r;
                }

                if accepted {
                    this_minus_prior.add(&js_value);
                }
            }
        }
        for utxo in prior.0.utxos.iter() {
            let utxo = &*utxo.0;
            let is_member = self.0.utxos.contains_key(&utxo);
            if !is_member {
                let js_value = utxo_to_value(&utxo)?;
                let mut accepted = true;

                if let Some(filter_by) = filter_by.clone() {
                    let resp = filter_by
                        .call1(&JsValue::NULL, &js_value)
                        .map_err(|_| JsError::new("callback error"))?;

                    let r = resp
                        .as_bool()
                        .ok_or(JsError::new("non boolean received from the callback"))?;
                    accepted = r;
                }

                if accepted {
                    prior_minus_this.add(&utxo_to_value(&utxo)?);
                }
            }
        }

        let tuple = Array::new();
        tuple.push(&this_minus_prior);
        tuple.push(&prior_minus_this);
        Ok(tuple)
    }
}
