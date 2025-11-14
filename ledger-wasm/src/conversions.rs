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

use crate::contract::{ContractCall, ContractCallTypes, ContractDeploy, MaintenanceUpdate};
use crate::intent::Intent;
use crate::zswap_wasm::ZswapOffer;
use base_crypto::cost_model::FeePrices;
use coin_structure::coin::{Info as ShieldedCoinInfo, QualifiedInfo as QualifiedShieldedCoinInfo};
use coin_structure::coin::{ShieldedTokenType, UnshieldedTokenType};
use hex::{FromHex, ToHex};
use js_sys::{BigInt, Date, Function, JsString, Map, Number};
use ledger::structure::UtxoMeta;
use ledger::structure::{ClaimKind, SignatureKind};
use serde::{Deserialize, Serialize};
use serialize::{Deserializable, Serializable};
use std::io::Read;
use std::marker::PhantomData;
use std::ops::Deref;
use storage::storage::HashMap;
use transient_crypto::curve::Fr;
use wasm_bindgen::convert::RefFromWasmAbi;
use wasm_bindgen::{JsCast, JsError, JsValue};

use base_crypto::time::{Duration, Timestamp};
use ledger::dust::{
    DustGenerationInfo, DustParameters, DustPublicKey, InitialNonce, QualifiedDustOutput,
};
use ledger::structure::{
    ContractAction, Intent as LedgerIntent, ProofKind, ProofMarker, ProofPreimageMarker, Utxo,
    UtxoOutput, UtxoSpend,
};
pub use serde_wasm_bindgen::from_value;
use storage::{Storable, db::InMemoryDB};

pub trait TryRef: RefFromWasmAbi<Abi = u32> {
    fn instanceof(val: &JsValue) -> bool;
    unsafe fn unchecked_ref(_val: &JsValue) -> Result<Self::Anchor, JsError>;
    fn try_ref(val: &JsValue) -> Result<Option<Self::Anchor>, JsError> {
        if Self::instanceof(val) {
            Ok(Some(unsafe { Self::unchecked_ref(val)? }))
        } else {
            Ok(None)
        }
    }
}

#[macro_export]
macro_rules! try_ref_for_exported {
    ($ty:ident) => {
        paste::paste! {
            // Re-import $ty from the wasm-output, and expose it as a function returning the class
            // object. This is necessary, as wasm-bindgen does not provide a means to inspect the
            // class object itself, and just declaring `extern "C" type $ty;` will complain about a
            // conflicting declaration for $ty (even if renamed).
            #[wasm_bindgen(inline_js = "import * as wasm from '#self'; export function " $ty _ "() { return wasm." $ty "; }")]
            extern "C" {
                #[wasm_bindgen(js_name = [<$ty _>])]
                pub fn [<Js $ty>]() -> JsValue;
            }
            impl TryRef for $ty {
                unsafe fn unchecked_ref(val: &JsValue) -> Result<Self::Anchor, JsError> { unsafe {
                    // Unchecked ref works by reading `__wbg_ptr`, interpreting it as a u32,
                    // and using RefFromWasmAbi on this. Very unsafe!
                    let ptr = js_sys::Reflect::get(&val, &"__wbg_ptr".into())
                        .map_err(|_| JsError::new("Pointer not found"))?
                        .as_f64()
                        .and_then(|f| {
                            if f.fract() != 0.0 || f > u32::MAX as f64 || f < 0.0 {
                                None
                            } else {
                                Some(f as u32)
                            }
                        })
                        .ok_or_else(|| JsError::new("Pointer is not a u32"))?;
                    Ok(<Self as wasm_bindgen::convert::RefFromWasmAbi>::ref_from_abi(ptr))
                }}
                // instanceof checks is a value has the same prototype object as the class object
                // of $ty. If this is the case, unchecked_ref *should* be safe.
                fn instanceof(val: &JsValue) -> bool {
                    let val_prototype = match js_sys::Reflect::get_prototype_of(val) {
                        Ok(v) => v,
                        Err(_) => return false,
                    };
                    let ty_obj = [<Js $ty>]();
                    let ty_prototype = match js_sys::Reflect::get(&ty_obj, &"prototype".into()) {
                        Ok(v) => v,
                        Err(_) => return false,
                    };
                    JsValue::from(val_prototype) == ty_prototype
                }
            }
        }
    }
}

pub use try_ref_for_exported;

