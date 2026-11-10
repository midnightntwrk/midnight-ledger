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

//! State slot indices of the `ledger` fields declared in `zswap/zswap.compact`.
//!
//! The on-chain runtime addresses contract state positionally, so each `ledger`
//! declaration in the Compact source is reachable under `Key::Value(n)` where
//! `n` is its zero-based declaration index. These constants are the single
//! definition of that mapping for the crate; they are mirrored by hand, because
//! the build consumes the pre-compiled ZKIR in `zkir-precompiles/` rather than
//! invoking `compactc` (see `flake.nix`).
//!
//! Reordering, inserting or removing a `ledger` declaration in `zswap.compact`
//! therefore requires updating these values in lockstep. Getting them wrong
//! produces a public transcript that does not match the circuit's verifier key,
//! so every affected zswap proof fails to verify.

/// `ledger merkleTree: HistoricMerkleTree<32, Bytes<32>>`
pub(crate) const ZSWAP_IDX_MERKLE_TREE: u8 = 0;
/// `ledger nullifiers: Set<Bytes<32>>`
pub(crate) const ZSWAP_IDX_NULLIFIERS: u8 = 1;
/// `ledger valueCom: CurvePoint`
pub(crate) const ZSWAP_IDX_VALUE_COM: u8 = 2;
/// `ledger contractAddr: ContractAddress`
pub(crate) const ZSWAP_IDX_CONTRACT_ADDR: u8 = 3;
/// `ledger publicKey: ZswapCoinPublicKey`
pub(crate) const ZSWAP_IDX_PUBLIC_KEY: u8 = 4;
/// `ledger segment: Uint<16>`
pub(crate) const ZSWAP_IDX_SEGMENT: u8 = 5;

#[cfg(test)]
mod tests {
    use super::*;

    /// The `ledger` field names declared in a Compact source, in declaration order.
    fn ledger_fields(src: &str) -> Vec<&str> {
        src.lines()
            .map(str::trim)
            .map(|line| line.strip_prefix("export ").unwrap_or(line))
            .filter_map(|line| line.strip_prefix("ledger "))
            .map(|rest| rest.split(':').next().unwrap_or("").trim())
            .collect()
    }

    /// Pins the constants above to the declaration order in `zswap.compact`, so that
    /// reordering, inserting, removing or renaming a `ledger` field there fails here
    /// rather than silently invalidating every zswap proof.
    #[test]
    fn slot_indices_match_compact_source() {
        let fields = ledger_fields(include_str!("../zswap.compact"));
        let expected = [
            (ZSWAP_IDX_MERKLE_TREE, "merkleTree"),
            (ZSWAP_IDX_NULLIFIERS, "nullifiers"),
            (ZSWAP_IDX_VALUE_COM, "valueCom"),
            (ZSWAP_IDX_CONTRACT_ADDR, "contractAddr"),
            (ZSWAP_IDX_PUBLIC_KEY, "publicKey"),
            (ZSWAP_IDX_SEGMENT, "segment"),
        ];
        assert_eq!(
            fields.len(),
            expected.len(),
            "zswap.compact declares {} ledger fields, but {} slot indices are defined \
             in compact_slots.rs; found {fields:?}",
            fields.len(),
            expected.len(),
        );
        for (idx, name) in expected {
            assert_eq!(
                fields[usize::from(idx)],
                name,
                "slot {idx} is documented as `{name}`, but zswap.compact declares \
                 `{}` at that position",
                fields[usize::from(idx)],
            );
        }
    }

    #[test]
    fn ledger_fields_ignores_non_declarations() {
        let src = "\
// ledger commentedOut: Field;
ledger a: Field;
struct NotALedger { ledger: Field }
export ledger b: Set<Field>;
";
        assert_eq!(ledger_fields(src), vec!["a", "b"]);
    }
}
