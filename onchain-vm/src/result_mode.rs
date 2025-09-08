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

use crate::error::OnchainProgramError;
use base_crypto::fab::AlignedValue;
use derive_where::derive_where;
use runtime_state::state::StateValue;
use serde::{Deserialize, Serialize};
use serialize::{Deserializable, Serializable, Tagged};
use std::fmt::Debug;
use storage::Storable;
use storage::db::DB;

pub trait ResultMode<D: DB>: Clone + Debug + 'static {
    type ReadResult: Eq
        + PartialEq
        + Clone
        + Debug
        + Serializable
        + Deserializable
        + Storable<D>
        + Tagged;
    type Event;
    fn process_read(
        result: &Self::ReadResult,
        real: &AlignedValue,
    ) -> Result<Option<Self::Event>, OnchainProgramError<D>>;
    fn process_log(event: &StateValue<D>) -> Option<Self::Event>;
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ResultModeVerify;

impl<D: DB> ResultMode<D> for ResultModeVerify {
    type ReadResult = AlignedValue;
    type Event = StateValue<D>;
    fn process_read(
        expected: &Self::ReadResult,
        actual: &AlignedValue,
    ) -> Result<Option<Self::Event>, OnchainProgramError<D>> {
        if expected != actual {
            Err(OnchainProgramError::ReadMismatch {
                expected: expected.clone(),
                actual: actual.clone(),
            })
        } else {
            Ok(None)
        }
    }
    fn process_log(event: &StateValue<D>) -> Option<Self::Event> {
        Some(event.clone())
    }
}

#[derive_where(Debug, PartialEq, Eq)]
#[derive(Serialize, Deserialize)]
#[serde(
    rename_all = "camelCase",
    tag = "tag",
    content = "content",
    bound(
        serialize = "StateValue<D>: Serialize",
        deserialize = "StateValue<D>: Deserialize<'de>"
    )
)]
pub enum GatherEvent<D: DB> {
    Read(AlignedValue),
    Log(StateValue<D>),
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ResultModeGather;

impl<D: DB> ResultMode<D> for ResultModeGather {
    type ReadResult = ();
    type Event = GatherEvent<D>;
    fn process_read(
        (): &Self::ReadResult,
        real: &AlignedValue,
    ) -> Result<Option<Self::Event>, OnchainProgramError<D>> {
        Ok(Some(GatherEvent::Read(real.clone())))
    }
    fn process_log(event: &StateValue<D>) -> Option<Self::Event> {
        Some(GatherEvent::Log(event.clone()))
    }
}
