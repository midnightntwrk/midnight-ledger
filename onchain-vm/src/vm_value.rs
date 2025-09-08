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
use crate::state_value_ext::*;
use base_crypto::fab::AlignedValue;
use derive_where::derive_where;
use runtime_state::state::StateValue;
use serialize::Serializable;
use std::fmt::Debug;
use std::ops::BitAnd;
use storage::arena::Sp;

#[derive(Copy, Clone, PartialOrd, Ord, PartialEq, Eq, Hash)]
pub enum ValueStrength {
    Weak,
    Strong,
}

use ValueStrength::*;
use storage::db::{DB, InMemoryDB};

impl BitAnd for ValueStrength {
    type Output = ValueStrength;

    fn bitand(self, rhs: Self) -> Self::Output {
        match (self, rhs) {
            (Strong, Strong) => Strong,
            _ => Weak,
        }
    }
}

impl Debug for ValueStrength {
    fn fmt(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Weak => write!(formatter, "#"),
            Strong => Ok(()),
        }
    }
}

#[derive_where(Eq, PartialEq, Clone)]
pub struct VmValue<D: DB = InMemoryDB> {
    pub strength: ValueStrength,
    pub value: StateValue<D>,
}

impl<D: DB> VmValue<D> {
    pub fn as_cell(&self) -> Result<Sp<AlignedValue, D>, OnchainProgramError<D>> {
        self.value.as_cell()
    }

    pub(crate) fn as_cell_ref(&self) -> Result<&AlignedValue, OnchainProgramError<D>> {
        self.value.as_cell_ref()
    }

    /// The serialized size of this value as a cell.
    ///
    /// Panics if the underlying value is not a cell.
    ///
    /// This function is used by VM op benchmarking to calculate parameter sizes
    /// the same way the VM does.
    pub fn serialized_size_as_cell(&self) -> usize {
        <AlignedValue as Serializable>::serialized_size(
            self.value.as_cell_ref().expect("must be a cell"),
        )
    }

    /// The log size of this value.
    ///
    /// This function is used by VM op benchmarking to calculate parameter sizes
    /// the same way the VM does.
    pub fn log_size(&self) -> usize {
        self.value.log_size()
    }
}

impl<D: DB> Debug for VmValue<D> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{:?}{:?}", self.strength, self.value)
    }
}

impl<D: DB> VmValue<D> {
    pub fn new(strength: ValueStrength, value: StateValue<D>) -> Self {
        VmValue { strength, value }
    }
}

#[macro_export]
macro_rules! vmval {
    (# $($val:tt)*) => {
        VmValue {
            strength: ValueStrength::Weak,
            value: stval!($($val)*),
        }
    };
    ($($val:tt)*) => {
        VmValue {
            strength: ValueStrength::Strong,
            value: stval!($($val)*),
        }
    };
}
