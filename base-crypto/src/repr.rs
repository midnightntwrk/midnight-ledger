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

//! This module deals with representing data as sequences of binary objects for
//! use in persistent hashing.

pub use derive::BinaryHashRepr;
use serialize::VecExt;
use std::io;

/// Something that can be written to from in-memory buffers
pub trait MemWrite<T> {
    /// Write a buffer into memory.
    fn write(&mut self, buf: &[T]);
}

impl<T: Copy> MemWrite<T> for Vec<T> {
    fn write(&mut self, buf: &[T]) {
        self.extend(buf);
    }
}

impl<T, W: MemWrite<T>> MemWrite<T> for &mut W {
    fn write(&mut self, buf: &[T]) {
        W::write(self, buf);
    }
}

/// TODO: describe
pub struct IoWrite<W: MemWrite<u8>>(pub W);

impl<W: MemWrite<u8>> io::Write for IoWrite<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.write(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// An object that can be represented as a sequence of hash-able chunks.
pub trait BinaryHashRepr {
    /// Writes out the binary representation of this value into a writer.
    fn binary_repr<W: MemWrite<u8>>(&self, writer: &mut W);
    /// The size of an object's binary representation.
    fn binary_len(&self) -> usize;
    /// Writes the hash repr into a vector
    fn binary_vec(&self) -> Vec<u8> {
        let mut res = Vec::with_bounded_capacity(self.binary_len());
        self.binary_repr(&mut res);
        res
    }
}

impl BinaryHashRepr for [u8] {
    fn binary_repr<W: MemWrite<u8>>(&self, writer: &mut W) {
        writer.write(self);
    }
    fn binary_len(&self) -> usize {
        self.len()
    }
}

impl<const N: usize> BinaryHashRepr for [u8; N] {
    fn binary_repr<W: MemWrite<u8>>(&self, writer: &mut W) {
        writer.write(self)
    }
    fn binary_len(&self) -> usize {
        N
    }
}

macro_rules! integer_hash_repr {
    ($($ty:ty),*) => {
        $(
            impl BinaryHashRepr for $ty {
                fn binary_repr<W: MemWrite<u8>>(&self, writer: &mut W) {
                    writer.write(&self.to_le_bytes());
                }
                fn binary_len(&self) -> usize {
                    <$ty>::BITS as usize / 8
                }
            }
        )*
    }
}

macro_rules! tuple_repr {
    ($head:ident$(, $tail:ident)*) => {
        #[allow(unused_parens, non_snake_case)]
        impl<$head: BinaryHashRepr$(, $tail: BinaryHashRepr)*> BinaryHashRepr for ($head, $($tail),*) {
            fn binary_repr<W: MemWrite<u8>>(&self, writer: &mut W) {
                let ($head, $($tail),*) = self;
                $head.binary_repr(writer);
                $($tail.binary_repr(writer);)*
            }
            fn binary_len(&self) -> usize {
                let ($head, $($tail),*) = self;
                $head.binary_len() $(+ $tail.binary_len())*
            }
        }
        tuple_repr!($($tail),*);
    };
    () => {
        impl BinaryHashRepr for () {
            fn binary_repr<W: MemWrite<u8>>(&self, _: &mut W) {
            }
            fn binary_len(&self) -> usize {
                0
            }
        }
    };
}

tuple_repr!(A, B, C, D, E, F, G, H, I, J, K, L);

integer_hash_repr!(u8, u16, u32, u64, u128, i8, i16, i32, i64, i128);

impl BinaryHashRepr for bool {
    fn binary_repr<W: MemWrite<u8>>(&self, writer: &mut W) {
        writer.write(&[*self as u8]);
    }
    fn binary_len(&self) -> usize {
        1
    }
}
