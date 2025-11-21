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

use serialize::tagged_serialize;
use std::io::Cursor;
use zkir::{IrSource, version::Version};

const V2_ZKIR_JSON: &str = r#"{
  "version": { "major": 2, "minor": 0 },
  "do_communications_commitment": true,
  "num_inputs": 1,
  "instructions": [
    { "op": "load_imm", "imm": "01" },
    { "op": "load_imm", "imm": "70" },
    { "op": "load_imm", "imm": "00" },
    { "op": "declare_pub_input", "var": 2 },
    { "op": "declare_pub_input", "var": 1 },
    { "op": "declare_pub_input", "var": 1 },
    { "op": "declare_pub_input", "var": 3 },
    { "op": "pi_skip", "guard": 1, "count": 4 },
    { "op": "declare_pub_input", "var": 2 },
    { "op": "declare_pub_input", "var": 1 },
    { "op": "declare_pub_input", "var": 1 },
    { "op": "declare_pub_input", "var": 3 },
    { "op": "pi_skip", "guard": 1, "count": 4 },
    { "op": "load_imm", "imm": "32" },
    { "op": "declare_pub_input", "var": 4 },
    { "op": "pi_skip", "guard": 1, "count": 1 },
    { "op": "load_imm", "imm": "50" },
    { "op": "declare_pub_input", "var": 5 },
    { "op": "declare_pub_input", "var": 1 },
    { "op": "declare_pub_input", "var": 1 },
    { "op": "declare_pub_input", "var": 1 },
    { "op": "pi_skip", "guard": 1, "count": 4 },
    { "op": "load_imm", "imm": "11" },
    { "op": "load_imm", "imm": "6D646E3A6C68" },
    { "op": "persistent_hash", "alignment": [{ "tag": "atom", "value": { "length": 6, "tag": "bytes" } }, { "tag": "atom", "value": { "tag": "field" } }], "inputs": [7, 0] },
    { "op": "load_imm", "imm": "20" },
    { "op": "declare_pub_input", "var": 6 },
    { "op": "declare_pub_input", "var": 1 },
    { "op": "declare_pub_input", "var": 1 },
    { "op": "declare_pub_input", "var": 10 },
    { "op": "declare_pub_input", "var": 8 },
    { "op": "declare_pub_input", "var": 9 },
    { "op": "pi_skip", "guard": 1, "count": 6 },
    { "op": "load_imm", "imm": "91" },
    { "op": "declare_pub_input", "var": 11 },
    { "op": "pi_skip", "guard": 1, "count": 1 },
    { "op": "load_imm", "imm": "A1" },
    { "op": "declare_pub_input", "var": 12 },
    { "op": "pi_skip", "guard": 1, "count": 1 },
    { "op": "declare_pub_input", "var": 2 },
    { "op": "declare_pub_input", "var": 1 },
    { "op": "declare_pub_input", "var": 1 },
    { "op": "declare_pub_input", "var": 1 },
    { "op": "pi_skip", "guard": 1, "count": 4 },
    { "op": "load_imm", "imm": "0E" },
    { "op": "declare_pub_input", "var": 13 },
    { "op": "declare_pub_input", "var": 1 },
    { "op": "pi_skip", "guard": 1, "count": 2 },
    { "op": "load_imm", "imm": "A2" },
    { "op": "declare_pub_input", "var": 14 },
    { "op": "pi_skip", "guard": 1, "count": 1 }
  ]
}"#;

