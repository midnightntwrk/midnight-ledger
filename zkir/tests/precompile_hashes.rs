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

//! Regression tests pinning prover/verifier key SHA-256 hashes.
//!
//! `IrMinorVersion::V0` must stay byte-identical to pre-PR-#154 keys, since
//! deployed verifiers hold those exact bytes. Without these pins, a change
//! in `zkir`, `midnight-proofs`, `midnight-circuits`, or `serialize` could
//! silently shift the distributed keys and break on-chain verification.
//!
//! Refresh after an intentional key change:
//!
//! ```text
//! UPDATE_ZKIR_HASHES=1 cargo test -p midnight-zkir --test precompile_hashes
//! ```

use midnight_zkir::IrSource;
use serialize::tagged_serialize;
use sha2::{Digest, Sha256};
use std::ffi::OsStr;
use std::fs::{self, File};
use std::io::{BufReader, Write};
use std::path::{Path, PathBuf};
use transient_crypto::proofs::{ParamsProver, ParamsProverProvider, Zkir};

const UPDATE_ENV: &str = "UPDATE_ZKIR_HASHES";
const REFRESH_HINT: &str =
    "Run `UPDATE_ZKIR_HASHES=1 cargo test -p midnight-zkir --test precompile_hashes` to refresh.";

struct TestParams;

impl ParamsProverProvider for TestParams {
    async fn get_params(&self, k: u8) -> std::io::Result<ParamsProver> {
        const DIR: &str = env!("MIDNIGHT_PP");
        ParamsProver::read(BufReader::new(File::open(format!(
            "{DIR}/bls_midnight_2p{k}"
        ))?))
    }
}

fn precompiles_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("zkir-precompiles")
}

