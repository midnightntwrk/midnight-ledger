#![cfg(all(feature = "proof-server-http", not(target_os = "android")))]

use std::sync::Arc;
use std::time::{Duration, Instant};

use ledger::structure::{ProofPreimageVersioned, ProofVersioned};
use serialize::{tagged_deserialize, tagged_serialize};
use transient_crypto::curve::Fr;
use transient_crypto::proofs::{PARAMS_VERIFIER, ProvingKeyMaterial, Zkir};

use crate::zkir_example::{LABEL, minimal_preimage};
use crate::{Error, ProofRun, ProverCore, Result};

impl ProverCore {
    /// Proves the minimal zkir example by POSTing to a midnight-proof-server
    /// instance, then verifies the returned proof locally to confirm
    /// transport-equivalence with the in-process library path.
    pub async fn prove_via_http(&self, base_url: &str) -> Result<ProofRun> {
        let resolver = self.keygen_minimal().await?;
        let k = resolver.ir.k();
        let pkm = resolver
            .proving_key_material()
            .map_err(|e| Error::Anyhow(anyhow::anyhow!("serialize keys: {e}")))?;

        let preimage = minimal_preimage();
        let binding_input = preimage.binding_input;

        let body = build_payload(preimage, pkm)?;

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(600))
            .build()
            .map_err(|e| Error::Anyhow(anyhow::anyhow!("client: {e}")))?;

        let started = Instant::now();
        let resp = client
            .post(format!("{base_url}/prove"))
            .body(body)
            .send()
            .await
            .map_err(|e| Error::Anyhow(anyhow::anyhow!("post: {e}")))?;
        let status = resp.status();
        let response_bytes = resp
            .bytes()
            .await
            .map_err(|e| Error::Anyhow(anyhow::anyhow!("body: {e}")))?
            .to_vec();
        let elapsed = started.elapsed();

        if !status.is_success() {
            let body = String::from_utf8_lossy(&response_bytes);
            return Err(Error::Anyhow(anyhow::anyhow!(
                "/prove returned {status}: {body}"
            )));
        }

        let versioned: ProofVersioned = tagged_deserialize(&response_bytes[..])
            .map_err(|e| Error::Anyhow(anyhow::anyhow!("deserialize proof: {e}")))?;
        let ProofVersioned::V2(proof) = versioned else {
            return Err(Error::Anyhow(anyhow::anyhow!(
                "unexpected ProofVersioned variant"
            )));
        };

        let v_started = Instant::now();
        let verified = resolver
            .vk
            .verify(&PARAMS_VERIFIER, &proof, std::iter::once(binding_input))
            .is_ok();
        let verify_elapsed = v_started.elapsed();

        let mut proof_bytes = Vec::new();
        tagged_serialize(&proof, &mut proof_bytes)
            .map_err(|e| Error::Anyhow(anyhow::anyhow!("serialize proof: {e}")))?;

        Ok(ProofRun {
            label: LABEL,
            k,
            elapsed,
            verify_elapsed: Some(verify_elapsed),
            verified: Some(verified),
            proof_bytes,
        })
    }
}

fn build_payload(
    preimage: transient_crypto::proofs::ProofPreimage,
    pkm: ProvingKeyMaterial,
) -> Result<Vec<u8>> {
    // Mirrors proof-server/src/endpoints.rs:256-260 — the /prove endpoint
    // tagged-deserializes (ProofPreimageVersioned, Option<ProvingKeyMaterial>,
    // Option<Fr>). Supplying Some(pkm) makes the server skip key resolution,
    // which it would otherwise fail since "minimal" isn't a known KeyLocation.
    let triple: (
        ProofPreimageVersioned,
        Option<ProvingKeyMaterial>,
        Option<Fr>,
    ) = (
        ProofPreimageVersioned::V2(Arc::new(preimage)),
        Some(pkm),
        None,
    );
    let mut buf = Vec::new();
    tagged_serialize(&triple, &mut buf)
        .map_err(|e| Error::Anyhow(anyhow::anyhow!("serialize payload: {e}")))?;
    Ok(buf)
}