const V3_ZKIR_JSON: &str = r#"{
  "version": { "major": 3, "minor": 0 },
  "do_communications_commitment": true,
  "inputs": [
    "%x.0"
  ],
  "instructions": [
    { "op": "copy", "output": "%imm00.1", "val": "0x00" },
    { "op": "copy", "output": "%tmp.2", "val": "%x.0" },
    { "op": "constrain_bits", "val": "%tmp.2", "bits": 24 },
    { "op": "copy", "output": "%imm01.3", "val": "0x01" },
    { "op": "copy", "output": "%imm10.4", "val": "0x10" },
    { "op": "declare_pub_input", "val": "%imm10.4" },
    { "op": "declare_pub_input", "val": "%imm01.3" },
    { "op": "declare_pub_input", "val": "%imm01.3" },
    { "op": "declare_pub_input", "val": "%imm01.3" },
    { "op": "declare_pub_input", "val": "%imm00.1" },
    { "op": "pi_skip", "guard": "%imm01.3", "count": 5 },
    { "op": "copy", "output": "%imm11.5", "val": "0x11" },
    { "op": "copy", "output": "%imm03.6", "val": "0x03" },
    { "op": "declare_pub_input", "val": "%imm11.5" },
    { "op": "declare_pub_input", "val": "%imm01.3" },
    { "op": "declare_pub_input", "val": "%imm01.3" },
    { "op": "declare_pub_input", "val": "%imm03.6" },
    { "op": "declare_pub_input", "val": "%tmp.2" },
    { "op": "pi_skip", "guard": "%imm01.3", "count": 5 },
    { "op": "copy", "output": "%imm91.7", "val": "0x91" },
    { "op": "declare_pub_input", "val": "%imm91.7" },
    { "op": "pi_skip", "guard": "%imm01.3", "count": 1 },
    { "op": "copy", "output": "%imm30.8", "val": "0x30" },
    { "op": "declare_pub_input", "val": "%imm30.8" },
    { "op": "pi_skip", "guard": "%imm01.3", "count": 1 },
    { "op": "copy", "output": "%imm50.9", "val": "0x50" },
    { "op": "declare_pub_input", "val": "%imm50.9" },
    { "op": "declare_pub_input", "val": "%imm01.3" },
    { "op": "declare_pub_input", "val": "%imm01.3" },
    { "op": "declare_pub_input", "val": "%imm00.1" },
    { "op": "pi_skip", "guard": "%imm01.3", "count": 4 },
    { "op": "public_input", "output": "%t.10", "guard": null },
    { "op": "copy", "output": "%imm0C.11", "val": "0x0C" },
    { "op": "declare_pub_input", "val": "%imm0C.11" },
    { "op": "declare_pub_input", "val": "%imm01.3" },
    { "op": "declare_pub_input", "val": "%imm03.6" },
    { "op": "declare_pub_input", "val": "%t.10" },
    { "op": "pi_skip", "guard": "%imm01.3", "count": 4 },
    { "op": "output", "val": "%t.10" }
  ]
}"#;

#[test]
fn test_v2_json_deserialization() {
    let ir = IrSource::load(Cursor::new(V2_ZKIR_JSON)).unwrap();

    assert!(ir.as_v2().is_some(), "Expected V2 variant");
    assert!(ir.as_v3().is_none(), "Should not be V3 variant");

    assert_eq!(ir.version(), Version::V2);

    let v2_ir = ir.as_v2().unwrap();
    assert_eq!(v2_ir.num_inputs, 1);
    assert!(v2_ir.do_communications_commitment);
}

#[test]
fn test_v3_json_deserialization() {
    let ir = IrSource::load(Cursor::new(V3_ZKIR_JSON)).unwrap();

    assert!(ir.as_v3().is_some(), "Expected V3 variant");
    assert!(ir.as_v2().is_none(), "Should not be V2 variant");

    assert_eq!(ir.version(), Version::V3);

    let v3_ir = ir.as_v3().unwrap();
    assert_eq!(v3_ir.inputs.len(), 1);
    assert_eq!(v3_ir.inputs[0].0, "%x.0");
    assert!(v3_ir.do_communications_commitment);
}

#[test]
fn test_v2_binary_serialization_roundtrip() {
    let ir = IrSource::load(Cursor::new(V2_ZKIR_JSON)).unwrap();
    let v2_original = ir.as_v2().expect("Expected V2 variant");

    let mut binary = Vec::new();
    tagged_serialize(v2_original, &mut binary).unwrap();

    let ir_roundtrip = IrSource::from_tagged_reader(Cursor::new(&binary)).unwrap();

    assert!(
        ir_roundtrip.as_v2().is_some(),
        "Expected V2 variant after roundtrip"
    );
    assert_eq!(ir_roundtrip.version(), Version::V2);

    let v2_roundtrip = ir_roundtrip.as_v2().unwrap();
    assert_eq!(v2_original.num_inputs, v2_roundtrip.num_inputs);
    assert_eq!(
        v2_original.do_communications_commitment,
        v2_roundtrip.do_communications_commitment
    );
    assert_eq!(
        v2_original.instructions.len(),
        v2_roundtrip.instructions.len()
    );
}

