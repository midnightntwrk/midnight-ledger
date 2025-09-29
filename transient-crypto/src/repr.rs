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

//! This module deals with representing data as sequences of binary objects for
//! use in persistent hashing, and as field elements for use in proofs,
//! primarily through the [`FieldRepr`], [`BinaryHashRepr`], and
//! [`FromFieldRepr`] traits.

use crate::curve::{FR_BYTES, FR_BYTES_STORED, Fr};
use crate::hash::hash_to_field;
use base_crypto::repr::{BinaryHashRepr, MemWrite};
use base_crypto::time::Timestamp;
pub use derive::{FieldRepr, FromFieldRepr};
use serialize::{Deserializable, Serializable, VecExt};
use storage::Storable;
use storage::db::DB;
use storage::storage::Map;

/// A type this implements this can be transformed into an iterator of [`Fr`]s.
pub trait FieldRepr {
    /// Writes out `self` as a sequence of [Fr] elements.
    /// As a general rule of thumb, this should usually produces a known number of elements.
    fn field_repr<W: MemWrite<Fr>>(&self, writer: &mut W);
    /// The size of a value when represented as field elements.
    fn field_size(&self) -> usize;
    /// Writes the hash repr into a vector
    fn field_vec(&self) -> Vec<Fr> {
        let mut res = Vec::with_bounded_capacity(self.field_size());
        self.field_repr(&mut res);
        res
    }
}

/// A type than can be parsed from a sequence of [`Fr`]s.
pub trait FromFieldRepr: Sized {
    /// The number of elements this type can be reconstructed from.
    const FIELD_SIZE: usize;
    /// Attempts to parse from a slice of [`FIELD_SIZE`](Self::FIELD_SIZE) elements.
    fn from_field_repr(repr: &[Fr]) -> Option<Self>;
}

impl BinaryHashRepr for Fr {
    fn binary_repr<W: MemWrite<u8>>(&self, writer: &mut W) {
        writer.write(&self.as_le_bytes())
    }
    fn binary_len(&self) -> usize {
        FR_BYTES
    }
}

macro_rules! tuple_repr {
    ($head:ident$(, $tail:ident)*) => {
        #[allow(unused_parens, non_snake_case)]
        impl<$head: FieldRepr$(, $tail: FieldRepr)*> FieldRepr for ($head, $($tail),*) {
            fn field_repr<W: MemWrite<Fr>>(&self, writer: &mut W) {
                let ($head, $($tail),*) = self;
                $head.field_repr(writer);
                $($tail.field_repr(writer);)*
            }
            fn field_size(&self) -> usize {
                let ($head, $($tail),*) = self;
                $head.field_size() $(+ $tail.field_size())*
            }
        }
        #[allow(unused_parens, non_snake_case)]
        impl<$head: FromFieldRepr$(, $tail: FromFieldRepr)*> FromFieldRepr for ($head, $($tail),*) {
            const FIELD_SIZE: usize = <$head as FromFieldRepr>::FIELD_SIZE$( + <$tail as FromFieldRepr>::FIELD_SIZE)*;
            fn from_field_repr(repr: &[Fr]) -> Option<Self> {
                if repr.len() != Self::FIELD_SIZE {
                    return None;
                }
                let __head_size = <$head as FromFieldRepr>::FIELD_SIZE;
                let $head = <$head as FromFieldRepr>::from_field_repr(&repr[..__head_size])?;
                let ($($tail, )*) = <($($tail, )*) as FromFieldRepr>::from_field_repr(&repr[__head_size..])?;
                Some(($head, $($tail),*))
            }
        }
        tuple_repr!($($tail),*);
    };
    () => {
        impl FieldRepr for () {
            fn field_repr<W: MemWrite<Fr>>(&self, _: &mut W) {
            }
            fn field_size(&self) -> usize {
                0
            }
        }
        impl FromFieldRepr for () {
            const FIELD_SIZE: usize = 0;
            fn from_field_repr(repr: &[Fr]) -> Option<Self> {
                if repr.is_empty() {
                    Some(())
                } else {
                    None
                }
            }
        }
    };
}

tuple_repr!(A, B, C, D, E, F, G, H, I, J, K, L);

impl FromFieldRepr for [u8; 32] {
    const FIELD_SIZE: usize = 2;
    fn from_field_repr(repr: &[Fr]) -> Option<Self> {
        if repr.len() != 2 {
            return None;
        }
        let repr0 = repr[0].0.to_bytes_le();
        let repr1 = repr[1].0.to_bytes_le();
        if repr0[1..].iter().any(|ch| *ch != 0) {
            return None;
        }
        if repr1[31..].iter().any(|ch| *ch != 0) {
            return None;
        }
        let mut res = [0u8; 32];
        res[31..].copy_from_slice(&repr0[..1]);
        res[0..31].copy_from_slice(&repr1[..31]);
        Some(res)
    }
}

/// Converts a sequence of field elements into a corresponding byte vector.
/// Guarantees that the results [`FieldRepr`] matches the input.
pub fn bytes_from_field_repr(repr: &mut &[Fr], n: usize) -> Option<Vec<u8>> {
    let stray = n % FR_BYTES_STORED;
    let chunks = n / FR_BYTES_STORED;
    let expected_size = chunks + (stray != 0) as usize;
    if repr.len() < expected_size {
        return None;
    }
    let mut res = vec![0u8; n];
    let bytes_from = |slice: &mut [u8], k, f: Fr| {
        let repr = f.as_le_bytes();
        if repr[k..].iter().any(|b| *b != 0) {
            None
        } else {
            slice.copy_from_slice(&repr[..k]);
            Some(())
        }
    };
    if stray > 0 {
        bytes_from(&mut res[n - stray..], stray, repr[0])?;
        *repr = &repr[1..];
    }
    for i in 0..chunks {
        bytes_from(
            &mut res[i * FR_BYTES_STORED..(i + 1) * FR_BYTES_STORED],
            FR_BYTES_STORED,
            repr[chunks - 1 - i],
        )?;
    }
    *repr = &repr[chunks..];
    Some(res)
}

