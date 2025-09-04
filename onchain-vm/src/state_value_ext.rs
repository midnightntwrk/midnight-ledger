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

use storage::{arena::Sp, db::DB};

use crate::error::OnchainProgramError;
use base_crypto::fab::AlignedValue;
use runtime_state::state::StateValue;

pub trait StateValueExt<D: DB> {
    fn as_cell(&self) -> Result<Sp<AlignedValue, D>, OnchainProgramError<D>>;
    fn as_cell_ref(&self) -> Result<&AlignedValue, OnchainProgramError<D>>;
}

impl<D: DB> StateValueExt<D> for StateValue<D> {
    fn as_cell(&self) -> Result<Sp<AlignedValue, D>, OnchainProgramError<D>> {
        match self {
            StateValue::Cell(value) => Ok(value.clone()),
            _ => Err(OnchainProgramError::ExpectedCell(self.clone())),
        }
    }

    fn as_cell_ref(&self) -> Result<&AlignedValue, OnchainProgramError<D>> {
        match self {
            StateValue::Cell(value) => Ok(value),
            _ => Err(OnchainProgramError::ExpectedCell(self.clone())),
        }
    }
}
