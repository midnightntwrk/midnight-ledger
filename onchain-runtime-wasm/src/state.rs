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

use std::ops::Deref;

use crate::context::CostModel;
use crate::conversions::{token_type_to_value, value_to_token_type};
use crate::{
    ensure_ops_valid, from_value, from_value_hex_ser, from_value_ser, to_value, to_value_hex_ser,
    to_value_ser,
};
use base_crypto::fab::AlignedValue;
use coin_structure::coin::TokenType;
use js_sys::{Array, BigInt, JsString, Map, Uint8Array};
use onchain_runtime::contract_state_ext::ContractStateExt;
use onchain_runtime::ops::Op;
use onchain_runtime::result_mode::ResultModeGather;
use onchain_runtime::state;
use storage::db::InMemoryDB;
use storage::storage::HashMap;
use transient_crypto::fab::ValueReprAlignedValue;
use transient_crypto::merkle_tree::MerkleTree;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct StateMap(pub(crate) HashMap<AlignedValue, state::StateValue<InMemoryDB>>);

#[wasm_bindgen]
impl StateMap {
    #[allow(clippy::new_without_default)]
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        StateMap(HashMap::new())
    }

    pub fn keys(&self) -> Result<Vec<JsValue>, JsError> {
        Ok(self
            .0
            .iter()
            .map(|kv| to_value(&kv.0.deref().clone()))
            .collect::<Result<_, _>>()?)
    }

    pub fn get(&self, key: JsValue) -> Result<Option<StateValue>, JsError> {
        let key: AlignedValue = from_value(key)?;
        Ok(self
            .0
            .get(&key)
            .map(|sp| sp.deref().clone())
            .map(StateValue))
    }

    pub fn insert(&self, key: JsValue, value: &StateValue) -> Result<StateMap, JsError> {
        let key: AlignedValue = from_value(key)?;
        Ok(StateMap(self.0.insert(key, value.0.clone())))
    }

    pub fn remove(&self, key: JsValue) -> Result<StateMap, JsError> {
        let key: AlignedValue = from_value(key)?;
        Ok(StateMap(self.0.remove(&key)))
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
pub struct StateBoundedMerkleTree(pub(crate) MerkleTree<()>);

#[wasm_bindgen]
impl StateBoundedMerkleTree {
    #[wasm_bindgen(constructor)]
    pub fn blank(height: u8) -> Self {
        StateBoundedMerkleTree(MerkleTree::blank(height))
    }

    #[wasm_bindgen(getter)]
    pub fn height(&self) -> u8 {
        self.0.height()
    }

    pub fn root(&self) -> Result<JsValue, JsError> {
        Ok(self
            .0
            .root()
            .map(|v| to_value(&AlignedValue::from(v)))
            .transpose()?
            .unwrap_or(JsValue::UNDEFINED))
    }

    // path_for_leaf(leaf: AlignedValue): AlignedValue
    #[wasm_bindgen(js_name = "findPathForLeaf")]
    pub fn find_path_for_leaf(&self, leaf: JsValue) -> Result<JsValue, JsError> {
        let leaf: AlignedValue = from_value(leaf)?;
        Ok(self
            .0
            .find_path_for_leaf(ValueReprAlignedValue(leaf))
            .map(|v| to_value(&AlignedValue::from(v)))
            .transpose()?
            .unwrap_or(JsValue::UNDEFINED))
    }

    // path_for_leaf(index: number, leaf: AlignedValue): AlignedValue
    #[wasm_bindgen(js_name = "pathForLeaf")]
    pub fn path_for_leaf(&self, index: u64, leaf: JsValue) -> Result<JsValue, JsError> {
        let leaf: AlignedValue = from_value(leaf)?;
        Ok(self
            .0
            .path_for_leaf(index, ValueReprAlignedValue(leaf))
            .map(|v| to_value(&AlignedValue::from(v)))??)
    }

    // update(index: number, leaf: AlignedValue): MerkleTree
    pub fn update(&self, index: u64, leaf: JsValue) -> Result<StateBoundedMerkleTree, JsError> {
        let leaf: AlignedValue = from_value(leaf)?;
        Ok(StateBoundedMerkleTree(self.0.update(
            index,
            &ValueReprAlignedValue(leaf),
            (),
        )))
    }

    pub fn rehash(&self) -> StateBoundedMerkleTree {
        StateBoundedMerkleTree(self.0.rehash())
    }

    pub fn collapse(&self, start: u64, end: u64) -> StateBoundedMerkleTree {
        StateBoundedMerkleTree(self.0.collapse(start, end))
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
pub struct ChargedState(pub(crate) state::ChargedState<InMemoryDB>);

impl From<ChargedState> for state::ChargedState<InMemoryDB> {
    fn from(value: ChargedState) -> Self {
        value.0
    }
}

#[wasm_bindgen]
impl ChargedState {
    #[wasm_bindgen(constructor)]
    pub fn new(state: &StateValue) -> ChargedState {
        ChargedState(state::ChargedState::new(state.0.clone()))
    }

    #[wasm_bindgen(getter)]
    pub fn state(&self) -> StateValue {
        StateValue((*self.0.get()).clone())
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
pub struct StateValue(pub(crate) state::StateValue<InMemoryDB>);

impl From<StateValue> for state::StateValue<InMemoryDB> {
    fn from(value: StateValue) -> state::StateValue<InMemoryDB> {
        value.0
    }
}

#[wasm_bindgen]
impl StateValue {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Result<StateValue, JsError> {
        Err(JsError::new(
            "StateValue cannot be constructed directly through the WASM API. Use the static constructors instead.",
        ))
    }

    #[wasm_bindgen(js_name = "type")]
    pub fn type_(&self) -> String {
        use state::StateValue::*;
        match self.0 {
            Null => "null",
            Cell(_) => "cell",
            Map(_) => "map",
            Array(_) => "array",
            BoundedMerkleTree(_) => "boundedMerkleTree",
            _ => panic!("match should be exhaustive"),
        }
        .to_owned()
    }

    #[wasm_bindgen(js_name = "newNull")]
    pub fn new_null() -> StateValue {
        StateValue(state::StateValue::Null)
    }

    #[wasm_bindgen(js_name = "newCell")]
    pub fn new_cell(value: JsValue) -> Result<StateValue, JsError> {
        Ok(StateValue(state::StateValue::Cell(from_value(value)?)))
    }

    #[wasm_bindgen(js_name = "newMap")]
    pub fn new_map(map: &StateMap) -> StateValue {
        StateValue(state::StateValue::Map(map.0.clone()))
    }

    #[wasm_bindgen(js_name = "newBoundedMerkleTree")]
    pub fn new_bounded_merkle_tree(tree: &StateBoundedMerkleTree) -> StateValue {
        StateValue(state::StateValue::BoundedMerkleTree(tree.0.clone()))
    }

    #[wasm_bindgen(js_name = "newArray")]
    pub fn new_array() -> StateValue {
        StateValue(state::StateValue::Array(vec![].into()))
    }

    #[wasm_bindgen(js_name = "arrayPush")]
    pub fn array_push(&self, value: &StateValue) -> Result<StateValue, JsError> {
        match self {
            StateValue(state::StateValue::Array(vec)) => {
                if vec.len() < 15 {
                    Ok(StateValue(state::StateValue::Array(
                        vec.iter()
                            .map(|x| (*x).clone())
                            .chain(std::iter::once(value.0.clone()))
                            .collect::<Vec<state::StateValue<InMemoryDB>>>()
                            .into(),
                    )))
                } else {
                    Err(JsError::new("Push would cause array to exceed 15 elements"))
                }
            }
            _ => Err(JsError::new(&format!(
                "Target of push must be an array (got: {})",
                self.type_()
            ))),
        }
    }

    #[wasm_bindgen(js_name = "asCell")]
    pub fn as_cell(&self) -> Result<JsValue, JsError> {
        match &self.0 {
            state::StateValue::Cell(v) => Ok(to_value(v)?),
            _ => Ok(JsValue::NULL),
        }
    }

    #[wasm_bindgen(js_name = "asMap")]
    pub fn as_map(&self) -> Result<Option<StateMap>, JsError> {
        match &self.0 {
            state::StateValue::Map(m) => Ok(Some(StateMap(m.clone()))),
            _ => Ok(None),
        }
    }

    #[wasm_bindgen(js_name = "asBoundedMerkleTree")]
    pub fn as_bounded_merkle_tree(&self) -> Result<Option<StateBoundedMerkleTree>, JsError> {
        match &self.0 {
            state::StateValue::BoundedMerkleTree(t) => Ok(Some(StateBoundedMerkleTree(t.clone()))),
            _ => Ok(None),
        }
    }

    #[wasm_bindgen(js_name = "asArray")]
    pub fn as_array(&self) -> Result<Option<Vec<JsValue>>, JsError> {
        match &self.0 {
            state::StateValue::Array(arr) => Ok(Some(
                arr.iter()
                    .map(|x| (*x).clone())
                    .map(StateValue)
                    .map(Into::into)
                    .collect(),
            )),
            _ => Ok(None),
        }
    }

    #[wasm_bindgen(js_name = "logSize")]
    pub fn log_size(&self) -> usize {
        self.0.log_size()
    }

    pub fn encode(&self) -> Result<JsValue, JsError> {
        Ok(to_value(&self.0)?)
    }

    pub fn decode(value: JsValue) -> Result<StateValue, JsError> {
        Ok(StateValue(from_value(value)?))
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
pub struct ContractState(pub(crate) state::ContractState<InMemoryDB>);

impl From<state::ContractState<InMemoryDB>> for ContractState {
    fn from(state: state::ContractState<InMemoryDB>) -> ContractState {
        ContractState(state)
    }
}

impl From<ContractState> for state::ContractState<InMemoryDB> {
    fn from(state: ContractState) -> state::ContractState<InMemoryDB> {
        state.0
    }
}

#[wasm_bindgen]
impl ContractState {
    #[allow(clippy::new_without_default)]
    #[wasm_bindgen(constructor)]
    pub fn new() -> ContractState {
        ContractState(Default::default())
    }

    #[wasm_bindgen(getter = data)]
    pub fn data(&self) -> ChargedState {
        ChargedState(self.0.data.clone())
    }

    #[wasm_bindgen(setter = data)]
    pub fn set_data(&mut self, data: &ChargedState) {
        self.0.data = data.0.clone();
    }

    #[wasm_bindgen(getter = maintenanceAuthority)]
    pub fn maintenance_authority(&self) -> ContractMaintenanceAuthority {
        ContractMaintenanceAuthority(self.0.maintenance_authority.clone())
    }

    #[wasm_bindgen(setter = maintenanceAuthority)]
    pub fn set_maintenance_authority(&mut self, authority: &ContractMaintenanceAuthority) {
        self.0.maintenance_authority = authority.0.clone();
    }

    #[wasm_bindgen(getter)]
    pub fn balance(&self) -> Result<Map, JsError> {
        let res = Map::new();
        for item in self.0.balance.iter() {
            let (token, balance) = item.deref();
            res.set(&token_type_to_value(&token)?, &to_value(&balance)?);
        }
        Ok(res)
    }

    #[wasm_bindgen(setter, js_name = "balance")]
    pub fn set_balance(&mut self, value_map: Map) -> Result<(), JsError> {
        let mut balance = HashMap::new();
        for key in value_map.keys() {
            let key = key.unwrap();
            let value = value_map.get(&key);
            let token_type: TokenType = value_to_token_type(key)?;
            balance = balance.insert(token_type, from_value(value)?);
        }
        self.0.balance = balance.into();
        Ok(())
    }

    // operations(): Array<Uint8Array | String>
    pub fn operations(&self) -> Vec<JsValue> {
        self.0
            .operations
            .iter()
            .map(|a| maybe_string(&a.0))
            .collect()
    }

    pub fn operation(&self, operation: JsValue) -> Result<Option<ContractOperation>, JsError> {
        Ok(self
            .0
            .operations
            .get(&state::EntryPointBuf(from_maybe_string(operation)?))
            .map(|o| ContractOperation(o.deref().clone())))
    }

    #[wasm_bindgen(js_name = "setOperation")]
    pub fn set_operation(
        &mut self,
        operation: JsValue,
        value: &ContractOperation,
    ) -> Result<(), JsError> {
        self.0.operations = self.0.operations.insert(
            state::EntryPointBuf(from_maybe_string(operation)?),
            value.0.clone(),
        );
        Ok(())
    }

    // query(ty: QueryType, args: Value, address: ContractAddress | undefined): [ContractState, AlignedValue]
    pub fn query(&self, query: JsValue, cost_model: &CostModel) -> Result<JsValue, JsError> {
        let query: Vec<Op<ResultModeGather, InMemoryDB>> = from_value(query)?;
        ensure_ops_valid(&query)?;
        let res = self.0.query(&query, &cost_model.0)?;
        let state = JsValue::from(ContractState(res.0));
        let gather_events = to_value(&res.1)?;
        let res = Array::new();
        res.push(&state);
        res.push(&gather_events);
        Ok(res.into())
    }

    pub fn serialize(&self) -> Result<JsValue, JsError> {
        to_value_ser(&self.0)
    }

    pub fn deserialize(raw: Uint8Array) -> Result<ContractState, JsError> {
        Ok(ContractState(from_value_ser(raw, "ContractState")?))
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
pub struct ContractOperation(state::ContractOperation);

impl From<state::ContractOperation> for ContractOperation {
    fn from(op: state::ContractOperation) -> ContractOperation {
        ContractOperation(op)
    }
}

impl From<ContractOperation> for state::ContractOperation {
    fn from(op: ContractOperation) -> state::ContractOperation {
        op.0
    }
}

#[wasm_bindgen]
impl ContractOperation {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Result<ContractOperation, JsError> {
        Ok(ContractOperation(state::ContractOperation::new(None)))
    }

    #[wasm_bindgen(getter = verifierKey)]
    pub fn verifier_key(&self) -> Result<JsValue, JsError> {
        match self.0.latest() {
            Some(vk) => to_value_ser(vk),
            None => Ok(JsValue::UNDEFINED),
        }
    }

    #[wasm_bindgen(setter = verifierKey)]
    pub fn set_verifier_key(&mut self, key: Uint8Array) -> Result<(), JsError> {
        *self.0.latest_mut() = Some(from_value_ser(key, "ContractOperation")?);
        Ok(())
    }

    pub fn serialize(&self) -> Result<JsValue, JsError> {
        to_value_ser(&self.0)
    }

    pub fn deserialize(raw: Uint8Array) -> Result<ContractOperation, JsError> {
        Ok(ContractOperation(from_value_ser(raw, "ContractOperation")?))
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
pub struct ContractMaintenanceAuthority(state::ContractMaintenanceAuthority);

impl From<state::ContractMaintenanceAuthority> for ContractMaintenanceAuthority {
    fn from(cma: state::ContractMaintenanceAuthority) -> ContractMaintenanceAuthority {
        ContractMaintenanceAuthority(cma)
    }
}

impl From<ContractMaintenanceAuthority> for state::ContractMaintenanceAuthority {
    fn from(cma: ContractMaintenanceAuthority) -> state::ContractMaintenanceAuthority {
        cma.0
    }
}

#[wasm_bindgen]
impl ContractMaintenanceAuthority {
    #[wasm_bindgen(constructor)]
    pub fn new(
        committee: Array,
        threshold: u32,
        counter: Option<BigInt>,
    ) -> Result<ContractMaintenanceAuthority, JsError> {
        let committee = committee
            .iter()
            .map(|val| {
                from_value_hex_ser(
                    &val.as_string()
                        .ok_or_else(|| JsError::new("expected string"))?,
                )
            })
            .collect::<Result<Vec<_>, JsError>>()?;
        let counter = u32::try_from(
            counter
                .map(|c| u64::try_from(c))
                .transpose()
                .map_err(|_| JsError::new("counter out of range"))?
                .unwrap_or(0),
        )
        .map_err(|_| JsError::new("counter out of range"))?;
        Ok(ContractMaintenanceAuthority(
            state::ContractMaintenanceAuthority {
                committee,
                threshold,
                counter,
            },
        ))
    }

    #[wasm_bindgen(getter)]
    pub fn committee(&self) -> Result<Array, JsError> {
        let com_arr = Array::new();
        for member in self.0.committee.iter() {
            com_arr.push(&to_value_hex_ser(member)?.into());
        }
        Ok(com_arr)
    }

    #[wasm_bindgen(getter)]
    pub fn threshold(&self) -> u32 {
        self.0.threshold
    }

    #[wasm_bindgen(getter)]
    pub fn counter(&self) -> BigInt {
        self.0.counter.into()
    }

    pub fn serialize(&self) -> Result<JsValue, JsError> {
        to_value_ser(&self.0)
    }

    pub fn deserialize(raw: Uint8Array) -> Result<ContractMaintenanceAuthority, JsError> {
        Ok(ContractMaintenanceAuthority(from_value_ser(
            raw,
            "ContractMaintenanceAuthority",
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

pub fn maybe_string(bytes: &[u8]) -> JsValue {
    match std::str::from_utf8(bytes) {
        Ok(v) => JsString::from(v).into(),
        Err(_) => Uint8Array::from(bytes).into(),
    }
}

pub fn from_maybe_string(js: JsValue) -> Result<Vec<u8>, JsError> {
    if let Some(s) = js.as_string() {
        return Ok(s.into_bytes());
    }
    match js.dyn_into::<Uint8Array>() {
        Ok(arr) => Ok(arr.to_vec()),
        Err(_) => Err(JsError::new(
            "expected either a valid UTF string, or a Uint8Array",
        )),
    }
}
