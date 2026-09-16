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

#[macro_use]
extern crate tracing;

pub mod ir;
pub mod ir_instructions;
pub mod ir_types;
pub mod ir_vm;

pub use ir::{Identifier, Instruction, IrSource};
pub use ir_vm::Preprocessed;

use base_crypto::rng::SplittableRng;
use rand::{CryptoRng, Rng};

/// Implements `ProvingProvider` for zkir-v3 circuits.
pub struct LocalProvingProvider<'a, R: Rng + CryptoRng + SplittableRng, S, P> {
    pub rng: R,
    pub resolver: &'a S,
    pub params: &'a P,
}

impl<
    'a,
    R: Rng + CryptoRng + SplittableRng,
    S: transient_crypto::proofs::Resolver,
    P: transient_crypto::proofs::ParamsProverProvider,
> transient_crypto::proofs::ProvingProvider for LocalProvingProvider<'a, R, S, P>
{
    async fn check(
        &self,
        preimage: &transient_crypto::proofs::ProofPreimage,
    ) -> Result<Vec<Option<usize>>, anyhow::Error> {
        use transient_crypto::proofs::Zkir as _;
        let proving_data = self
            .resolver
            .resolve_key(preimage.key_location.clone())
            .await?
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "attempted to check proof for '{}' without circuit data!",
                    preimage.key_location.0
                )
            })?;
        let ir = IrSource::load_ir_from_tagged(std::io::Cursor::new(&proving_data.ir_source[..]))?;
        ir.check(preimage)
    }

    async fn prove(
        self,
        preimage: &transient_crypto::proofs::ProofPreimage,
        overwrite_binding_input: Option<transient_crypto::curve::Fr>,
    ) -> Result<transient_crypto::proofs::Proof, anyhow::Error> {
        let mut preimage = preimage.clone();
        if let Some(binding_input) = overwrite_binding_input {
            preimage.binding_input = binding_input;
        }
        let (proof, _) = preimage
            .prove::<IrSource>(self.rng, self.params, self.resolver)
            .await?;
        Ok(proof)
    }

    fn split(&mut self) -> Self {
        Self {
            rng: self.rng.split(),
            resolver: self.resolver,
            params: self.params,
        }
    }

    fn resolver(&self) -> &impl transient_crypto::proofs::Resolver {
        self.resolver
    }
}
