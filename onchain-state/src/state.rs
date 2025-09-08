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

use base_crypto::cost_model::RunningCost;
use base_crypto::fab::{Aligned, AlignedValue, Alignment, AlignmentAtom};
use base_crypto::hash::{HashOutput, persistent_commit};
use base_crypto::repr::MemWrite;
use base_crypto::signatures::VerifyingKey;
use coin_structure::coin::TokenType;
use derive_where::derive_where;
use fake::Dummy;
use hex::ToHex;
#[cfg(feature = "proptest")]
use proptest::arbitrary::Arbitrary;
#[cfg(feature = "proptest")]
use proptest_derive::Arbitrary;
use rand::Rng;
use rand::distributions::{Distribution, Standard};
use serde::{
    Deserialize, Deserializer, Serialize, Serializer, de, de::MapAccess, de::SeqAccess,
    de::Visitor, ser::SerializeStruct,
};
#[cfg(feature = "proptest")]
use serialize::NoStrategy;
#[cfg(feature = "proptest")]
use serialize::randomised_serialization_test;
#[cfg(feature = "proptest")]
use serialize::simple_arbitrary;
use serialize::{self, Deserializable, Serializable, Tagged, tag_enforcement_test};
use std::borrow::Borrow;
use std::fmt::{self, Debug, Formatter};
use std::hash::Hash;
use std::io::{self, Read, Write};
use std::marker::PhantomData;
use std::ops::Deref;
use storage::Storable;
use storage::arena::Sp;
use storage::db::{DB, InMemoryDB};
use storage::delta_tracking::{incremental_write_delete_costs, initial_write_delete_costs};
use storage::{
    arena::ArenaKey,
    delta_tracking::RcMap,
    storable::Loader,
    storage::{Array, HashMap},
};
use transient_crypto::curve::Fr;
use transient_crypto::merkle_tree::MerkleTree;
use transient_crypto::proofs::VerifierKey;
use transient_crypto::repr::FieldRepr;

#[cfg(feature = "proptest")]
fn proptest_valid<D: DB>(value: &StateValue<D>) -> bool {
    match value {
        StateValue::Array(arr) => arr.len() <= 16,
        _ => true,
    }
}

/// The size limit for Cell's. Currently 32 kiB
pub const CELL_BOUND: usize = 1 << 15;

#[derive(Default, Storable)]
#[derive_where(Clone, PartialEq, Eq, Ord, PartialOrd, Hash)]
#[cfg_attr(feature = "proptest", derive(Arbitrary))]
#[cfg_attr(feature = "proptest", proptest(filter = "proptest_valid"))]
#[storable(db = D, invariant = StateValue::invariant)]
#[tag = "impact-state-value[v2]"]
#[non_exhaustive]
pub enum StateValue<D: DB = InMemoryDB> {
    #[default]
    Null,
    Cell(#[storable(child)] Sp<AlignedValue, D>),
    Map(HashMap<AlignedValue, StateValue<D>, D>),
    /// A fixed size array, with `0 <= len <= 16`. The upper 5 bits of the
    /// argument to the `new` opcode specify the length at creation time. The
    /// underlying `storage::Array` type is not fixed length, but in the VM we
    /// only allow size preserving operations.
    Array(Array<StateValue<D>, D>),
    /// Merkle tree with `0 < height <= 32`.
    BoundedMerkleTree(
        // The `Serializable::unversioned_serialize` impl requires this.
        #[cfg_attr(
            feature = "proptest",
            proptest(filter = "|mt| !(mt.height() == 0 || mt.height() > 32)")
        )]
        MerkleTree<(), D>,
    ),
}
tag_enforcement_test!(StateValue);

impl<D: DB> From<u64> for StateValue<D> {
    fn from(value: u64) -> Self {
        StateValue::Cell(Sp::new(value.into()))
    }
}

