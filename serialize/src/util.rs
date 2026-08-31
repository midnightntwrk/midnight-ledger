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

use crate::{Deserializable, Serializable, Tagged};
#[cfg(feature = "proptest")]
use proptest::strategy::ValueTree;
#[cfg(feature = "proptest")]
use proptest::{
    strategy::{NewTree, Strategy},
    test_runner::TestRunner,
};
#[cfg(feature = "proptest")]
use rand::Rng;
#[cfg(feature = "proptest")]
use rand::distributions::{Distribution, Standard};
#[cfg(feature = "proptest")]
use std::fmt::Debug;
use std::io::{BufWriter, Read};
#[cfg(feature = "proptest")]
use std::marker::PhantomData;

pub trait VecExt {
    fn with_bounded_capacity(n: usize) -> Self;
}

impl<T> VecExt for Vec<T> {
    fn with_bounded_capacity(n: usize) -> Self {
        const MEMORY_LIMIT: usize = 1 << 25; // 32 MiB
        let alloc_limit = MEMORY_LIMIT / std::mem::size_of::<T>();
        Self::with_capacity(usize::min(alloc_limit, n))
    }
}

pub trait ReadExt: Read {
    fn read_exact_to_vec(&mut self, n: usize) -> std::io::Result<Vec<u8>> {
        const CHUNK_SIZE: usize = 4096;
        let mut res = Vec::with_capacity(CHUNK_SIZE);
        let mut len = 0;
        while n > len {
            let new_len = usize::min(n, len + CHUNK_SIZE);
            res.resize(new_len, 0);
            self.read_exact(&mut res[len..])?;
            len = new_len;
        }
        Ok(res)
    }
}

impl<R: Read> ReadExt for R {}

impl Serializable for () {
    fn serialize(&self, _writer: &mut impl std::io::Write) -> std::io::Result<()> {
        Ok(())
    }
    fn serialized_size(&self) -> usize {
        0
    }
}

impl Deserializable for () {
    fn deserialize(_reader: &mut impl Read, _recursion_depth: u32) -> std::io::Result<Self> {
        Ok(())
    }
}

impl Tagged for () {
    fn tag() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("()")
    }
    fn tag_unique_factor() -> String {
        "()".into()
    }
}

impl Serializable for bool {
    fn serialize(&self, writer: &mut impl std::io::Write) -> std::io::Result<()> {
        writer.write_all(&[*self as u8])
    }
    fn serialized_size(&self) -> usize {
        1
    }
}

impl Deserializable for bool {
    fn deserialize(reader: &mut impl Read, _recursion_depth: u32) -> std::io::Result<Self> {
        let mut buf = [0u8];
        reader.read_exact(&mut buf[..])?;
        match buf[0] {
            0 => Ok(false),
            1 => Ok(true),
            v => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("cannot deserialize {v} as bool"),
            )),
        }
    }
}

impl Tagged for bool {
    fn tag() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("bool")
    }
    fn tag_unique_factor() -> String {
        "bool".into()
    }
}

macro_rules! via_le_bytes {
    ($ty:ty, $len:expr_2021) => {
        impl Serializable for $ty {
            fn serialize(&self, writer: &mut impl std::io::Write) -> std::io::Result<()> {
                writer.write_all(&self.to_le_bytes()[..])
            }
            fn serialized_size(&self) -> usize {
                $len
            }
        }

        impl Deserializable for $ty {
            fn deserialize(reader: &mut impl Read, _recursion_depth: u32) -> std::io::Result<Self> {
                let mut buf = [0u8; $len];
                reader.read_exact(&mut buf[..])?;
                Ok(<$ty>::from_le_bytes(buf))
            }
        }

        impl Tagged for $ty {
            fn tag() -> std::borrow::Cow<'static, str> {
                std::borrow::Cow::Borrowed(stringify!($ty))
            }
            fn tag_unique_factor() -> String {
                stringify!($ty).into()
            }
        }
    };
}

