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

use crate::VecExt;
use crate::serializable::GLOBAL_TAG;
use crate::tagged::Tagged;
use std::borrow::Cow;
use std::io::{self, BufRead, Read, Seek};
use std::marker::PhantomData;
use std::sync::Arc;
use std::{collections::HashMap, collections::HashSet, hash::Hash};

#[cfg(debug_assertions)]
pub const RECURSION_LIMIT: u32 = 50;
#[cfg(not(debug_assertions))]
pub const RECURSION_LIMIT: u32 = 250;

// Top-level deserialization function
pub fn tagged_deserialize<T: Deserializable + Tagged>(reader: impl Read) -> std::io::Result<T> {
    tagged_deserialize_inner(reader, true)
}

pub fn tagged_deserialize_sequence<T: Deserializable + Tagged>(
    mut reader: impl BufRead,
) -> std::io::Result<Vec<T>> {
    let mut res = vec![];
    while !reader.fill_buf()?.is_empty() {
        res.push(tagged_deserialize_inner(&mut reader, false)?);
    }
    Ok(res)
}

/// Attempts to identify the tag a stream starts with without consuming it, allowing determining a
/// stream's type *before* deserializing it.
pub fn peek_tag(reader: &mut (impl Read + Seek)) -> std::io::Result<String> {
    let position = reader.stream_position()?;
    // Note that colons are special-cased -- we should expect two, one for `GLOBAL_TAG`, and one
    // for the end of the read tag. We read up to a limit of 512 bytes, and then take up to the
    // second b':', converting to string, returning an error if not possible.
    const READ_LIMIT: usize = 512;
    let mut buf = [0u8; READ_LIMIT];
    let mut offset = 0;
    while offset < READ_LIMIT {
        let read = reader.read(&mut buf[offset..])?;
        if read == 0 {
            break;
        }
        offset += read;
    }
    reader.seek(std::io::SeekFrom::Start(position))?;
    let err = |msg| io::Error::new(io::ErrorKind::InvalidData, msg);
    if !buf.starts_with(GLOBAL_TAG.as_bytes()) {
        return Err(err(format!(
            "tagged data does not begin with '{GLOBAL_TAG}'"
        )));
    }
    let second_colon = buf
        .iter()
        .enumerate()
        .filter(|(_, b)| **b == b':')
        .nth(1)
        .ok_or_else(|| {
            err(format!(
                "tagged data does not begin with a colon-separated tag"
            ))
        })?
        .0;
    let raw_tag = &buf[GLOBAL_TAG.len()..second_colon];
    String::from_utf8(raw_tag.to_owned())
        .map_err(|e| err(format!("tag not utf-8: {e}")))
        .map(|s| {
            s.replace(
                |c: char| -> bool { !c.is_ascii_alphanumeric() && !":_-()[],".contains(c) },
                "�",
            )
        })
}

fn tagged_deserialize_inner<T: Deserializable + Tagged>(
    mut reader: impl Read,
    ensure_consumed: bool,
) -> std::io::Result<T> {
    let tag_expected = format!("{GLOBAL_TAG}{}:", T::tag());
    let mut read_tag = vec![0u8; tag_expected.len()];
    let mut remaining_tag_buf = &mut read_tag[..];
    while !remaining_tag_buf.is_empty() {
        let read = reader.read(remaining_tag_buf)?;
        if read == 0 {
            let rem = remaining_tag_buf.len();
            let len = read_tag.len() - rem;
            read_tag.truncate(len);
            break;
        }
        remaining_tag_buf = &mut remaining_tag_buf[read..];
    }
    if read_tag != tag_expected.as_bytes() {
        let sanitised = String::from_utf8_lossy(&read_tag).replace(
            |c: char| -> bool { !c.is_ascii_alphanumeric() && !":_-()[],".contains(c) },
            "�",
        );
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("expected header tag '{tag_expected}', got '{sanitised}'"),
        ));
    }
    let value = <T as Deserializable>::deserialize(&mut reader, 0)?;

    if !ensure_consumed {
        return Ok(value);
    }

    #[allow(clippy::unbuffered_bytes)] // we can permit a potentally inefficient count here, as in
    let count = reader.bytes().count(); // the happy path it should be 0

    if count == 0 {
        return Ok(value);
    }

    Err(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        format!(
            "Not all bytes read deserializing '{}'; {} bytes remaining",
            tag_expected, count
        ),
    ))
}

