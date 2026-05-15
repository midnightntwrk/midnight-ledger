//! Build an unproven deploy transaction. Pure function — no I/O,
//! decoupled from Wallet so the caller (Wallet::create_did in
//! Task 11) supplies the inputs.

use base_crypto::signatures::Signature;
use base_crypto::time::Timestamp;
use ledger::structure::{
    GUARANTEED_SEGMENT, Intent, ProofPreimageMarker, StandardTransaction, Transaction,
};
use rand::{CryptoRng, Rng};
use storage::DefaultDB;
use storage::storage::HashMap;
use transient_crypto::commitment::PedersenRandomness;

use crate::did::deploy::compose_deploy;
use super::TxError;

pub(crate) type UnprovenTx = Transaction<
    Signature,
    ProofPreimageMarker,
    PedersenRandomness,
    DefaultDB,
>;

#[allow(dead_code)] // Wired by Wallet::create_did in Task 11.
pub(crate) fn build_deploy<R: Rng + CryptoRng>(
    pk_commitment: [u8; 32],
    network_id: &str,
    timestamp_ms: u64,
    nonce: [u8; 32],
    ttl: Timestamp,
    rng: &mut R,
) -> Result<UnprovenTx, TxError> {
    let deploy = compose_deploy(pk_commitment, timestamp_ms, nonce);

    let intent: Intent<Signature, ProofPreimageMarker, PedersenRandomness, DefaultDB> =
        Intent::empty(rng, ttl);
    let intent = intent.add_deploy(deploy);

    let mut intents = HashMap::new();
    intents = intents.insert(GUARANTEED_SEGMENT, intent);

    let stx = StandardTransaction::new(network_id, intents, None, HashMap::new());
    Ok(Transaction::Standard(stx))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;
    use rand_chacha::ChaCha20Rng;

    #[test]
    fn builds_a_standard_transaction_with_one_intent() {
        let mut rng = ChaCha20Rng::seed_from_u64(0x42);
        let pk = [0xabu8; 32];
        let now = 1_777_840_000_000u64;
        let nonce = [0x99u8; 32];
        let ttl = Timestamp::from_secs(now / 1000 + 3600);

        let tx = build_deploy(pk, "undeployed", now, nonce, ttl, &mut rng)
            .expect("build");

        match &tx {
            Transaction::Standard(stx) => {
                let deploys: Vec<_> = stx.deploys().collect();
                assert_eq!(deploys.len(), 1);
            }
            _ => panic!("expected Transaction::Standard"),
        }
    }

    #[test]
    fn build_is_deterministic_per_inputs() {
        let mut rng_a = ChaCha20Rng::seed_from_u64(0x42);
        let mut rng_b = ChaCha20Rng::seed_from_u64(0x42);
        let pk = [0xabu8; 32];
        let now = 1_777_840_000_000u64;
        let nonce = [0x99u8; 32];
        let ttl = Timestamp::from_secs(now / 1000 + 3600);

        let a = build_deploy(pk, "undeployed", now, nonce, ttl, &mut rng_a).unwrap();
        let b = build_deploy(pk, "undeployed", now, nonce, ttl, &mut rng_b).unwrap();

        let mut ba = Vec::new();
        let mut bb = Vec::new();
        serialize::tagged_serialize(&a, &mut ba).unwrap();
        serialize::tagged_serialize(&b, &mut bb).unwrap();
        assert_eq!(ba, bb);
    }
}