// We need to manually implement `Drop` to avoid implicit unbounded recursion, which could lead to
// stack overflows. See https://rust-unofficial.github.io/too-many-lists/first-drop.html.
impl<D: DB> Drop for StateValue<D> {
    fn drop(&mut self) {
        // Early return for non-recursive types. This ensures that we have a base-case for Drop,
        // as we'll end up recursing at least once otherwise, because we keep a queue of state
        // values otherwise.
        match self {
            StateValue::Null | StateValue::Cell(_) | StateValue::BoundedMerkleTree(_) => return,
            StateValue::Map(m) if m.size() == 0 => return,
            StateValue::Array(a) if a.is_empty() => return,
            _ => {}
        }
        // This allows us to escape from the &mut to a owned reference
        // Note that this relies on the `Default` of `Null` falling into our base case.
        let mut frontier = vec![std::mem::take(self)];
        while let Some(mut curr) = frontier.pop() {
            match &mut curr {
                StateValue::Map(m) => {
                    let mut tmp = HashMap::new();
                    std::mem::swap(m, &mut tmp);
                    frontier.extend(tmp.into_inner_for_drop().flat_map(|(_, v)| v.into_iter()));
                }
                StateValue::Array(a) => {
                    let mut tmp = Array::new();
                    std::mem::swap(a, &mut tmp);
                    frontier.extend(tmp.into_inner_for_drop());
                }
                _ => {}
            }
            // It is now safe to drop curr, as this has an empty map/array in it.
            drop(curr);
        }
    }
}

