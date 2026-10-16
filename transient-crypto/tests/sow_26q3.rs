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

//! Verifier keys record the zk-stdlib chips their circuit uses. Without the
//! `sow-26q3` feature only the 2026-Q2 chips are accepted, so a contract
//! built directly against midnight-zk cannot bring Q3 functionality (P256,
//! Curve25519, SHA-512, in-circuit BLS12-381, ...) on chain via ZKIR bypass.
//!
//! The keys here are an architecture header followed by junk: the gate must
//! decide on the header alone, before the expensive full parse.

use midnight_transient_crypto::proofs::VerifierKey;
use midnight_zk_stdlib::ZkStdLibArch;
use serialize::{Deserializable, Serializable};

fn vk_with(arch: ZkStdLibArch) -> VerifierKey {
    let mut raw = Vec::new();
    arch.write(&mut raw).unwrap();
    raw.push(10); // k
    raw.extend(0u32.to_le_bytes()); // number of public inputs
    raw.extend([0u8; 16]);
    let mut ser = Vec::new();
    Serializable::serialize(&raw, &mut ser).unwrap();
    Deserializable::deserialize(&mut &ser[..], 0).unwrap()
}

fn q2_arch() -> ZkStdLibArch {
    ZkStdLibArch {
        jubjub: true,
        poseidon: true,
        sha2_256: true,
        keccak_256: true,
        secp256k1: true,
        ..Default::default()
    }
}

/// Every chip outside the Q2 set, each switched on alone over a Q2 base.
fn q3_archs() -> Vec<(&'static str, ZkStdLibArch)> {
    let with = |name, f: fn(&mut ZkStdLibArch)| {
        let mut a = q2_arch();
        f(&mut a);
        (name, a)
    };
    vec![
        with("p256", |a| a.p256 = true),
        with("curve25519", |a| a.curve25519 = true),
        with("sha2_512", |a| a.sha2_512 = true),
        with("sha3_256", |a| a.sha3_256 = true),
        with("blake2b", |a| a.blake2b = true),
        with("bls12_381", |a| a.bls12_381 = true),
        with("base64", |a| a.base64 = true),
        with("automaton", |a| a.automaton = true),
    ]
}

#[test]
fn sow_26q3_q2_chips_pass_the_gate() {
    let err = vk_with(q2_arch()).init().unwrap_err().to_string();
    assert!(
        !err.contains("sow-26q3"),
        "a Q2 chip set must get past the gate (and then fail on the junk body), got {err:?}"
    );
}

#[cfg(not(feature = "sow-26q3"))]
#[test]
fn sow_26q3_refuses_q3_chips() {
    for (chip, arch) in q3_archs() {
        let err = vk_with(arch)
            .init()
            .expect_err("a Q3 chip must be refused")
            .to_string();
        assert!(
            err.contains("sow-26q3") && err.contains(chip),
            "refusal {err:?} must name the `sow-26q3` feature and the `{chip}` chip"
        );
    }
}

#[cfg(feature = "sow-26q3")]
#[test]
fn sow_26q3_enabled_admits_q3_chips() {
    for (chip, arch) in q3_archs() {
        let err = vk_with(arch).init().unwrap_err().to_string();
        assert!(
            !err.contains("sow-26q3"),
            "with `sow-26q3` the `{chip}` chip must get past the gate, got {err:?}"
        );
    }
}
