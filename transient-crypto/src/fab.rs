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

//! Defines the primitives of the field-aligned binary representation, where
//! values are represented as sequences of binary strings, that are tied to an
//! alignment which can be used to interpret them either as binary data, or a
//! sequence of field elements for proving.

use std::iter::{once, repeat};

use crate::curve::{EmbeddedFr, EmbeddedGroupAffine};
use crate::curve::{FR_BYTES, FR_BYTES_STORED, Fr};
use crate::hash::transient_commit;
use crate::merkle_tree::{MerklePath, MerkleTreeDigest};
use crate::repr::{FieldRepr, bytes_from_field_repr};
use base_crypto::fab::{
    Aligned, AlignedValue, Alignment, AlignmentAtom, AlignmentSegment, DynAligned,
    InvalidBuiltinDecode, Value, ValueAtom, ValueSlice, int_size,
};
use base_crypto::repr::{BinaryHashRepr, MemWrite};
use rand::Rng;
use rand::distributions::Standard;
use rand::prelude::Distribution;

/// An extension for the value in the field-aligned binary encoding.
pub(crate) trait ValueExt {
    fn repr_traverse<
        T,
        F: Fn(T, &AlignmentAtom, &ValueAtom) -> T,
        L: Fn(&Alignment) -> usize,
        P: Fn(T, usize) -> T,
    >(
        atom_slice: &mut &[ValueAtom],
        align: &Alignment,
        f: &F,
        len: &L,
        pad: &P,
        acc: T,
    ) -> T;
    fn field_repr_unchecked<W: MemWrite<Fr>>(&self, align: &Alignment, writer: &mut W);
    fn binary_repr_unchecked<W: MemWrite<u8>>(&self, align: &Alignment, writer: &mut W);
}

impl From<MerkleTreeDigest> for ValueAtom {
    fn from(val: MerkleTreeDigest) -> ValueAtom {
        Fr::from(val).into()
    }
}

impl TryFrom<&ValueAtom> for MerkleTreeDigest {
    type Error = InvalidBuiltinDecode;

    fn try_from(value: &ValueAtom) -> Result<MerkleTreeDigest, InvalidBuiltinDecode> {
        Ok(Fr::try_from(value)?.into())
    }
}

impl<T: Into<Value>> From<MerklePath<T>> for Value {
    fn from(path: MerklePath<T>) -> Value {
        let mut parts = Vec::new();
        parts.push(path.leaf.into());
        for entry in path.path.iter() {
            parts.push(entry.sibling.into());
            parts.push(entry.goes_left.into());
        }
        Value::concat(parts.iter())
    }
}

impl From<MerkleTreeDigest> for Value {
    fn from(val: MerkleTreeDigest) -> Value {
        Value(vec![val.into()])
    }
}

impl TryFrom<&ValueSlice> for MerkleTreeDigest {
    type Error = InvalidBuiltinDecode;

    fn try_from(value: &ValueSlice) -> Result<MerkleTreeDigest, InvalidBuiltinDecode> {
        if value.0.len() == 1 {
            Ok(MerkleTreeDigest::try_from(&value.0[0])?)
        } else {
            Err(InvalidBuiltinDecode(stringify!($ty)))
        }
    }
}

impl Aligned for MerkleTreeDigest {
    fn alignment() -> Alignment {
        Alignment::singleton(AlignmentAtom::Field)
    }
}

impl<T: DynAligned> DynAligned for MerklePath<T> {
    fn dyn_alignment(&self) -> Alignment {
        let leaf_align = self.leaf.dyn_alignment();
        let entry_align = Alignment::concat([&MerkleTreeDigest::alignment(), &bool::alignment()]);
        Alignment::concat(once(&leaf_align).chain(repeat(&entry_align).take(self.path.len())))
    }
}

impl From<EmbeddedGroupAffine> for Value {
    fn from(value: EmbeddedGroupAffine) -> Value {
        Value(vec![
            value.x().unwrap_or(0.into()).into(),
            value.y().unwrap_or(0.into()).into(),
        ])
    }
}

impl From<EmbeddedFr> for Value {
    fn from(val: EmbeddedFr) -> Value {
        Value(vec![val.into()])
    }
}

impl TryFrom<&ValueSlice> for EmbeddedGroupAffine {
    type Error = InvalidBuiltinDecode;