impl<D: DB> Distribution<StateValue<D>> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> StateValue<D> {
        let disc = rng.gen_range(0..40);
        match disc {
            20..=36 => StateValue::Cell(Sp::new(rng.r#gen())),
            37..=38 => {
                let mut mt: MerkleTree<(), D> = rng.r#gen();
                // The `Serializable::unversioned_serialize` impl requires this.
                while mt.height() == 0 || mt.height() > 32 {
                    mt = rng.r#gen();
                }
                StateValue::BoundedMerkleTree(mt)
            }
            39 => StateValue::Map(rng.r#gen()),
            40 => StateValue::Array(rng.r#gen()),
            _ => StateValue::Null,
        }
    }
}

impl<D: DB> FieldRepr for StateValue<D> {
    fn field_repr<W: MemWrite<Fr>>(&self, writer: &mut W) {
        use StateValue::*;
        match self {
            Null => writer.write(&[0.into()]),
            Cell(v) => {
                writer.write(&[1.into()]);
                v.field_repr(writer);
            }
            Map(m) => {
                writer.write(&[(2u128 | ((m.size() as u128) << 4)).into()]);
                let mut sorted = m.iter().collect::<Vec<_>>();
                sorted.sort();
                for kv in sorted.into_iter() {
                    kv.0.field_repr(writer);
                    kv.1.field_repr(writer);
                }
            }
            Array(arr) => {
                writer.write(&[(3u64 | ((arr.len() as u64) << 4)).into()]);
                for elem in arr.iter() {
                    elem.field_repr(writer);
                }
            }
            BoundedMerkleTree(t) => {
                let entries = t.iter().collect::<Vec<_>>();
                writer.write(&[(4u128
                    | ((t.height() as u128) << 4)
                    | ((entries.len() as u128) << 12))
                    .into()]);
                for entry in entries.into_iter() {
                    entry.field_repr(writer);
                }
            }
        }
    }

    fn field_size(&self) -> usize {
        use StateValue::*;
        match self {
            Null => 1,
            Cell(v) => 1 + v.field_size(),
            Map(m) => {
                1 + m
                    .iter()
                    .map(|kv| kv.0.field_size() + kv.1.field_size())
                    .sum::<usize>()
            }
            Array(arr) => 1 + arr.iter().map(|s| s.field_size()).sum::<usize>(),
            BoundedMerkleTree(t) => 1 + t.iter().map(|(_, v)| 1 + v.field_size()).sum::<usize>(),
        }
    }
}

impl<D: DB> Serialize for StateValue<D> {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        match self {
            StateValue::Null => {
                let mut ser = ser.serialize_struct("StateValue", 1)?;
                ser.serialize_field("tag", "null")?;
                ser.end()
            }
            StateValue::Cell(val) => {
                let mut ser = ser.serialize_struct("StateValue", 2)?;
                ser.serialize_field("tag", "cell")?;
                ser.serialize_field("content", &**val)?;
                ser.end()
            }
            StateValue::Map(val) => {
                let mut ser = ser.serialize_struct("StateValue", 2)?;
                ser.serialize_field("tag", "map")?;
                ser.serialize_field("content", val)?;
                ser.end()
            }
            StateValue::Array(val) => {
                let mut ser = ser.serialize_struct("StateValue", 2)?;
                ser.serialize_field("tag", "array")?;
                ser.serialize_field("content", val)?;
                ser.end()
            }
            StateValue::BoundedMerkleTree(val) => {
                let mut ser = ser.serialize_struct("StateValue", 2)?;
                ser.serialize_field("tag", "boundedMerkleTree")?;
                ser.serialize_field("content", val)?;
                ser.end()
            }
        }
    }
}

struct StateValueVisitor<D: DB>(PhantomData<D>);

impl<'de, D: DB> Visitor<'de> for StateValueVisitor<D> {
    type Value = StateValue<D>;
    fn expecting(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(formatter, "a state value")
    }

    fn visit_seq<V: SeqAccess<'de>>(self, mut seq: V) -> Result<StateValue<D>, V::Error> {
        let tag: String = seq
            .next_element()?
            .ok_or_else(|| de::Error::invalid_length(0, &self))?;
        match &tag[..] {
            "null" => Ok(StateValue::Null),
            "cell" => Ok(StateValue::Cell(Sp::new(
                seq.next_element()?
                    .ok_or_else(|| de::Error::invalid_length(1, &self))?,
            ))),
            "map" => Ok(StateValue::Map(
                seq.next_element()?
                    .ok_or_else(|| de::Error::invalid_length(1, &self))?,
            )),
            "array" => Ok(StateValue::Array(
                seq.next_element()?
                    .ok_or_else(|| de::Error::invalid_length(1, &self))?,
            )),
            "boundedMerkleTree" => Ok(StateValue::BoundedMerkleTree(
                seq.next_element()?
                    .ok_or_else(|| de::Error::invalid_length(1, &self))?,
            )),
            tag => Err(de::Error::unknown_variant(
                tag,
                &["null", "cell", "map", "array", "boundedMerkleTree"],
            )),
        }
    }

    fn visit_map<V: MapAccess<'de>>(self, mut map: V) -> Result<StateValue<D>, V::Error> {
        let first_key: String = map
            .next_key()?
            .ok_or_else(|| de::Error::missing_field("tag"))?;
        match &first_key[..] {
            "tag" => {
                let tag: String = map.next_value()?;
                fn get_content<'de2, V: MapAccess<'de2>, T: Deserialize<'de2>>(
                    map: &mut V,
                ) -> Result<T, V::Error> {
                    let entry: (String, T) = map
                        .next_entry()?
                        .ok_or_else(|| de::Error::missing_field("content"))?;
                    if &entry.0[..] == "content" {
                        Ok(entry.1)
                    } else {
                        Err(de::Error::unknown_field(&entry.0[..], &["tag", "content"]))
                    }
                }
                match &tag[..] {
                    "null" => Ok(StateValue::Null),
                    "cell" => Ok(StateValue::Cell(Sp::new(get_content(&mut map)?))),
                    "map" => Ok(StateValue::Map(get_content(&mut map)?)),
                    "array" => Ok(StateValue::Array(get_content(&mut map)?)),
                    "boundedMerkleTree" => {
                        Ok(StateValue::BoundedMerkleTree(get_content(&mut map)?))
                    }
                    tag => Err(de::Error::unknown_variant(
                        tag,
                        &["null", "cell", "map", "array", "boundedMerkleTree"],
                    )),
                }
            }
            "content" => Err(de::Error::custom(
                "limitation of current deserialization: StateValue tag must preceed contents",
            )),
            field => Err(de::Error::unknown_field(field, &["tag", "content"])),
        }
    }
}

impl<'de, D1: DB> Deserialize<'de> for StateValue<D1> {
    fn deserialize<D: Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        de.deserialize_struct(
            "StateValue",
            &["tag", "content"],
            StateValueVisitor(PhantomData),
        )
    }
}

