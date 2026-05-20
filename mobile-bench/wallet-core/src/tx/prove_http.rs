//! `HttpProvingProvider` — same `ProvingProvider` trait surface as
//! `zkir_v2::LocalProvingProvider`, but each per-circuit `prove`
//! call is delegated to a remote `midnight-proof-server` over HTTP
//! instead of running the in-process zkir prover.
//!
//! The proof-server runs release-built with a `WorkerPool`
//! ([proof-server/src/worker_pool.rs:200](file:///Users/ysh/iohk/midnight-ledger/proof-server/src/worker_pool.rs:200))
//! so individual proofs complete in seconds rather than the
//! many-minutes our debug-built `LocalProvingProvider` takes.
//! Upstream's `httpClientProofProvider` (JS, used by
//! `midnight-did-manager-service`) hits the same `/prove` endpoint
//! with the same `(ProofPreimageVersioned, Option<ProvingKeyMaterial>,
//! Option<Fr>)` wire format.
//!
//! The dioxus-wallet App boots an embedded proof-server when the
//! `js-bridge` feature is on (see
//! [bridge.rs:243](file:///Users/ysh/iohk/midnight-ledger/.claude/worktrees/thirsty-lovelace-092f50/mobile-bench/dioxus-wallet/src/bridge.rs:243))
//! — that's the URL we route to here.
//!
//! `check()` runs locally — the proof-server doesn't expose a
//! /check endpoint and `ProofPreimage::check` is a cheap local
//! computation against the IR.

use std::sync::Arc;

use base_crypto::rng::SplittableRng;
use ledger::prove::Resolver;
use ledger::structure::{ProofPreimageVersioned, ProofVersioned};
use rand::{CryptoRng, Rng};
use serialize::{tagged_deserialize, tagged_serialize};
use transient_crypto::curve::Fr;
use transient_crypto::proofs::{
    Proof, ProofPreimage, ProvingKeyMaterial, ProvingProvider, Resolver as ResolverTrait,
};
use zkir_v2::IrSource;

/// `ProvingProvider` impl that POSTs each `ProofPreimage` to a
/// `midnight-proof-server` `/prove` endpoint. The local `Resolver`
/// is still used to look up the per-circuit `ProvingKeyMaterial`
/// (the proof-server's built-in resolver only knows zswap + dust;
/// it can't fetch DID circuit keys), then the keys are sent
/// alongside the preimage so the server skips its own resolution
/// step (`endpoints.rs:291-302`).
pub(crate) struct HttpProvingProvider<'a, R> {
    /// RNG kept for `split()` parity with `LocalProvingProvider`.
    /// Not used for anything else — the server uses its own RNG.
    pub rng: R,
    pub resolver: &'a Resolver,
    pub base_url: String,
    pub client: reqwest::Client,
}

impl<'a, R: Rng + CryptoRng + SplittableRng> ProvingProvider
    for HttpProvingProvider<'a, R>
{
    async fn check(
        &self,
        preimage: &ProofPreimage,
    ) -> Result<Vec<Option<usize>>, anyhow::Error> {
        // Same as `LocalProvingProvider::check`. The check runs
        // against the bundled IR; no network round-trip needed.
        let proving_data = self
            .resolver
            .resolve_key(preimage.key_location.clone())
            .await?
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "attempted to check proof for '{}' without circuit data",
                    preimage.key_location.0,
                )
            })?;
        let ir: IrSource = tagged_deserialize(&mut &proving_data.ir_source[..])?;
        preimage.check(&ir)
    }

    async fn prove(
        self,
        preimage: &ProofPreimage,
        overwrite_binding_input: Option<Fr>,
    ) -> Result<Proof, anyhow::Error> {
        // 1. Resolve key material locally. The proof-server's
        //    built-in resolver doesn't know DID circuits, so we
        //    bundle the keys with the request and the server uses
        //    them directly (`endpoints.rs:291-302`).
        let pkm: ProvingKeyMaterial = self
            .resolver
            .resolve_key(preimage.key_location.clone())
            .await?
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "no proving key material for '{}'",
                    preimage.key_location.0,
                )
            })?;

        // 2. Build the request payload — same shape upstream's
        //    `httpClientProofProvider` uses. Tagged-serialised
        //    `(ProofPreimageVersioned::V2, Some(pkm),
        //    overwrite_binding_input)`.
        let payload: (
            ProofPreimageVersioned,
            Option<ProvingKeyMaterial>,
            Option<Fr>,
        ) = (
            ProofPreimageVersioned::V2(Arc::new(preimage.clone())),
            Some(pkm),
            overwrite_binding_input,
        );
        let mut body = Vec::new();
        tagged_serialize(&payload, &mut body)
            .map_err(|e| anyhow::anyhow!("serialize prove payload: {e}"))?;

        // 3. POST. The proof-server's `/prove` is a long-running
        //    handler (the worker pool can queue while a proof
        //    runs); a 10-minute ceiling matches the JS
        //    `httpClientProofProvider` default.
        let url = format!("{}/prove", self.base_url.trim_end_matches('/'));
        let resp = self
            .client
            .post(&url)
            .body(body)
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("POST {url}: {e}"))?;

        let status = resp.status();
        let bytes = resp
            .bytes()
            .await
            .map_err(|e| anyhow::anyhow!("read response: {e}"))?;
        if !status.is_success() {
            return Err(anyhow::anyhow!(
                "proof-server {status}: {}",
                String::from_utf8_lossy(&bytes),
            ));
        }

        // 4. Response is a tagged-serialised `ProofVersioned::V2`.
        let versioned: ProofVersioned =
            tagged_deserialize(&bytes[..]).map_err(|e| {
                anyhow::anyhow!("deserialize ProofVersioned: {e}")
            })?;
        let ProofVersioned::V2(proof) = versioned else {
            return Err(anyhow::anyhow!(
                "unexpected ProofVersioned variant (server upgrade?)"
            ));
        };
        Ok(proof)
    }

    fn split(&mut self) -> Self {
        Self {
            rng: self.rng.split(),
            resolver: self.resolver,
            base_url: self.base_url.clone(),
            client: self.client.clone(),
        }
    }
}
