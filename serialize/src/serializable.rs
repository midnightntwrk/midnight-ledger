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
        kvs.sort_by_key(|(k1, _)| *k1);
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
        str::serialize(self, writer)
    }

    fn serialized_size(&self) -> usize {
        str::serialized_size(self)
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

impl<T: ?Sized> Serializable for PhantomData<T> {
    fn serialize(&self, _writer: &mut impl Write) -> std::io::Result<()> {
        Ok(())
    }
    fn serialized_size(&self) -> usize {
        0
    }
}

impl<T: ?Sized> Tagged for PhantomData<T> {
    fn tag() -> Cow<'static, str> {
        Cow::Borrowed("()")
    }
    fn tag_unique_factor() -> String {
        "()".into()
    }
}

impl<T: Serializable> Serializable for Box<T> {
    fn serialize(&self, writer: &mut impl Write) -> std::io::Result<()> {
        T::serialize(self, writer)
    }
    fn serialized_size(&self) -> usize {
        T::serialized_size(self)
    }
}

impl<T: Serializable> Serializable for Arc<T> {
    fn serialize(&self, writer: &mut impl Write) -> std::io::Result<()> {
        T::serialize(self, writer)
    }
    fn serialized_size(&self) -> usize {
        T::serialized_size(self)
    }
}

impl<'a, T: ToOwned + ?Sized> Serializable for Cow<'a, T>
where
    T: Serializable,
{
    fn serialize(&self, writer: &mut impl Write) -> std::io::Result<()> {
        T::serialize(self, writer)
    }
    fn serialized_size(&self) -> usize {
        T::serialized_size(self)
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

// ---------------------------------------------------------------------------
// Area C: `serialized_size()` must equal the number of bytes `serialize`
// actually writes, for any `HashMap` / `HashSet`.
//
// The length prefix is written as a `u32` via the crate's SCALE varint
// (`via_scale!(u32, 4)` in util.rs:209; the encoded width is 1/2/4/(n+1) bytes
// depending on the *value*, see `ScaleBigInt::serialize`/`serialized_size` at
// util.rs:227/251). `Vec::serialized_size` (serializable.rs:57) correctly
// accounts for this by computing `(self.len() as u64).serialized_size()`.
// However, `HashMap::serialized_size` (serializable.rs:76) and
// `HashSet::serialized_size` (serializable.rs:94) both start their fold at the
// hard-coded constant `4`, as if the prefix were a fixed 4-byte integer. It is
// not: for any map/set whose `len` encodes to fewer than 4 bytes (i.e. every
// map/set with `len < 2^14`, and in particular the *empty* map/set) the
// reported size *overestimates* the bytes `serialize` writes. This breaks the
// `serialized_size == bytes-written` invariant that the existing
// `randomised_serialization_test!` oracle (util.rs:662) checks for other
// types.
// ---------------------------------------------------------------------------
#[cfg(all(test, feature = "proptest"))]
mod map_set_size_props {
    use crate::Serializable;
    use proptest::prelude::*;

    /// Reusable oracle mirroring `randomised_serialization_test!`'s
    /// `proptest_serialized_size_*`: the number of bytes written by `serialize`
    /// must equal `serialized_size()`.
    fn size_matches<T: Serializable>(v: &T) -> Result<(), TestCaseError> {
        let mut bytes = Vec::new();
        v.serialize(&mut bytes).unwrap();
        prop_assert_eq!(
            bytes.len(),
            v.serialized_size(),
            "serialize wrote {} bytes but serialized_size() reported {}",
            bytes.len(),
            v.serialized_size()
        );
        Ok(())
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        // (C) HashMap: reported size must equal bytes written.
        #[test]
        fn hashmap_size_matches(m in prop::collection::hash_map(any::<u8>(), any::<u16>(), 0..40)) {
            size_matches(&m)?;
        }

        // (C) HashSet: reported size must equal bytes written.
        #[test]
        fn hashset_size_matches(s in prop::collection::hash_set(any::<u16>(), 0..40)) {
            size_matches(&s)?;
        }

        // The equivalent `Vec` property, as a cross-check on the oracle itself.
        #[test]
        fn vec_size_matches(v in prop::collection::vec(any::<u16>(), 0..40)) {
            size_matches(&v)?;
        }
    }
}