pub fn to_value<T: Serialize + ?Sized>(value: &T) -> Result<JsValue, serde_wasm_bindgen::Error> {
    value.serialize(
        &serde_wasm_bindgen::Serializer::new().serialize_large_number_types_as_bigints(true),
    )
}

fn from_hex_ser_checked<T: Deserializable, R: Read>(mut bytes: R) -> Result<T, std::io::Error> {
    let value = T::deserialize(&mut bytes, 0)?;

    let count = bytes.bytes().count();

    if count == 0 {
        return Ok(value);
    }

    Err(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        format!("Not all bytes read, {} bytes remaining", count),
    ))
}

pub fn from_hex_ser<T: Deserializable>(data: &str) -> Result<T, JsError> {
    from_hex_ser_checked(&mut &<Vec<u8>>::from_hex(data.as_bytes())?[..]).map_err(Into::into)
}

pub fn to_hex_ser<T: Serializable + ?Sized>(value: &T) -> Result<String, JsError> {
    let mut bytes = Vec::new();
    T::serialize(value, &mut bytes).map_err(Into::<JsError>::into)?;
    Ok(bytes).map(|ser| ser.encode_hex())
}

fn deserialize_optional_date<'de, D: serde::Deserializer<'de>>(
    de: D,
) -> Result<Option<Date>, D::Error> {
    let value: JsValue = serde_wasm_bindgen::preserve::deserialize(de)?;
    Ok(if value.is_null() || value.is_undefined() {
        None
    } else {
        Some(
            value
                .dyn_into()
                .map_err(|e| serde::de::Error::custom::<JsString>(e.into()))?,
        )
    })
}

fn serialize_optional_date<S: serde::Serializer>(
    date: &Option<Date>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    if let Some(date) = date {
        serde_wasm_bindgen::preserve::serialize(date, serializer)
    } else {
        serializer.serialize_none()
    }
}

#[repr(u8)]
#[non_exhaustive]
pub enum Proofish {
    Proof = 0,
    PreProof = 1,
    NoProof = 2,
}

pub fn text_to_proofish(value: &str) -> Result<Proofish, JsError> {
    Ok(match value {
        "proof" => Proofish::Proof,
        "pre-proof" => Proofish::PreProof,
        "no-proof" => Proofish::NoProof,
        _ => return Err(JsError::new("Invalid proof value.")),
    })
}

#[repr(u8)]
#[non_exhaustive]
pub enum Bindingish {
    Binding = 0,
    PreBinding = 1,
    NoBinding = 2,
}

pub fn text_to_bindingish(value: &str) -> Result<Bindingish, JsError> {
    Ok(match value {
        "binding" => Bindingish::Binding,
        "pre-binding" => Bindingish::PreBinding,
        "no-binding" => Bindingish::NoBinding,
        _ => return Err(JsError::new("Invalid binding value.")),
    })
}

#[repr(u8)]
#[non_exhaustive]
pub enum Signaturish {
    Signature = 0,
    SignatureErased = 1,
}

pub fn text_to_signaturish(value: &str) -> Result<Signaturish, JsError> {
    Ok(match value {
        "signature" => Signaturish::Signature,
        "signature-erased" => Signaturish::SignatureErased,
        _ => return Err(JsError::new("Invalid signature value.")),
    })
}

#[derive(Serialize, Deserialize)]
struct PreShieldedCoinInfo {
    #[serde(rename = "type")]
    type_: String,
    nonce: String,
    value: u128,
}

#[derive(Serialize, Deserialize)]
struct PreQualifiedShieldedCoinInfo {
    #[serde(rename = "type")]
    type_: String,
    nonce: String,
    value: u128,
    mt_index: u64,
}

#[derive(Serialize, Deserialize)]
struct PreUtxoSpend {
    value: u128,
    owner: String,
    #[serde(rename = "type")]
    type_: String,
    #[serde(rename = "intentHash")]
    intent_hash: String,
    #[serde(rename = "outputNo")]
    output_no: u32,
}

#[derive(Serialize, Deserialize)]
struct PreUtxoOutput {
    value: u128,
    owner: String,
    #[serde(rename = "type")]
    type_: String,
}

#[derive(Serialize, Deserialize)]
struct PreUtxo {
    value: u128,
    owner: String,
    #[serde(rename = "type")]
    type_: String,
    #[serde(rename = "intentHash")]
    intent_hash: String,
    #[serde(rename = "outputNo")]
    output_no: u32,
}

#[derive(Serialize, Deserialize)]
struct PreUtxoMeta {
    #[serde(with = "serde_wasm_bindgen::preserve")]
    ctime: Date,
}