#[macro_export]
macro_rules! stval {
    (null) => {
        StateValue::Null
    };
    (($val:expr_2021)) => {
        StateValue::Cell(Sp::new($val.into()))
    };
    ({MT($height:expr_2021) {$($key:expr_2021 => $val:expr_2021),*}}) => {
        StateValue::BoundedMerkleTree(MerkleTree::blank($height)$(.update_hash($key, $val, ()))*.rehash())
    };
    ({$($key:expr_2021 => $val:tt),*}) => {
        StateValue::Map(HashMap::new()$(.insert($key.into(), stval!($val)))*)
    };
    ({$key:expr_2021 => $val:tt}; $n:expr_2021) => {
        {
            StateValue::Map((0..$n).into_iter().map(|x|{
                (AlignedValue::from($key + x as u32), stval!($val))
            }).collect())
        }
    };
    ([$($val:tt),*]) => {
        StateValue::Array(vec![$(stval!($val)),*].into())
    };
    ([$elem:tt; $n:expr_2021]) => {
        StateValue::Array(vec![stval!($elem); $n].into())
    };
}

pub use stval;

impl<D: DB> Debug for StateValue<D> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use StateValue::*;
        match self {
            Null => write!(formatter, "null"),
            Cell(v) => write!(formatter, "{v:?}"),
            Map(m) => {
                write!(formatter, "Map ")?;
                formatter
                    .debug_map()
                    .entries(m.iter().map(|kv| (kv.0.clone(), kv.1.clone())))
                    .finish()
            }
            Array(arr) => {
                write!(formatter, "Array({}) ", arr.len())?;
                formatter.debug_list().entries(arr.iter()).finish()
            }
            BoundedMerkleTree(t) => {
                write!(formatter, "MerkleTree({}) ", t.height())?;
                formatter.debug_map().entries(t.iter()).finish()
            }
        }
    }
}

impl<D: DB> StateValue<D> {
    fn invariant(&self) -> std::io::Result<()> {
        match self {
            StateValue::Null | StateValue::Map(_) => {}
            StateValue::Cell(v) => {
                if (**v).serialized_size() > CELL_BOUND {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("Cell exceeded maximum bound of {CELL_BOUND}"),
                    ));
                }
            }
            StateValue::Array(arr) => {
                if arr.len() > 16 {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "Array eceeded maximum length of 16",
                    ));
                }
            }
            StateValue::BoundedMerkleTree(bmt) => {
                if bmt.height() > 32 {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "BMT eceeded maximum height of 32",
                    ));
                }
                if bmt.root().is_none() {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "BMT must be rehashed",
                    ));
                }
            }
        }
        Ok(())
    }

    pub fn log_size(&self) -> usize {
        use StateValue::*;
        match self {
            Null => 0,
            Cell(a) => {
                // TODO: this is O(n), but probably needs to be O(1).
                //
                // Possible fixes: cache the size of the AlignedValue in the
                // constructor. Not sure if this "size" necessarily needs to be the serialized
                // size.
                <AlignedValue as Serializable>::serialized_size(&**a)
                    .next_power_of_two()
                    .ilog2() as usize
            }
            Map(m) => (m.size() as u128).next_power_of_two().ilog2() as usize,
            Array(a) => (a.len() as u128).next_power_of_two().ilog2() as usize,
            BoundedMerkleTree(t) => t.height() as usize,
        }
    }
}

impl<D: DB> From<AlignedValue> for StateValue<D> {
    fn from(val: AlignedValue) -> StateValue<D> {
        StateValue::Cell(Sp::new(val))
    }
}

pub fn write_int<W: Write>(writer: &mut W, int: u64) -> io::Result<()> {
    match int {
        0..=0x7F => writer.write_all(&[int as u8][..]),
        0x80..=0x3FFF => writer.write_all(&[0x80 | (int % 0x80) as u8, (int >> 7) as u8][..]),
        0x4000..=0x1FFFFF => writer.write_all(
            &[
                0x80 | (int % 0x80) as u8,
                0x80 | ((int >> 7) % 0x80) as u8,
                (int >> 14) as u8,
            ][..],
        ),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "too many entries to serialize state value length!",
        )),
    }
}

pub fn int_size(int: u64) -> usize {
    match int {
        0..=0x7F => 1,
        0x80..=0x3FFF => 2,
        0x4000..=0x1FFFFF => 3,
        _ => 4,
    }
}

