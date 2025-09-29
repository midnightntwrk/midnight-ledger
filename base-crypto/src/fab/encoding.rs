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

use crate::fab::serialize::write_flagged_int;
use crate::repr::{BinaryHashRepr, IoWrite, MemWrite};
use const_hex::ToHexExt;
use fake::Dummy;
#[cfg(feature = "proptest")]
use proptest::arbitrary::Arbitrary;
use rand::Rng;
use rand::distributions::Standard;
use rand::prelude::Distribution;
use rand::seq::SliceRandom;
use serde::{Deserialize, Serialize};
#[cfg(feature = "proptest")]
use serialize::{NoStrategy, simple_arbitrary};
use serialize::{Serializable, Tagged, tag_enforcement_test};
use std::borrow::Borrow;
use std::fmt::{self, Debug, Formatter};
use std::iter::{empty, once};
#[cfg(feature = "proptest")]
use std::marker::PhantomData;
use std::ops::{
    Deref, Index, Range, RangeFrom, RangeFull, RangeInclusive, RangeTo, RangeToInclusive,
};
use std::sync::Arc;

use super::serialize::flagged_int_size;

/// A value in the field-aligned binary encoding. A sequence of byte strings.
#[derive(Clone, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Dummy)]
#[serde(transparent)]
pub struct Value(pub Vec<ValueAtom>);

impl Tagged for Value {
    fn tag() -> std::borrow::Cow<'static, str> {
        "fab-value[v1]".into()
    }
    fn tag_unique_factor() -> String {
        "vec(vec(u8))".into()
    }
}
tag_enforcement_test!(Value);

/// Borrowed form of [Value].
#[derive(PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
#[repr(transparent)]
pub struct ValueSlice(pub [ValueAtom]);

impl Index<Range<usize>> for ValueSlice {
    type Output = ValueSlice;
    fn index(&self, range: Range<usize>) -> &Self::Output {
        ValueSlice::from_prim_slice(&self.0[range])
    }
}

impl Index<RangeFrom<usize>> for ValueSlice {
    type Output = ValueSlice;
    fn index(&self, range: RangeFrom<usize>) -> &Self::Output {
        ValueSlice::from_prim_slice(&self.0[range])
    }
}

impl Index<RangeFull> for ValueSlice {
    type Output = ValueSlice;
    fn index(&self, range: RangeFull) -> &Self::Output {
        ValueSlice::from_prim_slice(&self.0[range])
    }
}

impl Index<RangeInclusive<usize>> for ValueSlice {
    type Output = ValueSlice;
    fn index(&self, range: RangeInclusive<usize>) -> &Self::Output {
        ValueSlice::from_prim_slice(&self.0[range])
    }
}

impl Index<RangeTo<usize>> for ValueSlice {
    type Output = ValueSlice;
    fn index(&self, range: RangeTo<usize>) -> &Self::Output {
        ValueSlice::from_prim_slice(&self.0[range])
    }
}

impl Index<RangeToInclusive<usize>> for ValueSlice {
    type Output = ValueSlice;
    fn index(&self, range: RangeToInclusive<usize>) -> &Self::Output {
        ValueSlice::from_prim_slice(&self.0[range])
    }
}

impl AsRef<Value> for Value {
    fn as_ref(&self) -> &Value {
        self
    }
}

impl Deref for Value {
    type Target = ValueSlice;
    fn deref(&self) -> &ValueSlice {
        ValueSlice::from_prim_slice(&self.0[..])
    }
}

impl Borrow<ValueSlice> for Value {
    fn borrow(&self) -> &ValueSlice {
        self
    }
}

impl ToOwned for ValueSlice {
    type Owned = Value;
    fn to_owned(&self) -> Value {
        Value::concat([self])
    }
}

impl Value {
    /// Concatenates an iterator of values.
    pub fn concat<'a, V: Borrow<ValueSlice> + 'a + ?Sized, I: IntoIterator<Item = &'a V>>(
        iter: I,
    ) -> Value {
        Value(
            iter.into_iter()
                .flat_map(|vs| vs.borrow().0.iter())
                .cloned()
                .collect(),
        )
    }
}

impl Debug for Value {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        (**self).fmt(formatter)
    }
}