    fn try_from(value: &ValueSlice) -> Result<EmbeddedGroupAffine, InvalidBuiltinDecode> {
        if value.0.len() == 2 {
            let x: Fr = (&value.0[0]).try_into()?;
            let y: Fr = (&value.0[1]).try_into()?;
            let is_identity = EmbeddedGroupAffine::HAS_INFINITY && x == 0.into() && y == 0.into();
            if is_identity {
                Ok(EmbeddedGroupAffine::identity())
            } else {
                Ok(EmbeddedGroupAffine::new(x, y)
                    .ok_or(InvalidBuiltinDecode("EmbeddedGroupAffine"))?)
            }
        } else {
            Err(InvalidBuiltinDecode("EmbeddedGroupAffine"))
        }
    }
}

impl TryFrom<&ValueSlice> for EmbeddedFr {
    type Error = InvalidBuiltinDecode;

    fn try_from(value: &ValueSlice) -> Result<EmbeddedFr, InvalidBuiltinDecode> {
        if value.0.len() == 1 {
            Ok(<EmbeddedFr>::try_from(&value.0[0])?)
        } else {
            Err(InvalidBuiltinDecode(stringify!(EmbeddedFr)))
        }
    }
}

impl From<EmbeddedFr> for ValueAtom {
    fn from(val: EmbeddedFr) -> ValueAtom {
        ValueAtom(val.as_le_bytes()).normalize()
    }
}

impl TryFrom<&ValueAtom> for EmbeddedFr {
    type Error = InvalidBuiltinDecode;

