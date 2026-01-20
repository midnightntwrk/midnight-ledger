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

//! JSON transformation module for resolving variable references in symbolic ops.
//!
//! This module provides functionality to walk a JSON tree and resolve any variable
//! references (strings starting with '%') to their concrete hex values using a
//! memory map. After resolution, the JSON can be deserialized to concrete `Op` types.

use crate::ir::Identifier;
use anyhow::{anyhow, Result};
use serde_json::Value;
use std::collections::HashMap;
use transient_crypto::curve::Fr;

/// Resolve all variable references in a JSON value tree.
///
/// Variables are identified by strings starting with '%'.
/// After resolution, the JSON can be deserialized to concrete Op types.
///
/// # Arguments
/// * `value` - The JSON value tree to transform (mutated in place)
/// * `memory` - Map from variable identifiers to their field element values
///
/// # Returns
/// * `Ok(())` if all variables were resolved successfully
/// * `Err` if a variable reference was not found in the memory map
pub fn resolve_operands_in_json(
    value: &mut Value,
    memory: &HashMap<Identifier, Fr>,
) -> Result<()> {
    match value {
        Value::String(s) if s.starts_with('%') => {
            // This is a variable reference - resolve it
            let id = Identifier(s.clone());
            let fr = memory
                .get(&id)
                .ok_or_else(|| anyhow!("Variable {} not found in memory", s))?;

            // Convert Fr to hex string format expected by AlignedValue
            let mut repr = fr.as_le_bytes();
            while repr.last() == Some(&0) && repr.len() > 1 {
                repr.pop();
            }
            *value = Value::String(format!("0x{}", const_hex::encode(&repr)));
            Ok(())
        }
        Value::Array(arr) => {
            for v in arr.iter_mut() {
                resolve_operands_in_json(v, memory)?;
            }
            Ok(())
        }
        Value::Object(obj) => {
            for (_, v) in obj.iter_mut() {
                resolve_operands_in_json(v, memory)?;
            }
            Ok(())
        }
        // Numbers, bools, null, non-variable strings - no transformation needed
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_resolve_simple_variable() {
        let mut memory = HashMap::new();
        memory.insert(Identifier("%x.0".to_string()), Fr::from(42));

        let mut value = json!("%x.0");
        resolve_operands_in_json(&mut value, &memory).unwrap();

        // 42 in little-endian hex is "0x2a"
        assert_eq!(value, json!("0x2a"));
    }

    #[test]
    fn test_resolve_nested_object() {
        let mut memory = HashMap::new();
        memory.insert(Identifier("%t.0".to_string()), Fr::from(1000)); // 0x03e8

        let mut value = json!({
            "dup": { "n": 0 },
            "popeq": {
                "cached": false,
                "result": {
                    "value": ["%t.0"],
                    "alignment": []
                }
            }
        });

        resolve_operands_in_json(&mut value, &memory).unwrap();

        // Check that the variable was resolved
        let result_value = &value["popeq"]["result"]["value"][0];
        assert!(result_value.as_str().unwrap().starts_with("0x"));
    }

    #[test]
    fn test_resolve_missing_variable() {
        let memory = HashMap::new();
        let mut value = json!("%unknown.0");

        let result = resolve_operands_in_json(&mut value, &memory);
        assert!(result.is_err());
    }

    #[test]
    fn test_non_variable_strings_unchanged() {
        let memory = HashMap::new();
        let mut value = json!("0x42");

        resolve_operands_in_json(&mut value, &memory).unwrap();
        assert_eq!(value, json!("0x42"));
    }
}
