//! Digital-passport credential request/response flow.
//!
//! Handles the passport-issuer's OID4VCI credential endpoint which
//! uses a richer request body (includes `midnight` extension with
//! holder binding and public key) and a different response shape
//! (compact-value-v1.base64url encoding, holderBinding, and
//! field-name-keyed openings).

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::Deserialize;

use crate::clock::Clock;
use crate::http::HttpClient;
use crate::js_bridge::JsBridge;
use crate::oid4vci_client::token::TokenResponse;
use crate::oid4vci_client::credential::CredentialFlowError;
use crate::oid4vci_client::proof::CredentialCoordinator;
use crate::secret_storage::SecretStorage;
use crate::vc_store::{StoredVc, VcOpening, VcStorage};
use crate::wallet::Wallet;
use crate::DidId;

// ---------------------------------------------------------------------------
// Wire types for passport-issuer credential response
// ---------------------------------------------------------------------------

/// Top-level OID4VCI credential response from the passport issuer.
#[derive(Debug, Clone, Deserialize)]
pub struct DigitalPassportResponse {
    pub credential: DigitalPassportCredential,
    #[serde(default)]
    pub openings: Vec<DigitalPassportOpening>,
}

/// The `credential` object inside the passport-issuer response.
#[derive(Debug, Clone, Deserialize)]
pub struct DigitalPassportCredential {
    /// The Compact-serialized credential body, encoded as base64url.
    pub credential: CompactValueField,
    /// The detached proof, encoded as base64url.
    #[serde(rename = "credentialProof")]
    pub credential_proof: CompactValueField,
    /// Holder binding information returned by the issuer.
    #[serde(rename = "holderBinding")]
    pub holder_binding: HolderBinding,
}

/// A `{ encoding, payload }` pair. The only encoding we support
/// right now is `"compact-value-v1.base64url"`.
#[derive(Debug, Clone, Deserialize)]
pub struct CompactValueField {
    pub encoding: String,
    pub payload: String,
}

/// Holder binding from the issuer response.
#[derive(Debug, Clone, Deserialize)]
pub struct HolderBinding {
    #[serde(rename = "holderDidMethod")]
    pub holder_did_method: HolderDidMethod,
}

/// The `holderDidMethod` sub-object.
#[derive(Debug, Clone, Deserialize)]
pub struct HolderDidMethod {
    pub did: String,
    #[serde(rename = "methodId")]
    #[allow(dead_code)]
    pub method_id: String,
    #[serde(rename = "keyType")]
    #[allow(dead_code)]
    pub key_type: String,
}

/// One opening in the passport-issuer response. The `fieldName`
/// is the claim name (e.g. `"dateOfBirth"`) which we map to a
/// JSON-Pointer-style `claim_path` (`/credentialSubject/dateOfBirth`).
#[derive(Debug, Clone, Deserialize)]
pub struct DigitalPassportOpening {
    #[serde(rename = "fieldName")]
    pub field_name: String,
    #[serde(rename = "plaintextB64")]
    pub plaintext_b64: String,
    #[serde(rename = "openingB64")]
    pub opening_b64: String,
}

// ---------------------------------------------------------------------------
// Decoded credential response (after base64url decode)
// ---------------------------------------------------------------------------

/// Result of decoding and landing a digital-passport credential.
pub struct DecodedPassportCredential {
    pub body: Vec<u8>,
    pub proof: Vec<u8>,
    pub holder_did: String,
}

// ---------------------------------------------------------------------------
// JS bridge response shape for issuer_did extraction
// ---------------------------------------------------------------------------

/// Response shape from `decodeDigitalPassportCredential` JS bridge call.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct DecodedCredential {
    #[serde(rename = "issuerDid")]
    pub issuer_did: String,
}

// ---------------------------------------------------------------------------
// Credential request
// ---------------------------------------------------------------------------

