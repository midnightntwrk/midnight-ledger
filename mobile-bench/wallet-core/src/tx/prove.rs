//! Generate ZK proofs for the DUST spend offers added during
//! balancing. The deploy itself carries no proof preimages —
//! ContractDeploy's payload is `(initial_state, nonce)` — but
//! each DUST spend the balancer added is a ProofPreimage that
//! must become a Proof before SCALE encoding.

use base_crypto::data_provider::{FetchMode, MidnightDataProvider, OutputMode};
use base_crypto::rng::SplittableRng;
use base_crypto::signatures::Signature;
use ledger::prove::Resolver;
use ledger::structure::{ProofMarker, Transaction};
use onchain_runtime::cost_model::INITIAL_COST_MODEL;
use rand::{CryptoRng, Rng};
use storage::DefaultDB;
use transient_crypto::commitment::PureGeneratorPedersen;
use transient_crypto::proofs::ProvingKeyMaterial;
use zkir_v2::LocalProvingProvider;
use zswap::prove::ZswapResolver;
use zswap::ZSWAP_EXPECTED_FILES;

use crate::artifacts::dust::dust_resolver;
use crate::did::artifacts::circuit_artifacts;
use super::TxError;
use super::build::UnprovenTx;

/// The `KeyLocation` prefix the low-level
/// `inspectCircuit` path embeds. The newer
/// `createUnprovenCallTxFromInitialStates` path used by
/// `prepareUnprovenCallTx` emits the bare circuit name instead
/// — both shapes are handled below.
const DID_KEY_LOCATION_PREFIX: &str = "midnight/did/";

/// Final proved-and-sealed tx — same shape as
/// `test_utilities::TxBound<S, D>`. The chain expects this exact
/// header tag `transaction[v9](signature[v1],proof,pedersen-schnorr[v1])`;
/// the unsealed `PedersenRandomness` form (`embedded-fr[v1]`) is
/// rejected with "Invalid Transaction (1010)".
pub(crate) type ProvenTx = Transaction<Signature, ProofMarker, PureGeneratorPedersen, DefaultDB>;

/// Build a `Resolver` with bundled DUST keys + fetched zswap
/// params + an external resolver that serves the 11 DID circuit
/// prover keys from `crate::did::artifacts::CIRCUIT_ARTIFACTS`.
/// Every DID `ProofPreimage` carries `key_location =
/// "midnight/did/<circuit>"`; the closure below strips that
/// prefix, looks the matching `CircuitArtifacts` up, and returns
/// `ProvingKeyMaterial { prover_key, verifier_key, ir_source =
/// bzkir }`.
fn build_resolver() -> Result<Resolver, TxError> {
    let zswap = ZswapResolver(
        MidnightDataProvider::new(
            FetchMode::OnDemand,
            OutputMode::Log,
            ZSWAP_EXPECTED_FILES.to_owned(),
        )
        .map_err(|e| TxError::Prove(format!("zswap params: {e}")))?,
    );
    let dust = dust_resolver().map_err(|e| TxError::Prove(format!("dust resolver: {e}")))?;
    Ok(Resolver::new(
        zswap,
        dust,
        Box::new(|loc| {
            let path = loc.0.into_owned();
            Box::pin(async move {
                // Two key-location shapes flow in from the two
                // harness paths: prefixed (`midnight/did/<name>`)
                // from the low-level `inspectCircuit` path, and
                // bare (`<name>`) from
                // `createUnprovenCallTxFromInitialStates`. Strip
                // the prefix if present, then look up.
                let name = path
                    .strip_prefix(DID_KEY_LOCATION_PREFIX)
                    .unwrap_or(&path);
                let Some(art) = circuit_artifacts(name) else {
                    return Ok(None);
                };
                Ok(Some(ProvingKeyMaterial {
                    prover_key: art.prover_key.to_vec(),
                    verifier_key: art.verifier_key.to_vec(),
                    ir_source: art.bzkir.to_vec(),
                }))
            })
        }),
    ))
}

#[allow(dead_code)] // Wired by Wallet::create_did in Task 11.
pub(crate) async fn prove<R: Rng + CryptoRng + SplittableRng>(
    tx: UnprovenTx,
    mut rng: R,
) -> Result<ProvenTx, TxError> {
    let resolver = build_resolver()?;
    let provider = LocalProvingProvider {
        rng: rng.split(),
        params: &resolver,
        resolver: &resolver,
    };
    let proved = tx
        .prove(provider, &INITIAL_COST_MODEL)
        .await
        .map_err(|e| TxError::Prove(e.to_string()))?;
    // Seal: PedersenRandomness → PureGeneratorPedersen so the
    // serialized tx carries the `pedersen-schnorr[v1]` header tag
    // the chain's deserializer expects. Without this, the node
    // rejects with "Invalid Transaction (1010)".
    Ok(proved.seal(rng))
}

/// Same as [`prove`], but routes each per-preimage `prove` call to
/// a `midnight-proof-server` `/prove` endpoint instead of running
/// the in-process zkir prover. Used by `Wallet::call_did_circuit`
/// when the App has booted an embedded proof-server (see
/// [bridge.rs:243](file:///Users/ysh/iohk/midnight-ledger/.claude/worktrees/thirsty-lovelace-092f50/mobile-bench/dioxus-wallet/src/bridge.rs:243)).
/// Matches what upstream's `httpClientProofProvider` does in the
/// `midnight-did-manager-service` flow — release-built server with
/// a worker pool, so a debug-built wallet doesn't pay the
/// multi-minute proving penalty per call.
///
/// `base_url` is e.g. `http://127.0.0.1:57610` — no trailing
/// `/prove`, the provider appends that itself.
#[allow(dead_code)]
pub(crate) async fn prove_via_http<R: Rng + CryptoRng + SplittableRng>(
    tx: UnprovenTx,
    mut rng: R,
    base_url: String,
) -> Result<ProvenTx, TxError> {
    tracing::info!(target: "wallet-core", url = %base_url, "proving via HTTP proof-server");
    let resolver = build_resolver()?;
    let provider = super::prove_http::HttpProvingProvider {
        rng: rng.split(),
        resolver: &resolver,
        base_url,
        client: reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(600))
            .build()
            .map_err(|e| TxError::Prove(format!("http client: {e}")))?,
    };
    let proved = tx
        .prove(provider, &INITIAL_COST_MODEL)
        .await
        .map_err(|e| TxError::Prove(e.to_string()))?;
    Ok(proved.seal(rng))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::rngs::StdRng;

    /// Typecheck-only. Real exercise lives in Task 12's live
    /// integration test (the proof step is heavy and requires
    /// the bundled DUST artifacts). StdRng implements
    /// SplittableRng; ChaCha20Rng doesn't.
    #[test]
    fn prove_signature_typechecks() {
        let _: fn(UnprovenTx, StdRng) -> _ = prove::<StdRng>;
    }
}
