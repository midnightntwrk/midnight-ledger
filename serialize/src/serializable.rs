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

use crate::tagged::Tagged;
use std::{
    borrow::{Borrow, Cow},
    collections::{HashMap, HashSet},
    io::Write,
    marker::PhantomData,
    sync::Arc,
};

pub const GLOBAL_TAG: &str = "midnight:";

// Top-level serialization function
pub fn tagged_serialize<T: Serializable + Tagged>(
    value: &T,
    mut writer: impl Write,
) -> std::io::Result<()> {
    let tag = T::tag();
    write!(&mut writer, "{GLOBAL_TAG}{tag}:")?;
    value.serialize(&mut writer)
}

pub fn tagged_serialized_size<T: Serializable + Tagged>(value: &T) -> usize {
    T::tag().len() + GLOBAL_TAG.len() + 1 + T::serialized_size(value)
}

/// Binary serialization with embedded versioning.
///
/// See [`crate::Deserializable`] for the deserialization counterpart.
pub trait Serializable {
    fn serialize(&self, writer: &mut impl Write) -> std::io::Result<()>;
    fn serialized_size(&self) -> usize;
}

impl<T: Serializable> Serializable for Vec<T> {
    fn serialize(&self, writer: &mut impl Write) -> std::io::Result<()> {
        (self.len() as u32).serialize(writer)?;
        for elem in self {
            elem.serialize(writer)?;
        }
        Ok(())
    }
    fn serialized_size(&self) -> usize {
        self.iter()
            .fold((self.len() as u64).serialized_size(), |acc, x| {
                acc + x.serialized_size()
            })
    }
}

impl<K: Serializable + Ord, V: Serializable> Serializable for HashMap<K, V> {
    fn serialize(&self, writer: &mut impl Write) -> std::io::Result<()> {
        (self.len() as u32).serialize(writer)?;
        let mut kvs = self.iter().collect::<Vec<_>>();
        kvs.sort_by(|(k1, _), (k2, _)| k1.cmp(k2));
        for (k, v) in kvs.into_iter() {
            k.serialize(writer)?;
            v.serialize(writer)?;
        }
        Ok(())
    }

    fn serialized_size(&self) -> usize {
        self.iter().fold(4, |acc, (k, v)| {
            acc + k.serialized_size() + v.serialized_size()
        })
    }
}

impl<T: Serializable + Ord> Serializable for HashSet<T> {
    fn serialize(&self, writer: &mut impl Write) -> std::io::Result<()> {
        (self.len() as u32).serialize(writer)?;
        let mut elems = self.iter().collect::<Vec<_>>();
        elems.sort();
        for elem in elems.into_iter() {
            elem.serialize(writer)?;
        }

        Ok(())
    }

    fn serialized_size(&self) -> usize {
        self.iter()
            .fold(4, |acc, elem| acc + elem.serialized_size())
    }
}

impl<'a, T> Serializable for &'a T
where
    T: Serializable + 'a,
    Self: Borrow<T>,
{
    fn serialize(&self, writer: &mut impl Write) -> std::io::Result<()> {
        T::serialize(self, writer)
    }

    fn serialized_size(&self) -> usize {
        T::serialized_size(self)
    }
}

impl<T: Serializable> Serializable for Option<T> {
    fn serialize(&self, writer: &mut impl Write) -> std::io::Result<()> {
        match self {
            Some(v) => {
                1u8.serialize(writer)?;
                v.serialize(writer)?;
                Ok(())
            }
            None => {
                0u8.serialize(writer)?;
                Ok(())
            }
        }
    }

    fn serialized_size(&self) -> usize {
        match self {
            Some(v) => 1 + v.serialized_size(),
            None => 1,
        }
    }
}

impl Serializable for str {
    fn serialize(&self, writer: &mut impl Write) -> std::io::Result<()> {
        (self.len() as u64).serialize(writer)?;
        writer.write_all(self.as_bytes())
    }

    fn serialized_size(&self) -> usize {
        let len = self.len();
        (len as u64).serialized_size() + len
    }
}

impl Serializable for String {
    fn serialize(&self, writer: &mut impl Write) -> std::io::Result<()> {
        str::serialize(&self, writer)
    }

    fn serialized_size(&self) -> usize {
        str::serialized_size(&self)
    }
}

impl Serializable for &str {
    fn serialize(&self, writer: &mut impl Write) -> std::io::Result<()> {
        str::serialize(self, writer)
    }

    fn serialized_size(&self) -> usize {
        str::serialized_size(self)
    }
}

impl<const N: usize> Serializable for [u8; N] {
    fn serialize(&self, writer: &mut impl Write) -> std::io::Result<()> {
        writer.write_all(&self[..])
    }
    fn serialized_size(&self) -> usize {
        N
    }
}

impl<T> Serializable for PhantomData<T> {
    fn serialize(&self, _writer: &mut impl Write) -> std::io::Result<()> {
        Ok(())
    }
    fn serialized_size(&self) -> usize {
        0
    }
}

impl<T: Serializable> Serializable for Box<T> {
    fn serialize(&self, writer: &mut impl Write) -> std::io::Result<()> {
        T::serialize(&self, writer)
    }
    fn serialized_size(&self) -> usize {
        T::serialized_size(&self)
    }
}

impl<T: Serializable> Serializable for Arc<T> {
    fn serialize(&self, writer: &mut impl Write) -> std::io::Result<()> {
        T::serialize(&self, writer)
    }
    fn serialized_size(&self) -> usize {
        T::serialized_size(&self)
    }
}

impl<'a, T: ToOwned + ?Sized> Serializable for Cow<'a, T>
where
    T: Serializable,
{
    fn serialize(&self, writer: &mut impl Write) -> std::io::Result<()> {
        T::serialize(&self, writer)
    }
    fn serialized_size(&self) -> usize {
        T::serialized_size(&self)
    }
}

impl<'a, T: ToOwned + ?Sized + Tagged> Tagged for Cow<'a, T> {
    fn tag() -> Cow<'static, str> {
        T::tag()
    }
    fn tag_unique_factor() -> String {
        T::tag_unique_factor()
    }
}
