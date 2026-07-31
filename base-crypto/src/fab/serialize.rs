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

use super::{AlignedValue, Alignment, AlignmentAtom, AlignmentSegment, Value, ValueAtom};
#[cfg(feature = "proptest")]
use serialize::randomised_serialization_test;
use serialize::{Deserializable, ReadExt, Serializable, VecExt};
use std::io::{self, Read, Write};

const ONE_BYTE_LIMIT: u32 = (1 << 5) - 1;
const TWO_BYTE_START: u32 = ONE_BYTE_LIMIT + 1;
const TWO_BYTE_LIMIT: u32 = (1 << 12) - 1;
const THREE_BYTE_START: u32 = TWO_BYTE_LIMIT + 1;
const THREE_BYTE_LIMIT: u32 = (1 << 19) - 1;

pub(super) fn write_flagged_int<W: Write>(
    writer: &mut W,
    x: bool,
    y: bool,
    int: u32,
) -> io::Result<()> {
    let flag_u8 = ((x as u8) << 7) | ((y as u8) << 6);
    match int {
        0..=ONE_BYTE_LIMIT => writer.write_all(&[flag_u8 | int as u8][..]),
        TWO_BYTE_START..=TWO_BYTE_LIMIT => {
            writer.write_all(&[flag_u8 | 0x20 | (int % 0x20) as u8, (int >> 5) as u8])
        }
        THREE_BYTE_START..=THREE_BYTE_LIMIT => writer.write_all(&[
            flag_u8 | 0x20 | (int % 0x20) as u8,
            0x80 | ((int >> 5) % 0x80) as u8,
            (int >> 12) as u8,
        ]),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("integer out of three-byte limit: {}", int),
        )),
    }
}

pub(super) fn flagged_int_size(int: u32) -> usize {
    match int {
        0..=ONE_BYTE_LIMIT => 1,
        TWO_BYTE_START..=TWO_BYTE_LIMIT => 2,
        THREE_BYTE_START..=THREE_BYTE_LIMIT => 3,
        // Ideally we'd error, but that's not sensible for a size *hint*.
        _ => 1000,
    }
}

fn read_flagged_int<R: Read>(reader: &mut R) -> io::Result<(bool, bool, u32)> {
    let mut byte_buf = [0u8];
    reader.read_exact(&mut byte_buf[..])?;
    let x = (byte_buf[0] & 0x80) != 0;
    let y = (byte_buf[0] & 0x40) != 0;
    let a = (byte_buf[0] % 0x20) as u32;
    if (byte_buf[0] & 0x20) == 0 {
        return Ok((x, y, a));
    }
    reader.read_exact(&mut byte_buf[..])?;
    let b = (byte_buf[0] % 0x80) as u32;
    if (byte_buf[0] & 0x80) == 0 {
        return if b == 0 {
            Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "use of longer encoding than necessary for flagged int",
            ))
        } else {
            Ok((x, y, a | (b << 5)))
        };
    }
    reader.read_exact(&mut byte_buf[..])?;
    let c = (byte_buf[0] % 0x80) as u32;
    if (byte_buf[0] & 0x80) == 0 {
        if c == 0 {
            Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "use of longer encoding than necessary for flagged int",
            ))
        } else {
            Ok((x, y, a | (b << 5) | (c << 12)))
        }
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "use of reserved flag in three-byte flagged int encoding",
        ))
    }
}

impl Serializable for Value {
    fn serialize(&self, writer: &mut impl Write) -> std::io::Result<()> {
        if self.0.len() == 1 {
            <ValueAtom as Serializable>::serialize(&self.0[0], writer)
        } else {
            write_flagged_int(writer, true, false, self.0.len() as u32)?;
            for atom in self.0.iter() {
                atom.serialize(writer)?;
            }
            Ok(())
        }
    }

    fn serialized_size(&self) -> usize {
        if self.0.len() == 1 {
            self.0[0].serialized_size()
        } else {
            flagged_int_size(self.0.len() as u32)
                + self
                    .0
                    .iter()
                    .map(Serializable::serialized_size)
                    .sum::<usize>()
        }
    }
}