#[derive(Serialize, Deserialize)]
struct PreQualifiedDustOutput {
    #[serde(rename = "initialValue")]
    initial_value: u128,
    #[serde(with = "serde_wasm_bindgen::preserve")]
    owner: BigInt,
    #[serde(with = "serde_wasm_bindgen::preserve")]
    nonce: BigInt,
    seq: u32,
    #[serde(with = "serde_wasm_bindgen::preserve")]
    ctime: Date,
    #[serde(rename = "backingNight")]
    backing_night: String,
    #[serde(rename = "mtIndex")]
    mt_index: u64,
}

#[derive(Serialize, Deserialize)]
struct PreDustGenerationInfo {
    value: u128,
    #[serde(with = "serde_wasm_bindgen::preserve")]
    owner: BigInt,
    nonce: String,
    #[serde(
        serialize_with = "serialize_optional_date",
        deserialize_with = "deserialize_optional_date"
    )]
    dtime: Option<Date>,
}

#[derive(Serialize, Deserialize)]
struct PreDustParameters {
    #[serde(rename = "nightDustRatio", with = "serde_wasm_bindgen::preserve")]
    night_dust_ratio: BigInt,
    #[serde(rename = "generationDecayRate", with = "serde_wasm_bindgen::preserve")]
    generation_decay_rate: BigInt,
    #[serde(
        rename = "dustGracePeriodSeconds",
        with = "serde_wasm_bindgen::preserve"
    )]
    dust_grace_period_seconds: BigInt,
}

#[derive(Serialize, Deserialize)]
struct PreFeePrices {
    #[serde(rename = "readPrice")]
    read_price: f64,
    #[serde(rename = "computePrice")]
    compute_price: f64,
    #[serde(rename = "blockUsagePrice")]
    block_usage_price: f64,
    #[serde(rename = "writePrice")]
    write_price: f64,
}

pub fn value_to_shielded_coininfo(value: JsValue) -> Result<ShieldedCoinInfo, JsError> {
    let pre: PreShieldedCoinInfo = from_value(value)?;
    Ok(ShieldedCoinInfo {
        type_: ShieldedTokenType(from_hex_ser(&pre.type_)?),
        nonce: from_hex_ser(&pre.nonce)?,
        value: pre.value,
    })
}

pub fn value_to_qualified_shielded_coininfo(
    value: JsValue,
) -> Result<QualifiedShieldedCoinInfo, JsError> {
    let pre: PreQualifiedShieldedCoinInfo = from_value(value)?;
    Ok(QualifiedShieldedCoinInfo {
        type_: ShieldedTokenType(from_hex_ser(&pre.type_)?),
        nonce: from_hex_ser(&pre.nonce)?,
        value: pre.value,
        mt_index: pre.mt_index,
    })
}

pub fn shielded_coininfo_to_value(coin: &ShieldedCoinInfo) -> Result<JsValue, JsError> {
    Ok(to_value(&PreShieldedCoinInfo {
        type_: to_hex_ser(&coin.type_.0)?,
        nonce: to_hex_ser(&coin.nonce)?,
        value: coin.value,
    })?)
}

pub fn qualified_shielded_coininfo_to_value(
    coin: &QualifiedShieldedCoinInfo,
) -> Result<JsValue, JsError> {
    Ok(to_value(&PreQualifiedShieldedCoinInfo {
        type_: to_hex_ser(&coin.type_.0)?,
        nonce: to_hex_ser(&coin.nonce)?,
        value: coin.value,
        mt_index: coin.mt_index,
    })?)
}

pub fn utxo_spend_to_value(utxo: &UtxoSpend) -> Result<JsValue, JsError> {
    Ok(to_value(&PreUtxoSpend {
        value: utxo.value,
        owner: to_hex_ser(&utxo.owner)?,
        type_: to_hex_ser(&utxo.type_.0)?,
        intent_hash: to_hex_ser(&utxo.intent_hash)?,
        output_no: utxo.output_no,
    })?)
}

pub fn value_to_utxo_spend(value: JsValue) -> Result<UtxoSpend, JsError> {
    let pre: PreUtxoSpend = from_value(value)?;
    Ok(UtxoSpend {
        value: pre.value,
        owner: from_hex_ser(&pre.owner)?,
        type_: UnshieldedTokenType(from_hex_ser(&pre.type_)?),
        intent_hash: from_hex_ser(&pre.intent_hash)?,
        output_no: pre.output_no,
    })
}

