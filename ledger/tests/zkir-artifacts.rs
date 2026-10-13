// This file is part of midnight-ledger.
// Copyright (C) Midnight Foundation
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

//! Decode-only checks over the static test artifacts
//! (`MIDNIGHT_LEDGER_TEST_STATIC_DIR`).
//!
//! Proving tests only run under `--features proving`, which CI's PR gate does
//! not enable - so a zkir format change that breaks decoding of the compiled
//! artifacts is otherwise invisible until a full-proving (nightly) run. This
//! test exercises just the decode path, which is where format drift between
//! the zkir tool that compiled the artifacts and the workspace zkir crates
//! shows up, without paying for any proving.

#![deny(warnings)]

use serialize::{peek_tag, tagged_deserialize};
use std::path::Path;
use std::{env, fs};

fn check_bzkir(path: &Path) {
    let bytes = fs::read(path).unwrap_or_else(|e| panic!("read {path:?}: {e}"));
    let tag = peek_tag(&mut std::io::Cursor::new(&bytes))
        .unwrap_or_else(|e| panic!("peek tag of {path:?}: {e}"));
    match tag.as_str() {
        "ir-source[v2]" | "ir-source[v2-generic]" => {
            zkir_v2::IrSource::load_from_tagged(std::io::Cursor::new(&bytes))
                .unwrap_or_else(|e| panic!("decode v2 bzkir {path:?}: {e}"));
        }
        "ir-source[v3-generic]" => {
            let _: zkir_v3::IrSource = tagged_deserialize(&mut &bytes[..])
                .unwrap_or_else(|e| panic!("decode v3 bzkir {path:?}: {e}"));
        }
        other => panic!("unknown zkir tag '{other}' in {path:?}"),
    }
}

fn check_zkir_json(path: &Path) {
    let contents = fs::read(path).unwrap_or_else(|e| panic!("read {path:?}: {e}"));
    let doc: serde_json::Value =
        serde_json::from_slice(&contents).unwrap_or_else(|e| panic!("parse JSON {path:?}: {e}"));
    let major = doc["version"]["major"].as_u64();
    match major {
        Some(2) => {
            zkir_v2::IrSource::load(&contents[..])
                .unwrap_or_else(|e| panic!("load v2 zkir {path:?}: {e}"));
        }
        Some(3) => {
            zkir_v3::IrSource::load(&contents[..])
                .unwrap_or_else(|e| panic!("load v3 zkir {path:?}: {e}"));
        }
        v => panic!("unsupported zkir major version {v:?} in {path:?}"),
    }
}

#[test]
fn static_zkir_artifacts_decode() {
    let test_dir = env::var("MIDNIGHT_LEDGER_TEST_STATIC_DIR")
        .expect("MIDNIGHT_LEDGER_TEST_STATIC_DIR should be set as env variable");
    let mut checked = 0;

    for contract in fs::read_dir(&test_dir).expect("read static dir") {
        let zkir_dir = contract.expect("dir entry").path().join("zkir");
        if !zkir_dir.is_dir() {
            continue;
        }
        for entry in fs::read_dir(&zkir_dir).expect("read zkir dir") {
            let path = entry.expect("dir entry").path();
            match path.extension().and_then(|e| e.to_str()) {
                Some("bzkir") => {
                    check_bzkir(&path);
                    checked += 1;
                }
                Some("zkir") => {
                    check_zkir_json(&path);
                    checked += 1;
                }
                _ => {}
            }
        }
    }
    assert!(
        checked > 0,
        "no zkir artifacts found under {test_dir:?} - static dir misconfigured?"
    );
}
