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

//! This module deals with cryptographic envelopes, specifically through the `[Envelope]` trait.

use std::{io::{self, Write}, marker::PhantomData};
use serialize::{Serializable, Tagged, tagged_serialize};

pub use derive::Envelope;

/// The envelope trait defines a conversion between a 'base' type into a cryptographic envelope
/// type. This is typically a 1:1 copy of the base, but often with translation or erasure of certain
/// fields, specifically to avoid self-reference in cryptographic envelopes (e.g. signing the
/// signature itself).
///
/// Note that `T` is the target (envelope) type, and `Self` is the source type (the data *including*
/// cryptographic values).
///
/// The primary benefit of the `Envelope` trait is it's support for `#[derive(Envelope)]`, which
/// ensures that the cryptographic envelope does not become detached from the source type by making
/// any misalignment a compilation error.
///
/// An example of how an `Envelope` 
pub trait Envelope<T> {
    /// Converts this value into its envelope.
    fn into_envelope(&self) -> T;

    /// Writes the serialization of the envelope into a bytestring.
    fn envelope_data(&self) -> Vec<u8> where T: Serializable + Tagged {
        let mut output = Vec::new();
        self.envelope_write(&mut output).expect("in-memory serialization should succeed");
        output
    }
    /// Writes the serialization of the envelope into a [Write]r.
    fn envelope_write(&self, writer: impl Write) -> io::Result<()> where T: Serializable + Tagged{
        tagged_serialize(&self.into_envelope(), writer)
    }
}

impl<T: Clone> Envelope<T> for T {
    fn into_envelope(&self) -> T {
        self.clone()
    }
}

impl<T> Envelope<PhantomData<T>> for T {
    fn into_envelope(&self) -> PhantomData<T> {
        PhantomData
    }
}

#[cfg(test)]
mod tests {
    use std::marker::PhantomData;
    use serialize::{Serializable, Deserializable, Tagged};

    use super::Envelope;

    #[derive(Serializable, Clone, PartialEq, Eq, Debug)]
    #[tag = "bar"]
    struct Bar(u64);
    #[derive(Serializable, Clone, PartialEq, Eq, Debug)]
    #[tag = "baz"]
    struct Baz(u64);

    #[derive(Envelope)]
    #[envelope(BazEnvelope)]
    #[envelope(BarEnvelope)]
    struct Foo {
        bar: Bar,
        baz: Baz,
    }

    #[derive(Serializable, Clone, PartialEq, Eq, Debug)]
    #[tag = "bar-envelope"]
    struct BarEnvelope {
        bar: Bar,
        baz: PhantomData<Baz>,
    }

    #[derive(Serializable, Clone, PartialEq, Eq, Debug)]
    #[tag = "bar-envelope"]
    struct BazEnvelope {
        bar: PhantomData<Bar>,
        baz: Baz,
    }

    #[test]
    fn test_envelope() {
        let foo = Foo { bar: Bar(42), baz: Baz(64) };
        assert_eq!(BarEnvelope { bar: Bar(42), baz: PhantomData }, foo.into_envelope());
        assert_eq!(BazEnvelope { bar: PhantomData, baz: Baz(64) }, foo.into_envelope());
    }
}
