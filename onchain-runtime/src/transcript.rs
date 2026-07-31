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

#[cfg(feature = "proptest")]
impl<D: DB> rand::distributions::Distribution<Transcript<D>> for rand::distributions::Standard {
    fn sample<R: rand::Rng + ?Sized>(&self, rng: &mut R) -> Transcript<D> {
        // `Op` has no `Distribution`, so the sampled program is empty; the gas,
        // effects and (optional) version fields carry the randomness. This keeps
        // the generator total while still producing a structurally-valid,
        // serializable `Transcript` for the contract-action property tests.
        let version = if rng.r#gen::<bool>() {
            Some(Sp::new(TranscriptVersion {
                major: rng.r#gen(),
                minor: rng.r#gen(),
            }))
        } else {
            None
        };
        Transcript {
            gas: rng.r#gen(),
            effects: rng.r#gen(),
            program: storage::storage::Array::new(),
            version,
        }
    }
}

impl<D: DB> Transcript<D> {
    pub const VERSION: TranscriptVersion = TranscriptVersion { major: 2, minor: 3 };
}
