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
        .ok_or_else(|| err("tagged data does not begin with a colon-separated tag".to_string()))?
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
    fn deserialize(reader: &mut impl Read, mut recursion_depth: u32) -> std::io::Result<Self> {
        Self::check_rec(&mut recursion_depth)?;
        <T::Owned>::deserialize(reader, recursion_depth).map(Cow::Owned)
    }
}

// ---------------------------------------------------------------------------
// Area F: canonicity / injectivity of `HashMap` / `HashSet` `Deserializable`.
//
// The `Serializable` impls (see serializable.rs) emit a *canonical* wire form:
// the length prefix followed by entries sorted ascending by key/element, with
// no duplicates (a map/set cannot contain duplicate keys/elements). The
// `Deserializable` impls above, however, read `len` entries and blindly
// `insert` them into a fresh `HashMap`/`HashSet` with no ordering check and no
// duplicate-key rejection. As a consequence:
//   * any permutation of the entries decodes to the same value, and
//   * an encoding whose `len` disagrees with the number of distinct keys
//     (duplicate keys) is silently accepted, decoding to a *smaller* value.
// Both break the "single canonical wire form" property the sorted encoder is
// clearly trying to establish.
// ---------------------------------------------------------------------------
#[cfg(all(test, feature = "proptest"))]
mod canonicity_props {
    use crate::{Deserializable, Serializable};
    use proptest::prelude::*;
    use std::collections::{HashMap, HashSet};

    /// Build the wire form the `HashMap`/`HashSet` decoders read: a `u32`
    /// length prefix followed by each entry, in the *given* order (no sorting).
    fn map_wire(entries: &[(u8, u8)]) -> Vec<u8> {
        let mut bytes = Vec::new();
        (entries.len() as u32).serialize(&mut bytes).unwrap();
        for (k, v) in entries {
            k.serialize(&mut bytes).unwrap();
            v.serialize(&mut bytes).unwrap();
        }
        bytes
    }

    fn set_wire(elems: &[u8]) -> Vec<u8> {
        let mut bytes = Vec::new();
        (elems.len() as u32).serialize(&mut bytes).unwrap();
        for e in elems {
            e.serialize(&mut bytes).unwrap();
        }
        bytes
    }

    fn decode_map(bytes: &[u8]) -> std::io::Result<HashMap<u8, u8>> {
        HashMap::deserialize(&mut &bytes[..], 0)
    }
    fn decode_set(bytes: &[u8]) -> std::io::Result<HashSet<u8>> {
        HashSet::deserialize(&mut &bytes[..], 0)
    }

    proptest! {
        // Totality: decoders never panic on arbitrary bytes.
        #[test]
        fn map_decode_total(bytes in prop::collection::vec(any::<u8>(), 0..64)) {
            let _ = decode_map(&bytes);
        }
        #[test]
        fn set_decode_total(bytes in prop::collection::vec(any::<u8>(), 0..64)) {
            let _ = decode_set(&bytes);
        }
    }

    // ===================================================================
    // Canonical-decode regressions (minimized): the decoder rejects unsorted
    // order and duplicate keys/elements (strict-ascending normal form).
    // ===================================================================

    #[test]
    fn map_rejects_unsorted_order() {
        // Two distinct keys in descending order -- not the canonical (ascending)
        // encoding.
        let unsorted = map_wire(&[(1, 0), (0, 0)]);
        assert!(
            decode_map(&unsorted).is_err(),
            "unsorted map encoding must be rejected"
        );
    }

    #[test]
    fn map_rejects_duplicate_keys() {
        // len says 2 but both entries share key 0.
        let dup = map_wire(&[(0, 1), (0, 2)]);
        assert!(
            decode_map(&dup).is_err(),
            "duplicate-key map encoding must be rejected (got {:?})",
            decode_map(&dup)
        );
    }

    #[test]
    fn set_rejects_unsorted_order() {
        let unsorted = set_wire(&[1, 0]);
        assert!(
            decode_set(&unsorted).is_err(),
            "unsorted set encoding must be rejected"
        );
    }

    #[test]
    fn set_rejects_duplicate_elems() {
        let dup = set_wire(&[0, 0]);
        assert!(
            decode_set(&dup).is_err(),
            "duplicate-element set encoding must be rejected (got {:?})",
            decode_set(&dup)
        );
    }
}

// ---------------------------------------------------------------------------
// Area A: totality of `Vec::<T>::deserialize`, including zero-sized element
// types.
//
// `Vec::<T>::deserialize` (deserializable.rs:157) reads a `u32` length prefix
// and then pre-allocates via `Vec::with_bounded_capacity(len as usize)`
// (util.rs:36). That helper computes
//     let alloc_limit = MEMORY_LIMIT / std::mem::size_of::<T>();
// (util.rs:39). For a zero-sized element type such as `()`,
// `size_of::<T>() == 0`, so this is an **integer divide-by-zero** and the
// decoder *panics* before it can return either `Ok` or `Err` — violating the
// totality property (a decoder must never panic on any declared length).
//
// The panic fires for *every* `Vec<()>` decode with a validly-decoded length
// prefix, including the empty vector (`len == 0`), because the division is
// evaluated unconditionally regardless of `n`.
// ---------------------------------------------------------------------------
#[cfg(all(test, feature = "proptest"))]
mod vec_totality_props {
    use crate::{Deserializable, Serializable};
    use proptest::prelude::*;

