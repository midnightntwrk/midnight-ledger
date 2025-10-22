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

//! Version detection and dispatching for ZKIR

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::Read;

/// Version information for ZKIR
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
}

impl Version {
    /// Create a new version
    pub fn new(major: u32, minor: u32) -> Self {
        Version { major, minor }
    }

    /// ZKIR version 2
    pub const V2: Version = Version { major: 2, minor: 0 };

    /// ZKIR version 3
    pub const V3: Version = Version { major: 3, minor: 0 };
}

/// Detect the version from a JSON-encoded ZKIR source
pub fn detect_version(data: &[u8]) -> Result<Version> {
    // Parse as generic JSON to extract the version field
    let value: Value = serde_json::from_slice(data)
        .map_err(|e| anyhow!("Failed to parse ZKIR as JSON: {}", e))?;

    // Extract version field
    let version_obj = value
        .get("version")
        .ok_or_else(|| anyhow!("ZKIR missing 'version' field"))?;

    let version: Version = serde_json::from_value(version_obj.clone())
        .map_err(|e| anyhow!("Failed to parse version field: {}", e))?;

    Ok(version)
}

/// Detect version from a reader
pub fn detect_version_from_reader(mut reader: impl Read) -> Result<Version> {
    let mut buffer = Vec::new();
    reader.read_to_end(&mut buffer)?;
    detect_version(&buffer)
}