/// Drive the full Pre-Authorized Code Flow for a digital-passport
/// credential:
///
/// 1. `/token` → access token + c_nonce
/// 2. Build JWS proof + midnight extension (holder binding + Jubjub
///    public key)
/// 3. POST `{format, credential_configuration_id, proof, midnight}`
///    to the credential endpoint
/// 4. Decode the response, extract `issuer_did` via the JS bridge,
///    generate a client-side `vc_uri`, and land in `vc_store`
pub async fn request_credential(
    http: &dyn HttpClient,
    clock: &dyn Clock,
    js_bridge: &dyn JsBridge,
    credential_issuer: &str,
    token_endpoint: &str,
    credential_endpoint: &str,
    format: &str,
    credential_configuration_id: &str,
    pre_authorized_code: &str,
    coordinator: &CredentialCoordinator,
    _wallet: &Wallet,
    secret_store: &dyn SecretStorage,
    holder_did: &DidId,
    vc_store: &dyn VcStorage,
) -> Result<String, CredentialFlowError> {
    // 1. Obtain access token + c_nonce
    let token: TokenResponse =
        crate::oid4vci_client::token::request_token(http, token_endpoint, pre_authorized_code)
            .await?;

    // 2. Build JWS proof over c_nonce through the coordinator's
    //    proof builder. The coordinator wraps the
    //    IdTokenProofBuilder which calls sign_id_token_with_ports
    //    — same single-resolve / single-sign discipline OID4VP
    //    uses, with aud = credential_issuer and nonce = c_nonce.
    let proof = coordinator
        .proof_builder
        .build(credential_issuer, &token.c_nonce)
        .await?;

    // 3. Derive the Jubjub holder public key from the secret store.
    //    The assertion key is stored with kid = "<did>#key-assert".
    let holder_did_str = holder_did.to_did_string();
    let assert_kid = format!("{holder_did_str}#key-assert");
    let assert_ref = secret_store
        .find_by_kid(&assert_kid)
        .await
        .ok_or_else(|| {
            CredentialFlowError::SecretStore(format!(
                "no key with kid={assert_kid} in secret store"
            ))
        })?;
    let jwk = secret_store
        .get_public_key(assert_ref.uuid())
        .await
        .map_err(|e| CredentialFlowError::SecretStore(e.to_string()))?;

    // Decode x/y from base64url JWK to hex strings for the midnight
    // extension. Jubjub keys have both x and y coordinates.
    let x_bytes = URL_SAFE_NO_PAD
        .decode(jwk.x.as_bytes())
        .map_err(|e| CredentialFlowError::SecretStore(format!("decode Jubjub x: {e}")))?;
    let y_bytes = URL_SAFE_NO_PAD
        .decode(jwk.y.as_ref().ok_or_else(|| {
            CredentialFlowError::SecretStore("Jubjub JWK missing y coordinate".into())
        })?.as_bytes())
        .map_err(|e| CredentialFlowError::SecretStore(format!("decode Jubjub y: {e}")))?;

    // Hex-encode the 32-byte coordinates — the midnight extension
    // sends these as 64-char hex strings.
    let x_hex = hex::encode(&x_bytes);
    let y_hex = hex::encode(&y_bytes);

    // 4. Build the credential request body with the midnight extension.
    let body = serde_json::json!({
        "format": format,
        "credential_configuration_id": credential_configuration_id,
        "proof": {
            "proof_type": proof.proof_type,
            "jwt": proof.payload,
        },
        "midnight": {
            "holderBinding": {
                "method": "explicit_did_method",
                "challenge": token.c_nonce,
                "holderDidMethod": {
                    "did": holder_did_str,
                    "methodId": "#key-assert",
                    "keyType": "jubjub",
                },
            },
            "holderPublicKey": {
                "x": x_hex,
                "y": y_hex,
            },
            "requestedClaims": [],
        },
    });

    // 5. POST credential request
    let url = credential_endpoint.to_string();
    let resp = http
        .post_json(&url, &body, Some(&token.access_token))
        .await?;
    let text = resp
        .body_text()
        .map_err(|e| CredentialFlowError::Http(e))?
        .to_string();
    if !resp.is_success() {
        return Err(CredentialFlowError::Status {
            status: resp.status,
            body: text,
        });
    }

    // 6. Parse the passport-issuer response
    let response: DigitalPassportResponse = serde_json::from_str(&text)?;

    // 6a. Validate encoding field
    if response.credential.credential.encoding != "compact-value-v1.base64url" {
        return Err(CredentialFlowError::Decode(serde::de::Error::custom(format!(
            "unsupported credential encoding: {}",
            response.credential.credential.encoding
        ))));
    }
    if response.credential.credential_proof.encoding != "compact-value-v1.base64url" {
        return Err(CredentialFlowError::Decode(serde::de::Error::custom(format!(
            "unsupported credentialProof encoding: {}",
            response.credential.credential_proof.encoding
        ))));
    }

    // 6b. Decode credential body and proof from base64url
    let credential_body = URL_SAFE_NO_PAD
        .decode(&response.credential.credential.payload)
        .map_err(|e| CredentialFlowError::Base64(e))?;
    let credential_proof = URL_SAFE_NO_PAD
        .decode(&response.credential.credential_proof.payload)
        .map_err(|e| CredentialFlowError::Base64(e))?;

    let holder_did_str = response.credential.holder_binding.holder_did_method.did;

    // 7. Extract issuer_did via JS bridge
    let issuer_did = decode_issuer_did(js_bridge, &response.credential.credential.payload).await?;
    let issuer_did = issuer_did.issuer_did;

    // 8. Generate client-side vc_uri with a digital-passport
    //    namespace prefix so the UI's `is_digital_passport`
    //    heuristic can route these VCs to `DigitalPassportCard`
    //    instead of the generic row renderer.
    let vc_uri = format!("urn:vc:digital-passport:{}", uuid::Uuid::new_v4());

    // 9. Build StoredVc
    let vc = StoredVc {
        vc_uri: vc_uri.clone(),
        issuer_did,
        holder_did: holder_did_str,
        format: format.to_string(),
        body: credential_body,
        proof: credential_proof,
        issued_at_ms: clock.now_ms(),
    };

    // 10. Map openings with /credentialSubject/{fieldName} convention
    let openings: Vec<VcOpening> = response
        .openings
        .into_iter()
        .map(|o| {
            Ok(VcOpening {
                vc_uri: vc.vc_uri.clone(),
                claim_path: format!("/credentialSubject/{}", o.field_name),
                plaintext: URL_SAFE_NO_PAD.decode(&o.plaintext_b64)?,
                opening: URL_SAFE_NO_PAD.decode(&o.opening_b64)?,
            })
        })
        .collect::<Result<_, base64::DecodeError>>()?;

    vc_store
        .insert_vc_with_openings(&vc, &openings)
        .map_err(|e| CredentialFlowError::Store(e.to_string()))?;

    Ok(vc.vc_uri)
}

