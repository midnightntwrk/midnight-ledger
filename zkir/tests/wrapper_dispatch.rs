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

//! Tests for wrapper dispatch between v2 and v3 ZKIR

use serialize::tagged_serialize;
use std::io::Cursor;
use zkir::{version::Version, IrSource};

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
    { "op": "load_imm", "output": "%imm00.1", "imm": "00" },
    { "op": "copy", "output": "%tmp.2", "var": "%x.0" },
    { "op": "constrain_bits", "var": "%tmp.2", "bits": 24 },
    { "op": "load_imm", "output": "%imm01.3", "imm": "01" },
    { "op": "load_imm", "output": "%imm10.4", "imm": "10" },
    { "op": "declare_pub_input", "var": "%imm10.4" },
    { "op": "declare_pub_input", "var": "%imm01.3" },
    { "op": "declare_pub_input", "var": "%imm01.3" },
    { "op": "declare_pub_input", "var": "%imm01.3" },
    { "op": "declare_pub_input", "var": "%imm00.1" },
    { "op": "pi_skip", "guard": "%imm01.3", "count": 5 },
    { "op": "load_imm", "output": "%imm11.5", "imm": "11" },
    { "op": "load_imm", "output": "%imm03.6", "imm": "03" },
    { "op": "declare_pub_input", "var": "%imm11.5" },
    { "op": "declare_pub_input", "var": "%imm01.3" },
    { "op": "declare_pub_input", "var": "%imm01.3" },
    { "op": "declare_pub_input", "var": "%imm03.6" },
    { "op": "declare_pub_input", "var": "%tmp.2" },
    { "op": "pi_skip", "guard": "%imm01.3", "count": 5 },
    { "op": "load_imm", "output": "%imm91.7", "imm": "91" },
    { "op": "declare_pub_input", "var": "%imm91.7" },
    { "op": "pi_skip", "guard": "%imm01.3", "count": 1 },
    { "op": "load_imm", "output": "%imm30.8", "imm": "30" },
    { "op": "declare_pub_input", "var": "%imm30.8" },
    { "op": "pi_skip", "guard": "%imm01.3", "count": 1 },
    { "op": "load_imm", "output": "%imm50.9", "imm": "50" },
    { "op": "declare_pub_input", "var": "%imm50.9" },
    { "op": "declare_pub_input", "var": "%imm01.3" },
    { "op": "declare_pub_input", "var": "%imm01.3" },
    { "op": "declare_pub_input", "var": "%imm00.1" },
    { "op": "pi_skip", "guard": "%imm01.3", "count": 4 },
    { "op": "public_input", "output": "%t.10", "guard": null },
    { "op": "load_imm", "output": "%imm0C.11", "imm": "0C" },
    { "op": "declare_pub_input", "var": "%imm0C.11" },
    { "op": "declare_pub_input", "var": "%imm01.3" },
    { "op": "declare_pub_input", "var": "%imm03.6" },
    { "op": "declare_pub_input", "var": "%t.10" },
    { "op": "pi_skip", "guard": "%imm01.3", "count": 4 },
    { "op": "output", "var": "%t.10" }
  ]
}"#;

#[test]
fn test_v2_json_deserialization() {
    // Deserialize v2 ZKIR from JSON
    let ir = IrSource::load(Cursor::new(V2_ZKIR_JSON)).unwrap();

    // Verify it's the V2 variant
    assert!(ir.as_v2().is_some(), "Expected V2 variant");
    assert!(ir.as_v3().is_none(), "Should not be V3 variant");

    // Verify version detection
    assert_eq!(ir.version(), Version::V2);

    // Verify we can access v2-specific fields through the wrapper
    let v2_ir = ir.as_v2().unwrap();
    assert_eq!(v2_ir.num_inputs, 1);
    assert!(v2_ir.do_communications_commitment);
}

#[test]
fn test_v3_json_deserialization() {
    // Deserialize v3 ZKIR from JSON
    let ir = IrSource::load(Cursor::new(V3_ZKIR_JSON)).unwrap();

    // Verify it's the V3 variant
    assert!(ir.as_v3().is_some(), "Expected V3 variant");
    assert!(ir.as_v2().is_none(), "Should not be V2 variant");

    // Verify version detection
    assert_eq!(ir.version(), Version::V3);

    // Verify we can access v3-specific fields through the wrapper
    let v3_ir = ir.as_v3().unwrap();
    assert_eq!(v3_ir.inputs.len(), 1);
    assert_eq!(v3_ir.inputs[0].0, "%x.0");
    assert!(v3_ir.do_communications_commitment);
}