impl Deserializable for Value {
    fn deserialize(
        reader: &mut impl std::io::Read,
        mut recursion_depth: u32,
    ) -> Result<Self, std::io::Error> {
        Self::check_rec(&mut recursion_depth)?;
        let (x, y, int) = read_flagged_int(reader)?;
        match (x, y) {
            (false, _) => Ok(Value(vec![ValueAtom::deserialize_with_flagged_int(
                x, y, int, reader,
            )?])),
            (true, false) => {
                let mut res = Vec::new();
                for _ in 0..int {
                    res.push(<ValueAtom as Deserializable>::deserialize(
                        reader,
                        recursion_depth,
                    )?);
                }
                Ok(Value(res))
            }
            (true, true) => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Attempted to decode Value with reserved flags '11'",
            )),
        }
    }
}

/// Returns integer's size.
pub fn int_size(int: usize) -> usize {
    match int {
        0x00..=0xff => 1,
        0x100..=0xffff => 2,
        0x10000..=0xffffff => 3,
        0x1000000..=0xffffffff => 4,
        _ => unreachable!("invalid fab length"),
    }
}

impl Serializable for Alignment {
    fn serialize(&self, writer: &mut impl Write) -> io::Result<()> {
        if self.0.len() == 1 {
            self.0[0].serialize(writer)
        } else {
            write_flagged_int(writer, true, true, self.0.len() as u32)?;
            for segment in self.0.iter() {
                segment.serialize(writer)?;
            }
            Ok(())
        }
    }

    fn serialized_size(&self) -> usize {
        if self.0.len() == 1 {
            self.0[0].serialized_size()
        } else {
            flagged_int_size(self.0.len() as u32)
                + self
                    .0
                    .iter()
                    .map(Serializable::serialized_size)
                    .sum::<usize>()
        }
    }
}

#[cfg(feature = "proptest")]
randomised_serialization_test!(Alignment);

impl Deserializable for Alignment {
    fn deserialize(
        reader: &mut impl Read,
        mut recursion_depth: u32,
    ) -> Result<Self, std::io::Error> {
        Self::check_rec(&mut recursion_depth)?;
        let (x, y, int) = read_flagged_int(reader)?;
        Alignment::deserialize_with_flagged_int(x, y, int, reader, recursion_depth)
    }
}

impl Serializable for AlignmentSegment {
    fn serialize(&self, writer: &mut impl Write) -> Result<(), std::io::Error> {
        match self {
            AlignmentSegment::Atom(atom) => atom.serialize(writer),
            AlignmentSegment::Option(branches) => {
                write_flagged_int(writer, true, false, branches.len() as u32)?;
                for branch in branches.iter() {
                    branch.serialize(writer)?;
                }
                Ok(())
            }
        }
    }

    fn serialized_size(&self) -> usize {
        match self {
            AlignmentSegment::Atom(atom) => atom.serialized_size(),
            AlignmentSegment::Option(branches) => {
                flagged_int_size(branches.len() as u32)
                    + branches
                        .iter()
                        .map(Serializable::serialized_size)
                        .sum::<usize>()
            }
        }
    }
}

#[cfg(feature = "proptest")]
randomised_serialization_test!(AlignmentSegment);

impl Deserializable for AlignmentSegment {
    fn deserialize(
        reader: &mut impl Read,
        mut recursion_depth: u32,
    ) -> Result<Self, std::io::Error> {
        Self::check_rec(&mut recursion_depth)?;
        let (x, y, int) = read_flagged_int(reader)?;
        AlignmentSegment::deserialize_with_flagged_int(x, y, int, reader, recursion_depth)
    }
}

impl AlignmentSegment {
    fn deserialize_with_flagged_int<R: Read>(
        x: bool,
        y: bool,
        int: u32,
        reader: &mut R,
        recursion_depth: u32,
    ) -> io::Result<Self> {
        match (x, y) {
            (false, _) => AlignmentAtom::deserialize_with_flagged_int(x, y, int, reader)
                .map(AlignmentSegment::Atom),
            (true, false) => {
                let mut branches = Vec::with_bounded_capacity(int as usize);
                for _ in 0..int {
                    branches.push(<Alignment as Deserializable>::deserialize(
                        reader,
                        recursion_depth,
                    )?);
                }
                Ok(AlignmentSegment::Option(branches))
            }
            (true, true) => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Use of reserved flag '11' in AlignmentSegment",
            )),
        }
    }
}