fn enumerate_zkir(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = fs::read_dir(&dir).unwrap_or_else(|e| panic!("read_dir {dir:?}: {e}"));
        for entry in entries {
            let path = entry.expect("dir entry").path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension() == Some(OsStr::new("zkir")) {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

fn hex_digest(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    hex::encode(h.finalize())
}

/// Write `<hex> <artifact_label>` to the pin file. Matches `sha256sum`
/// output format so the committed files are interchangeable with
/// `sha256sum -c` and with the digests `flake.nix:local-params` produces.
fn write_pin_file(pin_path: &Path, artifact_label: &str, bytes: &[u8]) {
    let mut f = File::create(pin_path).unwrap_or_else(|e| panic!("create {pin_path:?}: {e}"));
    writeln!(f, "{}  {}", hex_digest(bytes), artifact_label)
        .unwrap_or_else(|e| panic!("write {pin_path:?}: {e}"));
}

fn read_pinned_hex(pin_path: &Path) -> std::io::Result<String> {
    let contents = fs::read_to_string(pin_path)?;
    Ok(contents.split_whitespace().next().unwrap_or("").to_string())
}

async fn produce_key_bytes(ir: &IrSource, params: &TestParams) -> (Vec<u8>, Vec<u8>) {
    let (pk, vk) = ir.keygen(params).await.expect("keygen");
    let mut pk_bytes = Vec::new();
    IrSource::serialize_prover_key_to_tagged(ir.version, &pk, &mut pk_bytes)
        .expect("serialize prover key");
    let mut vk_bytes = Vec::new();
    tagged_serialize(&vk, &mut vk_bytes).expect("serialize verifier key");
    (pk_bytes, vk_bytes)
}

#[actix_rt::test]
async fn precompile_key_hashes_pinned() {
    let update = std::env::var_os(UPDATE_ENV).is_some();
    let root = precompiles_root();
    let files = enumerate_zkir(&root);
    assert!(!files.is_empty(), "no .zkir files found under {root:?}");

    let mut mismatches: Vec<String> = Vec::new();

    for zkir_path in &files {
        let ir = IrSource::load(BufReader::new(
            File::open(zkir_path).unwrap_or_else(|e| panic!("open {zkir_path:?}: {e}")),
        ))
        .unwrap_or_else(|e| panic!("load IR {zkir_path:?}: {e}"));

        let (pk_bytes, vk_bytes) = produce_key_bytes(&ir, &TestParams).await;

        let stem = zkir_path
            .file_stem()
            .expect("stem")
            .to_string_lossy()
            .into_owned();
        let pk_label = format!("{stem}.prover");
        let vk_label = format!("{stem}.verifier");
        let pk_pin = zkir_path.with_extension("prover.sha256");
        let vk_pin = zkir_path.with_extension("verifier.sha256");

        if update {
            write_pin_file(&pk_pin, &pk_label, &pk_bytes);
            write_pin_file(&vk_pin, &vk_label, &vk_bytes);
            continue;
        }

        for (label, bytes, pin_path) in [
            (&pk_label, &pk_bytes, &pk_pin),
            (&vk_label, &vk_bytes, &vk_pin),
        ] {
            let actual = hex_digest(bytes);
            let expected = match read_pinned_hex(pin_path) {
                Ok(s) => s,
                Err(e) => {
                    mismatches.push(format!(
                        "{label}: missing pinned hash file {pin_path:?} ({e})"
                    ));
                    continue;
                }
            };
            if actual != expected {
                mismatches.push(format!(
                    "{label}: hash drift\n    expected: {expected}\n    actual:   {actual}\n    pin file: {pin_path:?}"
                ));
            }
        }
    }

    if update {
        eprintln!(
            "Refreshed {} prover+verifier hash pin files under {root:?}.",
            files.len()
        );
        return;
    }

    assert!(
        mismatches.is_empty(),
        "{} key hash mismatch(es):\n\n{}\n\n{REFRESH_HINT}",
        mismatches.len(),
        mismatches.join("\n\n"),
    );
}

fn smoke_ir(minor: u8) -> IrSource {
    let json = format!(
        r#"{{
           "version": {{ "major": 2, "minor": {minor} }},
           "num_inputs": 1,
           "do_communications_commitment": false,
           "instructions": [
               {{ "op": "assert", "cond": 0 }}
           ]
        }}"#
    );
    IrSource::load(json.as_bytes()).expect("load smoke IR")
}

async fn smoke_check(minor: u8, version_label: &str) {
    let update = std::env::var_os(UPDATE_ENV).is_some();
    let ir = smoke_ir(minor);
    let (pk_bytes, vk_bytes) = produce_key_bytes(&ir, &TestParams).await;

    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("smoke_fixtures");
    fs::create_dir_all(&dir).expect("create smoke fixtures dir");
    let pk_pin = dir.join(format!("{version_label}.prover.sha256"));
    let vk_pin = dir.join(format!("{version_label}.verifier.sha256"));
    let pk_label = format!("{version_label}.prover");
    let vk_label = format!("{version_label}.verifier");

    if update {
        write_pin_file(&pk_pin, &pk_label, &pk_bytes);
        write_pin_file(&vk_pin, &vk_label, &vk_bytes);
        return;
    }

    for (label, bytes, pin_path) in [
        (&pk_label, &pk_bytes, &pk_pin),
        (&vk_label, &vk_bytes, &vk_pin),
    ] {
        let actual = hex_digest(bytes);
        let expected = read_pinned_hex(pin_path)
            .unwrap_or_else(|e| panic!("{label}: read {pin_path:?}: {e}. {REFRESH_HINT}"));
        assert_eq!(
            actual, expected,
            "\n{label}: hash drift\n  expected: {expected}\n  actual:   {actual}\n  pin file: {pin_path:?}\n{REFRESH_HINT}",
        );
    }
}

#[actix_rt::test]
async fn smoke_v0_hash_stable() {
    smoke_check(0, "smoke_v0").await;
}

#[actix_rt::test]
async fn smoke_v1_hash_stable() {
    smoke_check(1, "smoke_v1").await;
}
