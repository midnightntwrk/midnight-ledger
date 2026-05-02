//! Compose the initial `ContractState` a midnight-did deploy needs.
//!
//! Mirrors `Contract.initialState(constructorContext)` from
//! `midnight-did-contract/dist/managed/did/contract/index.js:691`,
//! but constructed directly in Rust against the workspace's
//! `onchain-state` types — no Compact VM execution needed because
//! the constructor's behaviour is small and fully captured by
//! `did.compact:102-112`:
//!
//! ```compact
//! constructor() {
//!   contractVersion = 1;
//!   id = kernel.self();              // resolved by runtime
//!   active = true;
//!   deactivated = false;
//!   controllerPublicKey = disclose(publicKey(localSecretKey()));
//!   const timestamp = disclose(currentTimestamp());
//!   created = timestamp;
//!   updated = timestamp;
//! }
//! ```
//!
//! The state tree's shape is the dual of `did/contract.rs`'s
//! decoder — root is `StateValue::Array([constants, mutable])`,
//! constants is `[contractVersion, controllerPublicKey]` and
//! mutable is the 15-entry sequence
//! `[id, alsoKnownAs, version, created, updated, deactivated,
//! active, operationCount, verificationMethods, +5 relations,
//! services]` whose indices we already extracted from the
//! ledger accessors in `index.js`.
//!
//! `ContractDeploy::address()` is `SHA-256(tagged_serialize(self))`,
//! so given a stable `(initial_state, nonce)` pair the new DID's
//! address is fully determined client-side. That lets us **preview**
//! the DID id before any extrinsic is submitted.

use base_crypto::fab::AlignedValue;
use base_crypto::hash::HashOutput;
use coin_structure::contract::ContractAddress;
use ledger::structure::ContractDeploy;
use onchain_state::state::{
    ChargedState, ContractMaintenanceAuthority, ContractOperation, ContractState,
    EntryPointBuf, StateValue,
};
use rand::RngCore;
use storage::DefaultDB;
use storage::arena::Sp;
use storage::storage::{Array, HashMap as StorageHashMap};

use crate::did::error::DidError;
use crate::did::id::{ContractAddressBytes, DidId};
use crate::network::Network;

/// The 11 entry-point names did.compact exposes. Order taken from
/// `Contract.initialState`'s `setOperation` calls
/// (`midnight-did-contract/.../contract/index.js`).
pub(crate) const DID_ENTRY_POINTS: &[&str] = &[
    "addAlsoKnownAs",
    "removeAlsoKnownAs",
    "addVerificationMethod",
    "updateVerificationMethod",
    "removeVerificationMethod",
    "addVerificationMethodRelation",
    "removeVerificationMethodRelation",
    "addService",
    "updateService",
    "removeService",
    "deactivate",
];

/// Compose the initial `ContractState` produced by `did.compact`'s
/// constructor at deploy time.
///
/// `controller_pk_commitment` is the 32-byte
/// `persistentHash(["did:controller:pk"+pad32, sk])` value the
/// constructor stores — see [`crate::Wallet::did_controller_public_key`].
/// `timestamp_ms` is the unix-ms `currentTimestamp()` witness that
/// flows into both `created` and `updated`.
pub(crate) fn compose_initial_state(
    controller_pk_commitment: [u8; 32],
    timestamp_ms: u64,
) -> ContractState<DefaultDB> {
    let constants = state_array(vec![
        // contractVersion: Uint<32> = 1
        cell_u32(1),
        // controllerPublicKey: Bytes<32>
        cell_bytes32(controller_pk_commitment),
    ]);

    let mutable = state_array(vec![
        // id: ContractAddress (zero — kernel.self() resolves at runtime,
        //                       client-side preview pre-resolution)
        cell_bytes32([0u8; 32]),
        // alsoKnownAs: Set<string> — empty Map<key, ()>
        empty_map(),
        // version: Counter (Uint<64>) = 0
        cell_u64(0),
        // created: Uint<64>
        cell_u64(timestamp_ms),
        // updated: Uint<64>
        cell_u64(timestamp_ms),
        // deactivated: Boolean = false
        cell_bool(false),
        // active: Boolean = true
        cell_bool(true),
        // operationCount: Counter = 0
        cell_u64(0),
        // verificationMethods: Map<string, VerificationMethod>
        empty_map(),
        // 5 relation Sets
        empty_map(),
        empty_map(),
        empty_map(),
        empty_map(),
        empty_map(),
        // services: Map<string, Service>
        empty_map(),
    ]);

    let root = state_array(vec![constants, mutable]);

    // Operations table: 11 named entry points, each with no
    // verifier-key today. Verifier keys are loaded later via
    // MaintenanceUpdate, matching the upstream pattern (the JS
    // `setOperation(name, new ContractOperation())` is also
    // empty at deploy time).
    let mut operations = StorageHashMap::<EntryPointBuf, ContractOperation, DefaultDB>::new();
    for name in DID_ENTRY_POINTS {
        let key = EntryPointBuf(name.as_bytes().to_vec());
        operations = operations.insert(key, ContractOperation::new(None));
    }

    ContractState {
        data: ChargedState::new(root),
        operations,
        maintenance_authority: ContractMaintenanceAuthority::default(),
        balance: StorageHashMap::new(),
    }
}