#[test]
fn test_v3_binary_serialization_roundtrip() {
    let ir = IrSource::load(Cursor::new(V3_ZKIR_JSON)).unwrap();
    let v3_original = ir.as_v3().expect("Expected V3 variant");

    let mut binary = Vec::new();
    tagged_serialize(v3_original, &mut binary).unwrap();

    let ir_roundtrip = IrSource::from_tagged_reader(Cursor::new(&binary)).unwrap();

    assert!(
        ir_roundtrip.as_v3().is_some(),
        "Expected V3 variant after roundtrip"
    );
    assert_eq!(ir_roundtrip.version(), Version::V3);

    let v3_roundtrip = ir_roundtrip.as_v3().unwrap();
    assert_eq!(v3_original.inputs.len(), v3_roundtrip.inputs.len());
    assert_eq!(v3_original.inputs[0], v3_roundtrip.inputs[0]);
    assert_eq!(
        v3_original.do_communications_commitment,
        v3_roundtrip.do_communications_commitment
    );
    assert_eq!(
        v3_original.instructions.len(),
        v3_roundtrip.instructions.len()
    );
}

#[test]
fn test_wrapper_model_v2() {
    let ir = IrSource::load(Cursor::new(V2_ZKIR_JSON)).unwrap();

    let model = ir.model(None);
    assert!(model.k() > 0, "Model should have non-zero k");
    assert!(model.rows() > 0, "Model should have non-zero rows");
}

#[test]
fn test_wrapper_model_v3() {
    let ir = IrSource::load(Cursor::new(V3_ZKIR_JSON)).unwrap();

    let model = ir.model(None);
    assert!(model.k() > 0, "Model should have non-zero k");
    assert!(model.rows() > 0, "Model should have non-zero rows");
}

#[test]
fn test_version_mismatch_detection() {
    let bad_json = r#"{
        "version": { "major": 3, "minor": 0 },
        "do_communications_commitment": true,
        "num_inputs": 0,
        "instructions": []
    }"#;

    let result = IrSource::load(Cursor::new(bad_json));
    assert!(
        result.is_err(),
        "Should fail to deserialize mismatched version/structure"
    );
}

#[test]
fn test_unsupported_version() {
    let bad_json = r#"{
        "version": { "major": 4, "minor": 0 },
        "do_communications_commitment": true,
        "num_inputs": 0,
        "instructions": []
    }"#;

    let result = IrSource::load(Cursor::new(bad_json));
    assert!(result.is_err(), "Should fail for unsupported version");

    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("Unsupported ZKIR version"),
        "Error should mention unsupported version, got: {}",
        err_msg
    );
}

#[test]
fn test_v3_invalid_identifier_format() {
    let bad_json = r#"{
      "version": { "major": 3, "minor": 0 },
      "do_communications_commitment": false,
      "inputs": ["v_0"],
      "instructions": [
        { "op": "assert", "cond": "v_0" }
      ]
    }"#;

    let result = IrSource::load(Cursor::new(bad_json));
    assert!(
        result.is_err(),
        "Should fail for identifier without '%' prefix"
    );

    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("Invalid operand format"),
        "Error should mention invalid operand format, got: {}",
        err_msg
    );
    assert!(
        err_msg.contains("Variables must start with '%'"),
        "Error should mention '%' requirement, got: {}",
        err_msg
    );
}

#[test]
fn test_v3_invalid_hex_odd_length() {
    // V3 hex immediates must have even length
    let bad_json = r#"{
      "version": { "major": 3, "minor": 0 },
      "do_communications_commitment": false,
      "inputs": ["%x.0"],
      "instructions": [
        { "op": "copy", "val": "0x1", "output": "%tmp.1" }
      ]
    }"#;

    let result = IrSource::load(Cursor::new(bad_json));
    assert!(result.is_err(), "Should fail for odd-length hex string");

    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("odd number of digits"),
        "Error should mention odd number of digits, got: {}",
        err_msg
    );
}