impl Debug for ValueSlice {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        // avoiding debug_list to get onto one line in alt-mode debug prints
        write!(formatter, "[")?;
        let mut first = true;
        for i in self.0.iter() {
            if first {
                first = false;
            } else {
                write!(formatter, ", ")?;
            }
            write!(formatter, "{:?}", i)?;
        }
        write!(formatter, "]")
    }
}

impl ValueSlice {
    pub(crate) fn from_prim_slice(prim_slice: &[ValueAtom]) -> &ValueSlice {
        // SAFETY: This is a safe cast from &[ValueAtom] to &ValueSlice,
        // which are guaranteed to have the same memory layout due to
        // #[repr(transparent)].
        unsafe { &*(prim_slice as *const [ValueAtom] as *const ValueSlice) }
    }

    /// Returns is this value is the empty value (not to be confused with an
    /// encoding of zero, which is a non-empty value with a single empty entry).
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// Encodes an alignment in the field-aligned binary encoding, as a sequence of
/// [`AlignmentSegment`]s.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Dummy)]
#[serde(transparent)]
pub struct Alignment(pub Vec<AlignmentSegment>);

impl Tagged for Alignment {
    fn tag() -> std::borrow::Cow<'static, str> {
        "fab-alignment[v1]".into()
    }
    fn tag_unique_factor() -> String {
        "vec([[(),(u32),()],vec(fab-alignment[v1])])".into()
    }
}
tag_enforcement_test!(Alignment);

impl Debug for Alignment {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        for segment in self.0.iter() {
            write!(f, "{segment:?}")?;
        }
        Ok(())
    }
}

#[cfg(feature = "proptest")]
simple_arbitrary!(Alignment);

impl Distribution<Alignment> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> Alignment {
        let size: usize = rng.gen_range(1..9);
        let mut segments: Vec<AlignmentSegment> = Vec::new();

        for _ in 0..size {
            segments.push(rng.r#gen());
        }

        Alignment(segments)
    }
}

impl BinaryHashRepr for Alignment {
    fn binary_repr<W: MemWrite<u8>>(&self, writer: &mut W) {
        (self.0.len() as u32).binary_repr(writer);
        for segment in self.0.iter() {
            segment.binary_repr(writer);
        }
    }

    fn binary_len(&self) -> usize {
        4 + self.0.iter().map(BinaryHashRepr::binary_len).sum::<usize>()
    }
}

impl Alignment {
    fn sample_value<R: Rng + ?Sized>(&self, rng: &mut R) -> Value {
        Value(
            self.0
                .iter()
                .flat_map(|a| a.sample_value(rng).0.into_iter())
                .collect(),
        )
    }
}

impl<'a> From<&'a [AlignmentAtom]> for Alignment {
    fn from(alignment: &'a [AlignmentAtom]) -> Alignment {
        Alignment(
            alignment
                .iter()
                .copied()
                .map(AlignmentSegment::Atom)
                .collect(),
        )
    }
}

impl AsRef<Alignment> for Alignment {
    fn as_ref(&self) -> &Alignment {
        self
    }
}

/// An alignment segment in the field-aligned binary encoding. Consists of
/// either an alignment atom, or a disjoint union of alignment options.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Dummy)]
#[serde(tag = "tag", content = "value", rename_all = "camelCase")]
pub enum AlignmentSegment {
    /// A single atom in the alignment, corresponding to a single byte string in
    /// the value sequence.
    Atom(AlignmentAtom),
    /// A disjoint union of possible alignments, with an implicit domain
    /// separator for the variant used.
    ///
    /// In the context of defaults, the first option is chosen.
    /// If there are no options, the empty value is used. As a result, a default
    /// need not type-check.
    Option(Vec<Alignment>),
}

#[cfg(feature = "proptest")]
simple_arbitrary!(AlignmentSegment);

impl Distribution<AlignmentSegment> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> AlignmentSegment {
        let discriminant = rng.gen_range(0..100);
        match discriminant {
            0 => {
                let size = rng.gen_range(1..10);
                let mut options = Vec::new();
                for _ in 0..size {
                    options.push(rng.r#gen())
                }
                AlignmentSegment::Option(options)
            }
            _ => AlignmentSegment::Atom(rng.r#gen()),
        }
    }
}

impl BinaryHashRepr for AlignmentSegment {
    fn binary_repr<W: MemWrite<u8>>(&self, mut writer: &mut W) {
        match self {
            AlignmentSegment::Atom(atom) => atom.binary_repr(writer),
            AlignmentSegment::Option(options) => {
                write_flagged_int(&mut IoWrite(&mut writer), true, false, options.len() as u32)
                    .expect("Memory write shouldn't fail");
                for option in options.iter() {
                    option.binary_repr(writer);
                }
            }
        }
    }