macro_rules! via_scale {
    ($ty:ty, $n:expr_2021) => {
        impl Serializable for $ty {
            fn serialize(&self, writer: &mut impl std::io::Write) -> std::io::Result<()> {
                ScaleBigInt::from(*self).serialize(writer)
            }
            fn serialized_size(&self) -> usize {
                ScaleBigInt::from(*self).serialized_size()
            }
        }

        impl Deserializable for $ty {
            fn deserialize(reader: &mut impl Read, recursion_depth: u32) -> std::io::Result<Self> {
                <$ty>::try_from(ScaleBigInt::deserialize(reader, recursion_depth)?)
            }
        }

        impl From<$ty> for ScaleBigInt {
            fn from(val: $ty) -> ScaleBigInt {
                let mut res = ScaleBigInt([0u8; SCALE_MAX_BYTES]);
                let le_bytes = val.to_le_bytes();
                res.0[..$n].copy_from_slice(&le_bytes[..]);
                res
            }
        }

        impl TryFrom<ScaleBigInt> for $ty {
            type Error = std::io::Error;
            fn try_from(val: ScaleBigInt) -> std::io::Result<$ty> {
                if val.0[$n..].iter().any(|b| *b != 0) {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        concat!("out of range for ", stringify!($ty)),
                    ));
                }
                Ok(<$ty>::from_le_bytes(
                    val.0[..$n]
                        .try_into()
                        .expect("slice of known size must coerce to array"),
                ))
            }
        }

        impl Tagged for $ty {
            fn tag() -> std::borrow::Cow<'static, str> {
                std::borrow::Cow::Borrowed(stringify!($ty))
            }
            fn tag_unique_factor() -> String {
                stringify!($ty).into()
            }
        }
    };
}

via_le_bytes!(u8, 1);
via_le_bytes!(u16, 2);
via_le_bytes!(i8, 1);
via_le_bytes!(i16, 2);
via_le_bytes!(i32, 4);
via_le_bytes!(i64, 8);
via_le_bytes!(i128, 16);
via_scale!(u32, 4);
via_scale!(u64, 8);
via_scale!(u128, 16);

const SCALE_MAX_BYTES: usize = 67;
pub struct ScaleBigInt(pub [u8; SCALE_MAX_BYTES]);

impl Default for ScaleBigInt {
    fn default() -> Self {
        ScaleBigInt([0u8; SCALE_MAX_BYTES])
    }
}

const SCALE_ONE_BYTE_MARKER: u8 = 0b00;
const SCALE_TWO_BYTE_MARKER: u8 = 0b01;
const SCALE_FOUR_BYTE_MARKER: u8 = 0b10;
const SCALE_N_BYTE_MARKER: u8 = 0b11;

impl Serializable for ScaleBigInt {
    fn serialize(&self, writer: &mut impl std::io::Write) -> std::io::Result<()> {
        let top2bits = |b| (b & 0b1100_0000) >> 6;
        let bot6bits = |b| (b & 0b0011_1111) << 2;
        match self.serialized_size() {
            1 => writer.write_all(&[bot6bits(self.0[0]) | SCALE_ONE_BYTE_MARKER]),
            2 => {
                let b0 = bot6bits(self.0[0]) | SCALE_TWO_BYTE_MARKER;
                let b1 = top2bits(self.0[0]) | bot6bits(self.0[1]);
                writer.write_all(&[b0, b1])
            }
            4 => {
                let b0 = bot6bits(self.0[0]) | SCALE_FOUR_BYTE_MARKER;
                let b1 = top2bits(self.0[0]) | bot6bits(self.0[1]);
                let b2 = top2bits(self.0[1]) | bot6bits(self.0[2]);
                let b3 = top2bits(self.0[2]) | bot6bits(self.0[3]);
                writer.write_all(&[b0, b1, b2, b3])
            }
            n => {
                writer.write_all(&[(n as u8 - 5) << 2 | SCALE_N_BYTE_MARKER])?;
                writer.write_all(&self.0[..n - 1])
            }
        }
    }
    fn serialized_size(&self) -> usize {
        let trailing_zeros = self.0.iter().rev().take_while(|x| **x == 0).count();
        let occupied = SCALE_MAX_BYTES - trailing_zeros;
        let can_squeeze = self.0[occupied.saturating_sub(1)] < 64;
        match (occupied, can_squeeze) {
            (0, _) | (1, true) => 1,
            (1, false) | (2, true) => 2,
            (2, false) | (3, _) | (4, true) => 4,
            (n, _) => n + 1,
        }
    }
}