pub trait Deserializable
where
    Self: Sized,
{
    const LIMIT_RECURSION: bool = true;

    fn deserialize(reader: &mut impl Read, recursion_depth: u32) -> std::io::Result<Self>;

    fn check_rec(depth: &mut u32) -> std::io::Result<()> {
        if Self::LIMIT_RECURSION {
            *depth += 1;
            if *depth > RECURSION_LIMIT {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "exceeded recursion depth deserializing",
                ));
            }
        }
        Ok(())
    }
}

impl<T: Deserializable> Deserializable for Vec<T> {
    fn deserialize(reader: &mut impl Read, mut recursion_depth: u32) -> std::io::Result<Self> {
        Self::check_rec(&mut recursion_depth)?;
        let len = <u32 as Deserializable>::deserialize(reader, recursion_depth)?;
        let mut result = Vec::with_bounded_capacity(len as usize);
        for _ in 0..len {
            result.push(<T as Deserializable>::deserialize(reader, recursion_depth)?);
        }
        Ok(result)
    }
}

impl<K: Deserializable + PartialOrd + Hash + Eq, V: Deserializable> Deserializable
    for HashMap<K, V>
{
    fn deserialize(reader: &mut impl Read, mut recursion_depth: u32) -> std::io::Result<Self> {
        Self::check_rec(&mut recursion_depth)?;
        let len = <u32 as Deserializable>::deserialize(reader, recursion_depth)?;
        let mut result = HashMap::new();
        for _ in 0..len {
            let k = <K as Deserializable>::deserialize(reader, recursion_depth)?;
            let v = <V as Deserializable>::deserialize(reader, recursion_depth)?;
            result.insert(k, v);
        }
        Ok(result)
    }
}

impl<T: Deserializable + Hash + Eq> Deserializable for HashSet<T> {
    fn deserialize(reader: &mut impl Read, mut recursion_depth: u32) -> std::io::Result<Self> {
        Self::check_rec(&mut recursion_depth)?;
        let len = <u32 as Deserializable>::deserialize(reader, recursion_depth)?;
        let mut result = HashSet::new();
        for _ in 0..len {
            result.insert(<T as Deserializable>::deserialize(reader, recursion_depth)?);
        }
        Ok(result)
    }
}

impl<T: Deserializable> Deserializable for Option<T> {
    fn deserialize(reader: &mut impl Read, mut recursion_depth: u32) -> std::io::Result<Self> {
        Self::check_rec(&mut recursion_depth)?;
        let some = <u8 as Deserializable>::deserialize(reader, recursion_depth)?;
        match some {
            0 => Ok(None),
            1 => Ok(Some(<T as Deserializable>::deserialize(
                reader,
                recursion_depth,
            )?)),
            _ => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Invalid discriminant: {}.", some),
            )),
        }
    }
}

impl<T: Deserializable> Deserializable for Arc<T> {
    fn deserialize(
        reader: &mut impl Read,
        mut recursion_depth: u32,
    ) -> Result<Self, std::io::Error> {
        Self::check_rec(&mut recursion_depth)?;
        Ok(Arc::new(T::deserialize(reader, recursion_depth)?))
    }
}

impl<const N: usize> Deserializable for [u8; N] {
    fn deserialize(reader: &mut impl Read, _recursion_depth: u32) -> std::io::Result<Self> {
        let mut res = [0u8; N];
        reader.read_exact(&mut res[..])?;
        Ok(res)
    }
}

impl<T: ?Sized> Deserializable for PhantomData<T> {
    fn deserialize(_reader: &mut impl Read, _recursion_depth: u32) -> std::io::Result<Self> {
        Ok(PhantomData)
    }
}

impl<T: Deserializable> Deserializable for Box<T> {
    fn deserialize(reader: &mut impl Read, recursion_depth: u32) -> std::io::Result<Self> {
        T::deserialize(reader, recursion_depth).map(Box::new)
    }
}

impl<'a, T: ToOwned + ?Sized> Deserializable for Cow<'a, T>
where
    T::Owned: Deserializable,
{
    fn deserialize(reader: &mut impl Read, recursion_depth: u32) -> std::io::Result<Self> {
        <T::Owned>::deserialize(reader, recursion_depth).map(Cow::Owned)
    }
}