    fn binary_len(&self) -> usize {
        match self {
            AlignmentSegment::Atom(atom) => atom.binary_len(),
            AlignmentSegment::Option(options) => {
                flagged_int_size(options.len() as u32)
                    + options
                        .iter()
                        .map(BinaryHashRepr::binary_len)
                        .sum::<usize>()
            }
        }
    }
}

impl AlignmentSegment {
    fn sample_value<R: Rng + ?Sized>(&self, rng: &mut R) -> Value {
        match self {
            Self::Atom(atom) => Value(vec![atom.sample_value_atom(rng)]),
            Self::Option(options) => options
                .choose(rng)
                .expect("AlignmentSegment::Option should not be empty")
                .sample_value(rng),
        }
    }
}

impl Debug for AlignmentSegment {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            AlignmentSegment::Atom(atom) => write!(f, "{atom:?}"),
            AlignmentSegment::Option(options) => {
                write!(f, "[")?;
                let mut first = true;
                for option in options {
                    if first {
                        first = false;
                    } else {
                        write!(f, "|")?;
                    }
                    write!(f, "{option:?}")?;
                }
                write!(f, "]")
            }
        }
    }
}

impl Alignment {
    /// Creates an alignment consisting of just an atom.
    pub fn singleton(pty: AlignmentAtom) -> Alignment {
        Alignment(vec![AlignmentSegment::Atom(pty)])
    }

    /// Tests if a value fits within an alignment.
    pub fn fits(&self, value: &ValueSlice) -> bool {
        self.consume(value).map(|(_, res)| res.0.is_empty()) == Some(true)
    }

    pub(crate) fn consume_internal<T, F: Fn(&mut T, AlignmentAtom), G: Fn(&T) -> usize>(
        &self,
        mut value: &ValueSlice,
        f: &F,
        len: &G,
        mut acc: T,
    ) -> Option<T> {
        for ts in self.0.iter() {
            match ts {
                AlignmentSegment::Atom(pty) => {
                    if value.0.is_empty() || !pty.fits(&value.0[0]) {
                        return None;
                    }
                    value = ValueSlice::from_prim_slice(&value.0[1..]);
                    f(&mut acc, *pty);
                }
                AlignmentSegment::Option(tys) => {
                    if value.0.is_empty() || !(AlignmentAtom::Bytes { length: 2 }).fits(&value.0[0])
                    {
                        return None;
                    }
                    let branch =
                        u16::try_from(ValueSlice::from_prim_slice(&value.0[..1])).ok()? as usize;
                    f(&mut acc, AlignmentAtom::Bytes { length: 2 });
                    value = ValueSlice::from_prim_slice(&value.0[1..]);
                    let prev_consumed = len(&acc);
                    let branch = tys.get(branch)?;
                    acc = branch.consume_internal(value, f, len, acc)?;
                    let consumed = len(&acc) - prev_consumed;
                    value = ValueSlice::from_prim_slice(&value.0[consumed..]);
                }
            }
        }
        Some(acc)
    }