impl Deserializable for ScaleBigInt {
    fn deserialize(reader: &mut impl Read, recursion_depth: u32) -> std::io::Result<Self> {
        let first = u8::deserialize(reader, recursion_depth)?;
        let mut res = ScaleBigInt([0u8; SCALE_MAX_BYTES]);
        let top6bits = |b| (b & 0b1111_1100) >> 2;
        let bot2bits = |b| (b & 0b0000_0011) << 6;
        match first & 0b11 {
            SCALE_ONE_BYTE_MARKER => res.0[0] = top6bits(first),
            SCALE_TWO_BYTE_MARKER => {
                let second = u8::deserialize(reader, recursion_depth)?;
                if second == 0 {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "non-canonical scale encoding",
                    ));
                }
                res.0[0] = top6bits(first) | bot2bits(second);
                res.0[1] = top6bits(second);
            }
            SCALE_FOUR_BYTE_MARKER => {
                let second = u8::deserialize(reader, recursion_depth)?;
                let third = u8::deserialize(reader, recursion_depth)?;
                let fourth = u8::deserialize(reader, recursion_depth)?;
                if third == 0 && fourth == 0 {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "non-canonical scale encoding",
                    ));
                }
                res.0[0] = top6bits(first) | bot2bits(second);
                res.0[1] = top6bits(second) | bot2bits(third);
                res.0[2] = top6bits(third) | bot2bits(fourth);
                res.0[3] = top6bits(fourth);
            }
            SCALE_N_BYTE_MARKER => {
                let n = top6bits(first) as usize + 4;
                reader.read_exact(&mut res.0[..n])?;
                if res.0[n - 1] == 0 {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "non-canonical scale encoding",
                    ));
                }
            }
            _ => unreachable!(),
        }
        Ok(res)
    }
}

macro_rules! tuple_serializable {
    (($a:tt, $aidx: tt)$(, ($as:tt, $asidx: tt))*) => {
        impl<$a: Serializable,$($as: Serializable,)*> Serializable for ($a,$($as,)*) {
            fn serialize(
                &self,
                writer: &mut impl std::io::Write,
            ) -> std::io::Result<()> {
                <$a as Serializable>::serialize(&(self.$aidx), writer)?;
                $(<$as as Serializable>::serialize(&(self.$asidx), writer)?;)*
                Ok(())
            }

            fn serialized_size(&self) -> usize {
                <$a as Serializable>::serialized_size(&(self.$aidx)) $(+ <$as as Serializable>::serialized_size(&(self.$asidx)))*
            }
        }

        impl<$a: Deserializable,$($as: Deserializable,)*> Deserializable for ($a,$($as,)*) {
            fn deserialize(reader: &mut impl std::io::Read, mut recursion_depth: u32) -> std::io::Result<Self> {
                <Self as Deserializable>::check_rec(&mut recursion_depth)?;
                Ok((
                <$a as Deserializable>::deserialize(reader, recursion_depth)?,
                $(<$as as Deserializable>::deserialize(reader, recursion_depth)?,)*
                ))
            }
        }

        impl<$a: Tagged,$($as: Tagged,)*> Tagged for ($a, $($as,)*) {
            fn tag() -> std::borrow::Cow<'static, str> {
                let mut res = String::new();
                res.push_str("(");
                res.push_str(&$a::tag());
                $(
                    res.push_str(",");
                    res.push_str(&$as::tag());
                )*
                res.push_str(")");
                std::borrow::Cow::Owned(res)
            }
            fn tag_unique_factor() -> String {
                let mut res = String::new();
                res.push_str("(");
                res.push_str(&$a::tag_unique_factor());
                $(
                    res.push_str(",");
                    res.push_str(&$as::tag_unique_factor());
                )*
                res.push_str(")");
                res
            }
        }
    }
}

