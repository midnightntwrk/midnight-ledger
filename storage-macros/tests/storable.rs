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

use derive_where::derive_where;
use midnight_storage_macros::Storable;
use proptest::arbitrary::Arbitrary;
use proptest_derive::Arbitrary;
use rand::distributions::Standard;
use rand::prelude::*;
use serialize::{
    self, Deserializable, NoStrategy, Serializable, Tagged, randomised_tagged_serialization_test,
    simple_arbitrary, tagged_deserialize, tagged_serialize,
};
use std::marker::PhantomData;
use storage::{
    DefaultDB,
    arena::{ArenaKey, BackendLoader, Sp},
    db::{DB, InMemoryDB},
    randomised_storable_test,
    storable::{Loader, Storable},
    storage::default_storage,
};

#[derive(Storable, Arbitrary)]
#[derive_where(Debug, Clone, PartialEq)]
#[storable(db = D)]
struct GenericFoo<D: DB>
where
    Sp<u8, D>: Arbitrary,
{
    #[storable(child)]
    child: Sp<u8, D>,
    data: u8,
}

#[derive(Debug, Storable, Clone, PartialEq, Arbitrary)]
struct Foo {
    #[storable(child)]
    child: Sp<u8>,
    data: u8,
}

#[derive(Debug, Storable, Clone, PartialEq, Arbitrary)]
struct MultiChildFoo {
    #[storable(child)]
    child_a: Sp<u8>,
    #[storable(child)]
    child_b: Sp<u8>,
    #[storable(child)]
    child_c: Sp<u8>,
}

#[derive(Debug, Storable, Clone, PartialEq, Arbitrary)]
#[tag = "tagged-foo"]
struct TaggedFoo {
    #[storable(child)]
    child: Sp<u8>,
    data: u8,
}

#[derive(Debug, Serializable, Storable, Clone, PartialEq, Arbitrary)]
#[storable(base)]
#[tag = "tagged-base-foo"]
struct TaggedBaseFoo {
    data: u8,
}

#[derive(Debug, Storable, Clone, PartialEq)]
struct RecursiveFoo {
    #[storable(child)]
    child: Sp<Option<Sp<Self>>>,
    in_line_child: Option<Sp<Self>>,
}

#[derive(Debug, Storable, PartialEq)]
#[storable(db = D, invariant = InvariantFoo::invariant)]
#[tag = "invariant-foo"]
struct InvariantFoo<D: DB> {
    #[storable(child)]
    child_a: Sp<u64, D>,
    #[storable(child)]
    child_b: Sp<u64, D>,
}

impl<D: DB> Clone for InvariantFoo<D> {
    fn clone(&self) -> Self {
        InvariantFoo {
            child_a: self.child_a.clone(),
            child_b: self.child_b.clone(),
        }
    }
}

impl<D: DB> InvariantFoo<D> {
    fn invariant(&self) -> std::io::Result<()> {
        if *self.child_a * 2 != *self.child_b {
            Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "b not double",
            ))
        } else {
            Ok(())
        }
    }
}

#[test]
fn test_invariant_foo() {
    let legal = InvariantFoo::<InMemoryDB> {
        child_a: Sp::new(42),
        child_b: Sp::new(84),
    };
    let illegal = InvariantFoo::<InMemoryDB> {
        child_a: Sp::new(42),
        child_b: Sp::new(42),
    };
    let mut legal_ser = Vec::new();
    tagged_serialize(&legal, &mut legal_ser).unwrap();
    let mut illegal_ser = Vec::new();
    tagged_serialize(&illegal, &mut illegal_ser).unwrap();
    tagged_deserialize::<InvariantFoo<InMemoryDB>>(&mut &legal_ser[..]).unwrap();
    assert!(tagged_deserialize::<InvariantFoo<InMemoryDB>>(&mut &illegal_ser[..]).is_err());
}

impl Distribution<RecursiveFoo> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> RecursiveFoo {
        let depth: u8 = rng.r#gen();
        let arena = default_storage().arena.clone();
        let mut leaf = RecursiveFoo {
            child: arena.alloc(None),
            in_line_child: None,
        };
        for _ in 0..depth {
            leaf = RecursiveFoo {
                child: arena.alloc(Some(arena.alloc(leaf.clone()))),
                in_line_child: Some(arena.alloc(leaf)),
            };
        }

        leaf
    }
}

#[derive(Debug, Storable, Clone, PartialEq, Arbitrary)]
struct ConcatFoo {
    foo: Foo,
    other_foo: Foo,
}

#[derive(Debug, Storable, Clone, PartialEq, Arbitrary)]
#[repr(transparent)]
struct UnnamedInlineFoo(u8);

#[derive(Debug, Storable, Clone, PartialEq, Arbitrary)]
struct UnnamedChildFoo(#[storable(child)] Sp<u8>);

simple_arbitrary!(RecursiveFoo);

#[cfg(test)]
mod tests {
    use super::*;

    randomised_storable_test!(Foo);
    randomised_storable_test!(MultiChildFoo);
    randomised_tagged_serialization_test!(TaggedFoo);
    randomised_tagged_serialization_test!(TaggedBaseFoo);
    randomised_storable_test!(TaggedFoo);
    randomised_storable_test!(TaggedBaseFoo);
    type GenericFooDefaultDB = GenericFoo<DefaultDB>;
    randomised_storable_test!(GenericFooDefaultDB);
    randomised_storable_test!(RecursiveFoo);
    randomised_storable_test!(ConcatFoo);
    randomised_storable_test!(UnnamedInlineFoo);
    randomised_storable_test!(UnnamedChildFoo);
}