    fn try_from(value: &ValueAtom) -> Result<EmbeddedFr, InvalidBuiltinDecode> {
        if value.0.len() <= FR_BYTES {
            EmbeddedFr::from_le_bytes(&value.0).ok_or(InvalidBuiltinDecode("EmbeddedFr"))
        } else {
            Err(InvalidBuiltinDecode("EmbeddedFr"))
        }
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

forward_primitive_value!(Fr);

impl From<Fr> for ValueAtom {
    fn from(val: Fr) -> ValueAtom {
        ValueAtom(val.as_le_bytes()).normalize()
    }
}

impl TryFrom<&ValueAtom> for Fr {
    type Error = InvalidBuiltinDecode;

    fn try_from(value: &ValueAtom) -> Result<Fr, InvalidBuiltinDecode> {
        if value.0.len() <= FR_BYTES {
            Fr::from_le_bytes(&value.0).ok_or(InvalidBuiltinDecode("Fr"))
        } else {
            Err(InvalidBuiltinDecode("Fr"))
        }
    }
}

impl ValueExt for Value {
    fn repr_traverse<
        T,
        F: Fn(T, &AlignmentAtom, &ValueAtom) -> T,
        L: Fn(&Alignment) -> usize,
        P: Fn(T, usize) -> T,
    >(
        atom_slice: &mut &[ValueAtom],
        align: &Alignment,
        f: &F,
        len: &L,
        pad: &P,
        mut acc: T,
    ) -> T {
        for segment in align.0.iter() {
            match segment {
                AlignmentSegment::Atom(atom) => {
                    acc = f(acc, atom, &atom_slice[0]);
                    *atom_slice = &atom_slice[1..];
                }
                AlignmentSegment::Option(options) => {
                    let discriminant = u16::try_from(&atom_slice[0])
                        .expect("unchecked discriminant should decode");
                    let choice = &options[discriminant as usize];
                    acc = Value::repr_traverse(atom_slice, choice, f, len, pad, acc);
                    let padding = options.iter().map(len).max().unwrap_or(0) - len(choice);
                    acc = pad(acc, padding);
                }
            }
        }
        acc
    }

    fn field_repr_unchecked<W: MemWrite<Fr>>(&self, align: &Alignment, writer: &mut W) {
        Value::repr_traverse(
            &mut &self.0[..],
            align,
            &|mut w: &mut W, a, v| {
                v.field_repr_unchecked(a, &mut w);
                w
            },
            &Alignment::field_len,
            &|w, n| {
                w.write(&vec![Fr::from(0); n]);
                w
            },
            writer,
        );
    }

    fn binary_repr_unchecked<W: MemWrite<u8>>(&self, align: &Alignment, writer: &mut W) {
        Value::repr_traverse(
            &mut &self.0[..],
            align,
            &|mut w: &mut W, a, v| {
                v.binary_repr_unchecked(a, &mut w);
                w
            },
            &Alignment::bin_len,
            &|w, n| {
                w.write(&vec![0u8; n]);
                w
            },
            writer,
        );
    }
}

/// An extension or the alignment in the field-aligned binary encoding.
pub trait AlignmentExt {
    /// Parses a given field representation as this alignment, and returns the
    /// corresponding aligned value.
    fn parse_field_repr(&self, repr: &[Fr]) -> Option<AlignedValue>;

    /// The maximum size of an `AlignedValue` within this alignment.
    fn max_aligned_size(&self) -> usize;

    /// Returns a field length.
    fn field_len(&self) -> usize;

    /// Returns a binary length.
    fn bin_len(&self) -> usize;
}

/// Parses a given field representation to construct a vector `ValueAtom`s for use in `parse_field_repr`.
fn parse_field_repr_inner(
    segments: &[AlignmentSegment],
    repr: &mut &[Fr],
    val: &mut Vec<ValueAtom>,
) -> Option<()> {
    for segment in segments.iter() {
        match segment {
            AlignmentSegment::Atom(atom) => val.push(atom.parse_field_repr(repr)?),
            AlignmentSegment::Option(options) => {
                let variant = u16::try_from(*repr.first()?).ok()?;
                *repr = &repr[1..];
                val.push(variant.into());
                let choice = options.get(variant as usize)?;
                parse_field_repr_inner(&choice.0, repr, val)?;
                let padding = options.iter().map(Alignment::field_len).max().unwrap_or(0)
                    - choice.field_len();
                if repr.len() < padding || repr[..padding].iter().any(|f| *f != Fr::from(0)) {
                    return None;
                }
                *repr = &repr[padding..];
            }
        }
    }
    Some(())
}

impl AlignmentExt for Alignment {
    fn parse_field_repr(&self, mut repr: &[Fr]) -> Option<AlignedValue> {
        let mut value = Vec::new();
        parse_field_repr_inner(&self.0, &mut repr, &mut value)?;
        Some(AlignedValue {
            value: Value(value),
            alignment: self.clone(),
        })
    }

    fn max_aligned_size(&self) -> usize {
        1 + int_size(self.0.len())
            + self
                .0
                .iter()
                .map(AlignmentSegment::max_aligned_size)
                .sum::<usize>()
    }

    fn field_len(&self) -> usize {
        self.0.iter().map(AlignmentSegment::field_len).sum()
    }

    fn bin_len(&self) -> usize {
        self.0.iter().map(AlignmentSegment::bin_len).sum()
    }
}

impl FieldRepr for Alignment {
    fn field_repr<W: MemWrite<Fr>>(&self, writer: &mut W) {
        (self.0.len() as u32).field_repr(writer);
        for segment in self.0.iter() {
            segment.field_repr(writer);
        }
    }

    fn field_size(&self) -> usize {
        1 + self.0.iter().map(FieldRepr::field_size).sum::<usize>()
    }
}

impl FieldRepr for AlignedValue {
    fn field_repr<W: MemWrite<Fr>>(&self, writer: &mut W) {
        self.alignment.field_repr(writer);
        self.value.field_repr_unchecked(&self.alignment, writer);
    }

    fn field_size(&self) -> usize {
        self.alignment.field_size() + self.alignment.field_len()
    }
}

/// An extension for the `AlignedValue`.
pub trait AlignedValueExt {
    /// Iterate over the field elements in this value, not encoding the alignment itself.
    fn value_only_field_repr<W: MemWrite<Fr>>(&self, writer: &mut W);

    /// Returns the number of elements output by [`Self::value_only_field_repr`].
    fn value_only_field_size(&self) -> usize;
}

impl AlignedValueExt for AlignedValue {
    fn value_only_field_repr<W: MemWrite<Fr>>(&self, writer: &mut W) {
        self.value.field_repr_unchecked(&self.alignment, writer)
    }

    fn value_only_field_size(&self) -> usize {
        self.alignment.field_len()
    }
}

/// Wrapper around [`AlignedValue`] whose [`FieldRepr`] implementation uses
/// [`AlignedValue::value_only_field_repr`].
pub struct ValueReprAlignedValue(pub AlignedValue);

impl From<ValueReprAlignedValue> for Value {
    fn from(value: ValueReprAlignedValue) -> Value {
        value.0.value
    }
}

impl FieldRepr for ValueReprAlignedValue {
    fn field_repr<W: MemWrite<Fr>>(&self, writer: &mut W) {
        self.0.value_only_field_repr(writer);
    }

    fn field_size(&self) -> usize {
        self.0.value_only_field_size()
    }
}

impl BinaryHashRepr for ValueReprAlignedValue {
    fn binary_repr<W: MemWrite<u8>>(&self, writer: &mut W) {
        self.0
            .value
            .binary_repr_unchecked(&self.0.alignment, writer);
    }
    fn binary_len(&self) -> usize {
        self.0.alignment.bin_len()
    }
}

impl DynAligned for ValueReprAlignedValue {
    fn dyn_alignment(&self) -> Alignment {
        self.0.dyn_alignment()
    }
}

/// An extension for the `ValueAtom`.
pub(crate) trait ValueAtomExt {
    /// The field representation of this atom, against a matching alignment
    /// atom. Returns `false` if the value does not fit.
    #[allow(dead_code)]
    fn field_repr<W: MemWrite<Fr>>(&self, ty: &AlignmentAtom, writer: &mut W) -> bool;

    /// Returns the field representation of a primitive value wrt. a primitive
    /// type.
    ///
    /// # Safety
    ///
    /// This is safe to call iff `ty.`[`fits`](AlignmentAtom::fits)`(self)` has
    /// returns `true`.
    fn field_repr_unchecked<W: MemWrite<Fr>>(&self, ty: &AlignmentAtom, writer: &mut W);
    fn binary_repr_unchecked<W: MemWrite<u8>>(&self, ty: &AlignmentAtom, writer: &mut W);
}

impl ValueAtomExt for ValueAtom {
    fn field_repr<W: MemWrite<Fr>>(&self, ty: &AlignmentAtom, writer: &mut W) -> bool {
        if !ty.fits(self) {
            false
        } else {
            self.field_repr_unchecked(ty, writer);
            true
        }
    }

    fn field_repr_unchecked<W: MemWrite<Fr>>(&self, ty: &AlignmentAtom, writer: &mut W) {
        match ty {
            AlignmentAtom::Compress => {
                // Special case for the empty string to make defaults work well.
                if self.0.is_empty() {
                    writer.write(&[0.into()]);
                } else {
                    writer.write(&[transient_commit(&self.0[..], (self.0.len() as u64).into())])
                }
            }
            AlignmentAtom::Bytes { length } => {
                let prepend_zeros = (*length as usize).div_ceil(FR_BYTES_STORED)
                    - self.0.len().div_ceil(FR_BYTES_STORED);
                let raw = self
                    .0
                    .chunks(FR_BYTES_STORED)
                    .map(|bytes| {
                        Fr::from_le_bytes(bytes).expect("Bytes must fit into FR_BYTES_STORED chunk")
                    })
                    .rev();
                writer.write(&vec![Fr::from(0); prepend_zeros]);
                writer.write(&raw.collect::<Vec<_>>());
            }
            AlignmentAtom::Field => writer
                .write(&[Fr::from_le_bytes(&self.0)
                    .expect("Unchecked field repr field should be in range")]),
        }
    }

    fn binary_repr_unchecked<W: MemWrite<u8>>(&self, ty: &AlignmentAtom, writer: &mut W) {
        match ty {
            AlignmentAtom::Compress => {
                transient_commit(&self.0[..], (self.0.len() as u64).into()).binary_repr(writer);
            }
            AlignmentAtom::Bytes { length } => {
                writer.write(&self.0);
                let missing_bytes = (*length as usize) - self.0.len();
                let zeroes = vec![0u8; missing_bytes];
                writer.write(&zeroes);
            }
            AlignmentAtom::Field => {
                Fr::from_le_bytes(&self.0)
                    .expect("Unchecked field repr field should be in range")
                    .binary_repr(writer);
            }
        }
    }
}

/// An extension for the `AlignmentAtom`.
pub(crate) trait AlignmentAtomExt {
    fn parse_field_repr(&self, repr: &mut &[Fr]) -> Option<ValueAtom>;
    #[allow(dead_code)]
    fn sample_value_atom<R: Rng + ?Sized>(&self, rng: &mut R) -> ValueAtom;
    fn max_aligned_size(&self) -> usize;
    fn field_len(&self) -> usize;
    fn bin_len(&self) -> usize;
}

impl AlignmentAtomExt for AlignmentAtom {
    fn parse_field_repr(&self, repr: &mut &[Fr]) -> Option<ValueAtom> {
        match self {
            // Impossible to parse compress from a field!
            AlignmentAtom::Compress => None,
            AlignmentAtom::Field => {
                let res = repr.first()?;
                *repr = &repr[1..];
                Some(ValueAtom(res.as_le_bytes()).normalize())
            }
            AlignmentAtom::Bytes { length } => {
                bytes_from_field_repr(repr, *length as usize).map(ValueAtom)
            }
        }
    }

    fn sample_value_atom<R: Rng + ?Sized>(&self, rng: &mut R) -> ValueAtom
    where
        Standard: Distribution<Fr>,
    {
        match self {
            Self::Compress | Self::Field => {
                let val = rng.r#gen::<Fr>().as_le_bytes();
                ValueAtom(val).normalize()
            }
            Self::Bytes { length } => {
                let mut bytes: Vec<u8> = vec![0; *length as usize];
                rng.fill_bytes(&mut bytes);
                ValueAtom(bytes).normalize()
            }
        }
    }

    fn max_aligned_size(&self) -> usize {
        match self {
            AlignmentAtom::Compress | AlignmentAtom::Field => 2 + FR_BYTES,
            AlignmentAtom::Bytes { length } => 2 + int_size(*length as usize) + *length as usize,
        }
    }

    fn field_len(&self) -> usize {
        match self {
            AlignmentAtom::Compress | AlignmentAtom::Field => 1,
            AlignmentAtom::Bytes { length } => length.div_ceil(FR_BYTES_STORED as u32) as usize,
        }
    }

    fn bin_len(&self) -> usize {
        match self {
            AlignmentAtom::Compress | AlignmentAtom::Field => FR_BYTES,
            AlignmentAtom::Bytes { length } => *length as usize,
        }
    }
}

impl FieldRepr for AlignmentAtom {
    fn field_repr<W: MemWrite<Fr>>(&self, writer: &mut W) {
        match self {
            AlignmentAtom::Bytes { length } => writer.write(&[(*length).into()]),
            AlignmentAtom::Compress => writer.write(&[(-1).into()]),
            AlignmentAtom::Field => writer.write(&[(-2).into()]),
        }
    }

    fn field_size(&self) -> usize {
        1
    }
}

/// An extension for the `AlignmentSegmentExt`.
pub(crate) trait AlignmentSegmentExt {
    fn max_aligned_size(&self) -> usize;
    fn field_len(&self) -> usize;
    fn bin_len(&self) -> usize;
}

impl AlignmentSegmentExt for AlignmentSegment {
    fn max_aligned_size(&self) -> usize {
        match self {
            AlignmentSegment::Atom(atom) => atom.max_aligned_size(),
            AlignmentSegment::Option(options) => options
                .iter()
                .map(Alignment::max_aligned_size)
                .max()
                .unwrap_or(0),
        }
    }

    fn field_len(&self) -> usize {
        match self {
            AlignmentSegment::Atom(atom) => atom.field_len(),
            AlignmentSegment::Option(options) => {
                1 + options.iter().map(Alignment::field_len).max().unwrap_or(0)
            }
        }
    }

    fn bin_len(&self) -> usize {
        match self {
            AlignmentSegment::Atom(atom) => atom.bin_len(),
            AlignmentSegment::Option(options) => {
                2 + options.iter().map(Alignment::bin_len).max().unwrap_or(0)
            }
        }
    }
}

impl FieldRepr for AlignmentSegment {
    fn field_repr<W: MemWrite<Fr>>(&self, writer: &mut W) {
        match self {
            AlignmentSegment::Atom(atom) => atom.field_repr(writer),
            AlignmentSegment::Option(options) => {
                writer.write(&[(-3).into(), (options.len() as u32).into()]);
                for option in options {
                    option.field_repr(writer);
                }
            }
        }
    }

    fn field_size(&self) -> usize {
        match self {
            AlignmentSegment::Atom(atom) => atom.field_size(),
            AlignmentSegment::Option(options) => {
                2 + options.iter().map(FieldRepr::field_size).sum::<usize>()
            }
        }
    }
}
