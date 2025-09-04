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

use onchain_runtime::{ops::Op, result_mode::ResultMode};
use std::io::Read;
use storage::db::InMemoryDB;

use hex::FromHex;

use serialize::{Deserializable, Serializable, Tagged, tagged_deserialize, tagged_serialize};

pub mod context;
pub mod conversions;
pub mod primitives;
pub mod state;
pub mod vm;

pub(crate) use serde_wasm_bindgen::from_value;
use wasm_bindgen::JsError;

pub(crate) fn ensure_ops_valid<M: ResultMode<InMemoryDB>>(
    ops: &[Op<M, InMemoryDB>],
) -> Result<(), JsError>
where
    Op<M, InMemoryDB>: Eq,
{
    for op in ops.iter() {
        // Just serialize and deserialize, checking equality
        let mut ser = Vec::new();
        tagged_serialize(op, &mut ser)?;
        let op2: Op<M, InMemoryDB> = tagged_deserialize(&ser[..])?;
        if op != &op2 {
            return Err(JsError::new(
                "Operations didn't survive serialization check",
            ));
        }
    }
    Ok(())
}

pub(crate) fn to_value<T: serde::Serialize + ?Sized>(
    value: &T,
) -> Result<wasm_bindgen::JsValue, serde_wasm_bindgen::Error> {
    value.serialize(
        &serde_wasm_bindgen::Serializer::new().serialize_large_number_types_as_bigints(true),
    )
}

pub fn from_value_ser<T: Deserializable + Tagged>(
    data: js_sys::Uint8Array,
    struct_name: &str,
) -> Result<T, wasm_bindgen::JsError> {
    use std::io::{Error, ErrorKind};
    Ok(
        tagged_deserialize(&mut data.to_vec().as_slice()).map_err(|e| {
            Error::new(
                ErrorKind::InvalidData,
                format!(
                    "Unable to deserialize {}. Error: {}",
                    struct_name,
                    e.to_string()
                ),
            )
        })?,
    )
}

pub fn from_value_hex_ser<T: Deserializable>(data: &str) -> Result<T, wasm_bindgen::JsError> {
    from_hex_ser_checked(&mut &<Vec<u8>>::from_hex(data.as_bytes())?[..]).map_err(Into::into)
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

pub(crate) fn to_value_ser<T: Serializable + Tagged>(
    value: &T,
) -> Result<wasm_bindgen::JsValue, wasm_bindgen::JsError> {
    let mut result = Vec::new();
    tagged_serialize(value, &mut result)?;
    Ok(js_sys::Uint8Array::from(&result[..]).into())
}

pub fn to_value_hex_ser<T: Serializable + ?Sized>(
    value: &T,
) -> Result<String, wasm_bindgen::JsError> {
    let mut result = Vec::new();
    T::serialize(value, &mut result)?;
    Ok(hex::encode(&result))
}
