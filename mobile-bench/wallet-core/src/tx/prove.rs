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
use zkir_v2::LocalProvingProvider;
use zswap::prove::ZswapResolver;
use zswap::ZSWAP_EXPECTED_FILES;

use crate::artifacts::dust::dust_resolver;
use super::TxError;
use super::build::UnprovenTx;

/// Final proved-and-sealed tx — same shape as
/// `test_utilities::TxBound<S, D>`. The chain expects this exact
/// header tag `transaction[v9](signature[v1],proof,pedersen-schnorr[v1])`;
/// the unsealed `PedersenRandomness` form (`embedded-fr[v1]`) is
/// rejected with "Invalid Transaction (1010)".
pub(crate) type ProvenTx = Transaction<Signature, ProofMarker, PureGeneratorPedersen, DefaultDB>;

/// Build a `Resolver` with bundled DUST keys + fetched zswap
/// params. The external_resolver returns None for every key
/// location since DID write circuits (which would need their
/// own proving keys) are out of scope for this slice.
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
        Box::new(|_loc| Box::pin(std::future::ready(Ok(None)))),
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
