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

//! This module defines traits for rust types that are *aligned* in the sense of
//! field-aligned binary values.

use crate::fab::{AlignedValue, AlignedValueSlice, Alignment, AlignmentAtom};
use crate::hash::{HashOutput, PERSISTENT_HASH_BYTES as PHB};

/// A type that has a static alignment, that is, one which is shared by all
/// members of the type.
pub trait Aligned {
    /// Returns the alignment of this rust type.
    fn alignment() -> Alignment;
}

/// A type that has a dynamic alignment, that is, an alignment that depends on
/// the instance of this type presented.
pub trait DynAligned {
    /// Returns the alignment of this instance the given type.
    fn dyn_alignment(&self) -> Alignment;
}

impl<T: Aligned> DynAligned for T {
    fn dyn_alignment(&self) -> Alignment {
        T::alignment()
    }
}

macro_rules! tuple_aligned {
    () => {
        impl Aligned for () {
            fn alignment() -> Alignment {
                Alignment(Vec::new())
            }
        }
    };
    ($a:ident$(, $as:ident)*) => {
        impl<$a: Aligned$(, $as: Aligned)*> Aligned for ($a$(, $as)* ,) {
            fn alignment() -> Alignment {
                Alignment::concat([&$a::alignment()$(, &$as::alignment())*])
            }
        }

        tuple_aligned!($($as),*);
    }
}

tuple_aligned!(A, B, C, D, E, F, G, H, I, J, K);

macro_rules! fixed_bytes_aligned {
    ($($ty:ty; $size:expr_2021),*) => {
        $(
            impl Aligned for $ty {
                fn alignment() -> Alignment {
                    Alignment::singleton(AlignmentAtom::Bytes { length: $size as u32 })
                }
            }
        )*
    }
}

fixed_bytes_aligned!(
    HashOutput; PHB,
    bool; 1,
    u8; 1,
    u16; 2,
    u32; 4,
    u64; 8,
    u128; 16
);

impl<const N: usize> Aligned for [u8; N] {
    fn alignment() -> Alignment {
        Alignment::singleton(AlignmentAtom::Bytes { length: N as u32 })
    }
}

impl Aligned for &[u8] {
    fn alignment() -> Alignment {
        Alignment::singleton(AlignmentAtom::Compress)
    }
}

impl Aligned for Vec<u8> {
    fn alignment() -> Alignment {
        Alignment::singleton(AlignmentAtom::Compress)
    }
}

impl<T: Aligned> Aligned for Option<T> {
    fn alignment() -> Alignment {
        Alignment::concat([&bool::alignment(), &T::alignment()])
    }
}

impl Alignment {
    /// The alignment of `Option<T>` for this `T`. Useful if an `Alignment` is
    /// known, but a corresponding rust type *isn't*.
    pub fn option_of(&self) -> Alignment {
        Alignment::concat([&bool::alignment(), self])
    }
}

impl DynAligned for AlignedValue {
    fn dyn_alignment(&self) -> Alignment {
        self.alignment.clone()
    }
}

impl DynAligned for AlignedValueSlice<'_> {
    fn dyn_alignment(&self) -> Alignment {
        self.1.clone()
    }
}
