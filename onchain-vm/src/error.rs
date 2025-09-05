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

use crate::vm::MAX_LOG_SIZE;
use base_crypto::fab::{AlignedValue, InvalidBuiltinDecode};
use derive_where::derive_where;
use runtime_state::state::{CELL_BOUND, StateValue};
use std::convert::Infallible;
use std::error::Error;
use std::fmt::{self, Display, Formatter};
use storage::db::DB;

#[derive_where(Debug, Clone, PartialEq, Eq)]
pub enum OnchainProgramError<D: DB> {
    RanOffStack,
    RanPastProgramEnd,
    ExpectedCell(StateValue<D>),
    Decode(InvalidBuiltinDecode),
    ArithmeticOverflow,
    TooLongForEqual,
    /// Type error with description.
    TypeError(String),
    OutOfGas,
    BoundsExceeded,
    LogBoundExceeded,
    InvalidArgs(String),
    MissingKey,
    CacheMiss,
    AttemptedArrayDelete,
    ReadMismatch {
        expected: AlignedValue,
        actual: AlignedValue,
    },
    CellBoundExceeded,
}

impl<D: DB> From<Infallible> for OnchainProgramError<D> {
    fn from(err: Infallible) -> OnchainProgramError<D> {
        match err {}
    }
}

impl<D: DB> From<InvalidBuiltinDecode> for OnchainProgramError<D> {
    fn from(err: InvalidBuiltinDecode) -> OnchainProgramError<D> {
        OnchainProgramError::Decode(err)
    }
}

impl<D: DB> Display for OnchainProgramError<D> {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        use OnchainProgramError::*;
        match self {
            RanOffStack => write!(f, "ran off stack"),
            RanPastProgramEnd => write!(f, "ran past end of program"),
            ExpectedCell(state_value) => {
                let descriptor = match state_value {
                    StateValue::Null => Some("null"),
                    StateValue::Cell(_) => Some("cell"),
                    StateValue::BoundedMerkleTree(_) => Some("bounded Merkle tree"),
                    StateValue::Map(_) => Some("map"),
                    StateValue::Array(_) => Some("array"),
                    _ => None,
                };
                match descriptor {
                    Some(d) => write!(f, "expected a cell, received {d}"),
                    None => write!(f, "bug found: unexpected state value"),
                }
            }
            ArithmeticOverflow => write!(f, "arithmetic overflow"),
            TooLongForEqual => write!(f, "data is too long for equality check"),
            TypeError(msg) => write!(f, "invalid operation for type: {}", msg),
            OutOfGas => write!(f, "ran out of gas budget"),
            BoundsExceeded => write!(f, "exceeded structure bounds"),
            InvalidArgs(msg) => write!(f, "invalid argument to primitive operation: {msg}"),
            MissingKey => write!(f, "key not found"),
            CacheMiss => write!(f, "value declared to be in cache wasn't"),
            AttemptedArrayDelete => write!(f, "attempted to remove from an array type"),
            ReadMismatch { expected, actual } => write!(
                f,
                "mismatch between expected ({expected:?}) and actual ({actual:?}) read"
            ),
            Decode(err) => err.fmt(f),
            CellBoundExceeded => write!(f, "exceeded the maximum bound for cells: {CELL_BOUND}"),
            LogBoundExceeded => write!(
                f,
                "exceeded the maximum bound for log instruction: {MAX_LOG_SIZE}"
            ),
        }
    }
}

impl<D: DB> Error for OnchainProgramError<D> {}
