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

use storage::db::DB;

use crate::context::QueryContext;
use crate::cost_model::CostModel;
use crate::error::TranscriptRejected;
use crate::ops::Op;
use crate::result_mode::{GatherEvent, ResultModeGather};
use crate::state::ContractState;

pub trait ContractStateExt<D: DB> {
    #[allow(clippy::type_complexity)]
    fn query(
        &self,
        query: &[Op<ResultModeGather, D>],
        cost_model: &CostModel,
    ) -> Result<(ContractState<D>, Vec<GatherEvent<D>>), TranscriptRejected<D>>;
}

impl<D: DB> ContractStateExt<D> for ContractState<D> {
    fn query(
        &self,
        query: &[Op<ResultModeGather, D>],
        cost_model: &CostModel,
    ) -> Result<(ContractState<D>, Vec<GatherEvent<D>>), TranscriptRejected<D>> {
        let qc = QueryContext {
            state: self.data.clone(),
            address: Default::default(),
            effects: Default::default(),
            call_context: Default::default(),
        };
        let res = qc.query(query, None, cost_model)?;
        Ok((
            ContractState {
                data: res.context.state,
                ..self.clone()
            },
            res.events,
        ))
    }
}