pub fn utxo_output_to_value(utxo: &UtxoOutput) -> Result<JsValue, JsError> {
    Ok(to_value(&PreUtxoOutput {
        value: utxo.value,
        owner: to_hex_ser(&utxo.owner)?,
        type_: to_hex_ser(&utxo.type_.0)?,
    })?)
}

pub fn value_to_utxo_output(value: JsValue) -> Result<UtxoOutput, JsError> {
    let pre: PreUtxoOutput = from_value(value)?;
    Ok(UtxoOutput {
        value: pre.value,
        owner: from_hex_ser(&pre.owner)?,
        type_: UnshieldedTokenType(from_hex_ser(&pre.type_)?),
    })
}

pub fn utxo_to_value(utxo: &Utxo) -> Result<JsValue, JsError> {
    Ok(to_value(&PreUtxo {
        value: utxo.value,
        owner: to_hex_ser(&utxo.owner)?,
        type_: to_hex_ser(&utxo.type_.0)?,
        intent_hash: to_hex_ser(&utxo.intent_hash)?,
        output_no: utxo.output_no,
    })?)
}

pub fn value_to_utxo(value: JsValue) -> Result<Utxo, JsError> {
    let pre: PreUtxo = from_value(value)?;
    Ok(Utxo {
        value: pre.value,
        owner: from_hex_ser(&pre.owner)?,
        type_: UnshieldedTokenType(from_hex_ser(&pre.type_)?),
        intent_hash: from_hex_ser(&pre.intent_hash)?,
        output_no: pre.output_no,
    })
}

pub fn value_to_utxo_meta(value: JsValue) -> Result<UtxoMeta, JsError> {
    let pre: PreUtxoMeta = from_value(value)?;
    Ok(UtxoMeta {
        ctime: Timestamp::from_secs(js_date_to_seconds(&pre.ctime)),
    })
}

pub fn qdo_to_value(qdo: &QualifiedDustOutput) -> Result<JsValue, JsError> {
    Ok(to_value(&PreQualifiedDustOutput {
        initial_value: qdo.initial_value,
        owner: fr_to_bigint(qdo.owner.0),
        nonce: fr_to_bigint(qdo.nonce),
        seq: qdo.seq,
        ctime: seconds_to_js_date(qdo.ctime.to_secs()),
        backing_night: to_hex_ser(&qdo.backing_night.0)?,
        mt_index: qdo.mt_index,
    })?)
}

pub fn value_to_qdo(value: JsValue) -> Result<QualifiedDustOutput, JsError> {
    let pre: PreQualifiedDustOutput = from_value(value)?;
    Ok(QualifiedDustOutput {
        initial_value: pre.initial_value,
        owner: DustPublicKey(bigint_to_fr(pre.owner)?),
        nonce: bigint_to_fr(pre.nonce)?,
        seq: pre.seq,
        ctime: Timestamp::from_secs(js_date_to_seconds(&pre.ctime)),
        backing_night: InitialNonce(from_hex_ser(&pre.backing_night)?),
        mt_index: pre.mt_index,
    })
}

pub fn dust_gen_info_to_value(gen_info: &DustGenerationInfo) -> Result<JsValue, JsError> {
    Ok(to_value(&PreDustGenerationInfo {
        value: gen_info.value,
        owner: fr_to_bigint(gen_info.owner.0),
        nonce: to_hex_ser(&gen_info.nonce.0)?,
        dtime: if gen_info.dtime == Timestamp::MAX {
            None
        } else {
            Some(seconds_to_js_date(gen_info.dtime.to_secs()))
        },
    })?)
}

pub fn value_to_dust_gen_info(value: JsValue) -> Result<DustGenerationInfo, JsError> {
    let pre: PreDustGenerationInfo = from_value(value)?;
    let dtime = pre
        .dtime
        .map(|time| Timestamp::from_secs(js_date_to_seconds(&time)))
        .unwrap_or(Timestamp::MAX);
    Ok(DustGenerationInfo {
        value: pre.value,
        owner: DustPublicKey(bigint_to_fr(pre.owner)?),
        nonce: InitialNonce(from_hex_ser(&pre.nonce)?),
        dtime,
    })
}

pub fn value_to_dust_params(value: JsValue) -> Result<DustParameters, JsError> {
    let pre: PreDustParameters = from_value(value)?;
    construct_dust_parameters(
        pre.night_dust_ratio,
        pre.generation_decay_rate,
        pre.dust_grace_period_seconds,
    )
}

