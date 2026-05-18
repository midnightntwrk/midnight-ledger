//! Compiled `did.compact` artifacts vendored from
//! `midnight-did/contract/dist/managed/did/`.
//!
//! Two bundle shapes coexist:
//!
//! - [`CIRCUIT_ARTIFACTS`] — full bundles (prover key, verifier
//!   key, bzkir, zkir source) for every circuit. The prover
//!   pipeline ([`tx::prove`]) looks the per-circuit prover key
//!   up by `KeyLocation = "midnight/did/<name>"`.
//! - [`VERIFIER_KEYS`] — verifier-only registry. Used by
//!   `Wallet::load_did_circuit` to push any of the 11 circuits
//!   onto a freshly-deployed DID via MaintenanceUpdate. Bytes
//!   are tagged-serialized `transient_crypto::proofs::VerifierKey`.
//!
//! The Compact source is also vendored at
//! `contracts/midnight-did/did.compact` for documentation — it's
//! the canonical reference for state-field ordering used by
//! `wallet-core::did::contract`'s decoder.

#![allow(dead_code)] // surface lights up when Wallet::create_did wires through

/// Bundle of artifacts for a single circuit.
pub(crate) struct CircuitArtifacts {
    pub name: &'static str,
    pub prover_key: &'static [u8],
    pub verifier_key: &'static [u8],
    pub bzkir: &'static [u8],
    /// Human-readable JSON IR. Useful for diagnostics; not loaded
    /// at prove time. Same circuit as `bzkir` but in source form.
    pub zkir_json: &'static [u8],
}

/// Helper for the full-bundle constants below — DRY around the
/// `include_bytes!` path templating. Could be a `macro_rules!`
/// but `concat!` already does the heavy lifting at the call site.
macro_rules! full_bundle {
    ($name:literal) => {
        CircuitArtifacts {
            name: $name,
            prover_key: include_bytes!(concat!("../../contracts/midnight-did/", $name, ".prover")),
            verifier_key: include_bytes!(concat!(
                "../../contracts/midnight-did/",
                $name,
                ".verifier"
            )),
            bzkir: include_bytes!(concat!("../../contracts/midnight-did/", $name, ".bzkir")),
            zkir_json: include_bytes!(concat!("../../contracts/midnight-did/", $name, ".zkir")),
        }
    };
}

pub(crate) const ADD_VERIFICATION_METHOD: CircuitArtifacts =
    full_bundle!("addVerificationMethod");

/// Full-bundle registry for every DID circuit (11 entries).
/// Order matches `CIRCUIT_NAMES`. `Wallet::call_did_circuit`'s
/// prover step looks the right entry up by name.
pub(crate) const CIRCUIT_ARTIFACTS: &[CircuitArtifacts] = &[
    full_bundle!("addAlsoKnownAs"),
    full_bundle!("addService"),
    full_bundle!("addVerificationMethod"),
    full_bundle!("addVerificationMethodRelation"),
    full_bundle!("deactivate"),
    full_bundle!("removeAlsoKnownAs"),
    full_bundle!("removeService"),
    full_bundle!("removeVerificationMethod"),
    full_bundle!("removeVerificationMethodRelation"),
    full_bundle!("updateService"),
    full_bundle!("updateVerificationMethod"),
];

/// Look up the full artifact bundle for `name`. Returns `None`
/// if no circuit with that name is bundled.
pub(crate) fn circuit_artifacts(name: &str) -> Option<&'static CircuitArtifacts> {
    CIRCUIT_ARTIFACTS.iter().find(|c| c.name == name)
}

/// Verifier-only registry: every DID circuit's verifier key,
/// keyed by the camelCase entry-point name the contract uses.
/// Sorted by name. `Wallet::load_did_circuit` calls
/// [`verifier_key_bytes`] to pick the right entry.
pub(crate) const VERIFIER_KEYS: &[(&str, &[u8])] = &[
    (
        "addAlsoKnownAs",
        include_bytes!("../../contracts/midnight-did/addAlsoKnownAs.verifier"),
    ),
    (
        "addService",
        include_bytes!("../../contracts/midnight-did/addService.verifier"),
    ),
    (
        "addVerificationMethod",
        include_bytes!("../../contracts/midnight-did/addVerificationMethod.verifier"),
    ),
    (
        "addVerificationMethodRelation",
        include_bytes!("../../contracts/midnight-did/addVerificationMethodRelation.verifier"),
    ),
    (
        "deactivate",
        include_bytes!("../../contracts/midnight-did/deactivate.verifier"),
    ),
    (
        "removeAlsoKnownAs",
        include_bytes!("../../contracts/midnight-did/removeAlsoKnownAs.verifier"),
    ),
    (
        "removeService",
        include_bytes!("../../contracts/midnight-did/removeService.verifier"),
    ),
    (
        "removeVerificationMethod",
        include_bytes!("../../contracts/midnight-did/removeVerificationMethod.verifier"),
    ),
    (
        "removeVerificationMethodRelation",
        include_bytes!("../../contracts/midnight-did/removeVerificationMethodRelation.verifier"),
    ),
    (
        "updateService",
        include_bytes!("../../contracts/midnight-did/updateService.verifier"),
    ),
    (
        "updateVerificationMethod",
        include_bytes!("../../contracts/midnight-did/updateVerificationMethod.verifier"),
    ),
];