tuple_serializable!((A, 0));
tuple_serializable!((A, 0), (B, 1));
tuple_serializable!((A, 0), (B, 1), (C, 2));
tuple_serializable!((A, 0), (B, 1), (C, 2), (D, 3));
tuple_serializable!((A, 0), (B, 1), (C, 2), (D, 3), (E, 4));
tuple_serializable!((A, 0), (B, 1), (C, 2), (D, 3), (E, 4), (F, 5));
tuple_serializable!((A, 0), (B, 1), (C, 2), (D, 3), (E, 4), (F, 5), (G, 6));
tuple_serializable!(
    (A, 0),
    (B, 1),
    (C, 2),
    (D, 3),
    (E, 4),
    (F, 5),
    (G, 6),
    (H, 7)
);
tuple_serializable!(
    (A, 0),
    (B, 1),
    (C, 2),
    (D, 3),
    (E, 4),
    (F, 5),
    (G, 6),
    (H, 7),
    (I, 8)
);
tuple_serializable!(
    (A, 0),
    (B, 1),
    (C, 2),
    (D, 3),
    (E, 4),
    (F, 5),
    (G, 6),
    (H, 7),
    (I, 8),
    (J, 9)
);
tuple_serializable!(
    (A, 0),
    (B, 1),
    (C, 2),
    (D, 3),
    (E, 4),
    (F, 5),
    (G, 6),
    (H, 7),
    (I, 8),
    (J, 9),
    (K, 10)
);
tuple_serializable!(
    (A, 0),
    (B, 1),
    (C, 2),
    (D, 3),
    (E, 4),
    (F, 5),
    (G, 6),
    (H, 7),
    (I, 8),
    (J, 9),
    (K, 10),
    (L, 11)
);
tuple_serializable!(
    (A, 0),
    (B, 1),
    (C, 2),
    (D, 3),
    (E, 4),
    (F, 5),
    (G, 6),
    (H, 7),
    (I, 8),
    (J, 9),
    (K, 10),
    (L, 11),
    (M, 12)
);
tuple_serializable!(
    (A, 0),
    (B, 1),
    (C, 2),
    (D, 3),
    (E, 4),
    (F, 5),
    (G, 6),
    (H, 7),
    (I, 8),
    (J, 9),
    (K, 10),
    (L, 11),
    (M, 12),
    (N, 13)
);
tuple_serializable!(
    (A, 0),
    (B, 1),
    (C, 2),
    (D, 3),
    (E, 4),
    (F, 5),
    (G, 6),
    (H, 7),
    (I, 8),
    (J, 9),
    (K, 10),
    (L, 11),
    (M, 12),
    (N, 13),
    (O, 14)
);
tuple_serializable!(
    (A, 0),
    (B, 1),
    (C, 2),
    (D, 3),
    (E, 4),
    (F, 5),
    (G, 6),
    (H, 7),
    (I, 8),
    (J, 9),
    (K, 10),
    (L, 11),
    (M, 12),
    (N, 13),
    (O, 14),
    (P, 15)
);

impl Deserializable for String {
    fn deserialize(reader: &mut impl Read, recursion_depth: u32) -> std::io::Result<Self> {
        let vec = <Vec<u8>>::deserialize(reader, recursion_depth)?;
        String::from_utf8(vec).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }
}

impl Tagged for String {
    fn tag() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("string")
    }
    fn tag_unique_factor() -> String {
        "string".into()
    }
}

impl Tagged for str {
    fn tag() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("string")
    }
    fn tag_unique_factor() -> String {
        "string".into()
    }
}

pub fn gen_static_serialize_file<T: Serializable + Tagged>(
    value: &T,
) -> Result<(), std::io::Error> {
    let mut file = BufWriter::new(std::fs::File::create(format!("{}.bin", T::tag(),))?);
    crate::tagged_serialize(&value, &mut file)
}

