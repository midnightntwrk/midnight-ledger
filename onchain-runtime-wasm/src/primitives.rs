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

use std::borrow::Cow;

use crate::state::from_maybe_string;
use crate::{ensure_ops_valid, from_value, from_value_hex_ser, to_value, to_value_hex_ser};
use base_crypto::fab::{AlignedValue, Alignment, Value};
use base_crypto::hash::{HashOutput, PERSISTENT_HASH_BYTES, PersistentHashWriter};
use base_crypto::repr::BinaryHashRepr;
use base_crypto::{hash, signatures};
use coin_structure::coin::{ShieldedTokenType, UserAddress};
use coin_structure::contract::ContractAddress;
use hex::{FromHex, ToHex};
use js_sys::{BigInt, JsString, Uint8Array};
use onchain_runtime::ops::Op;
use onchain_runtime::result_mode::ResultModeVerify;
use onchain_runtime::state::EntryPointBuf;
use rand::Rng;
use rand::rngs::OsRng;
use serialize::tagged_serialize;
use storage::db::InMemoryDB;
use transient_crypto;
use transient_crypto::curve;
use transient_crypto::curve::Fr;
use transient_crypto::fab::{AlignedValueExt, AlignmentExt, ValueReprAlignedValue};
use transient_crypto::repr::FieldRepr;
use wasm_bindgen::prelude::*;

#[wasm_bindgen(js_name = "entryPointHash")]
pub fn entry_point_hash(entry_point: JsValue) -> Result<String, JsError> {
    let entry_point = EntryPointBuf(from_maybe_string(entry_point)?);
    to_value_hex_ser(&entry_point.ep_hash())
}

