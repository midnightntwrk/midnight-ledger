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

use base_crypto::cost_model::CostDuration;
use storage::arena::Sp;
use storage::db::InMemoryDB;
use storage::state_translation::TypedTranslationState;
use v6_to_v7_state_translation::StateTranslationTable;

fn main() {
    let v6_state = ledger_v6::structure::LedgerState::new("local-test");
    let tl_state = TypedTranslationState::<
        ledger_v6::structure::LedgerState<InMemoryDB>,
        ledger_v7::structure::LedgerState<InMemoryDB>,
        StateTranslationTable,
        InMemoryDB,
    >::start(Sp::new(v6_state))
    .unwrap();
    let cost = CostDuration::from_picoseconds(1_000_000_000_000);

    let mut v7_state = tl_state.run(cost).unwrap();
    while None == v7_state.result().unwrap() {
        v7_state = tl_state.run(cost).unwrap();
    }
}
