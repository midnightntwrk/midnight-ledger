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

//! Descriptions of exactly which byte strings decode as a type, as finite
//! automata, so that many encodings sharing structure can be checked without
//! enumerating them.

use std::fmt::Debug;
use std::hash::Hash;

/// The byte strings that [`crate::Deserializable::deserialize`] reads in full, as a
/// finite automaton over bytes.
///
/// Encodings must be prefix-free, as self-delimiting encodings are: no
/// accepted string continues into another. Checking a set of encodings costs
/// time proportional to the number of distinct states reached, so `State`
/// should take few values.
pub trait EncodingShape {
    /// A position in the automaton.
    type State: Clone + Eq + Hash + Debug + 'static;
    /// The state before any byte is read.
    fn start() -> Self::State;
    /// The state after reading `byte`, or `None` if no encoding continues so.
    fn step(state: &Self::State, byte: u8) -> Option<Self::State>;
    /// Whether the bytes read so far are a complete encoding.
    fn accepts(state: &Self::State) -> bool;
}

/// Whether `T`'s shape accepts `bytes`.
#[cfg(any(test, feature = "proptest"))]
fn shape_accepts<T: EncodingShape>(bytes: &[u8]) -> bool {
    bytes
        .iter()
        .try_fold(T::start(), |state, b| T::step(&state, *b))
        .is_some_and(|state| T::accepts(&state))
}

/// Implements [`EncodingShape`] for a type by delegating to a shape type.
#[macro_export]
macro_rules! encoding_shape_as {
    ($ty:ty, $shape:ty) => {
        impl $crate::EncodingShape for $ty {
            type State = <$shape as $crate::EncodingShape>::State;
            fn start() -> Self::State {
                <$shape as $crate::EncodingShape>::start()
            }
            fn step(state: &Self::State, byte: u8) -> Option<Self::State> {
                <$shape as $crate::EncodingShape>::step(state, byte)
            }
            fn accepts(state: &Self::State) -> bool {
                <$shape as $crate::EncodingShape>::accepts(state)
            }
        }
    };
}

impl<const N: usize> EncodingShape for [u8; N] {
    type State = usize;
    fn start() -> usize {
        0
    }
    fn step(read: &usize, _: u8) -> Option<usize> {
        (*read < N).then_some(read + 1)
    }
    fn accepts(read: &usize) -> bool {
        *read == N
    }
}

impl EncodingShape for bool {
    type State = bool;
    fn start() -> bool {
        false
    }
    fn step(done: &bool, byte: u8) -> Option<bool> {
        (!done && byte <= 1).then_some(true)
    }
    fn accepts(done: &bool) -> bool {
        *done
    }
}

/// A position in a pair's shape.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum SeqState<A, B> {
    /// Inside the first encoding.
    First(A),
    /// Inside the second encoding.
    Second(B),
}

// Prefix-freedom makes the switch to `B` unambiguous.
fn seq_normalize<A: EncodingShape, B: EncodingShape>(
    state: SeqState<A::State, B::State>,
) -> SeqState<A::State, B::State> {
    match state {
        SeqState::First(a) if A::accepts(&a) => SeqState::Second(B::start()),
        other => other,
    }
}

impl<A: EncodingShape, B: EncodingShape> EncodingShape for (A, B) {
    type State = SeqState<A::State, B::State>;
    fn start() -> Self::State {
        seq_normalize::<A, B>(SeqState::First(A::start()))
    }
    fn step(state: &Self::State, byte: u8) -> Option<Self::State> {
        Some(seq_normalize::<A, B>(match state {
            SeqState::First(a) => SeqState::First(A::step(a, byte)?),
            SeqState::Second(b) => SeqState::Second(B::step(b, byte)?),
        }))
    }
    fn accepts(state: &Self::State) -> bool {
        matches!(state, SeqState::Second(b) if B::accepts(b))
    }
}

/// A position in the shape of a canonical SCALE compact integer.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum ScaleState {
    /// Before the first byte.
    Start,
    /// A complete encoding.
    Done,
    /// Two-byte mode, before its nonzero second byte.
    TwoByte,
    /// Four-byte mode, with this many bytes left, and whether the third byte
    /// was nonzero.
    FourByte(u8, bool),
    /// Big-integer mode, with this many of its `n` bytes left.
    NByte { left: u8, n: u8 },
}

/// One step of a canonical SCALE compact integer whose value fits in `M`
/// bytes, as `u32`, `u64` and `u128` encode.
fn scale_step<const M: usize>(state: &ScaleState, byte: u8) -> Option<ScaleState> {
    use ScaleState::*;
    match *state {
        Start => match byte & 0b11 {
            0b00 => Some(Done),
            0b01 => Some(TwoByte),
            0b10 if M >= 4 => Some(FourByte(3, false)),
            0b11 => {
                // Big-integer mode is n = 4 + (byte >> 2) bytes, which must fit in `M`.
                let n = (byte >> 2) + 4;
                (n as usize <= M).then_some(NByte { left: n, n })
            }
            _ => None,
        },
        Done => None,
        TwoByte => (byte != 0).then_some(Done),
        FourByte(3, _) => Some(FourByte(2, false)),
        FourByte(2, _) => Some(FourByte(1, byte != 0)),
        FourByte(_, third_nonzero) => (third_nonzero || byte != 0).then_some(Done),
        // Canonical: no zero top byte, and 4-byte big integers only above
        // 2^30, which the shorter modes cover.
        NByte { left: 1, n } => (byte != 0 && (n != 4 || byte >= 64)).then_some(Done),
        NByte { left, n } => Some(NByte { left: left - 1, n }),
    }
}