    /// Consumes part of a value with this alignment, returning its aligned
    /// form, and the remaining value, if the prefix fits this alignment.
    pub fn consume<'a>(
        &'a self,
        value: &'a ValueSlice,
    ) -> Option<(AlignedValueSlice<'a>, &'a ValueSlice)> {
        let split_point = self.consume_internal(value, &|ctr, _| *ctr += 1, &|ctr| *ctr, 0)?;
        Some((
            AlignedValueSlice(ValueSlice::from_prim_slice(&value.0[..split_point]), self),
            ValueSlice::from_prim_slice(&value.0[split_point..]),
        ))
    }

    /// Concatenates multiple alignments.
    pub fn concat<'a, I: IntoIterator<Item = &'a Alignment>>(iter: I) -> Alignment {
        Alignment(iter.into_iter().flat_map(|a| a.0.clone()).collect())
    }

    /// Samples a default value for this alignment. Guaranteed to be aligned,
    /// except if the alignment contains an empty disjoint union.
    pub fn default(&self) -> Value {
        Value(
            self.0
                .iter()
                .flat_map(
                    |ts: &AlignmentSegment| -> Box<dyn Iterator<Item = ValueAtom>> {
                        match ts {
                            AlignmentSegment::Atom(_) => Box::new(once(Default::default())),
                            AlignmentSegment::Option(tys) if tys.is_empty() => Box::new(empty()),
                            AlignmentSegment::Option(tys) => {
                                Box::new(tys[0].default().0.into_iter())
                            }
                        }
                    },
                )
                .collect(),
        )
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
// An internal helper struct for `serde` deserialization. This provides the raw structure, but uses
// `serde`s `try_from` feature to perform additional checks to allow deserialization.
struct AlignedValueUnchecked {
    value: Value,
    alignment: Alignment,
}

impl TryFrom<AlignedValueUnchecked> for AlignedValue {
    type Error = String;
    fn try_from(unchecked: AlignedValueUnchecked) -> Result<AlignedValue, Self::Error> {
        if !unchecked.alignment.fits(&unchecked.value) {
            Err(format!(
                "value deserialized as aligned failed alignment check (value: {:?}; alignment: {:?})",
                &unchecked.value, &unchecked.alignment
            ))
        } else if !unchecked.value.0.iter().all(ValueAtom::is_in_normal_form) {
            Err("aligned value is not in normal form (has trailing zero bytes)".into())
        } else {
            Ok(AlignedValue {
                value: unchecked.value,
                alignment: unchecked.alignment,
            })
        }
    }
}

impl From<AlignedValue> for AlignedValueUnchecked {
    fn from(checked: AlignedValue) -> AlignedValueUnchecked {
        AlignedValueUnchecked {
            value: checked.value,
            alignment: checked.alignment,
        }
    }
}

/// A field-aligned binary value, annotated with its instantiated alignment.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(
    rename_all = "camelCase",
    try_from = "AlignedValueUnchecked",
    into = "AlignedValueUnchecked"
)]
pub struct AlignedValue {
    /// A field-aligned binary value.
    pub value: Value,
    /// A field-aligned instantiated alignment.
    pub alignment: Alignment,
}

impl Tagged for AlignedValue {
    fn tag() -> std::borrow::Cow<'static, str> {
        "fab-aligned-value[v1]".into()
    }
    fn tag_unique_factor() -> String {
        "(fab-value[v1],fab-alignment[v1])".into()
    }
}
tag_enforcement_test!(AlignedValue);

#[cfg(feature = "proptest")]
simple_arbitrary!(AlignedValue);

impl Distribution<AlignedValue> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> AlignedValue {
        let alignment: Alignment = rng.r#gen();
        let value = alignment.sample_value(rng);
        AlignedValue { value, alignment }
    }
}

impl Debug for AlignedValue {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        write!(fmt, "<{:?}: {:?}>", self.value, self.alignment)
    }
}

impl AlignedValue {
    /// Attempts to construct an aligned value from a value and alignment.
    pub fn new(value: Value, alignment: Alignment) -> Option<Self> {
        if alignment.fits(&value) {
            Some(AlignedValue { value, alignment })
        } else {
            None
        }
    }

    /// Concatenates two aligned values.
    pub fn concat<'a, I: IntoIterator<Item = &'a AlignedValue>>(iter: I) -> AlignedValue {
        let mut val = Vec::new();
        let mut align = Vec::new();
        for i in iter.into_iter() {
            val.extend(i.value.0.iter().cloned());
            align.extend(i.alignment.0.iter().cloned());
        }
        AlignedValue {
            value: Value(val),
            alignment: Alignment(align),
        }
    }

    /// Interprets this aligned value as its borrowed form.
    pub fn as_slice(&self) -> AlignedValueSlice<'_> {
        AlignedValueSlice(&self.value, &self.alignment)
    }
}

impl AsRef<Value> for AlignedValue {
    fn as_ref(&self) -> &Value {
        &self.value
    }
}

impl AsRef<Alignment> for AlignedValue {
    fn as_ref(&self) -> &Alignment {
        &self.alignment
    }
}

impl AsRef<Value> for Arc<AlignedValue> {
    fn as_ref(&self) -> &Value {
        &self.value
    }
}

/// The borrowed form of [`AlignedValue`].
#[derive(Clone, Serialize)]
#[serde(into = "Value")]
pub struct AlignedValueSlice<'a>(pub(crate) &'a ValueSlice, pub(crate) &'a Alignment);