pub fn read_int<R: Read>(reader: &mut R) -> io::Result<u64> {
    let mut buf = [0u8; 3];
    reader.read_exact(&mut buf[0..1])?;
    if (buf[0] & 0x80) == 0 {
        return Ok(buf[0] as u64);
    }
    reader.read_exact(&mut buf[1..2])?;
    if (buf[1] & 0x80) == 0 {
        return Ok((buf[0] & 0x7f) as u64 | ((buf[1] as u64) << 7));
    }
    reader.read_exact(&mut buf[2..3])?;
    if (buf[2] & 0x80) == 0 {
        Ok((buf[0] & 0x7f) as u64 | (((buf[1] & 0x7f) as u64) << 7) | ((buf[2] as u64) << 14))
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "reserved range for deserializing state value length",
        ))
    }
}

enum MaybeStr<'a> {
    Str(&'a str),
    Bytes(&'a [u8]),
}

struct MaybeStrVisitor<T>(PhantomData<T>);

impl<'de, T: From<Vec<u8>>> serde::de::Visitor<'de> for MaybeStrVisitor<T> {
    type Value = T;
    fn expecting(&self, formatter: &mut Formatter) -> fmt::Result {
        formatter.write_str("[byte]string")
    }

    fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Self::Value, E> {
        self.visit_string(v.to_owned())
    }

    fn visit_string<E: serde::de::Error>(self, v: String) -> Result<Self::Value, E> {
        Ok(v.into_bytes().into())
    }

    fn visit_bytes<E: serde::de::Error>(self, v: &[u8]) -> Result<Self::Value, E> {
        self.visit_byte_buf(v.to_vec())
    }

    fn visit_byte_buf<E: serde::de::Error>(self, v: Vec<u8>) -> Result<Self::Value, E> {
        Ok(v.into())
    }
    // Required for serde_json compatibility. See
    // https://github.com/serde-rs/json/pull/557
    fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
        let mut res = Vec::new();
        while let Some(byte) = seq.next_element()? {
            res.push(byte);
        }
        Ok(res.into())
    }
}

fn maybe_str(buf: &[u8]) -> MaybeStr<'_> {
    // For alphanumeric characters, as well as the following: '+-_":/\?#$%^*&.
    // we will use a string as-is. For others, we will use byte enocding.
    // This is to permit arbitrary bytes, while presenting strings to users
    // where sensible.
    fn permitted(c: u8) -> bool {
        c.is_ascii_alphanumeric() || b"'+-_\":/\\?#$^*&.".contains(&c)
    }
    if buf.iter().copied().all(permitted) {
        if let Ok(s) = std::str::from_utf8(buf) {
            return MaybeStr::Str(s);
        }
    }
    MaybeStr::Bytes(buf)
}

macro_rules! idty {
    ($refty:ident, $bufty:ident) => {
        pub type $refty<'a> = &'a [u8];

        #[derive(
            FieldRepr, Clone, Hash, PartialEq, Eq, PartialOrd, Ord, Serializable, Dummy, Storable,
        )]
        #[storable(base)]
        #[cfg_attr(feature = "proptest", derive(Arbitrary))]
        pub struct $bufty(pub Vec<u8>);

        impl Serialize for $bufty {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                match maybe_str(&self.0) {
                    MaybeStr::Str(s) => serializer.serialize_str(s),
                    MaybeStr::Bytes(b) => serializer.serialize_bytes(b),
                }
            }
        }

        impl From<Vec<u8>> for $bufty {
            fn from(vec: Vec<u8>) -> $bufty {
                $bufty(vec)
            }
        }

        impl<'de> Deserialize<'de> for $bufty {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                deserializer.deserialize_any(MaybeStrVisitor(PhantomData))
            }
        }

        impl Debug for $bufty {
            fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
                match maybe_str(&self.0) {
                    MaybeStr::Str(s) => formatter.write_str(s),
                    MaybeStr::Bytes(b) => formatter.write_str(&b.encode_hex::<String>()),
                }
            }
        }

        impl Deref for $bufty {
            type Target = [u8];
            fn deref(&self) -> &[u8] {
                &self.0
            }
        }

        impl Borrow<[u8]> for $bufty {
            fn borrow(&self) -> &[u8] {
                &self.0
            }
        }

        impl From<&[u8]> for $bufty {
            fn from(e: &[u8]) -> $bufty {
                $bufty(e.to_owned())
            }
        }
    };
}

idty!(EntryPoint, EntryPointBuf);
#[cfg(feature = "proptest")]
randomised_serialization_test!(EntryPointBuf);

