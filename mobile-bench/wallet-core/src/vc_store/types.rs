use serde::{Deserialize, Serialize};

/// Persisted VC envelope. `body` is the canonical signed bytes the
/// issuer returned — the Compact serialization. `format` allows
/// future non-Compact VC families to coexist.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredVc {
    pub vc_uri: String,
    pub issuer_did: String,
    pub holder_did: String,
    pub format: String, // e.g. "midnight-vc-compact"
    pub body: Vec<u8>,
    pub issued_at_ms: u64,
}

/// One private claim's value + opening randomness, keyed by JSON-Pointer-style path.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VcOpening {
    pub vc_uri: String,
    pub claim_path: String, // e.g. "/credentialSubject/dateOfBirth"
    pub plaintext: Vec<u8>,
    pub opening: Vec<u8>,
}

/// Display + telemetry data. Mutates over the VC's lifetime.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct VcMetadata {
    pub vc_uri: String,
    pub display_order: u32,
    pub last_verified_ms: Option<u64>,
    pub last_verify_outcome: Option<String>, // "Valid" | "Invalid: <reason>" — see vc_self_verify
    pub custom_labels: Vec<(String, String)>,
}
