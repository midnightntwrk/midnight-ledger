//! Compiled `did.compact` artifacts vendored from
//! `midnight-did/contract/dist/managed/did/`.
//!
//! Each circuit ships three files:
//! - `<name>.prover`   — proving key material (PKM) bytes
//! - `<name>.verifier` — verifier key bytes
//! - `<name>.zkir`     — JSON IR (human-readable, for debug)
//! - `<name>.bzkir`    — binary IR (loaded at prove time)
//!
//! We `include_bytes!` only the circuits we need today:
//! `addVerificationMethod` (used by `Wallet::create_did` to write
//! the initial VM after the contract is deployed). Phase 4 lands
//! the remaining 10 circuits in their own commits.
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

const ROOT: &str = "../contracts/midnight-did";

pub(crate) const ADD_VERIFICATION_METHOD: CircuitArtifacts = CircuitArtifacts {
    name: "addVerificationMethod",
    prover_key: include_bytes!(concat!(
        "../../contracts/midnight-did/addVerificationMethod.prover"
    )),
    verifier_key: include_bytes!(concat!(
        "../../contracts/midnight-did/addVerificationMethod.verifier"
    )),
    bzkir: include_bytes!(concat!(
        "../../contracts/midnight-did/addVerificationMethod.bzkir"
    )),
    zkir_json: include_bytes!(concat!(
        "../../contracts/midnight-did/addVerificationMethod.zkir"
    )),
};

/// Just for tests / diagnostics.
#[allow(dead_code)]
const _: &str = ROOT;

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
        // Cheap shape check: starts with `{` and ends with `}`.
        let trimmed = s.trim();
        assert!(trimmed.starts_with('{'));
        assert!(trimmed.ends_with('}'));
    }
}