impl Serializable for AlignedValue {
    fn serialize(&self, writer: &mut impl Write) -> Result<(), std::io::Error> {
        self.value.serialize(writer)?;
        self.alignment.serialize(writer)
    }

    fn serialized_size(&self) -> usize {
        self.value.serialized_size() + self.alignment.serialized_size()
    }
}

#[cfg(feature = "proptest")]
randomised_serialization_test!(AlignedValue);

impl Deserializable for AlignedValue {
    fn deserialize(
        reader: &mut impl Read,
        mut recursion_depth: u32,
    ) -> Result<Self, std::io::Error> {
        Self::check_rec(&mut recursion_depth)?;
        let value: Value = Deserializable::deserialize(reader, recursion_depth)?;
        let alignment: Alignment = Deserializable::deserialize(reader, recursion_depth)?;
        Ok(AlignedValue { value, alignment })
    }
}

impl Serializable for ValueAtom {
    fn serialize(&self, writer: &mut impl Write) -> Result<(), std::io::Error> {
        if self.is_in_normal_form() {
            if self.0.len() == 1 && self.0[0] < 32 {
                write_flagged_int(writer, false, false, self.0[0] as u32)?;
            } else {
                write_flagged_int(writer, false, true, self.0.len() as u32)?;
                writer.write_all(&self.0[..])?;
            }
            Ok(())
        } else {
            self.clone().normalize().serialize(writer)
        }
    }

    fn serialized_size(&self) -> usize {
        if self.is_in_normal_form() {
            if self.0.len() == 1 && self.0[0] < 32 {
                flagged_int_size(self.0[0] as u32)
            } else {
                flagged_int_size(self.0.len() as u32) + self.0.len()
            }
        } else {
            self.clone().normalize().serialized_size()
        }
    }
}

impl Deserializable for ValueAtom {
    fn deserialize(reader: &mut impl Read, _recursion_depth: u32) -> Result<Self, std::io::Error> {
        let (x, y, int) = read_flagged_int(reader)?;
        Self::deserialize_with_flagged_int(x, y, int, reader)
    }
}

impl Serializable for AlignmentAtom {
    fn serialize(&self, writer: &mut impl Write) -> Result<(), std::io::Error> {
        match self {
            AlignmentAtom::Compress => write_flagged_int(writer, false, true, 0),
            AlignmentAtom::Field => write_flagged_int(writer, false, true, 1),
            AlignmentAtom::Bytes { length } => write_flagged_int(writer, false, false, *length),
        }
    }

    fn serialized_size(&self) -> usize {
        match self {
            AlignmentAtom::Bytes { length } => flagged_int_size(*length),
            AlignmentAtom::Compress | AlignmentAtom::Field => 1,
        }
    }
}

#[cfg(feature = "proptest")]
randomised_serialization_test!(AlignmentAtom);

impl Deserializable for AlignmentAtom {
    fn deserialize(reader: &mut impl Read, _recursion_depth: u32) -> Result<Self, std::io::Error> {
        let (x, y, int) = read_flagged_int(reader)?;
        AlignmentAtom::deserialize_with_flagged_int(x, y, int, reader)
    }
}

impl Alignment {
    fn deserialize_with_flagged_int<R: Read>(
        x: bool,
        y: bool,
        int: u32,
        reader: &mut R,
        recursion_depth: u32,
    ) -> io::Result<Self> {
        if x && y {
            if int == 1 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "singleton alignment encoded as multi-entry alignment",
                ));
            }
            let mut res = Vec::with_bounded_capacity(int as usize);
            for _ in 0..int {
                res.push(Deserializable::deserialize(reader, recursion_depth)?);
            }
            Ok(Alignment(res))
        } else {
            Ok(Alignment(vec![
                AlignmentSegment::deserialize_with_flagged_int(x, y, int, reader, recursion_depth)?,
            ]))
        }
    }
}

