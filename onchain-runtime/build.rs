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

use std::env;
use std::fs::File;
use std::path::Path;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../flake.lock");
    println!("cargo:rerun-if-changed=generate-rust-macros.ss");

    let vendored: String = env::var("CARGO_FEATURE_VENDORED").unwrap_or("0".into());
    let is_vendored = vendored == "1";

    if !is_vendored {
        let macro_out = Path::new(&"./vendored").join("program_fragments.rs");
        assert!(
            Command::new("scheme")
                .arg("--script")
                .arg("generate-rust-macros.ss")
                .stdout(File::create(macro_out).unwrap())
                .status()
                .unwrap()
                .success()
        );
    }
}