    fn decode<T: Deserializable>(bytes: &[u8]) -> std::io::Result<Vec<T>> {
        Vec::<T>::deserialize(&mut &bytes[..], 0)
    }

    /// The single canonical `u32` length prefix `0`, in the crate's SCALE
    /// varint form (a lone `0x00` byte). This is the smallest input that
    /// decodes a length successfully, i.e. the smallest input that reaches the
    /// `with_bounded_capacity` allocation.
    fn len_prefix(n: u32) -> Vec<u8> {
        let mut b = Vec::new();
        n.serialize(&mut b).unwrap();
        b
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(512))]

        // (A-tot) Non-ZST element types: decode is total on arbitrary bytes and
        // arbitrary declared length prefixes (these hold today — the bounded
        // allocation caps memory and a short buffer just yields an EOF `Err`).
        #[test]
        fn vec_u8_decode_total(bytes in prop::collection::vec(any::<u8>(), 0..128)) {
            let _ = decode::<u8>(&bytes);
        }
        #[test]
        fn vec_u32_decode_total(bytes in prop::collection::vec(any::<u8>(), 0..128)) {
            let _ = decode::<u32>(&bytes);
        }
        #[test]
        fn vec_pair_decode_total(bytes in prop::collection::vec(any::<u8>(), 0..128)) {
            let _ = decode::<(u8, u8)>(&bytes);
        }
        // A huge declared length must not OOM or panic: `with_bounded_capacity`
        // caps the pre-allocation, and the element loop hits EOF and returns
        // `Err` almost immediately.
        #[test]
        fn vec_u8_huge_len_is_err(extra in prop::collection::vec(any::<u8>(), 0..8)) {
            let mut bytes = len_prefix(u32::MAX);
            bytes.extend_from_slice(&extra);
            // Never panics; a length of ~4e9 with too few bytes is an error.
            prop_assert!(decode::<u8>(&bytes).is_err());
        }

        // A zero-sized element type must not divide-by-zero on the capacity
        // bound; a declared length of `n` decodes to `n` units.
        #[test]
        fn vec_zst_decodes_to_declared_length(n in 0u32..=8) {
            let decoded = decode::<()>(&len_prefix(n));
            prop_assert!(decoded.is_ok());
            prop_assert_eq!(decoded.unwrap().len(), n as usize);
        }
    }
}

// ---------------------------------------------------------------------------
// Area D: the recursion-depth guard must bound decoding of *any* recursive
// type, regardless of which smart pointer forms the recursive edge.
//
// `Deserializable::check_rec` (deserializable.rs:142) increments and range-
// checks the depth counter against `RECURSION_LIMIT`. Container decoders that
// can form the recursive edge of a type are expected to invoke it. `Arc`
// (deserializable.rs:214) *does* call `check_rec` before recursing; but `Box`
// (deserializable.rs:238) and `Cow` (deserializable.rs:244) do NOT — they
// simply forward the *unchanged* depth to the inner `deserialize`. Any
// recursive type whose recursive edge is a `Box` or `Cow` therefore decodes
// with an unbounded depth counter, so a crafted deeply-nested encoding recurses
// without limit and overflows the stack instead of being rejected — while the
// identical shape built with `Arc` is correctly rejected at `RECURSION_LIMIT`.
//
// The three chain types below are deliberately identical except for the smart
// pointer on the recursive edge, and each type's own `deserialize` intentionally
// delegates depth accounting to the pointer (as a real recursive `Deserializable`
// impl relies on its fields' decoders to advance the counter). This isolates the
// pointer as the sole difference.
// ---------------------------------------------------------------------------
#[cfg(all(test, feature = "proptest"))]
mod recursion_depth_props {
    use super::RECURSION_LIMIT;
    use crate::Deserializable;
    use proptest::prelude::*;
    use std::borrow::Cow;
    use std::io::Read;
    use std::sync::Arc;

    // Recursive edge = Arc (calls check_rec -> bounded).
    #[derive(Clone, Debug)]
    enum ArcChain {
        Leaf,
        Link(#[allow(dead_code)] Arc<ArcChain>),
    }
    impl Deserializable for ArcChain {
        fn deserialize(reader: &mut impl Read, depth: u32) -> std::io::Result<Self> {
            // Depth accounting is delegated to the recursive edge's decoder.
            match u8::deserialize(reader, depth)? {
                0 => Ok(ArcChain::Leaf),
                _ => Ok(ArcChain::Link(<Arc<ArcChain>>::deserialize(reader, depth)?)),
            }
        }
    }