impl Distribution<EntryPointBuf> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> EntryPointBuf {
        let length = rng.gen_range(0..10);
        EntryPointBuf(
            vec![0; length]
                .iter()
                .map(|_| rng.r#gen::<u8>())
                .collect::<Vec<u8>>()
                .to_owned(),
        )
    }
}

impl Tagged for EntryPointBuf {
    fn tag() -> std::borrow::Cow<'static, str> {
        "entry-point".into()
    }
    fn tag_unique_factor() -> String {
        "vec(u8)".into()
    }
}
tag_enforcement_test!(EntryPointBuf);

impl EntryPointBuf {
    pub fn ep_hash(&self) -> HashOutput {
        persistent_commit(
            &self[..],
            HashOutput(*b"midnight:entry-point\0\0\0\0\0\0\0\0\0\0\0\0"),
        )
    }
}

impl Aligned for EntryPointBuf {
    fn alignment() -> Alignment {
        Alignment::singleton(AlignmentAtom::Compress)
    }
}

#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serializable,
    Storable,
    Serialize,
    Deserialize,
)]
#[storable(base)]
#[tag = "contract-maintenance-authority[v1]"]
#[cfg_attr(feature = "proptest", derive(Arbitrary))]
pub struct ContractMaintenanceAuthority {
    pub committee: Vec<VerifyingKey>,
    pub threshold: u32,
    pub counter: u32,
}
tag_enforcement_test!(ContractMaintenanceAuthority);

impl ContractMaintenanceAuthority {
    pub fn new() -> Self {
        ContractMaintenanceAuthority {
            committee: vec![],
            threshold: 1,
            counter: 0,
        }
    }
}

impl Default for ContractMaintenanceAuthority {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Storable, Serialize, Deserialize)]
#[derive_where(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[storable(db = D)]
#[cfg_attr(feature = "proptest", derive(Arbitrary))]
#[serde(bound(serialize = "", deserialize = ""))]
#[serde(rename_all = "camelCase")]
#[tag = "contract-state[v4]"]
pub struct ContractState<D: DB> {
    pub data: ChargedState<D>,
    pub operations: HashMap<EntryPointBuf, ContractOperation, D>,
    pub maintenance_authority: ContractMaintenanceAuthority,
    pub balance: HashMap<TokenType, u128, D>,
}
tag_enforcement_test!(ContractState<InMemoryDB>);

#[derive(Storable, Serialize, Deserialize)]
#[derive_where(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[storable(db = D)]
#[cfg_attr(feature = "proptest", derive(Arbitrary))]
#[serde(bound(serialize = "", deserialize = ""))]
#[serde(rename_all = "camelCase")]
#[tag = "charged-state[v1]"]
pub struct ChargedState<D: DB> {
    pub(crate) state: Sp<StateValue<D>, D>,
    // TODO: it would be better to generate charged keys from `data`, since it's
    // an invariant that the chargable contract state is always a subset of the
    // `charged_keys`. I assume this implies a manual `Arbitrary`
    // implementation, but maybe this is some `proptest` magic that supports
    // deriving this ...
    #[cfg_attr(feature = "proptest", proptest(value = "RcMap::default()"))]
    pub(crate) charged_keys: RcMap<D>,
}
tag_enforcement_test!(ChargedState<InMemoryDB>);

impl<D: DB> Debug for ChargedState<D> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        self.state.fmt(f)
    }
}

impl<D: DB> ChargedState<D> {
    /// Creates a new charged state from a given state value. This assumes that
    /// this state's storage is paid for elsewhere, and therefore the resulting
    /// `ChargedState` *is* accounted for in its storage usage.
    ///
    /// Specifically, for contract deployments, the happens with a manual
    /// `tree_copy` costing of `ContractDeploy` operations.
    pub fn new(state: StateValue<D>) -> Self {
        let state = Sp::new(state);
        let charged_keys =
            initial_write_delete_costs(&[state.hash().into()].into_iter().collect(), |_, _| {
                Default::default()
            })
            .updated_charged_keys;
        ChargedState {
            state,
            charged_keys,
        }
    }

    pub fn get(&self) -> Sp<StateValue<D>, D> {
        self.state.clone()
    }

    pub fn get_ref(&self) -> &StateValue<D> {
        &*self.state
    }