impl FieldRepr for [u8] {
    fn field_repr<W: MemWrite<Fr>>(&self, writer: &mut W) {
        let mut slice = self;
        while !slice.is_empty() {
            let len = slice.len();
            let stray = len % FR_BYTES_STORED;
            if stray != 0 {
                writer.write(&[Fr::from_le_bytes(&slice[len - stray..])
                    .expect("Must fall in storable byte range")]);
                slice = &slice[..len - stray];
            } else {
                let start = len - usize::min(FR_BYTES_STORED, len);
                writer
                    .write(&[Fr::from_le_bytes(&slice[start..])
                        .expect("Must fall in storable byte range")]);
                slice = &slice[..start];
            }
        }
    }
    fn field_size(&self) -> usize {
        self.len().div_ceil(FR_BYTES_STORED)
    }
}

impl<const N: usize> FieldRepr for [u8; N] {
    fn field_repr<W: MemWrite<Fr>>(&self, writer: &mut W) {
        <[u8]>::field_repr(self, writer)
    }
    fn field_size(&self) -> usize {
        self.len() / FR_BYTES_STORED
            + if self.len() % FR_BYTES_STORED == 0 {
                0
            } else {
                1
            }
    }
}

impl<T: FieldRepr> FieldRepr for Vec<T> {
    fn field_repr<W: MemWrite<Fr>>(&self, writer: &mut W) {
        for e in self.iter() {
            e.field_repr(writer);
        }
    }
    fn field_size(&self) -> usize {
        self.iter().map(|e| e.field_size()).sum()
    }
}

// An odd one out, the assumption is this is a dynamically-sized string.
impl FieldRepr for str {
    fn field_repr<W: MemWrite<Fr>>(&self, writer: &mut W) {
        writer.write(&[hash_to_field(self.as_bytes())]);
    }
    fn field_size(&self) -> usize {
        1
    }
}

macro_rules! via_from_field {
    ($($ty:ty),*) => {
        $(
            impl FieldRepr for $ty {
                fn field_repr<W: MemWrite<Fr>>(&self, writer: &mut W) {
                    writer.write(&[Fr::from(*self)]);
                }
                fn field_size(&self) -> usize {
                    1
                }
            }

            impl FromFieldRepr for $ty {
                const FIELD_SIZE: usize = 1;
                fn from_field_repr(repr: &[Fr]) -> Option<Self> {
                    if repr.len() == 1 {
                        Some(repr[0].try_into().ok()?)
                    } else {
                        None
                    }
                }
            }
        )*
    }
}

via_from_field!(u128, u64, u32, u16, u8, i128, i64, i32, i16, i8, Fr, bool);

impl FieldRepr for Timestamp {
    fn field_repr<W: MemWrite<Fr>>(&self, writer: &mut W) {
        self.to_secs().field_repr(writer)
    }
    fn field_size(&self) -> usize {
        self.to_secs().field_size()
    }
}

impl<T: FieldRepr> FieldRepr for Option<T> {
    fn field_repr<W: MemWrite<Fr>>(&self, writer: &mut W) {
        match self {
            Some(v) => {
                writer.write(&[1u8.into()]);
                v.field_repr(writer);
            }
            None => writer.write(&[0u8.into()]),
        }
    }
    fn field_size(&self) -> usize {
        match self {
            Some(v) => 1 + v.field_size(),
            None => 1,
        }
    }
}

impl FieldRepr for [Fr] {
    fn field_repr<W: MemWrite<Fr>>(&self, writer: &mut W) {
        writer.write(self);
    }
    fn field_size(&self) -> usize {
        self.len()
    }
}

impl<const N: usize> FieldRepr for [Fr; N] {
    fn field_repr<W: MemWrite<Fr>>(&self, writer: &mut W) {
        writer.write(self)
    }
    fn field_size(&self) -> usize {
        N
    }
}

impl<K: FieldRepr + Serializable + Deserializable + Ord + Clone, V: FieldRepr + Storable<D>, D: DB>
    FieldRepr for Map<K, V, D>
{
    fn field_repr<W: MemWrite<Fr>>(&self, writer: &mut W) {
        writer.write(&[(self.size() as u64).into()]);
        let mut vec: Vec<_> = self.iter().collect();
        vec.sort_by_key(|(k, _)| k.clone());
        for (k, v) in vec.iter() {
            k.field_repr(writer);
            v.field_repr(writer);
        }
    }

    fn field_size(&self) -> usize {
        1 + self
            .iter()
            .map(|(k, v)| k.field_size() + v.field_size())
            .sum::<usize>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_field_repr_matches_compact() {
        let test_vector = b" 0 1 2 3 4 5 6 7 8 9101112131415161718192021222324252627282930";
        assert_eq!(
            test_vector.field_vec(),
            vec![
                Fr::from_le_bytes(b"5161718192021222324252627282930")
                    .expect("known value must be in range"),
                Fr::from_le_bytes(b" 0 1 2 3 4 5 6 7 8 910111213141")
                    .expect("known value must be in range"),
            ]
        )
    }

    #[test]
    fn byte32_encoding() {
        let test_vector = b" 0 1 2 3 4 5 6 7 8 9101112131415";
        assert_eq!(
            <[u8; 32]>::from_field_repr(&test_vector.field_vec()),
            Some(*test_vector)
        );
    }
}