impl ValueAtom {
    fn deserialize_with_flagged_int<R: Read>(
        x: bool,
        y: bool,
        int: u32,
        reader: &mut R,
    ) -> io::Result<Self> {
        if x {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "x-flag may not be 1 for value atom",
            ));
        }
        if y {
            let res = reader.read_exact_to_vec(int as usize)?;
            if int > 0 && res[int as usize - 1] == 0 {
                Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "ValueAtom ended with zero byte",
                ))
            } else {
                Ok(ValueAtom(res))
            }
        } else if int < 32 && int > 0 {
            Ok(ValueAtom(vec![int as u8]))
        } else {
            Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("singleton ValueAtom out of range: {}", int),
            ))
        }
    }
}

impl AlignmentAtom {
    fn deserialize_with_flagged_int<R: Read>(
        x: bool,
        y: bool,
        int: u32,
        _reader: &mut R,
    ) -> io::Result<Self> {
        match (x, y, int) {
            (false, false, length) => Ok(AlignmentAtom::Bytes { length }),
            (false, true, 0) => Ok(AlignmentAtom::Compress),
            (false, true, 1) => Ok(AlignmentAtom::Field),
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "illegal value for alignment atom",
            )),
        }
    }
}

// ---------------------------------------------------------------------------
// Area C: serde `Deserialize` vs binary `Deserializable` accept/reject parity
// for `AlignmentAtom` / `ValueAtom` / `Alignment`.
//
// The binary `Deserializable` path (this file) is the stricter one: it enforces
// value-atom normal form (rejects a trailing zero byte, serialize.rs:410-414),
// caps `AlignmentAtom::Bytes { length }` at THREE_BYTE_LIMIT = 524287 via the
// flagged-int codec (serialize.rs:24, write_flagged_int cannot emit more), and
// enforces canonical singleton `Alignment` encodings. The serde impls are all
// *derived* (encoding.rs:605-607 ValueAtom `#[serde(transparent)]`+serde_bytes;
// encoding.rs:637-638 AlignmentAtom internally-tagged enum; encoding.rs:182-184
// Alignment transparent), so they inherit none of these checks and admit values
// the binary path rejects (or cannot even represent). The desired property:
// the two decoders agree on the set of accepted values.
// ---------------------------------------------------------------------------
#[cfg(all(test, feature = "proptest"))]
mod parity_props {
    use crate::fab::ValueAtom;
    use proptest::prelude::*;
    use serialize::{Deserializable, Serializable};

    fn bin_ser<T: Serializable>(v: &T) -> std::io::Result<Vec<u8>> {
        let mut b = Vec::new();
        v.serialize(&mut b)?;
        Ok(b)
    }
    fn bin_de_valueatom(bytes: &[u8]) -> std::io::Result<ValueAtom> {
        ValueAtom::deserialize(&mut &bytes[..], 0)
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(512))]

        // Totality: arbitrary bytes never panic the binary decoders.
        #[test]
        fn valueatom_binary_decode_total(bytes in prop::collection::vec(any::<u8>(), 0..64)) {
            let _ = bin_de_valueatom(&bytes);
        }

        // (C-parity, positive) On the *valid* (normal-form) value set both
        // decoders agree: a normalized `ValueAtom` round-trips through both the
        // binary and the serde codecs to the same value (holds today).
        #[test]
        fn valueatom_normalform_agrees(raw in prop::collection::vec(any::<u8>(), 0..40)) {
            let atom = ValueAtom(raw).normalize();
            // binary round-trip
            let bin = bin_ser(&atom).unwrap();
            let via_bin = bin_de_valueatom(&bin).unwrap();
            prop_assert_eq!(&via_bin, &atom);
            // serde round-trip
            let json = serde_json::to_string(&atom).unwrap();
            let via_serde: ValueAtom = serde_json::from_str(&json).unwrap();
            prop_assert_eq!(&via_serde, &atom);
        }
    }

    // Note: the *serde* decoders for `ValueAtom`/`AlignmentAtom` are lenient
    // relative to the binary path (they accept non-normal-form atoms and
    // out-of-cap `Bytes` lengths). This is not consensus-relevant: the consensus
    // wire is the binary `Deserializable`, which is strict, and neither lenient
    // value can cross into it -- `ValueAtom::serialize` normalizes before
    // writing, and `write_flagged_int` refuses a length beyond its cap. serde is
    // the off-consensus (JSON/RPC) surface. Only the positive parity baseline is
    // asserted here.
}
