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

use super::{Aligned, DynAligned};
use super::{AlignedValue, Value, ValueAtom, ValueSlice};
use crate::hash::{HashOutput, PERSISTENT_HASH_BYTES as PHB};
use std::convert::Infallible;
use std::error::Error;
use std::fmt::{self, Debug, Display, Formatter};

impl<T: Clone + Into<Value>> From<&T> for Value {
    fn from(val: &T) -> Value {
        val.clone().into()
    }
}

macro_rules! forward_primitive_value {
    ($($ty:ty),*) => {
        $(
            impl From<$ty> for Value {
                fn from(val: $ty) -> Value {
                    Value(vec![val.into()])
                }
            }

            impl TryFrom<&ValueSlice> for $ty {
                type Error = InvalidBuiltinDecode;

                fn try_from(value: &ValueSlice) -> Result<$ty, InvalidBuiltinDecode> {
                    if value.0.len() == 1 {
                        Ok(<$ty>::try_from(&value.0[0])?)
                    } else {
                        Err(InvalidBuiltinDecode(stringify!($ty)))
                    }
                }
            }
        )*
    }
}

impl From<u128> for ValueAtom {
    fn from(val: u128) -> ValueAtom {
        ValueAtom(val.to_le_bytes().to_vec()).normalize()
    }
}

impl TryFrom<&ValueAtom> for u128 {
    type Error = InvalidBuiltinDecode;

    fn try_from(value: &ValueAtom) -> Result<Self, Self::Error> {
        if value.0.len() <= 128 / 8 {
            let mut le_bytes = [0u8; 128 / 8];
            le_bytes[..value.0.len()].copy_from_slice(&value.0);
            Ok(u128::from_le_bytes(le_bytes))
        } else {
            Err(InvalidBuiltinDecode("Fr"))
        }
    }
}

macro_rules! wrap_via_u128 {
    ($($ty:ty),*) => {
        $(
            impl From<$ty> for ValueAtom {
                fn from(val: $ty) -> ValueAtom {
                    u128::from(val).into()
                }
            }

            impl TryFrom<&ValueAtom> for $ty {
                type Error = InvalidBuiltinDecode;

                fn try_from(value: &ValueAtom) -> Result<$ty, InvalidBuiltinDecode> {
                    u128::try_from(value)?.try_into().map_err(|_| InvalidBuiltinDecode(stringify!($ty)))
                }
            }
        )*
    }
}

impl From<bool> for ValueAtom {
    fn from(value: bool) -> Self {
        ValueAtom(vec![value as u8]).normalize()
    }
}

impl TryFrom<&ValueAtom> for bool {
    type Error = InvalidBuiltinDecode;

    fn try_from(value: &ValueAtom) -> Result<Self, Self::Error> {
        let byte = u8::try_from(value)?;
        match byte {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(InvalidBuiltinDecode("bool")),
        }
    }
}

wrap_via_u128!(u8, u16, u32, u64);

forward_primitive_value!(HashOutput, u8, u16, u32, u64, u128, bool, Vec<u8>);

impl From<HashOutput> for ValueAtom {
    fn from(hash: HashOutput) -> ValueAtom {
        ValueAtom(hash.0.to_vec()).normalize()
    }
}

impl TryFrom<&ValueAtom> for HashOutput {
    type Error = InvalidBuiltinDecode;

    fn try_from(value: &ValueAtom) -> Result<HashOutput, InvalidBuiltinDecode> {
        let mut buf = [0u8; PHB];
        if value.0.len() <= PHB {
            buf[..value.0.len()].copy_from_slice(&value.0[..]);
            Ok(HashOutput(buf))
        } else {
            Err(InvalidBuiltinDecode("HashOutput"))
        }
    }
}

impl From<&[u8]> for ValueAtom {
    fn from(val: &[u8]) -> ValueAtom {
        let mut vec = val.to_vec();
        while let Some(0u8) = vec.last() {
            vec.pop();
        }
        ValueAtom(vec)
    }
}

impl From<&ValueAtom> for Vec<u8> {
    fn from(value: &ValueAtom) -> Vec<u8> {
        value.0.clone()
    }
}

impl From<()> for ValueAtom {
    fn from((): ()) -> ValueAtom {
        ValueAtom(vec![])
    }
}

impl TryFrom<&ValueAtom> for () {
    type Error = InvalidBuiltinDecode;

    fn try_from(value: &ValueAtom) -> Result<(), InvalidBuiltinDecode> {
        if value.0.is_empty() {
            Ok(())
        } else {
            Err(InvalidBuiltinDecode("()"))
        }
    }
}

impl From<Vec<u8>> for ValueAtom {
    fn from(mut value: Vec<u8>) -> ValueAtom {
        while let Some(0u8) = value.last() {
            value.pop();
        }
        ValueAtom(value)
    }
}

