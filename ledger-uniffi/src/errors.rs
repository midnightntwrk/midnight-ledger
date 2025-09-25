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
// limitations under the License

use thiserror::Error;

#[derive(uniffi::Error, Debug, Error)]
pub enum FfiError {
    #[error("Invalid input: {details}")]
    InvalidInput { details: String },
    #[error("Deserialize error: {details}")]
    DeserializeError { details: String },
    #[error("Unsupported variant: {details}")]
    UnsupportedVariant { details: String },
    #[error("Segment mismatch: {details}")]
    SegmentMismatch { details: String },
    #[error("Already proof-erased")] 
    AlreadyProofErased,
    #[error("Internal error: {details}")]
    Internal { details: String },
}

impl From<std::io::Error> for FfiError {
    fn from(e: std::io::Error) -> Self { Self::DeserializeError { details: e.to_string() } }
}

impl From<serde_json::Error> for FfiError {
    fn from(e: serde_json::Error) -> Self { Self::DeserializeError { details: e.to_string() } }
}

impl From<anyhow::Error> for FfiError {
    fn from(e: anyhow::Error) -> Self { Self::Internal { details: e.to_string() } }
}

impl From<String> for FfiError {
    fn from(e: String) -> Self { Self::InvalidInput { details: e } }
}