impl From<AlignedValueSlice<'_>> for Value {
    fn from(slice: AlignedValueSlice<'_>) -> Value {
        Value(slice.0.0.to_vec())
    }
}

impl AlignedValueSlice<'_> {
    /// Clones this borrowed aligned value.
    pub fn to_owned_aligned(&self) -> AlignedValue {
        AlignedValue::new(Value(self.0.0.to_vec()), self.1.clone())
            .expect("Already aligned value should still match")
    }
}

impl Deref for AlignedValueSlice<'_> {
    type Target = ValueSlice;

    fn deref(&self) -> &Self::Target {
        self.0
    }
}

/// A single part of a field-aligned binary value, corresponding to a single
/// [`AlignmentAtom`].
#[derive(Clone, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Dummy)]
#[serde(transparent)]
pub struct ValueAtom(#[serde(with = "serde_bytes")] pub Vec<u8>);

impl ValueAtom {
    /// Normalizes this atom, by removing trailing zeros. Some operations may
    /// fail with non-normalized atoms.
    pub fn normalize(mut self) -> ValueAtom {
        while let Some(0) = self.0.last() {
            self.0.pop();
        }
        self
    }

    /// Tests if this atom is in normal form, as output by `normalize`.
    pub fn is_in_normal_form(&self) -> bool {
        self.0.last() != Some(&0)
    }
}

impl Debug for ValueAtom {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        if self.0.is_empty() {
            formatter.write_str("-")
        } else {
            formatter.write_str(&self.0.encode_hex())
        }
    }
}

/// A single alignment entry, typically matching with a field holding a
/// primitive data type.
#[derive(Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Dummy)]
#[serde(tag = "tag", rename_all = "camelCase")]
pub enum AlignmentAtom {
    /// This atom should be represented by its hash in a field representation.
    Compress,
    /// This atom has a known binary size of `length` bytes. Note that the value
    /// may be encoded with less, due to omitting trailing zeros.
    Bytes {
        /// The length of the atom in bytes.
        length: u32,
    },
    /// This atom encodes a native field element.
    Field,
}

#[cfg(feature = "proptest")]
simple_arbitrary!(AlignmentAtom);

impl Distribution<AlignmentAtom> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> AlignmentAtom {
        let disc = rng.gen_range(0..3);
        match disc {
            0 => AlignmentAtom::Compress,
            1 => AlignmentAtom::Bytes {
                length: rng.gen_range(0..8),
            },
            2 => AlignmentAtom::Field,
            _ => unreachable!(),
        }
    }
}

impl AlignmentAtom {
    fn sample_value_atom<R: Rng + ?Sized>(&self, rng: &mut R) -> ValueAtom {
        match self {
            Self::Compress | Self::Field => {
                let mut bytes: Vec<u8> = vec![0; FIELD_BYTE_LIMIT];
                rng.fill_bytes(&mut bytes);
                ValueAtom(bytes).normalize()
            }
            Self::Bytes { length } => {
                let mut bytes: Vec<u8> = vec![0; *length as usize];
                rng.fill_bytes(&mut bytes);
                ValueAtom(bytes).normalize()
            }
        }
    }
}

impl Debug for AlignmentAtom {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            AlignmentAtom::Compress => write!(f, "c"),
            AlignmentAtom::Bytes { length } => write!(f, "b{length}"),
            AlignmentAtom::Field => write!(f, "f"),
        }
    }
}

impl BinaryHashRepr for AlignmentAtom {
    fn binary_repr<W: MemWrite<u8>>(&self, writer: &mut W) {
        Serializable::serialize(self, &mut IoWrite(writer)).ok();
    }
    fn binary_len(&self) -> usize {
        Serializable::serialized_size(self)
    }
}

/// The number of bytes required to represent a field.
pub const FIELD_BYTE_LIMIT: usize = 64;

impl AlignmentAtom {
    /// Tests if a [`ValueAtom`] fits within the alignment.
    pub fn fits(&self, value: &ValueAtom) -> bool {
        match self {
            AlignmentAtom::Compress => true,
            AlignmentAtom::Bytes { length } => {
                *length >= value.0.len() as u32 && value.is_in_normal_form()
            }

            AlignmentAtom::Field => FIELD_BYTE_LIMIT >= value.0.len() && value.is_in_normal_form(),
        }
    }
}