#[test]
fn test_v2_binary_serialization_roundtrip() {
    // Deserialize v2 from JSON
    let ir = IrSource::load(Cursor::new(V2_ZKIR_JSON)).unwrap();
    let v2_original = ir.as_v2().expect("Expected V2 variant");

    // Serialize the inner v2 type to binary format (not the wrapper)
    let mut binary = Vec::new();
    tagged_serialize(v2_original, &mut binary).unwrap();

    // Deserialize from binary using from_tagged_reader (which auto-detects version)
    let ir_roundtrip =
        IrSource::from_tagged_reader(Cursor::new(&binary)).unwrap();

    // Verify it's still V2
    assert!(ir_roundtrip.as_v2().is_some(), "Expected V2 variant after roundtrip");
    assert_eq!(ir_roundtrip.version(), Version::V2);

    // Verify data integrity
    let v2_roundtrip = ir_roundtrip.as_v2().unwrap();
    assert_eq!(v2_original.num_inputs, v2_roundtrip.num_inputs);
    assert_eq!(
        v2_original.do_communications_commitment,
        v2_roundtrip.do_communications_commitment
    );
    assert_eq!(v2_original.instructions.len(), v2_roundtrip.instructions.len());
}

#[test]
fn test_v3_binary_serialization_roundtrip() {
    // Deserialize v3 from JSON
    let ir = IrSource::load(Cursor::new(V3_ZKIR_JSON)).unwrap();
    let v3_original = ir.as_v3().expect("Expected V3 variant");

    // Serialize the inner v3 type to binary format (not the wrapper)
    let mut binary = Vec::new();
    tagged_serialize(v3_original, &mut binary).unwrap();

    // Deserialize from binary using from_tagged_reader (which auto-detects version)
    let ir_roundtrip =
        IrSource::from_tagged_reader(Cursor::new(&binary)).unwrap();

    // Verify it's still V3
    assert!(ir_roundtrip.as_v3().is_some(), "Expected V3 variant after roundtrip");
    assert_eq!(ir_roundtrip.version(), Version::V3);

    // Verify data integrity
    let v3_roundtrip = ir_roundtrip.as_v3().unwrap();
    assert_eq!(v3_original.inputs.len(), v3_roundtrip.inputs.len());
    assert_eq!(v3_original.inputs[0], v3_roundtrip.inputs[0]);
    assert_eq!(
        v3_original.do_communications_commitment,
        v3_roundtrip.do_communications_commitment
    );
    assert_eq!(v3_original.instructions.len(), v3_roundtrip.instructions.len());
}

#[test]
fn test_wrapper_model_v2() {
    // Deserialize v2 ZKIR
    let ir = IrSource::load(Cursor::new(V2_ZKIR_JSON)).unwrap();

    // Test model() works through wrapper
    let model = ir.model(None);
    assert!(model.k() > 0, "Model should have non-zero k");
    assert!(model.rows() > 0, "Model should have non-zero rows");
}

#[test]
fn test_wrapper_model_v3() {
    // Deserialize v3 ZKIR
    let ir = IrSource::load(Cursor::new(V3_ZKIR_JSON)).unwrap();

    // Test model() works through wrapper
    let model = ir.model(None);
    assert!(model.k() > 0, "Model should have non-zero k");
    assert!(model.rows() > 0, "Model should have non-zero rows");
}

#[test]
fn test_version_mismatch_detection() {
    // This JSON has version 3 but v2 structure (should fail)
    let bad_json = r#"{
        "version": { "major": 3, "minor": 0 },
        "do_communications_commitment": true,
        "num_inputs": 0,
        "instructions": []
    }"#;

    let result = IrSource::load(Cursor::new(bad_json));
    assert!(result.is_err(), "Should fail to deserialize mismatched version/structure");
}

#[test]
fn test_unsupported_version() {
    // Version 4.0 is not supported
    let bad_json = r#"{
        "version": { "major": 4, "minor": 0 },
        "do_communications_commitment": true,
        "num_inputs": 0,
        "instructions": []
    }"#;

    let result = IrSource::load(Cursor::new(bad_json));
    assert!(result.is_err(), "Should fail for unsupported version");

    // Check that error message mentions unsupported version
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("Unsupported ZKIR version"),
        "Error should mention unsupported version, got: {}",
        err_msg
    );
}
