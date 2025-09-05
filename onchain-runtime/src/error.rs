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

use base_crypto::fab::InvalidBuiltinDecode;
use storage::db::DB;

use crate::vm_error::OnchainProgramError;
use std::error::Error;
use std::fmt::{self, Display, Formatter};

#[derive(Debug, Clone)]
pub enum TranscriptRejected<D: DB> {
    Execution(OnchainProgramError<D>),
    Decode(InvalidBuiltinDecode),
    FinalStackWrongLength,
    WeakStateReturned,
    EffectDecodeError,
}

impl<D: DB> From<OnchainProgramError<D>> for TranscriptRejected<D> {
    fn from(err: OnchainProgramError<D>) -> Self {
        TranscriptRejected::Execution(err)
    }
}

impl<D: DB> From<InvalidBuiltinDecode> for TranscriptRejected<D> {
    fn from(err: InvalidBuiltinDecode) -> Self {
        TranscriptRejected::Decode(err)
    }
}

impl<D: DB> Display for TranscriptRejected<D> {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        use TranscriptRejected::*;
        match self {
            FinalStackWrongLength => write!(formatter, "final stack must be of length 3"),
            WeakStateReturned => write!(
                formatter,
                "result state is was weak, and cannot be persisted"
            ),
            EffectDecodeError => write!(formatter, "failed to decode effect object"),
            Execution(err) => err.fmt(formatter),
            Decode(err) => err.fmt(formatter),
        }
    }
}

impl<D: DB> Error for TranscriptRejected<D> {}
