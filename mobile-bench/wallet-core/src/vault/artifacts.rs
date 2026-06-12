//! Compiled `passport-vault.compact` circuit artifacts, embedded via
//! `include_bytes!` from `contracts/passport-vault/`. That dir is gitignored
//! and regenerated on every `cargo build` by `wallet-core/build.rs`, which
//! copies the blobs from
//! `midnight-identity-solution-examples/packages/contracts/vault/src/managed/
//! passport-vault/{keys,zkir}` (override with `MIDNIGHT_VAULT_MANAGED_DIR`).
//!
//! Same bundle shape as [`crate::did::artifacts`] (prover key,
//! verifier key, bzkir, zkir source per circuit). The prove pipeline
//! ([`crate::tx::prove::build_resolver`]) looks the per-circuit prover
//! key up by bare circuit name — the names here
//! (`depositFunds` / `claimFunds` / ...) don't collide with the DID
//! circuit names, so a single resolver can serve both contracts.

#![allow(dead_code)] // surface lights up when call_contract_circuit wires through

use crate::did::artifacts::CircuitArtifacts;

/// DRY helper around the `include_bytes!` path templating for the
/// vendored passport-vault circuit blobs. Mirrors `did::artifacts`'s
/// `full_bundle!`.
macro_rules! vault_bundle {
    ($name:literal) => {
        CircuitArtifacts {
            name: $name,
            prover_key: include_bytes!(concat!(
                "../../contracts/passport-vault/",
                $name,
                ".prover"
            )),
            verifier_key: include_bytes!(concat!(
                "../../contracts/passport-vault/",
                $name,
                ".verifier"
            )),
            bzkir: include_bytes!(concat!(
                "../../contracts/passport-vault/",
                $name,
                ".bzkir"
            )),
            zkir_json: include_bytes!(concat!(
                "../../contracts/passport-vault/",
                $name,
                ".zkir"
            )),
        }
    };
}

/// Full artifact bundle for every passport-vault circuit. The prover
/// step in [`crate::tx::prove`] looks the right entry up by name.
///
/// Multi-lock circuits (see `passport-vault.compact`): `createLock`
/// (defines a lock's policy + optional seed deposit), `depositToLock`
/// (creator top-up), `claimFromLock` (policy-gated redeem), and
/// `withdrawFromLock` (creator reclaim). `passportPolicyRequestFor`
/// is a PURE circuit (no prover key), so it has no bundle here.
pub(crate) const VAULT_CIRCUIT_ARTIFACTS: &[CircuitArtifacts] = &[
    vault_bundle!("createLock"),
    vault_bundle!("depositToLock"),
    vault_bundle!("claimFromLock"),
    vault_bundle!("withdrawFromLock"),
];

/// Look up the full artifact bundle for `name`. Returns `None` if no
/// passport-vault circuit with that name is bundled.
pub(crate) fn vault_circuit_artifacts(name: &str) -> Option<&'static CircuitArtifacts> {
    VAULT_CIRCUIT_ARTIFACTS.iter().find(|c| c.name == name)
}

/// All passport-vault circuit entry-point names, in registry order.
pub(crate) const VAULT_CIRCUIT_NAMES: &[&str] = &[
    "createLock",
    "depositToLock",
    "claimFromLock",
    "withdrawFromLock",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_vault_circuit_artifacts_present_and_non_empty() {
        assert_eq!(VAULT_CIRCUIT_ARTIFACTS.len(), VAULT_CIRCUIT_NAMES.len());
        for art in VAULT_CIRCUIT_ARTIFACTS {
            assert!(!art.prover_key.is_empty(), "{} prover key empty", art.name);
            assert!(!art.verifier_key.is_empty(), "{} verifier key empty", art.name);
            assert!(!art.bzkir.is_empty(), "{} bzkir empty", art.name);
        }
    }

    #[test]
    fn create_and_claim_resolve_by_name() {
        assert!(vault_circuit_artifacts("createLock").is_some());
        assert!(vault_circuit_artifacts("depositToLock").is_some());
        assert!(vault_circuit_artifacts("claimFromLock").is_some());
        assert!(vault_circuit_artifacts("withdrawFromLock").is_some());
        assert!(vault_circuit_artifacts("doesNotExist").is_none());
    }
}