    // Recursive edge = Box (does NOT call check_rec -> unbounded).
    #[derive(Clone, Debug)]
    enum BoxChain {
        Leaf,
        Link(#[allow(dead_code)] Box<BoxChain>),
    }
    impl Deserializable for BoxChain {
        fn deserialize(reader: &mut impl Read, depth: u32) -> std::io::Result<Self> {
            match u8::deserialize(reader, depth)? {
                0 => Ok(BoxChain::Leaf),
                _ => Ok(BoxChain::Link(<Box<BoxChain>>::deserialize(reader, depth)?)),
            }
        }
    }

    // A leaf decoder that records the `recursion_depth` it was invoked with.
    // Wrapping it in each smart pointer reveals whether that pointer advanced
    // the depth counter before delegating. (`Cow<'static, DepthProbe>` is a
    // *non-recursive* Cow, so it sidesteps the infinite-size cycle that a
    // self-referential Cow edge would create — a Cow can only introduce heap
    // indirection through an unsized target like `[T]`/`str`, whose `Owned`
    // decoder is `Vec`/`String` and *does* call `check_rec`.)
    #[derive(Clone, Debug, PartialEq)]
    struct DepthProbe(u32);
    impl Deserializable for DepthProbe {
        fn deserialize(reader: &mut impl Read, depth: u32) -> std::io::Result<Self> {
            let _ = u8::deserialize(reader, depth)?; // consume a byte for parity
            Ok(DepthProbe(depth))
        }
    }

    fn depth_seen_via_arc() -> u32 {
        <Arc<DepthProbe>>::deserialize(&mut &[0u8][..], 0).unwrap().0
    }
    fn depth_seen_via_box() -> u32 {
        <Box<DepthProbe>>::deserialize(&mut &[0u8][..], 0).unwrap().0
    }
    fn depth_seen_via_cow() -> u32 {
        <Cow<'static, DepthProbe>>::deserialize(&mut &[0u8][..], 0)
            .unwrap()
            .into_owned()
            .0
    }

    /// Wire form of a chain: `links` "Link" tags (`0x01`) followed by a single
    /// "Leaf" terminator (`0x00`).
    fn nested(links: u32) -> Vec<u8> {
        let mut b = vec![1u8; links as usize];
        b.push(0u8);
        b
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]

        // (D-holds) The Arc-edged chain IS bounded: any nesting deeper than
        // RECURSION_LIMIT is rejected with an error (never overflows). HOLDS.
        #[test]
        fn arc_chain_is_bounded(extra in 0u32..64) {
            let bytes = nested(RECURSION_LIMIT + 1 + extra);
            prop_assert!(
                ArcChain::deserialize(&mut &bytes[..], 0).is_err(),
                "Arc-edged recursion must be capped at RECURSION_LIMIT"
            );
        }

        // (D-holds) Shallow nesting (within the limit) decodes fine for both
        // pointer variants — the guard doesn't reject legitimate inputs. HOLDS.
        #[test]
        fn shallow_chains_decode_ok(links in 0u32..(RECURSION_LIMIT / 2)) {
            let bytes = nested(links);
            prop_assert!(ArcChain::deserialize(&mut &bytes[..], 0).is_ok());
            prop_assert!(BoxChain::deserialize(&mut &bytes[..], 0).is_ok());
        }
    }

    /// (D-holds) Root cause, directly observed: `Arc::deserialize` advances the
    /// recursion-depth counter before delegating (inner sees depth `1`). HOLDS.
    #[test]
    fn arc_advances_recursion_depth() {
        assert_eq!(depth_seen_via_arc(), 1);
    }

    /// `Box::deserialize` must advance the recursion-depth counter like `Arc`
    /// (inner sees depth `1`, not `0`), so a `Box` edge contributes to the
    /// recursion bound.
    #[test]
    fn box_advances_recursion_depth() {
        assert_eq!(
            depth_seen_via_box(),
            depth_seen_via_arc(),
            "Box must advance the recursion-depth counter like Arc"
        );
    }

    /// `Cow::deserialize` must likewise advance the depth counter.
    #[test]
    fn cow_advances_recursion_depth() {
        assert_eq!(
            depth_seen_via_cow(),
            depth_seen_via_arc(),
            "Cow must advance the recursion-depth counter like Arc"
        );
    }

    /// A `Box`-edged recursive type must respect the recursion limit: a chain
    /// nested `RECURSION_LIMIT + 1` deep — one level past where the identical
    /// `Arc`-edged type is rejected — must decode to an error, since the same
    /// unbounded recursion at a genuinely adversarial length (millions of
    /// `0x01` bytes) overflows the stack. The crafted encoding is only modestly
    /// deep here to keep the test binary alive for a clean assertion failure.
    #[test]
    fn box_chain_respects_recursion_limit() {
        let bytes = nested(RECURSION_LIMIT + 1);
        assert!(
            BoxChain::deserialize(&mut &bytes[..], 0).is_err(),
            "Box-edged recursion must be capped at RECURSION_LIMIT like Arc"
        );
    }

}