/// All circuit entry-point names, in registry order.
pub(crate) const CIRCUIT_NAMES: &[&str] = &[
    "addAlsoKnownAs",
    "addService",
    "addVerificationMethod",
    "addVerificationMethodRelation",
    "deactivate",
    "removeAlsoKnownAs",
    "removeService",
    "removeVerificationMethod",
    "removeVerificationMethodRelation",
    "updateService",
    "updateVerificationMethod",
];

/// Look up the raw verifier bytes for `name`. Returns `None` if
/// the name doesn't match any bundled circuit.
pub(crate) fn verifier_key_bytes(name: &str) -> Option<&'static [u8]> {
    VERIFIER_KEYS
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, b)| *b)
}

/// Parse the bundled verifier bytes for `name` into a typed
/// `VerifierKey`. Returns `None` if `name` is unknown; bubbles up
/// the IO error if the bytes don't decode (which would indicate a
/// bundled-asset corruption).
pub(crate) fn parsed_verifier_key_by_name(
    name: &str,
) -> Option<Result<transient_crypto::proofs::VerifierKey, std::io::Error>> {
    verifier_key_bytes(name).map(|bytes| {
        serialize::tagged_deserialize::<transient_crypto::proofs::VerifierKey>(bytes)
    })
}

impl CircuitArtifacts {
    /// Parse the bundled `.verifier` bytes into a typed
    /// `VerifierKey` (tagged-serialized form). Consumed by the
    /// MaintenanceUpdate pipeline that loads circuits onto a
    /// freshly-deployed DID contract.
    #[allow(dead_code)] // Wired by tx::maintain in the follow-up commit.
    pub(crate) fn parsed_verifier_key(
        &self,
    ) -> Result<transient_crypto::proofs::VerifierKey, std::io::Error> {
        serialize::tagged_deserialize::<transient_crypto::proofs::VerifierKey>(self.verifier_key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_verification_method_artifacts_present() {
        assert!(!ADD_VERIFICATION_METHOD.prover_key.is_empty());
        assert!(!ADD_VERIFICATION_METHOD.verifier_key.is_empty());
        assert!(!ADD_VERIFICATION_METHOD.bzkir.is_empty());
        assert!(!ADD_VERIFICATION_METHOD.zkir_json.is_empty());
    }

    #[test]
    fn zkir_json_is_valid_json() {
        let s = std::str::from_utf8(ADD_VERIFICATION_METHOD.zkir_json).expect("utf-8");
        let trimmed = s.trim();
        assert!(trimmed.starts_with('{'));
        assert!(trimmed.ends_with('}'));
    }

    #[test]
    fn all_eleven_verifier_keys_bundled_and_non_empty() {
        assert_eq!(VERIFIER_KEYS.len(), 11);
        assert_eq!(CIRCUIT_NAMES.len(), 11);
        for (name, bytes) in VERIFIER_KEYS {
            assert!(!bytes.is_empty(), "{name} verifier bundle is empty");
        }
    }

    #[test]
    fn all_eleven_verifier_keys_decode() {
        for (name, _) in VERIFIER_KEYS {
            parsed_verifier_key_by_name(name)
                .expect("registry hit")
                .unwrap_or_else(|e| panic!("decode {name}: {e}"));
        }
    }

    #[test]
    fn verifier_key_bytes_match_add_verification_method_full_bundle() {
        // The registry lookup must agree with the full-bundle
        // constant (otherwise we'd have two sources of truth).
        let from_registry = verifier_key_bytes("addVerificationMethod").expect("hit");
        assert_eq!(from_registry, ADD_VERIFICATION_METHOD.verifier_key);
    }

    #[test]
    fn verifier_key_bytes_returns_none_for_unknown_circuit() {
        assert!(verifier_key_bytes("doesNotExist").is_none());
    }
}
