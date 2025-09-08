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

#[cfg(test)]
mod tests {
    use midnight_storage::arena::ArenaKey;
    use midnight_storage::arena::TypedArenaKey;
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