macro_rules! scale_shape {
    ($ty:ty, $m:literal) => {
        impl EncodingShape for $ty {
            type State = ScaleState;
            fn start() -> ScaleState {
                ScaleState::Start
            }
            fn step(state: &ScaleState, byte: u8) -> Option<ScaleState> {
                scale_step::<$m>(state, byte)
            }
            fn accepts(state: &ScaleState) -> bool {
                *state == ScaleState::Done
            }
        }
    };
}

scale_shape!(u32, 4);
scale_shape!(u64, 8);
scale_shape!(u128, 16);

encoding_shape_as!((), [u8; 0]);
encoding_shape_as!(u8, [u8; 1]);
encoding_shape_as!(u16, [u8; 2]);
encoding_shape_as!(i8, [u8; 1]);
encoding_shape_as!(i16, [u8; 2]);
encoding_shape_as!(i32, [u8; 4]);
encoding_shape_as!(i64, [u8; 8]);
encoding_shape_as!(i128, [u8; 16]);

/// Checks that `T`'s shape accepts exactly the byte strings `T` decodes from
/// in full, over encodings of arbitrary values and corruptions of them.
#[cfg(feature = "proptest")]
pub fn check_encoding_shape<T>()
where
    T: EncodingShape + crate::Serializable + crate::Deserializable + proptest::arbitrary::Arbitrary,
{
    use proptest::prelude::*;
    use proptest::test_runner::{Config, TestRunner};

    let decodes = |bytes: &[u8]| {
        let mut reader = bytes;
        T::deserialize(&mut reader, 0).is_ok() && reader.is_empty()
    };
    let encoding = || {
        any::<T>().prop_map(|v| {
            let mut bytes = Vec::new();
            v.serialize(&mut bytes)
                .expect("in-memory serialization succeeds");
            bytes
        })
    };
    let corrupted = (
        encoding(),
        any::<prop::sample::Index>(),
        any::<u8>(),
        0u8..4,
    )
        .prop_map(|(mut bytes, i, b, how): (Vec<u8>, _, u8, u8)| {
            match how {
                0 if !bytes.is_empty() => bytes.truncate(i.index(bytes.len())),
                1 => bytes.push(b),
                2 if !bytes.is_empty() => {
                    let i = i.index(bytes.len());
                    bytes[i] = b;
                }
                _ => bytes.insert(0, b),
            }
            bytes
        });
    let short = prop::collection::vec(any::<u8>(), 0..12);
    let strategy = prop_oneof![encoding(), corrupted, short];
    TestRunner::new(Config::with_cases(4096))
        .run(&strategy, |bytes: Vec<u8>| {
            prop_assert_eq!(
                shape_accepts::<T>(&bytes),
                decodes(&bytes),
                "{:02x?}",
                bytes
            );
            Ok(())
        })
        .unwrap_or_else(|e| {
            panic!(
                "{} shape disagrees with decoding: {e}",
                std::any::type_name::<T>()
            )
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Deserializable, Serializable};

    #[test]
    fn primitive_shapes_match_decoding() {
        check_encoding_shape::<()>();
        check_encoding_shape::<bool>();
        check_encoding_shape::<u8>();
        check_encoding_shape::<u16>();
        check_encoding_shape::<i32>();
        check_encoding_shape::<i128>();
        check_encoding_shape::<u32>();
        check_encoding_shape::<u64>();
        check_encoding_shape::<u128>();
        check_encoding_shape::<[u8; 3]>();
        check_encoding_shape::<(u64, bool)>();
        check_encoding_shape::<(u8, u32)>();
    }

    // Arbitrary integers are mostly large, so cover each SCALE mode's
    // boundaries explicitly.
    #[test]
    fn scale_shapes_match_decoding_at_mode_boundaries() {
        fn check<T: EncodingShape + Serializable + Deserializable + TryFrom<u128>>(m: u32) {
            for bits in 0..(8 * m) {
                for v in [
                    (1u128 << bits).wrapping_sub(1),
                    1u128 << bits,
                    (1u128 << bits) + 1,
                ] {
                    let Ok(v) = T::try_from(v) else { continue };
                    let mut bytes = Vec::new();
                    v.serialize(&mut bytes).unwrap();
                    assert!(shape_accepts::<T>(&bytes), "{bytes:02x?}");
                    for i in 0..bytes.len() {
                        assert!(!shape_accepts::<T>(&bytes[..i]), "{bytes:02x?}");
                    }
                }
            }
        }
        check::<u32>(4);
        check::<u64>(8);
        check::<u128>(16);
    }
}
