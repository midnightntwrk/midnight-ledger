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

#[cfg(test)]
mod tests {
    use midnight_storage::arena::{ArenaKey, TypedArenaKey};
    use midnight_storage::{Storable, db::DB, storable::Loader};
    #[cfg(feature = "proptest")]
    use proptest::arbitrary::Arbitrary;
    #[cfg(feature = "proptest")]
    use rand::Rng;
    use rand::distributions::Standard;
    use rand::prelude::*;
    use serialize::{Deserializable, Serializable, Tagged, tagged_deserialize};
    #[cfg(feature = "proptest")]
    use serialize::{NoStrategy, randomised_serialization_test, simple_arbitrary};
    #[cfg(feature = "proptest")]
    use std::marker::PhantomData;

    #[derive(Debug, Clone, Hash, PartialOrd, PartialEq, Ord, Eq, Serializable, Storable)]
    #[tag = "test-vec"]
    #[storable(base)]
    struct TestVec(std::vec::Vec<bool>);

    #[cfg(feature = "proptest")]
    simple_arbitrary!(TestVec);

    #[derive(Clone, PartialEq, Eq, Hash, Debug)]
    enum TestVecState {
        Len(serialize::ScaleState, std::vec::Vec<u8>),
        Elems(u32),
    }

    // A SCALE length, then that many booleans.
    impl serialize::EncodingShape for TestVec {
        type State = TestVecState;
        fn start() -> TestVecState {
            TestVecState::Len(<u32 as serialize::EncodingShape>::start(), vec![])
        }
        fn step(state: &TestVecState, byte: u8) -> Option<TestVecState> {
            match state {
                TestVecState::Len(len_state, bytes) => {
                    let len_state = <u32 as serialize::EncodingShape>::step(len_state, byte)?;
                    let mut bytes = bytes.clone();
                    bytes.push(byte);
                    if <u32 as serialize::EncodingShape>::accepts(&len_state) {
                        Some(TestVecState::Elems(u32::deserialize(&mut &bytes[..], 0).ok()?))
                    } else {
                        Some(TestVecState::Len(len_state, bytes))
                    }
                }
                TestVecState::Elems(left) => (*left > 0 && byte <= 1).then(|| TestVecState::Elems(left - 1)),
            }
        }
        fn accepts(state: &TestVecState) -> bool {
            *state == TestVecState::Elems(0)
        }
    }

    #[cfg(feature = "proptest")]
    #[test]
    fn test_vec_shape_matches_decoding() {
        serialize::check_encoding_shape::<TestVec>();
    }

    impl Distribution<TestVec> for Standard {
        fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> TestVec {
            let len = rng.gen_range(0..16);
            TestVec(rng.sample_iter(Standard).take(len).collect())
        }
    }

    #[test]
    fn arena_key_deserialization_error() {
        let bytes: std::vec::Vec<u8> = vec![0u8; 20];
        assert!(tagged_deserialize::<TypedArenaKey<TestVec, sha2::Sha256>>(&bytes[..],).is_err());
    }

    #[cfg(feature = "proptest")]
    type SimpleArray = midnight_storage::storage::Array<u8>;
    #[cfg(feature = "proptest")]
    randomised_serialization_test!(SimpleArray);
    #[cfg(feature = "proptest")]
    type SimpleMap = midnight_storage::storage::Map<TestVec, u8>;
    #[cfg(feature = "proptest")]
    randomised_serialization_test!(SimpleMap);
    #[cfg(feature = "proptest")]
    type SimpleHashMap = midnight_storage::storage::HashMap<TestVec, u8>;
    #[cfg(feature = "proptest")]
    type SimpleHashSet = midnight_storage::storage::HashSet<u8>;
    #[cfg(feature = "proptest")]
    randomised_serialization_test!(SimpleHashMap);
    #[cfg(feature = "proptest")]
    randomised_serialization_test!(SimpleHashSet);
}