macro_rules! tuple_conversions {
    () => {
        impl From<()> for Value {
            fn from(_: ()) -> Value {
                Value(Vec::new())
            }
        }

        impl TryFrom<&ValueSlice> for () {
            type Error = InvalidBuiltinDecode;

            fn try_from(value: &ValueSlice) -> Result<(), InvalidBuiltinDecode> {
                if value.0.is_empty() {
                    Ok(())
                } else {
                    Err(InvalidBuiltinDecode("()"))
                }
            }
        }
    };
    ($a:ident$(, $as:ident)*) => {
        impl<$a$(, $as)*> From<($a, $($as, )*)> for Value
            where Value: From<$a>$( + From<$as>)*
        {
            #[allow(non_snake_case)]
            fn from(($a, $($as, )*): ($a, $($as, )*)) -> Value {
                Value::concat([&Value::from($a)$(, &$as.into())*])
            }
        }

        impl<$a$(, $as)*> TryFrom<&ValueSlice> for ($a, $($as, )*)
            where
                $a: Aligned + for<'a> TryFrom<&'a ValueSlice, Error = InvalidBuiltinDecode>,
                $($as: Aligned + for<'a> TryFrom<&'a ValueSlice, Error = InvalidBuiltinDecode>,)*
        {
            type Error = InvalidBuiltinDecode;

            #[allow(non_snake_case)]
            fn try_from(mut val: &ValueSlice) -> Result<Self, InvalidBuiltinDecode> {
                let err = || InvalidBuiltinDecode(stringify!(($a, $($as),*)));
                let a_align = <$a>::alignment();
                let a_end = a_align.consume_internal(val, &|idx: &mut usize, _| *idx += 1, &|idx| *idx, 0usize).ok_or_else(err)?;
                let a_slice = ValueSlice::from_prim_slice(&val.0[..a_end]);
                let a_val = <$a>::try_from(a_slice)?;
                val = ValueSlice::from_prim_slice(&val.0[a_end..]);
                $(
                    let as_align = <$as>::alignment();
                    let as_end = as_align.consume_internal(val, &|idx: &mut usize, _| *idx += 1, &|idx| *idx, 0usize).ok_or_else(err)?;
                    let as_slice = ValueSlice::from_prim_slice(&val.0[..as_end]);
                    let $as = <$as>::try_from(as_slice)?;
                    val = ValueSlice::from_prim_slice(&val.0[as_end..]);
                )*
                if val.0.is_empty() {
                    Ok((a_val, $($as, )*))
                } else {
                    Err(err())
                }
            }
        }

        tuple_conversions!($($as),*);
    }
}

tuple_conversions!(A, B, C, D, E, F, G, H, I, J, K);

#[allow(clippy::from_over_into)]
impl Into<Value> for AlignedValue {
    fn into(self) -> Value {
        self.value.clone()
    }
}

// Implemented via From instead of into to allow Into<Value> impl above
impl<T: DynAligned> From<T> for AlignedValue
where
    Value: From<T>,
{
    fn from(inp: T) -> AlignedValue {
        let align = inp.dyn_alignment();
        let value = inp.into();
        AlignedValue::new(value, align).expect("Aligned value should match alignment")
    }
}

impl<T: Into<Value> + Default> From<Option<T>> for Value {
    fn from(inp: Option<T>) -> Value {
        let (is_some, value) = match inp {
            Some(val) => (true, val),
            None => (false, T::default()),
        };
        Value::concat([Value::from(is_some), value.into()].iter())
    }
}

impl<const N: usize> From<[u8; N]> for ValueAtom {
    fn from(inp: [u8; N]) -> ValueAtom {
        let mut vec = inp.to_vec();
        while let Some(0) = vec.last() {
            vec.pop();
        }
        ValueAtom(vec)
    }
}

impl<const N: usize> TryFrom<ValueAtom> for [u8; N] {
    type Error = InvalidBuiltinDecode;

    fn try_from(atom: ValueAtom) -> Result<[u8; N], InvalidBuiltinDecode> {
        let mut buf = [0u8; N];
        if atom.0.len() <= buf.len() {
            buf[..atom.0.len()].copy_from_slice(&atom.0);
            Ok(buf)
        } else {
            Err(InvalidBuiltinDecode(std::any::type_name::<[u8; N]>()))
        }
    }
}

impl<const N: usize> From<[u8; N]> for Value {
    fn from(inp: [u8; N]) -> Value {
        Value(vec![inp.into()])
    }
}

impl<const N: usize> TryFrom<Value> for [u8; N] {
    type Error = InvalidBuiltinDecode;

    fn try_from(mut value: Value) -> Result<[u8; N], InvalidBuiltinDecode> {
        if value.0.len() == 1 {
            value.0.remove(0).try_into()
        } else {
            Err(InvalidBuiltinDecode(std::any::type_name::<[u8; N]>()))
        }
    }
}

/// An error decoding data from a field-aligned binary format into a builtin
/// data type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidBuiltinDecode(pub &'static str);

impl Display for InvalidBuiltinDecode {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(
            formatter,
            "failed to decode for built-in type {} after successful typecheck",
            self.0
        )
    }
}

impl From<Infallible> for InvalidBuiltinDecode {
    fn from(e: Infallible) -> InvalidBuiltinDecode {
        match e {}
    }
}

impl Error for InvalidBuiltinDecode {}