    pub fn update(
        &self,
        new_state: StateValue<D>,
        cpu_cost: impl Fn(u64, u64) -> RunningCost,
        gc_limit: impl FnOnce(RunningCost) -> usize,
    ) -> (Self, RunningCost) {
        // WARNING: Need to be sure the old and new StateValue state is in the
        // backend before doing calcs over their keys. The old state is already
        // in the backend, because contract states get persisted after contract
        // calls. But the new state we're working with now has not been
        // persisted yet, indeed, it may never be, e.g. if we run out of gas
        // when we cost its writes+deletes.
        //
        // This sp creation here should be cheap, since we quickly run into sps
        // under the covers. However, the top level of the StateValue state is
        // *not* an sp. Another solution would be to require the top-level of
        // the StateValue state itself be an sp, e.g. by wrapping all fields of
        // the ContractState in sps.
        let new_state = Sp::new(new_state);
        let results = incremental_write_delete_costs(
            &self.charged_keys,
            &[new_state.hash().into()].into_iter().collect(),
            cpu_cost,
            gc_limit,
        );
        let cost = results.running_cost();
        let state = ChargedState {
            state: new_state,
            charged_keys: results.updated_charged_keys,
        };
        (state, cost)
    }
}

impl<D: DB> ContractState<D> {
    pub fn new(
        data: StateValue<D>,
        operations: HashMap<EntryPointBuf, ContractOperation, D>,
        maintenance_authority: ContractMaintenanceAuthority,
    ) -> Self {
        ContractState {
            data: ChargedState::new(data),
            operations,
            maintenance_authority,
            balance: HashMap::default(),
        }
    }
}

impl<D: DB> Debug for ContractState<D> {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(formatter, "ContractState (")?;
        self.data.state.fmt(formatter)?;
        self.operations.fmt(formatter)?;
        write!(formatter, "ContractState )")?;
        Ok(())
    }
}

impl<D: DB> Default for ContractState<D> {
    fn default() -> Self {
        Self::new(
            StateValue::Null,
            HashMap::new(),
            ContractMaintenanceAuthority::default(),
        )
    }
}

#[derive(
    Serializable, Clone, Hash, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Storable,
)]
#[storable(base)]
#[tag = "contract-operation[v2]"]
#[non_exhaustive]
pub struct ContractOperation {
    pub v2: Option<VerifierKey>,
}
tag_enforcement_test!(ContractOperation);

impl ContractOperation {
    pub fn new(vk: Option<VerifierKey>) -> Self {
        ContractOperation { v2: vk }
    }

    pub fn latest(&self) -> Option<&VerifierKey> {
        self.v2.as_ref()
    }

    pub fn latest_mut(&mut self) -> &mut Option<VerifierKey> {
        &mut self.v2
    }
}

#[cfg(feature = "proptest")]
simple_arbitrary!(ContractOperation);
#[cfg(feature = "proptest")]
randomised_serialization_test!(ContractOperation);

impl Distribution<ContractOperation> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> ContractOperation {
        let some: bool = rng.r#gen();
        if some {
            ContractOperation {
                v2: Some(rng.r#gen()),
            }
        } else {
            ContractOperation { v2: None }
        }
    }
}

impl FieldRepr for ContractOperation {
    fn field_repr<W: MemWrite<Fr>>(&self, writer: &mut W) {
        match self.v2 {
            Some(ref vk) => {
                writer.write(&[0x01.into()]);
                let mut bytes: Vec<u8> = Vec::new();
                <VerifierKey as Serializable>::serialize(vk, &mut bytes)
                    .expect("VerifierKey is serializable");
                bytes.field_repr(writer);
            }
            None => writer.write(&[0x00.into()]),
        }
    }

    fn field_size(&self) -> usize {
        match self.v2 {
            Some(ref vk) => {
                let mut bytes: Vec<u8> = Vec::new();
                <VerifierKey as Serializable>::serialize(vk, &mut bytes)
                    .expect("VerifierKey is serializable");
                1 + bytes.into_iter().fold(0, |acc, b| acc + b.field_size())
            }
            None => 1,
        }
    }
}

impl Debug for ContractOperation {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(formatter, "<verifier key>")
    }
}

