//! Build an unproven `MaintenanceUpdate` transaction. Same shape
//! as `tx::build_deploy` but with a `Maintain` action instead of
//! `Deploy`. Used to load verifier keys for the 11 DID circuits
//! onto a freshly-deployed contract whose maintenance committee
//! is set to the wallet's BIP340 verifying key.

use base_crypto::signatures::{Signature, SigningKey};
use base_crypto::time::Timestamp;
use coin_structure::contract::ContractAddress;
use ledger::structure::{
    Intent, MaintenanceUpdate, ProofPreimageMarker, SingleUpdate, StandardTransaction, Transaction,
};
use rand::{CryptoRng, Rng};
use storage::DefaultDB;
use storage::storage::HashMap;
use transient_crypto::commitment::PedersenRandomness;

use super::TxError;
use super::build::UnprovenTx;

/// Segment slot for the maintenance intent. The reference
/// `test_utilities::test_intents` always puts maintenance updates
/// at segment 1; deviating from that triggers an
/// `InvalidDustSpendProof` somewhere in the dust intent's tree
/// state check (root cause not yet pinned down — pragmatic fix
/// is to match the reference layout).
const MAINTAIN_SEGMENT: u16 = 1;

/// Compose a `MaintenanceUpdate` carrying a single
/// `VerifierKeyInsert(entry_point, V3(vk))` update, sign its
/// `data_to_sign()` with `sk`, and wrap in a Transaction::Standard.
///
/// The signature index in `signatures` is 0 — this is the
/// position of `sk.verifying_key()` in the contract's
/// `ContractMaintenanceAuthority.committee`. With our threshold-1
/// single-member-committee deployment, that's always 0.
#[allow(dead_code)] // Wired by Wallet::load_did_circuit in the wallet pipeline.
pub(crate) fn build_load_verifier_key<R: Rng + CryptoRng>(
    contract_address: ContractAddress,
    entry_point: &str,
    verifier_key: transient_crypto::proofs::VerifierKey,
    counter: u32,
    sk: &SigningKey,
    network_id: &str,
    ttl: Timestamp,
    rng: &mut R,
) -> Result<UnprovenTx, TxError> {
    let single = SingleUpdate::VerifierKeyInsert(
        onchain_state::state::EntryPointBuf(entry_point.as_bytes().to_vec()),
        ledger::structure::ContractOperationVersionedVerifierKey::V3(verifier_key),
    );

    // Use the canonical constructor + add_signature path
    // (`ledger/src/construct.rs:299-318`). `add_signature` sorts
    // signatures internally; manual struct construction was a
    // candidate cause of an InvalidDustSpendProof regression.
    let upd: MaintenanceUpdate<DefaultDB> =
        MaintenanceUpdate::new(contract_address, vec![single], counter);
    let payload = upd.data_to_sign();
    let sig = sk.sign(rng, &payload);
    let upd = upd.add_signature(0u32, sig);

    let intent: Intent<Signature, ProofPreimageMarker, PedersenRandomness, DefaultDB> =
        Intent::empty(rng, ttl);
    let intent = intent.add_maintenance_update(upd);

    let mut intents = HashMap::new();
    intents = intents.insert(MAINTAIN_SEGMENT, intent);

    let stx = StandardTransaction::new(network_id, intents, None, HashMap::new());
    Ok(Transaction::Standard(stx))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Compile-only typecheck. Real exercise: the live wallet
    /// pipeline test once the integration test for "load
    /// addVerificationMethod" is wired.
    #[test]
    fn signature_typechecks() {
        fn _check<R: Rng + CryptoRng>() {
            let _ = build_load_verifier_key::<R>;
        }
    }
}
