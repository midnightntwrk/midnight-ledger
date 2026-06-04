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

//! Backwards-compatible v1 (zk-stdlib v1) proving and verification helpers
//! for [`IrSource`].
//!
//! This module bridges the current `IrSource` to the pinned old crates
//! (`midnight-zkir` 2.1.0, `midnight-transient-crypto` 2.0.1) that use
//! zk-stdlib v1. V1 is the default proving pipeline; v2 is currently only
//! used in tests.
//!
//! The approach converts between current and old types via serialization,
//! then delegates proving to the old crate's `Zkir` implementation.

use super::ir::IrSource;

type OldIrSource = zkir_old::IrSource;
type OldProverKey = transient_crypto_old::proofs::ProverKey<OldIrSource>;

impl IrSource {
    /// Converts the current `IrSource` to the old (v1) `IrSource` via
    /// serialization round-trip. Both versions share the same binary
    /// format (`ir-source[v2-generic]` / `ir-source[v2]`).
    pub fn to_v1(&self) -> std::io::Result<OldIrSource> {
        let mut buf = Vec::new();
        serialize::Serializable::serialize(self, &mut buf)?;
        serialize_old::Deserializable::deserialize(&mut &buf[..], 0)
    }

    /// Performs key generation using the v1 (zk-stdlib v1) pipeline.
    ///
    /// Returns keys typed against the **old** transient-crypto crate.
    pub async fn v1_keygen(
        &self,
        params: &impl transient_crypto_old::proofs::ParamsProverProvider,
    ) -> anyhow::Result<(OldProverKey, transient_crypto_old::proofs::VerifierKey)> {
        use transient_crypto_old::proofs::Zkir as _;
        let old_ir = self.to_v1()?;
        old_ir.keygen(params).await
    }

    /// Checks a proof preimage against the v1 (zk-stdlib v1) circuit.
    pub fn v1_check(
        &self,
        preimage: &transient_crypto_old::proofs::ProofPreimage,
    ) -> Result<Vec<Option<usize>>, transient_crypto_old::proofs::ProvingError> {
        use transient_crypto_old::proofs::Zkir as _;
        let old_ir = self.to_v1().map_err(|e| anyhow::anyhow!(e))?;
        old_ir.check(preimage)
    }

    /// Proves using the v1 (zk-stdlib v1) pipeline.
    pub async fn v1_prove(
        &self,
        rng: impl rand::Rng + rand::CryptoRng,
        params: &impl transient_crypto_old::proofs::ParamsProverProvider,
        pk: OldProverKey,
        preimage: &transient_crypto_old::proofs::ProofPreimage,
    ) -> Result<
        (
            transient_crypto_old::proofs::Proof,
            Vec<transient_crypto_old::curve::Fr>,
            Vec<Option<usize>>,
        ),
        transient_crypto_old::proofs::ProvingError,
    > {
        use transient_crypto_old::proofs::Zkir as _;
        let old_ir = self.to_v1().map_err(|e| anyhow::anyhow!(e))?;
        old_ir.prove(rng, params, pk, preimage).await
    }

    /// Serializes a v1 verifier key into bytes compatible with the current
    /// `VerifierKey` type from the updated transient-crypto.
    pub fn v1_vk_to_current(
        vk: &transient_crypto_old::proofs::VerifierKey,
    ) -> std::io::Result<transient_crypto::proofs::VerifierKey> {
        let mut buf = Vec::new();
        serialize_old::Serializable::serialize(vk, &mut buf)?;
        serialize::Deserializable::deserialize(&mut &buf[..], 0)
    }

    /// Serializes a current `VerifierKey` into bytes compatible with the old
    /// (v1) `VerifierKey` type.
    pub fn current_vk_to_v1(
        vk: &transient_crypto::proofs::VerifierKey,
    ) -> std::io::Result<transient_crypto_old::proofs::VerifierKey> {
        let mut buf = Vec::new();
        serialize::Serializable::serialize(vk, &mut buf)?;
        serialize_old::Deserializable::deserialize(&mut &buf[..], 0)
    }

    /// Converts a v1 `Proof` into the current `Proof` type.
    pub fn v1_proof_to_current(
        proof: transient_crypto_old::proofs::Proof,
    ) -> transient_crypto::proofs::Proof {
        transient_crypto::proofs::Proof(proof.0)
    }

    /// Converts v1 `Fr` values into current `Fr` values via their inner
    /// scalar representation. The underlying field is the same; only the
    /// wrapper crate version differs.
    pub fn v1_fr_to_current(fr: transient_crypto_old::curve::Fr) -> transient_crypto::curve::Fr {
        // Both Fr types wrap outer::Scalar which is midnight_curves::Fq.
        // The byte representation is canonical, so round-tripping through
        // bytes is safe.
        let bytes = fr.as_le_bytes();
        transient_crypto::curve::Fr::from_le_bytes(&bytes)
            .expect("v1 Fr should round-trip to current Fr")
    }

    /// Converts a current `ProofPreimage` into a v1 `ProofPreimage`.
    pub fn current_preimage_to_v1(
        preimage: &transient_crypto::proofs::ProofPreimage,
    ) -> std::io::Result<transient_crypto_old::proofs::ProofPreimage> {
        let mut buf = Vec::new();
        serialize::Serializable::serialize(preimage, &mut buf)?;
        serialize_old::Deserializable::deserialize(&mut &buf[..], 0)
    }
}