impl<F> Dummy<F> for ContractOperation {
    fn dummy_with_rng<R: rand::Rng + ?Sized>(_config: &F, _rng: &mut R) -> Self {
        ContractOperation { v2: None }
    }
}

#[cfg(test)]
mod tests {
    use storage::db::InMemoryDB;

    use super::*;

    fn test_compact_int(x: u64) {
        let mut bytes = Vec::new();
        write_int(&mut bytes, x).unwrap();
        let ptr = &mut &bytes[..];
        let y = read_int(ptr).unwrap();
        assert_eq!(x, y);
        assert!(ptr.is_empty());
    }

    #[test]
    fn test_ints() {
        test_compact_int(0x0);
        test_compact_int(0x1);
        test_compact_int(0x42);
        test_compact_int(0x80);
        test_compact_int(0xff);
        test_compact_int(0x100);
        test_compact_int(0x1000);
        test_compact_int(0x10000);
    }

    #[test]
    fn test_nested_drop() {
        let mut sv: StateValue<InMemoryDB> = StateValue::Null;
        for i in 0..12_000 {
            sv = StateValue::Array(vec![sv].into());
            //sv = StateValue::Map(default_storage().new_map().insert(0u8.into(), sv));
            if i % 100 == 0 {
                dbg!(i);
            }
        }
        drop(sv);
        println!("drop(sv) finished!");
    }

    fn test_ser<T: Serializable + Deserializable + Eq + Debug>(val: T) {
        dbg!(&val);
        let mut bytes = Vec::new();
        T::serialize(&val, &mut bytes).unwrap();
        assert_eq!(bytes.len(), T::serialized_size(&val));
        let mut b = bytes.as_slice();
        let copy = T::deserialize(&mut b, 0).unwrap();
        assert_eq!(b.bytes().count(), 0);
        assert_eq!(val, copy);
    }

    #[test]
    fn test_state_ser() {
        test_ser::<ContractState<InMemoryDB>>(ContractState::default());
        test_ser::<StateValue<InMemoryDB>>(stval!((512u64)));
        test_ser::<StateValue<InMemoryDB>>(stval!({ 512u64 => (12u64) }));
        test_ser::<StateValue<InMemoryDB>>(stval!([(512u64)]));
        test_ser::<StateValue<InMemoryDB>>(stval!(null));
        test_ser::<StateValue<InMemoryDB>>(stval!({MT(12) {}}));
    }

    #[test]
    fn test_log_size() {
        use transient_crypto::merkle_tree::MerkleTree;

        // Like stval, but force database param to be InMemoryDB
        macro_rules! s {
            ($($tt:tt)*) => {
                {
                    let sv: StateValue<InMemoryDB> = stval!($($tt)*);
                    sv
                }
            };
        }

        assert_eq!(s!(null).log_size(), 0);

        assert_eq!(s!((0u8)).log_size(), 1);
        assert_eq!(s!((0u16)).log_size(), 1);
        assert_eq!(s!((0u32)).log_size(), 1);
        assert_eq!(s!((0u64)).log_size(), 1);

        assert_eq!(s!({}).log_size(), 0);
        assert_eq!(s!({ 0u32 => (1u32) }; 3).log_size(), 2);
        assert_eq!(s!({ 0u32 => (1u32) }; 4).log_size(), 2);
        assert_eq!(s!({ 0u32 => (1u32) }; 5).log_size(), 3);
        assert_eq!(s!({ 0u32 => (1u32) }; 7).log_size(), 3);
        assert_eq!(s!({ 0u32 => (1u32) }; 8).log_size(), 3);
        assert_eq!(s!({ 0u32 => (1u32) }; 9).log_size(), 4);

        assert_eq!(s!([]).log_size(), 0);
        assert_eq!(s!([(1u32); 3]).log_size(), 2);
        assert_eq!(s!([(1u32); 4]).log_size(), 2);
        assert_eq!(s!([(1u32); 7]).log_size(), 3);
        assert_eq!(s!([(1u32); 8]).log_size(), 3);
        assert_eq!(s!([(1u32); 15]).log_size(), 4);
        assert_eq!(s!([(1u32); 16]).log_size(), 4);

        for h in 0..16 {
            assert_eq!(s!({MT(h) {}}).log_size(), h as usize);
        }
    }
}