#[wasm_bindgen(js_name = "communicationCommitmentRandomness")]
pub fn communication_commitment_randomness() -> Result<String, JsError> {
    to_value_hex_ser(&OsRng.r#gen::<Fr>())
}

#[wasm_bindgen(js_name = "communicationCommitment")]
pub fn communication_commitment(
    input: JsValue,
    output: JsValue,
    rand: &str,
) -> Result<String, JsError> {
    let input = from_value(input)?;
    let output = from_value(output)?;
    let rand = from_value_hex_ser(rand)?;
    to_value_hex_ser(&onchain_runtime::communication_commitment(
        input, output, rand,
    ))
}

#[wasm_bindgen(js_name = "sampleSigningKey")]
pub fn sample_signing_key() -> Result<String, JsError> {
    to_value_hex_ser(&signatures::SigningKey::sample(OsRng))
}

#[wasm_bindgen(js_name = "signingKeyFromBip340")]
pub fn signing_key_from_bip_340(bytes: Uint8Array) -> Result<String, JsError> {
    to_value_hex_ser(
        &signatures::SigningKey::from_bytes(&bytes.to_vec())
            .map_err(|err| JsError::new(&String::from(err.to_string())))?,
    )
}

#[wasm_bindgen(js_name = "signData")]
pub fn sign_data(key: &str, data: Uint8Array) -> Result<String, JsError> {
    let key: signatures::SigningKey = from_value_hex_ser(key)?;
    to_value_hex_ser(&key.sign(&mut OsRng, &data.to_vec()))
}

#[wasm_bindgen(js_name = "signatureVerifyingKey")]
pub fn signature_verifying_key(key: &str) -> Result<String, JsError> {
    let key: signatures::SigningKey = from_value_hex_ser(key)?;
    to_value_hex_ser(&key.verifying_key())
}

#[wasm_bindgen(js_name = "verifySignature")]
pub fn verify_signature(key: &str, data: Uint8Array, signature: &str) -> Result<bool, JsError> {
    let key: signatures::VerifyingKey = from_value_hex_ser(key)?;
    let signature: signatures::Signature = from_value_hex_ser(signature)?;
    Ok(key.verify(&data.to_vec(), &signature))
}

#[wasm_bindgen(js_name = "rawTokenType")]
pub fn raw_token_type(domain_sep: Uint8Array, contract: &str) -> Result<String, JsError> {
    let contract: ContractAddress = from_value_hex_ser(contract)?;
    let domain_sep_buf = <[u8; PERSISTENT_HASH_BYTES]>::try_from(domain_sep.to_vec())
        .map_err(|_| JsError::new("Expected 32-byte bytearray"))?;
    to_value_hex_ser(
        &contract
            .custom_shielded_token_type(HashOutput(domain_sep_buf))
            .0,
    )
}

#[wasm_bindgen(js_name = "sampleContractAddress")]
pub fn sample_contract_address() -> Result<String, JsError> {
    to_value_hex_ser(&ContractAddress(OsRng.r#gen::<HashOutput>()))
}

#[wasm_bindgen(js_name = "sampleUserAddress")]
pub fn sample_user_address() -> Result<String, JsError> {
    to_value_hex_ser(&UserAddress(OsRng.r#gen::<HashOutput>()))
}

#[wasm_bindgen(js_name = "sampleRawTokenType")]
pub fn sample_raw_token_type() -> Result<String, JsError> {
    to_value_hex_ser(&ShieldedTokenType(OsRng.r#gen::<HashOutput>()).0)
}

#[wasm_bindgen(js_name = "dummyContractAddress")]
pub fn dummy_contract_address() -> Result<String, JsError> {
    to_value_hex_ser(&ContractAddress::default())
}

#[wasm_bindgen(js_name = "dummyUserAddress")]
pub fn dummy_user_address() -> Result<String, JsError> {
    to_value_hex_ser(&UserAddress::default())
}

#[wasm_bindgen(js_name = "runtimeCoinCommitment")]
pub fn runtime_coin_commitment(coin: JsValue, recipient: JsValue) -> Result<JsValue, JsError> {
    let coin: AlignedValue = from_value(coin)?;
    let recipient: AlignedValue = from_value(recipient)?;
    let coin = coin_structure::coin::Info::try_from(&**(AsRef::<Value>::as_ref(&coin)))?;
    let recipient =
        coin_structure::transfer::Recipient::try_from(&**(AsRef::<Value>::as_ref(&recipient)))?;
    Ok(to_value(&AlignedValue::from(coin.commitment(&recipient)))?)
}

#[wasm_bindgen(js_name = "runtimeCoinNullifier")]
pub fn runtime_coin_nullifier(coin: JsValue, sender_evidence: JsValue) -> Result<JsValue, JsError> {
    let coin: AlignedValue = from_value(coin)?;
    let sender_evidence: AlignedValue = from_value(sender_evidence)?;
    let coin = coin_structure::coin::Info::try_from(&**(AsRef::<Value>::as_ref(&coin)))?;
    let sender_evidence = coin_structure::transfer::SenderEvidence::try_from(
        &**(AsRef::<Value>::as_ref(&sender_evidence)),
    )?;
    Ok(to_value(&AlignedValue::from(
        coin.nullifier(&sender_evidence),
    ))?)
}

#[wasm_bindgen(js_name = "leafHash")]
pub fn leaf_hash(value: JsValue) -> Result<JsValue, JsError> {
    let value: AlignedValue = from_value(value)?;
    Ok(to_value(&AlignedValue::from(
        transient_crypto::merkle_tree::leaf_hash(&ValueReprAlignedValue(value)),
    ))?)
}

#[wasm_bindgen(js_name = "maxAlignedSize")]
pub fn max_aligned_size(alignment: JsValue) -> Result<u64, JsError> {
    let alignment: Alignment = from_value(alignment)?;
    Ok(alignment.max_aligned_size() as u64)
}

#[wasm_bindgen(js_name = "maxField")]
pub fn max_field() -> Result<BigInt, JsError> {
    // -1 is the largest representable value
    let mut bytes = (-curve::Fr::from(1)).as_le_bytes();
    bytes.reverse();
    BigInt::new(&JsString::from(format!(
        "0x{}",
        bytes.encode_hex::<String>()
    )))
    .map_err(|err| JsError::new(&String::from(err.to_string())))
}

#[wasm_bindgen(js_name = "proofDataIntoSerializedPreimage")]
pub fn proof_data_into_serialized_preimage(
    input: JsValue,
    output: JsValue,
    public_transcript: JsValue,
    private_transcript_outputs: JsValue,
    key_location: Option<String>,
) -> Result<Uint8Array, JsError> {
    let input: AlignedValue = from_value(input)?;
    let output: AlignedValue = from_value(output)?;
    let public_transcript: Vec<Op<ResultModeVerify, InMemoryDB>> = from_value(public_transcript)?;
    ensure_ops_valid(&public_transcript)?;
    let private_transcript_outputs: Vec<AlignedValue> = from_value(private_transcript_outputs)?;
    let mut private_transcript = Vec::new();
    for entry in private_transcript_outputs.iter() {
        entry.value_only_field_repr(&mut private_transcript);
    }
    let mut public_transcript_outputs = Vec::new();
    for op in public_transcript.iter() {
        if let Op::Popeq { result, .. } = op {
            result.value_only_field_repr(&mut public_transcript_outputs);
        }
    }
    let mut public_transcript_inputs = Vec::new();
    for op in public_transcript.iter() {
        op.field_repr(&mut public_transcript_inputs);
    }
    let mut comm_comm_preimage = vec![0.into()];
    input.value_only_field_repr(&mut comm_comm_preimage);
    output.value_only_field_repr(&mut comm_comm_preimage);
    let preimage = transient_crypto::proofs::ProofPreimage {
        inputs: ValueReprAlignedValue(input).field_vec(),
        binding_input: 0.into(),
        private_transcript,
        public_transcript_inputs,
        public_transcript_outputs,
        key_location: transient_crypto::proofs::KeyLocation(
            key_location
                .map(Cow::Owned)
                .unwrap_or(Cow::Borrowed("dummy")),
        ),
        communications_commitment: Some((
            transient_crypto::hash::transient_hash(&comm_comm_preimage),
            0.into(),
        )),
    };
    let mut buf = Vec::new();
    tagged_serialize(&preimage, &mut buf)?;
    Ok(buf[..].into())
}

#[wasm_bindgen(js_name = "bigIntModFr")]
// funciton bigIntModFr(x: BigInt): BigInt
pub fn bigint_mod_fr(x: BigInt) -> Result<BigInt, JsError> {
    value_to_bigint(bigint_to_value(x)?)
}

#[wasm_bindgen(js_name = "valueToBigInt")]
// function valueToBigInt(x: Value): BigInt
pub fn value_to_bigint(x: JsValue) -> Result<BigInt, JsError> {
    let x: Value = from_value(x)?;
    let mut bytes = curve::Fr::try_from(&*x)?.as_le_bytes();
    bytes.reverse();
    BigInt::new(&JsString::from(format!(
        "0x{}",
        bytes.encode_hex::<String>()
    )))
    .map_err(|err| JsError::new(&String::from(err.to_string())))
}

#[wasm_bindgen(js_name = "bigIntToValue")]
// function bigIntToValue(x: BigInt): Value
pub fn bigint_to_value(x: BigInt) -> Result<JsValue, JsError> {
    let hex_str = String::from(
        x.to_string(16)
            .map_err(|err| JsError::new(&String::from(err.to_string())))?,
    );
    let padded_str = if hex_str.len() % 2 == 1 {
        "0".to_owned() + &hex_str
    } else {
        hex_str
    };
    let mut bytes = <Vec<u8>>::from_hex(padded_str.as_bytes())?;
    bytes.reverse();
    Ok(to_value(&Value::from(
        curve::Fr::from_le_bytes(&bytes)
            .ok_or_else(|| JsError::new("out of bounds for prime field"))?,
    ))?)
}

// function valueToBool(x: Value): bool
pub fn value_to_bool(x: JsValue) -> Result<bool, JsError> {
    let x: Value = from_value(x)?;
    Ok((&*x).try_into()?)
}

pub fn bool_to_value(x: bool) -> Result<JsValue, JsError> {
    Ok(to_value(&Value::from(x))?)
}

#[wasm_bindgen(js_name = "transientHash")]
// function transientHash(align: Alignment, val: Value): Value
// circuit transient_hash[a](val: a): Field
pub fn transient_hash(align: JsValue, val: JsValue) -> Result<JsValue, JsError> {
    let val = AlignedValue::new(from_value(val)?, from_value(align)?)
        .ok_or(JsError::new("invalid alignment supplied"))?;
    let repr = ValueReprAlignedValue(val).field_vec();
    Ok(to_value(&Value::from(
        transient_crypto::hash::transient_hash(&repr),
    ))?)
}

#[wasm_bindgen(js_name = "transientCommit")]
// function transientCommit(align: Alignment, val: Value, opening: Value): Value
// circuit transient_commit[a](val: a, opening: Field): Field
pub fn transient_commit(
    align: JsValue,
    val: JsValue,
    opening: JsValue,
) -> Result<JsValue, JsError> {
    let val = AlignedValue::new(from_value(val)?, from_value(align)?)
        .ok_or(JsError::new("invalid alignment supplied"))?;
    let opening: Value = from_value(opening)?;
    Ok(to_value(&Value::from(
        transient_crypto::hash::transient_commit(
            &ValueReprAlignedValue(val),
            (&*opening).try_into()?,
        ),
    ))?)
}

#[wasm_bindgen(js_name = "persistentHash")]
// function persistentHash(a: Alignment, x: Value): Value
// circuit persistent_hash[a](x: a): Bytes[32]
pub fn persistent_hash(align: JsValue, val: JsValue) -> Result<JsValue, JsError> {
    let val = AlignedValue::new(from_value(val)?, from_value(align)?)
        .ok_or(JsError::new("invalid alignment supplied"))?;
    let mut hasher = PersistentHashWriter::default();
    ValueReprAlignedValue(val).binary_repr(&mut hasher);
    Ok(to_value(&Value::from(hasher.finalize()))?)
}

#[wasm_bindgen(js_name = "persistentCommit")]
// function persistentCommit(align: Alignment, val: Value, opening: Value): Value
// circuit persistent_commit[a](val: a, opening: Bytes[32]): Bytes[32]
pub fn persistent_commit(
    align: JsValue,
    val: JsValue,
    opening: JsValue,
) -> Result<JsValue, JsError> {
    let val = AlignedValue::new(from_value(val)?, from_value(align)?)
        .ok_or(JsError::new("invalid alignment supplied"))?;
    let opening: Value = from_value(opening)?;
    Ok(to_value(&Value::from(hash::persistent_commit(
        &ValueReprAlignedValue(val),
        (&*opening).try_into()?,
    )))?)
}

#[wasm_bindgen(js_name = "degradeToTransient")]
// function degradeToTransient(persistent: Value): Value
// circuit degrade_to_transient(persistent: Bytes[32]): Field
pub fn degrade_to_transient(persistent: JsValue) -> Result<JsValue, JsError> {
    let persistent: Value = from_value(persistent)?;
    Ok(to_value(&Value::from(
        transient_crypto::hash::degrade_to_transient((&*persistent).try_into()?),
    ))?)
}

#[wasm_bindgen(js_name = "upgradeFromTransient")]
// function upgradeFromTransient(transient: Value): Value
// circuit upgrade_from_transient(transient: Field): Bytes[32]
pub fn upgrade_from_transient(transient: JsValue) -> Result<JsValue, JsError> {
    let transient: Value = from_value(transient)?;
    Ok(to_value(&Value::from(
        transient_crypto::hash::upgrade_from_transient((&*transient).try_into()?),
    ))?)
}

#[wasm_bindgen(js_name = "hashToCurve")]
// function hashToCurve(align: Alignment, val: Value): AlignedValue
// circuit hash_to_curve[a](val: a): CurvePoint
pub fn hash_to_curve(align: JsValue, val: JsValue) -> Result<JsValue, JsError> {
    let val = AlignedValue::new(from_value(val)?, from_value(align)?)
        .ok_or(JsError::new("invalid alignment supplied"))?;
    Ok(to_value(&Value::from(
        transient_crypto::hash::hash_to_curve(&ValueReprAlignedValue(val)),
    ))?)
}

#[wasm_bindgen(js_name = "ecAdd")]
// function ecAdd(a: Value, b: Value): Value
// circuit ec_add(a: CurvePoint, b: CurvePoint): CurvePoint
pub fn ec_add(a: JsValue, b: JsValue) -> Result<JsValue, JsError> {
    let a: Value = from_value(a)?;
    let b: Value = from_value(b)?;
    let res =
        curve::EmbeddedGroupAffine::try_from(&*a)? + curve::EmbeddedGroupAffine::try_from(&*b)?;
    Ok(to_value(&Value::from(res))?)
}

#[wasm_bindgen(js_name = "ecMul")]
// function ecMul(a: Value, b: Value): Value
// circuit ec_mul(a: CurvePoint, b: Field): CurvePoint
pub fn ec_mul(a: JsValue, b: JsValue) -> Result<JsValue, JsError> {
    let a: Value = from_value(a)?;
    let b: Value = from_value(b)?;
    let res = curve::EmbeddedGroupAffine::try_from(&*a)? * curve::EmbeddedFr::try_from(&*b)?;
    Ok(to_value(&Value::from(res))?)
}

#[wasm_bindgen(js_name = "ecMulGenerator")]
// function ecMulGenerator(val: Value): Value
// circuit ec_mul_generator(val: Field): CurvePoint
pub fn ec_mul_generator(val: JsValue) -> Result<JsValue, JsError> {
    let val: Value = from_value(val)?;
    let res = curve::EmbeddedGroupAffine::generator() * curve::EmbeddedFr::try_from(&*val)?;
    Ok(to_value(&Value::from(res))?)
}
