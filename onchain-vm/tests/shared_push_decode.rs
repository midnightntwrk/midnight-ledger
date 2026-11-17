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

//! Decoding a `Push` must take time linear in its encoding, even when its
//! value shares subtrees and so stands for exponentially many entries.

use midnight_onchain_vm::ops::Op;
use midnight_onchain_vm::result_mode::ResultModeVerify;
use runtime_state::state::{AlignedValueChecked, StateValue};
use serialize::{tagged_deserialize, tagged_serialize};
use std::time::Duration;
use storage::db::InMemoryDB;
use storage::storage::Array;
use storage::test::assert_completes_within;

type D = InMemoryDB;

const LIMIT: Duration = Duration::from_secs(10);

#[test]
fn push_of_shared_oversized_array_is_rejected() {
    let value =
        StateValue::<D, AlignedValueChecked>::Array(Array::shared_dense(12, StateValue::Null));
    let op: Op<ResultModeVerify, D> = Op::Push {
        storage: false,
        value,
    };
    let mut bytes = Vec::new();
    tagged_serialize(&op, &mut bytes).unwrap();
    let res = assert_completes_within(LIMIT, move || {
        tagged_deserialize::<Op<ResultModeVerify, D>>(&mut &bytes[..]).map(|_| ())
    });
    assert!(res.is_err());
}
