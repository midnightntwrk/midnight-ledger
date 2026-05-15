//! Build an unproven `MaintenanceUpdate` transaction. Same shape
//! as `tx::build_deploy` but with a `Maintain` action instead of
//! `Deploy`. Used to load verifier keys for the 11 DID circuits
//! onto a freshly-deployed contract whose maintenance committee
//! is set to the wallet's BIP340 verifying key.

use base_crypto::signatures::{Signature, SigningKey};
use base_crypto::time::Timestamp;
use coin_structure::contract::ContractAddress;
use ledger::structure::{
    Intent, MaintenanceUpdate, ProofPreimageMarker, SignaturesValue, SingleUpdate,
    StandardTransaction, Transaction,
};
use rand::{CryptoRng, Rng};
use storage::DefaultDB;
use storage::storage::{Array, HashMap};
use transient_crypto::commitment::PedersenRandomness;

use super::TxError;
use super::build::UnprovenTx;

/// Segment slot for the maintenance intent. Distinct from the
/// deploy segment (1, see `tx/build.rs`) and the dust-balance
/// segment (0xFEED, see `tx/balance.rs`).
const MAINTAIN_SEGMENT: u16 = 2;

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

    let updates = {
        let mut a = Array::new();
        a = a.push(single);
        a
    };

    // Sign the data-to-sign before populating `signatures`, then
    // attach. The `data_to_sign()` covers address + updates + counter
    // (NOT signatures), so signing before/after attach doesn't
    // matter — but it's cleanest to compute it from a partially-
    // built update.
    let probe = MaintenanceUpdate::<DefaultDB> {
        address: contract_address,
        updates: updates.clone(),
        counter,
        signatures: Array::new(),
    };
    let payload = probe.data_to_sign();
    let sig = sk.sign(rng, &payload);
    let signatures = {
        let mut a = Array::new();
        a = a.push(SignaturesValue(0u32, sig));
        a
    };

    let upd: MaintenanceUpdate<DefaultDB> = MaintenanceUpdate {
        address: contract_address,
        updates,
        counter,
        signatures,
    };

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