/// Build a deterministic `ContractDeploy` for a given controller
/// commitment + timestamp + nonce. The deploy's address is
/// `SHA-256(tagged_serialize(self))` so callers can preview the
/// resulting DID id before any extrinsic is submitted.
pub(crate) fn compose_deploy(
    controller_pk_commitment: [u8; 32],
    timestamp_ms: u64,
    nonce: [u8; 32],
) -> ContractDeploy<DefaultDB> {
    ContractDeploy {
        initial_state: compose_initial_state(controller_pk_commitment, timestamp_ms),
        nonce: HashOutput(nonce),
    }
}

/// Compose the deploy + return the address as a `DidId` on the
/// chosen network. `nonce` is generated from the supplied RNG so
/// callers can produce deterministic previews from a seeded RNG
/// in tests.
pub(crate) fn preview_did_id<R: RngCore>(
    rng: &mut R,
    network: Network,
    controller_pk_commitment: [u8; 32],
    timestamp_ms: u64,
) -> Result<DidId, DidError> {
    let mut nonce = [0u8; 32];
    rng.fill_bytes(&mut nonce);
    let deploy = compose_deploy(controller_pk_commitment, timestamp_ms, nonce);
    let addr: ContractAddress = deploy.address();
    let bytes: ContractAddressBytes = addr.0.0;
    Ok(DidId::new(network, bytes))
}

// ─── tree-construction helpers ─────────────────────────────────────
//
// We build cells through the workspace's `From<T> for AlignedValue`
// blanket impl (`base-crypto/src/fab/conversions.rs`) instead of
// hand-rolling `Value` + `Alignment` pairs. The conversions normalise
// `ValueAtom`s (strip trailing zeros) which `AlignedValue::new`'s
// `is_in_normal_form` check requires; doing it by hand is fragile.

fn state_array(elems: Vec<StateValue<DefaultDB>>) -> StateValue<DefaultDB> {
    let mut arr = Array::<StateValue<DefaultDB>, DefaultDB>::new();
    for e in elems {
        arr = arr.push(e);
    }
    StateValue::Array(arr)
}

fn empty_map() -> StateValue<DefaultDB> {
    StateValue::Map(StorageHashMap::new())
}

/// Wrap an `AlignedValue` as a `StateValue::Cell`.
fn cell(av: AlignedValue) -> StateValue<DefaultDB> {
    StateValue::Cell(Sp::new(av))
}

/// Compact `Uint<32>` — 4-byte little-endian (workspace convention,
/// see `From<u128> for ValueAtom`).
fn cell_u32(value: u32) -> StateValue<DefaultDB> {
    cell(AlignedValue::from(value))
}

/// Compact `Uint<64>` / `Counter` — 8-byte little-endian.
fn cell_u64(value: u64) -> StateValue<DefaultDB> {
    cell(AlignedValue::from(value))
}

/// `Bytes<32>` cell — 32 raw bytes (a [`HashOutput`]-shaped slot).
/// Goes through `HashOutput`'s `From` impl so the alignment is
/// `Bytes { length: 32 }` and the atom is normalised.
fn cell_bytes32(bytes: [u8; 32]) -> StateValue<DefaultDB> {
    cell(AlignedValue::from(HashOutput(bytes)))
}

/// `Boolean` cell.
fn cell_bool(b: bool) -> StateValue<DefaultDB> {
    cell(AlignedValue::from(b))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand_chacha::ChaCha20Rng;
    use rand::SeedableRng;

    #[test]
    fn initial_state_decodes_back_to_constructor_values() {
        // Round-trip: build the initial state, serialise it as
        // `did/contract.rs::decode_did_ledger_state` would see it,
        // and confirm the scalar fields match.
        let pk = [0xabu8; 32];
        let ts = 1_777_840_000_000u64;
        let state = compose_initial_state(pk, ts);

        // Serialise through tagged_serialize so the decoder gets
        // the same bytes the indexer would return.
        let mut buf = Vec::new();
        serialize::tagged_serialize(&state, &mut buf)
            .expect("tagged_serialize");
        let hex = hex::encode(&buf);
        let decoded = crate::did::contract::decode_did_ledger_state(&hex)
            .expect("decode");

        assert_eq!(decoded.contract_version, 1, "contractVersion");
        assert_eq!(decoded.controller_public_key, pk, "controllerPublicKey");
        assert_eq!(decoded.id_bytes, [0u8; 32], "id starts at zero");
        assert_eq!(decoded.version, 0, "version");
        assert!(decoded.active, "active");
        assert!(!decoded.deactivated, "deactivated");
        assert_eq!(decoded.operation_count, 0, "operationCount");
        assert_eq!(decoded.mutable_field_count, 15, "mutable field count");
    }

    #[test]
    fn deploy_address_is_deterministic() {
        let pk = [0x42u8; 32];
        let ts = 1_777_840_000_000u64;
        let nonce = [0x99u8; 32];
        let a = compose_deploy(pk, ts, nonce).address();
        let b = compose_deploy(pk, ts, nonce).address();
        assert_eq!(a.0.0, b.0.0);
    }

    #[test]
    fn deploy_address_differs_per_nonce() {
        let pk = [0x42u8; 32];
        let ts = 1_777_840_000_000u64;
        let a = compose_deploy(pk, ts, [0x01u8; 32]).address();
        let b = compose_deploy(pk, ts, [0x02u8; 32]).address();
        assert_ne!(a.0.0, b.0.0);
    }

    #[test]
    fn preview_did_id_round_trips_through_codec() {
        let mut rng = ChaCha20Rng::seed_from_u64(0xdeadbeef);
        let id = preview_did_id(&mut rng, Network::PreProd, [0x77u8; 32], 0)
            .expect("preview");
        let s = id.to_did_string();
        let back = DidId::parse(&s).expect("parse");
        assert_eq!(back, id);
    }
}