/// Call the JS bridge to decode the compact credential payload and
/// extract the `issuerDid`.
async fn decode_issuer_did(
    js_bridge: &dyn JsBridge,
    credential_payload_b64url: &str,
) -> Result<DecodedCredential, CredentialFlowError> {
    use crate::js_bridge::JsBridgeExt;
    let params = serde_json::json!({
        "encoded": {
            "encoding": "compact-value-v1.base64url",
            "payload": credential_payload_b64url,
        },
    });
    js_bridge
        .call::<_, DecodedCredential>("decodeDigitalPassportCredential", &params)
        .await
        .map_err(|e| CredentialFlowError::JsBridge(e.to_string()))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::clock::FixedClock;
    use crate::http::mock::MockHttpClient;
    use crate::js_bridge::JsBridge;
    use crate::test_support::{
        stub_authn_discovery, stub_did_signer,
        stub_secret_store_with_bootstrapped_did, stub_wallet_with_bootstrapped_did,
    };
    use crate::vc_store::InMemoryVcStore;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use serde_json::json;

    /// Mock JS bridge that returns a fixed issuer_did for
    /// `decodeDigitalPassportCredential` calls.
    struct MockJsBridge {
        issuer_did: String,
    }

    #[async_trait::async_trait]
    impl JsBridge for MockJsBridge {
        async fn call_json(
            &self,
            method: &str,
            _params: serde_json::Value,
        ) -> Result<serde_json::Value, crate::js_bridge::JsBridgeError> {
            match method {
                "decodeDigitalPassportCredential" => Ok(serde_json::json!({
                    "issuerDid": self.issuer_did,
                })),
                _ => Err(crate::js_bridge::JsBridgeError::JsError(format!(
                    "unknown method: {method}"
                ))),
            }
        }
    }

    #[tokio::test]
    async fn request_credential_passport_round_trip() {
        let http = MockHttpClient::default();
        // 1. /token
        http.push_json(
            200,
            &json!({
                "access_token": "AT",
                "c_nonce": "CN",
                "token_type": "Bearer",
                "expires_in": 600,
            }),
        );

        // 2. /credential — passport-issuer response shape
        let body_payload = URL_SAFE_NO_PAD.encode(b"PASSPORT_CREDENTIAL_BODY");
        let proof_payload = URL_SAFE_NO_PAD.encode(b"PASSPORT_CREDENTIAL_PROOF");
        let plaintext_b64 = URL_SAFE_NO_PAD.encode(b"1985-01-01");
        let opening_b64 = URL_SAFE_NO_PAD.encode(b"opening-bytes");

        http.push_json(
            200,
            &json!({
                "credential": {
                    "credential": {
                        "encoding": "compact-value-v1.base64url",
                        "payload": body_payload,
                    },
                    "credentialProof": {
                        "encoding": "compact-value-v1.base64url",
                        "payload": proof_payload,
                    },
                    "holderBinding": {
                        "holderDidMethod": {
                            "did": "did:midnight:holder123",
                            "methodId": "#key-assert",
                            "keyType": "jubjub",
                        }
                    }
                },
                "openings": [
                    {
                        "fieldName": "dateOfBirth",
                        "plaintextB64": plaintext_b64,
                        "openingB64": opening_b64,
                    }
                ]
            }),
        );

        let seed = [23u8; 32];
        let (wallet, did) = stub_wallet_with_bootstrapped_did(seed).await;
        let store = stub_secret_store_with_bootstrapped_did(seed).await;
        let vc_store = InMemoryVcStore::default();
        let clock = FixedClock::new(1_700_000_001_000);
        let js_bridge = MockJsBridge {
            issuer_did: "did:midnight:issuer456".to_string(),
        };
        // Build the coordinator with the canonical JWT proof builder
        // — same pattern as the worker's handle_oid4vci_issuance.
        let discovery = stub_authn_discovery(wallet);
        let signer = stub_did_signer(store);
        let clock_arc: Arc<dyn crate::clock::Clock> = Arc::new(FixedClock::new(1_700_000_001_000));
        let coordinator = crate::oid4vci_client::CredentialCoordinator::jwt(
            crate::oid4vci_client::IdTokenProofBuilder::new(
                discovery,
                signer,
                clock_arc,
                did,
            ),
        );

        let vc_uri = request_credential(
            &http,
            &clock,
            &js_bridge,
            "https://passport-issuer.local",
            "https://passport-issuer.local/token",
            "https://passport-issuer.local/credential",
            "midnight_compact_vc",
            "digital_passport_v1",
            "CODE-PASSPORT",
            &coordinator,
            &wallet,
            &store,
            &did,
            &vc_store,
        )
        .await
        .expect("ok");

        // vc_uri should use the digital-passport namespace prefix
        // so the UI can route it to DigitalPassportCard.
        assert!(vc_uri.starts_with("urn:vc:digital-passport:"));

        let landed = vc_store.get_vc(&vc_uri).unwrap().expect("present");
        assert_eq!(landed.body, b"PASSPORT_CREDENTIAL_BODY");
        assert_eq!(landed.proof, b"PASSPORT_CREDENTIAL_PROOF");
        assert_eq!(landed.issuer_did, "did:midnight:issuer456");
        assert_eq!(landed.holder_did, "did:midnight:holder123");
        assert_eq!(landed.format, "midnight_compact_vc");
        assert_eq!(landed.issued_at_ms, 1_700_000_001_000);

        // Verify openings use /credentialSubject/{fieldName} convention
        let op = vc_store
            .get_opening(&vc_uri, "/credentialSubject/dateOfBirth")
            .unwrap()
            .expect("opening present");
        assert_eq!(op.plaintext, b"1985-01-01");
        assert_eq!(op.opening, b"opening-bytes");

        // Verify request body shape: midnight extension included
        let rec = http.recorded();
        assert_eq!(rec.len(), 2);
        assert_eq!(rec[0].url, "https://passport-issuer.local/token");
        assert!(rec[0].bearer.is_none());

        assert_eq!(rec[1].url, "https://passport-issuer.local/credential");
        assert_eq!(rec[1].bearer.as_deref(), Some("AT"));

        let posted = rec[1].body.as_ref().expect("credential body");
        assert_eq!(posted["format"], "midnight_compact_vc");
        assert_eq!(posted["credential_configuration_id"], "digital_passport_v1");
        assert_eq!(posted["proof"]["proof_type"], "jwt");
        assert!(posted["proof"]["jwt"].is_string());

        // Midnight extension
        let midnight = &posted["midnight"];
        assert_eq!(midnight["holderBinding"]["method"], "explicit_did_method");
        assert_eq!(midnight["holderBinding"]["challenge"], "CN");
        assert_eq!(
            midnight["holderBinding"]["holderDidMethod"]["methodId"],
            "#key-assert"
        );
        assert_eq!(
            midnight["holderBinding"]["holderDidMethod"]["keyType"],
            "jubjub"
        );
        assert!(midnight["holderBinding"]["holderDidMethod"]["did"].is_string());
        assert!(midnight["holderPublicKey"]["x"].is_string());
        assert!(midnight["holderPublicKey"]["y"].is_string());
        // x and y should be 64-char hex strings (32 bytes each)
        let x_str = midnight["holderPublicKey"]["x"].as_str().unwrap();
        let y_str = midnight["holderPublicKey"]["y"].as_str().unwrap();
        assert_eq!(x_str.len(), 64, "Jubjub x coordinate should be 64 hex chars");
        assert_eq!(y_str.len(), 64, "Jubjub y coordinate should be 64 hex chars");
        assert!(midnight["requestedClaims"].is_array());

        // Verify did in holder binding matches the holder_did we passed
        assert_eq!(
            midnight["holderBinding"]["holderDidMethod"]["did"],
            did.to_did_string()
        );
    }

    #[tokio::test]
    async fn request_credential_passport_missing_assertion_key_fails() {
        let http = MockHttpClient::default();
        http.push_json(
            200,
            &json!({
                "access_token": "AT",
                "c_nonce": "CN",
                "token_type": "Bearer",
            }),
        );

        // Use a bootstrapped wallet/secret store so the Ed25519
        // #key-auth key exists for the JWS proof, but then
        // delete the #key-assert Jubjub key so the passport
        // flow can't derive the holder public key.
        let seed = [99u8; 32];
        let (wallet, did) = stub_wallet_with_bootstrapped_did(seed).await;
        let mut store = stub_secret_store_with_bootstrapped_did(seed).await;
        let did_str = did.to_did_string();
        let assert_kid = format!("{did_str}#key-assert");
        let assert_ref = store.find_by_kid(&assert_kid).await.expect("key should exist");
        store.delete_key(assert_ref.uuid()).await.expect("delete should work");

        let vc_store = InMemoryVcStore::default();
        let clock = FixedClock::new(1_700_000_001_000);
        let js_bridge = MockJsBridge {
            issuer_did: "did:midnight:test".to_string(),
        };

        let discovery = stub_authn_discovery(wallet);
        let signer = stub_did_signer(store);
        let clock_arc: Arc<dyn crate::clock::Clock> = Arc::new(FixedClock::new(1_700_000_001_000));
        let coordinator = crate::oid4vci_client::CredentialCoordinator::jwt(
            crate::oid4vci_client::IdTokenProofBuilder::new(
                discovery,
                signer,
                clock_arc,
                did,
            ),
        );

        let err = request_credential(
            &http,
            &clock,
            &js_bridge,
            "https://passport-issuer.local",
            "https://passport-issuer.local/token",
            "https://passport-issuer.local/credential",
            "midnight_compact_vc",
            "digital_passport_v1",
            "CODE",
            &coordinator,
            &wallet,
            &store,
            &did,
            &vc_store,
        )
        .await
        .expect_err("should fail without assertion key");

        match err {
            CredentialFlowError::SecretStore(msg) => {
                assert!(msg.contains("#key-assert"), "error should mention #key-assert: {msg}");
            }
            other => panic!("expected SecretStore error, got {other:?}"),
        }
    }
}