pub fn construct_dust_parameters(
    night_dust_ratio: BigInt,
    generation_decay_rate: BigInt,
    dust_grace_period_seconds: BigInt,
) -> Result<DustParameters, JsError> {
    let night_dust_ratio = u64::try_from(night_dust_ratio)
        .map_err(|_| JsError::new("night_dust_ratio is out of range"))?;

    let generation_decay_rate = bigint_to_u32(generation_decay_rate)?;

    let dust_grace_period = Duration::from_secs(
        i128::try_from(dust_grace_period_seconds)
            .map_err(|_| JsError::new("dust_grace_period_seconds is out of range"))?,
    );

    Ok(DustParameters {
        night_dust_ratio,
        generation_decay_rate,
        dust_grace_period,
    })
}

pub fn contract_action_to_value<P: ProofKind<InMemoryDB>>(
    action: &ContractAction<P, InMemoryDB>,
) -> JsValue
where
    ContractCall: From<ledger::structure::ContractCall<P, InMemoryDB>>,
{
    match action {
        ContractAction::Call(call) => JsValue::from(ContractCall::from((**call).clone())),
        ContractAction::Deploy(deploy) => JsValue::from(ContractDeploy(deploy.clone())),
        ContractAction::Maintain(upd) => JsValue::from(MaintenanceUpdate(upd.clone())),
    }
}

pub struct ContractActionConverter<P: ProofKind<InMemoryDB>>(PhantomData<P>);

impl<P: ProofKind<InMemoryDB>> ContractActionConverter<P> {
    fn untyped_actions_try_from_value(
        action: &JsValue,
    ) -> Result<Option<ContractAction<P, InMemoryDB>>, JsError> {
        match ContractDeploy::try_ref(&action)? {
            Some(deploy) => Ok(Some(deploy.0.clone().into())),
            _ => match MaintenanceUpdate::try_ref(&action)? {
                Some(update) => Ok(Some(update.0.clone().into())),
                _ => Ok(None),
            },
        }
    }
}

impl ContractActionConverter<ProofPreimageMarker> {
    pub fn try_from_value(
        action: &JsValue,
    ) -> Result<ContractAction<ProofPreimageMarker, InMemoryDB>, JsError> {
        use ContractCallTypes::*;
        match ContractCall::try_ref(&action)? {
            Some(contract_call) => Ok(match &contract_call.0 {
                UnprovenContractCall(call) => call.clone().into(),
                _ => Err(JsError::new("Wrong ContractCall type."))?,
            }),
            _ => match Self::untyped_actions_try_from_value(&action)? {
                Some(contract_action) => Ok(contract_action),
                _ => Err(JsError::new("Unexpected action type provided.")),
            },
        }
    }
}
impl ContractActionConverter<ProofMarker> {
    pub fn try_from_value(
        action: &JsValue,
    ) -> Result<ContractAction<ProofMarker, InMemoryDB>, JsError> {
        use ContractCallTypes::*;
        match ContractCall::try_ref(&action)? {
            Some(contract_call) => Ok(match &contract_call.0 {
                ProvenContractCall(call) => call.clone().into(),
                _ => Err(JsError::new("Wrong ContractCall type"))?,
            }),
            _ => match Self::untyped_actions_try_from_value(&action)? {
                Some(contract_action) => Ok(contract_action),
                _ => Err(JsError::new("Expected action type")),
            },
        }
    }
}
impl ContractActionConverter<()> {
    pub fn try_from_value(action: &JsValue) -> Result<ContractAction<(), InMemoryDB>, JsError> {
        use ContractCallTypes::*;
        match ContractCall::try_ref(&action)? {
            Some(contract_call) => Ok(match &contract_call.0 {
                ProofErasedContractCall(call) => call.clone().into(),
                _ => Err(JsError::new("Wrong ContractCall type"))?,
            }),
            _ => match Self::untyped_actions_try_from_value(&action)? {
                Some(contract_action) => Ok(contract_action),
                _ => Err(JsError::new("Expected action type")),
            },
        }
    }
}

pub fn intents_to_value_map<
    S: SignatureKind<InMemoryDB>,
    P: ProofKind<InMemoryDB>,
    B: Storable<InMemoryDB>,
>(
    intents: storage::storage::HashMap<u16, LedgerIntent<S, P, B, InMemoryDB>, InMemoryDB>,
) -> Map
where
    Intent: From<LedgerIntent<S, P, B, InMemoryDB>>,
{
    let res = Map::new();
    for (segment_id, intent) in intents {
        res.set(
            &Number::from(segment_id),
            &JsValue::from(Intent::from(intent)),
        );
    }
    res
}

