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

use crate::context::Effects;
use crate::ops::Op;
use crate::result_mode::ResultModeVerify;
use base_crypto::cost_model::RunningCost;
use derive_where::derive_where;
use serialize::tag_enforcement_test;
use serialize::{Deserializable, Serializable, Tagged};
use storage::Storable;
use storage::arena::{ArenaKey, Sp};
use storage::dag_memo::DagMemo;
use storage::db::DB;
use storage::storable::Loader;
// #[cfg(feature = "proptest")] TODO WG
// use proptest_derive::Arbitrary;
use serde::{Deserialize, Serialize};

#[derive(Storable, Serialize, Deserialize, Serializable, Clone, PartialEq, Eq, Debug)]
#[storable(base)]
#[tag = "contract-transcript-version"]
pub struct TranscriptVersion {
    pub major: u8,
    pub minor: u8,
}
tag_enforcement_test!(TranscriptVersion);

// #[cfg_attr(feature = "proptest", derive(Arbitrary))] TODO WG
#[derive(Storable, Serialize, Deserialize)]
#[serde(bound(serialize = "", deserialize = ""))]
#[derive_where(Clone, PartialEq, Eq, Debug)]
#[storable(db = D)]
#[tag = "contract-transcript[v4]"]
pub struct Transcript<D: DB> {
    pub gas: RunningCost,
    pub effects: Effects<D>,
    pub program: storage::storage::Array<Op<ResultModeVerify, D>, D>,
    pub version: Option<Sp<TranscriptVersion, D>>,
}
tag_enforcement_test!(Transcript<storage::db::InMemoryDB>);

// TODO WG
// #[cfg(feature = "proptest")]
// impl<D: DB> rand::distributions::Distribution<Transcript<D>> for rand::distributions::Standard {
//     fn sample<R: rand::Rng + ?Sized>(&self, rng: &mut R) -> Transcript<D> {
//         Transcript {
//             gas: rng.gen(),
//             effects: rng.gen(),
//             program: storage::storage::Vec::from_std_vec(vec![]),
//             version: rng.gen(),
//         }
//     }
// }

/// The largest number of public inputs a single contract call may declare.
///
/// A call's public inputs are two fixed elements (the binding input and the
/// communication commitment) followed by the field representation of its
/// guaranteed and fallible transcripts' programs; see
/// [`Transcript::public_inputs_len`].
///
/// This is a backstop against `Op::Noop { n }`, the one opcode whose
/// public-input footprint is not proportional to its own encoding: `n` is a
/// `u32` carried in a 9-byte op, and each unit of it contributes a 32-byte
/// field element, so an uncapped `n` turns a 77-byte contract call into a
/// 128 GiB allocation. Every other opcode contributes a number of elements
/// bounded by its own size, and hence by the transaction byte limit.
///
/// The value is anchored on the verifier parameters: `PARAMS_VERIFIER` is a
/// 2^14 setup, so no circuit the ledger can verify against -- and therefore no
/// statement it accepts -- exceeds 2^14 rows. This leaves 4x headroom over
/// that, and caps a single call's public inputs at 2 MiB.
///
/// Anything that materializes a call's public inputs from an untrusted
/// transcript should check its count against this first.
pub const MAX_CALL_PUBLIC_INPUTS: usize = 1 << 16;

impl<D: DB> Transcript<D> {
    pub const VERSION: TranscriptVersion = TranscriptVersion { major: 2, minor: 3 };

    /// The number of field elements this transcript's program contributes to
    /// a contract call's public inputs, computed without materializing them.
    ///
    /// Saturates rather than overflowing, so it is safe to call on untrusted
    /// programs.
    pub fn public_inputs_len(&self) -> usize {
        self.public_inputs_len_memoized(&mut DagMemo::new())
    }

    /// As [`Self::public_inputs_len`], visiting each distinct op and pushed
    /// node once across every transcript sharing `memo`.
    pub fn public_inputs_len_memoized(&self, memo: &mut DagMemo<usize, D>) -> usize {
        self.program.fold(memo, |memo, op| {
            memo.get_or_compute(op, |memo, op| op.field_size_memoized(memo))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use storage::db::InMemoryDB;
    use transient_crypto::repr::FieldRepr;

    fn transcript(program: Vec<Op<ResultModeVerify, InMemoryDB>>) -> Transcript<InMemoryDB> {
        Transcript {
            gas: Default::default(),
            effects: Default::default(),
            program: program.into(),
            version: None,
        }
    }

    #[test]
    fn public_inputs_len_matches_field_repr() {
        let t = transcript(vec![Op::Noop { n: 300 }, Op::Add, Op::Jmp { skip: 1 }]);
        let materialized: usize = t.program.iter().map(|op| op.field_vec().len()).sum();
        assert_eq!(t.public_inputs_len(), materialized);
        assert_eq!(t.public_inputs_len(), 300 + 1 + 2);
    }

    #[test]
    fn public_inputs_len_does_not_materialize() {
        let t = transcript(vec![Op::Noop { n: u32::MAX }, Op::Add, Op::Noop { n: u32::MAX }]);
        assert_eq!(t.public_inputs_len(), 2 * u32::MAX as usize + 1);
        assert!(t.public_inputs_len() > MAX_CALL_PUBLIC_INPUTS);
    }
}