pub fn test_file_deserialize<T: Deserializable + Tagged>(
    path: std::path::PathBuf,
) -> Result<T, std::io::Error> {
    let bytes = std::fs::read(path)?;
    crate::tagged_deserialize(&mut bytes.as_slice())
}

#[cfg(feature = "proptest")]
pub struct NoSearch<T>(T);

#[cfg(feature = "proptest")]
impl<T: Debug + Clone> ValueTree for NoSearch<T> {
    type Value = T;

    fn current(&self) -> T {
        self.0.clone()
    }

    fn simplify(&mut self) -> bool {
        false
    }

    fn complicate(&mut self) -> bool {
        false
    }
}

#[derive(Debug)]
#[cfg(feature = "proptest")]
pub struct NoStrategy<T>(pub PhantomData<T>);

#[cfg(feature = "proptest")]
impl<T: Debug + Clone> Strategy for NoStrategy<T>
where
    Standard: Distribution<T>,
{
    type Tree = NoSearch<T>;
    type Value = T;

    fn new_tree(&self, runner: &mut TestRunner) -> NewTree<Self> {
        Ok(NoSearch(runner.rng().r#gen()))
    }
}

#[macro_export]
macro_rules! tag_enforcement_test {
    ($type:ident) => {
        serialize::tag_enforcement_test!($type < >);
    };
    ($type:ident < $($targ:ty),* >) => {
        #[cfg(test)]
        ::pastey::paste! {
            #[allow(non_snake_case)]
            #[test]
            fn [<tag_enforcement_test_ $type>]() {
                let tag = <$type<$($targ),*> as serialize::Tagged>::tag();
                println!("{tag}");
                let unique_factor = <$type<$($targ),*> as serialize::Tagged>::tag_unique_factor();
                let mut dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
                dir.pop();
                dir.push(".tag-decompositions");
                ::std::fs::create_dir_all(&dir).unwrap();
                let fpath = dir.join(tag.as_ref());
                if ::std::fs::exists(&fpath).unwrap() {
                    let read_factor = ::std::fs::read_to_string(&fpath).unwrap();
                    assert_eq!(read_factor, unique_factor);
                } else {
                    ::std::fs::write(&fpath, unique_factor).unwrap();
                }
            }
        }
    };
}

#[macro_export]
#[cfg(feature = "proptest")]
macro_rules! randomised_tagged_serialization_test {
    ($type:ident) => {
        serialize::randomised_tagged_serialization_test!($type < >);
    };
    ($type:ident < $($targ:ty),* >) => {
        #[cfg(test)]
        ::pastey::paste! {
            #[allow(non_snake_case)]
            #[test]
            fn [<proptest_deserialize_tagged_ $type>]() where $type<$($targ),*>: proptest::prelude::Arbitrary {
                let mut runner = proptest::test_runner::TestRunner::default();

                runner.run(&<$type<$($targ),*> as proptest::prelude::Arbitrary>::arbitrary(), |v| {
                    let mut bytes: Vec<u8> = Vec::new();
                    serialize::tagged_serialize(&v, &mut bytes).unwrap();
                    let des_result: $type<$($targ),*> = serialize::tagged_deserialize(&mut bytes.as_slice()).unwrap();
                    assert_eq!(des_result, v);

                    Ok(())
                }).unwrap();
            }
            serialize::randomised_serialization_test!($type<$($targ),*>);
        }
    }
}

#[macro_export]
#[cfg(feature = "proptest")]
macro_rules! randomised_serialization_test {
    ($type:ident) => {
        serialize::randomised_serialization_test!($type < >);
    };
    ($type:ident < $($targ:ty),* >) => {
        #[cfg(test)]
        ::pastey::paste! {
            #[allow(non_snake_case)]
            #[test]
            fn [<proptest_deserialize_ $type>]() where $type<$($targ),*>: proptest::prelude::Arbitrary {
                let mut runner = proptest::test_runner::TestRunner::default();

                runner.run(&<$type<$($targ),*> as proptest::prelude::Arbitrary>::arbitrary(), |v| {
                    let mut bytes: Vec<u8> = Vec::new();
                    <$type<$($targ),*> as serialize::Serializable>::serialize(&v, &mut bytes).unwrap();
                    let des_result: $type<$($targ),*> = <$type<$($targ),*> as serialize::Deserializable>::deserialize(&mut bytes.as_slice(), 0).unwrap();
                    assert_eq!(des_result, v);

                    Ok(())
                }).unwrap();
            }

            #[allow(non_snake_case)]
            #[test]
            fn [<proptest_serialized_size_ $type>]() where $type<$($targ),*>: proptest::prelude::Arbitrary {
                let mut runner = proptest::test_runner::TestRunner::default();

                runner.run(&<$type<$($targ),*> as proptest::prelude::Arbitrary>::arbitrary(), |v| {
                    let mut bytes: Vec<u8> = Vec::new();
                    <$type<$($targ),*> as serialize::Serializable>::serialize(&v, &mut bytes).unwrap();
                    assert_eq!(bytes.len(), <$type<$($targ),*> as serialize::Serializable>::serialized_size(&v));

                    Ok(())
                }).unwrap();
            }

            #[allow(non_snake_case)]
            #[test]
            fn [<proptest_random_data_deserialize_ $type>]() {
                use rand::Rng;
                let mut rng = rand::thread_rng();

                for _ in 0..100 {
                    let size: u8 = rng.r#gen();
                    let mut bytes: Vec<u8> = Vec::new();
                    for _i in 0..size {
                        bytes.push(rng.r#gen())
                    }
                    let _ = <$type<$($targ),*> as serialize::Deserializable>::deserialize(&mut bytes.as_slice(), 0);
                }
            }
        }
    };
}

/// Produce a single arbitrary value without the ability to simplify or complicate
#[macro_export]
#[cfg(feature = "proptest")]
macro_rules! simple_arbitrary {
    ($type:ty) => {
        impl Arbitrary for $type {
            type Parameters = ();
            type Strategy = NoStrategy<$type>;

            fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
                NoStrategy(PhantomData)
            }
        }
    };
}

#[cfg(feature = "proptest")]
#[allow(unused)]
use crate as serialize;

#[cfg(feature = "proptest")]
randomised_serialization_test!(String);

#[cfg(test)]
mod scale_compat {
    //! **The gate on any change to how integers serialise.**
    //!
    //! `ScaleBigInt` is the reference: the general path that every `via_scale!` type went
    //! through when this was written. Anything faster has to agree with it *byte for byte*,
    //! and this compares them directly rather than against a golden file, so the oracle
    //! cannot drift out of date.
    //!
    //! ⚠︎ Byte-identical is not a nicety here. Storage is content-addressed — a node's key
    //! *is* the hash of its serialisation — so a changed encoding changes every node hash,
    //! every parent that references it, and the state root. That is a migration of the whole
    //! ᴅᴀɢ, not a version bump.
    use super::*;

    /// Every value where the encoding changes width, either side of each boundary, plus the
    /// type extremes. Boundaries are where an encoder is wrong if it is wrong at all.
    fn corpus_u128() -> Vec<u128> {
        let mut v = vec![0u128, 1, 63, 64, 65, 255, 256, 16_383, 16_384, 16_385];
        for k in 0..128u32 {
            let p = 1u128 << k;
            v.extend([p.wrapping_sub(1), p, p.wrapping_add(1)]);
        }
        v.push(u128::MAX);
        v.push(u128::from(u64::MAX));
        v.push(u128::from(u32::MAX));
        // A deterministic spread, so the corpus is not only powers of two.
        let mut x = 0x243f_6a88_85a3_08d3u128;
        for _ in 0..512 {
            x = x
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            v.push(x);
        }
        v.sort_unstable();
        v.dedup();
        v
    }

    fn bytes_of(v: &impl Serializable) -> Vec<u8> {
        let mut b = Vec::new();
        v.serialize(&mut b).expect("write to a Vec cannot fail");
        b
    }

    /// `ScaleBigInt` and the integer impls must produce the same octets and agree on the
    /// size they claim — the second is what the *encoder* consults, so a disagreement there
    /// is a wrong length prefix somewhere else in the stream.
    #[test]
    fn integers_encode_exactly_as_the_big_int_path_does() {
        for x in corpus_u128() {
            let reference = bytes_of(&ScaleBigInt::from(x));
            assert_eq!(bytes_of(&x), reference, "u128 {x} encodes differently");
            assert_eq!(x.serialized_size(), reference.len(), "u128 {x} size");

            if let Ok(y) = u64::try_from(x) {
                assert_eq!(bytes_of(&y), reference, "u64 {y} encodes differently");
                assert_eq!(y.serialized_size(), reference.len(), "u64 {y} size");
            }
            if let Ok(y) = u32::try_from(x) {
                assert_eq!(bytes_of(&y), reference, "u32 {y} encodes differently");
                assert_eq!(y.serialized_size(), reference.len(), "u32 {y} size");
            }
        }
    }

    /// Round-trip, and the *reader position* afterwards: a decoder that consumes the wrong
    /// number of octets round-trips perfectly on its own and corrupts everything after it.
    #[test]
    fn integers_round_trip_and_consume_exactly_their_encoding() {
        for x in corpus_u128() {
            let b = bytes_of(&x);
            let mut r = &b[..];
            assert_eq!(u128::deserialize(&mut r, 0).expect("u128"), x);
            assert!(r.is_empty(), "u128 {x} left {} octet(s)", r.len());

            if let Ok(y) = u64::try_from(x) {
                let b = bytes_of(&y);
                let mut r = &b[..];
                assert_eq!(u64::deserialize(&mut r, 0).expect("u64"), y);
                assert!(r.is_empty(), "u64 {y} left {} octet(s)", r.len());
            }
            if let Ok(y) = u32::try_from(x) {
                let b = bytes_of(&y);
                let mut r = &b[..];
                assert_eq!(u32::deserialize(&mut r, 0).expect("u32"), y);
                assert!(r.is_empty(), "u32 {y} left {} octet(s)", r.len());
            }
        }
    }

    /// A value too wide for the target type is an error, not a truncation. Consensus code
    /// reading a `u32` field must not silently accept a `u64`'s worth of octets.
    #[test]
    fn a_value_too_wide_for_its_type_is_refused() {
        for (x, fits_u32, fits_u64) in [
            (u128::from(u32::MAX), true, true),
            (u128::from(u32::MAX) + 1, false, true),
            (u128::from(u64::MAX), false, true),
            (u128::from(u64::MAX) + 1, false, false),
            (u128::MAX, false, false),
        ] {
            let b = bytes_of(&x);
            assert_eq!(
                u32::deserialize(&mut &b[..], 0).is_ok(),
                fits_u32,
                "u32 {x}"
            );
            assert_eq!(
                u64::deserialize(&mut &b[..], 0).is_ok(),
                fits_u64,
                "u64 {x}"
            );
        }
    }

    /// The canonicity rejections are consensus rules: two encodings of one value would be
    /// two hashes for one node. Each case below is a value encoded one octet too wide.
    #[test]
    fn non_canonical_encodings_are_refused() {
        // 1 encoded in the two-octet form: (1 << 2) | 0b01, high octet zero.
        assert!(
            u64::deserialize(&mut &[0b0000_0101u8, 0][..], 0).is_err(),
            "two-octet"
        );
        // 1 in the four-octet form: (1 << 2) | 0b10, top two octets zero.
        assert!(
            u64::deserialize(&mut &[0b0000_0110u8, 0, 0, 0][..], 0).is_err(),
            "four-octet"
        );
        // 1 in the n-octet form: marker (4-4) << 2 | 0b11, four octets, the last zero.
        assert!(
            u64::deserialize(&mut &[0b0000_0011u8, 1, 0, 0, 0][..], 0).is_err(),
            "n-octet"
        );
        // And the canonical forms of the same value are accepted.
        assert_eq!(
            u64::deserialize(&mut &[0b0000_0100u8][..], 0).expect("canonical"),
            1
        );
    }
}