pub fn value_map_to_intent_vec(value_map: Map) -> Result<Vec<(u16, Intent)>, JsError> {
    let mut res = vec![];
    for key in value_map.keys() {
        let key = key.unwrap();
        let value = value_map.get(&key);
        let intent = Intent::try_ref(&value)?.ok_or(JsError::new("Unable to decode Intent."))?;
        res.push((from_value(key)?, intent.clone()));
    }
    Ok(res)
}

pub fn fallible_coins_to_value_map<P: ProofKind<InMemoryDB>>(
    coins: HashMap<u16, zswap::Offer<P::LatestProof, InMemoryDB>>,
) -> Option<Map>
where
    ZswapOffer: From<zswap::Offer<P::LatestProof, InMemoryDB>>,
{
    let res = Map::new();
    coins.iter().for_each(|coin| {
        res.set(
            &JsValue::from(coin.0.deref().clone()),
            &JsValue::from(ZswapOffer::from(coin.1.deref().clone())),
        );
    });
    if res.size() > 0 { Some(res) } else { None }
}

pub fn zswap_offers_to_fallible_coins<P: ProofKind<InMemoryDB>>(
    offers: Vec<(u16, ZswapOffer)>,
) -> Result<HashMap<u16, zswap::Offer<P::LatestProof, InMemoryDB>>, JsError>
where
    zswap::Offer<P::LatestProof, InMemoryDB>: TryFrom<ZswapOffer, Error = JsError>,
{
    let mut new_offers = HashMap::new();
    for (k, v) in offers {
        new_offers = new_offers.insert(k, v.try_into()?);
    }
    Ok(new_offers)
}

pub fn fee_prices_to_value(fee_prices: &FeePrices) -> Result<JsValue, JsError> {
    Ok(to_value(&PreFeePrices {
        read_price: fee_prices.read_price.into(),
        compute_price: fee_prices.compute_price.into(),
        block_usage_price: fee_prices.block_usage_price.into(),
        write_price: fee_prices.write_price.into(),
    })?)
}

pub fn do_vecs_match<T: PartialEq>(a: &[T], b: &[T]) -> bool {
    let matching = a.iter().zip(b).filter(|&(a, b)| a == b).count();
    matching == a.len() && matching == b.len()
}

pub fn seconds_to_js_date(seconds: u64) -> Date {
    let date = Date::new_0();
    date.set_time((seconds * 1000) as f64);
    date
}

pub fn js_date_to_seconds(date: &Date) -> u64 {
    (date.get_time() / 1000f64) as u64
}

pub fn try_to_string(jsv: JsValue) -> String {
    let res = js_sys::Reflect::get(&jsv, &"toString".into())
        .and_then(|f| f.dyn_into::<Function>())
        .and_then(|f| f.call0(&jsv))
        .and_then(|s| s.dyn_into::<JsString>());
    match res {
        Ok(s) => s.into(),
        Err(_) => "<failed to stringify>".into(),
    }
}

pub fn fr_to_bigint(fr: Fr) -> BigInt {
    let mut bytes = fr.as_le_bytes();
    bytes.reverse();
    BigInt::new(&JsString::from(format!(
        "0x{}",
        bytes.encode_hex::<String>()
    )))
    .expect("excoding to bigint should succeed")
}

pub fn bigint_to_fr(bigint: BigInt) -> Result<Fr, JsError> {
    let hex_str = String::from(
        bigint
            .to_string(16)
            .map_err(|err| JsError::new(&String::from(err.to_string())))?,
    );
    let padded_str = if hex_str.len() % 2 == 1 {
        "0".to_owned() + &hex_str
    } else {
        hex_str
    };
    let mut bytes = <Vec<u8>>::from_hex(padded_str.as_bytes())?;
    bytes.reverse();
    Fr::from_le_bytes(&bytes).ok_or_else(|| JsError::new("out of bounds for prime field"))
}

pub fn bigint_to_u32(bigint: BigInt) -> Result<u32, JsError> {
    let bigint = u64::try_from(bigint).map_err(|_| JsError::new("value out of range"))?;
    if bigint > u32::MAX as u64 {
        return Err(JsError::new("value exceeded u32 max"));
    }
    Ok(bigint as u32)
}

pub fn claim_kind_to_text(kind: &ClaimKind) -> String {
    match kind {
        ClaimKind::Reward => String::from("Reward"),
        ClaimKind::CardanoBridge => String::from("CardanoBridge"),
    }
}
