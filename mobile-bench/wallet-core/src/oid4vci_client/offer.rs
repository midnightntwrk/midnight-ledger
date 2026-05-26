//! Parser for the `openid-credential-offer://` QR URL.
//!
//! The QR carries one query param, `credential_offer`, whose value is
//! a URL-encoded JSON object matching the OID4VCI spec. We only need
//! the issuer URL + the pre-authorized code grant to drive the
//! `/token` + `/credential` endpoints.

use serde::{Deserialize, Serialize};
use url::Url;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CredentialOffer {
    pub credential_issuer: String,
    pub credential_configuration_ids: Vec<String>,
    pub grants: Grants,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Grants {
    #[serde(rename = "urn:ietf:params:oauth:grant-type:pre-authorized_code")]
    pub pre_authorized: PreAuthorized,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreAuthorized {
    #[serde(rename = "pre-authorized_code")]
    pub code: String,
}

#[derive(Debug, thiserror::Error)]
pub enum Oid4vciParseError {
    #[error("bad scheme: {0}")]
    BadScheme(String),
    #[error("missing query param: {0}")]
    MissingParam(&'static str),
    #[error("url parse: {0}")]
    Url(#[from] url::ParseError),
    #[error("json parse: {0}")]
    Json(#[from] serde_json::Error),
}

/// Extract + parse the `credential_offer=<json>` query param from
/// an `openid-credential-offer://` URL. The JSON value is
/// URL-encoded per the OID4VCI spec.
pub fn parse_offer_url(url: &str) -> Result<CredentialOffer, Oid4vciParseError> {
    let u = Url::parse(url)?;
    if u.scheme() != "openid-credential-offer" {
        return Err(Oid4vciParseError::BadScheme(u.scheme().into()));
    }
    let raw = u
        .query_pairs()
        .find(|(k, _)| k == "credential_offer")
        .map(|(_, v)| v.into_owned())
        .ok_or(Oid4vciParseError::MissingParam("credential_offer"))?;
    let offer: CredentialOffer = serde_json::from_str(&raw)?;
    Ok(offer)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_offer_url_works() {
        let offer_json = serde_json::json!({
            "credential_issuer": "https://issuer.local",
            "credential_configuration_ids": ["birth"],
            "grants": {
                "urn:ietf:params:oauth:grant-type:pre-authorized_code": {
                    "pre-authorized_code": "CODE-XYZ"
                }
            }
        })
        .to_string();
        let url = format!(
            "openid-credential-offer://issuer/?credential_offer={}",
            urlencoding::encode(&offer_json)
        );
        let offer = parse_offer_url(&url).expect("parse");
        assert_eq!(offer.credential_issuer, "https://issuer.local");
        assert_eq!(
            offer.credential_configuration_ids,
            vec!["birth".to_string()]
        );
        assert_eq!(offer.grants.pre_authorized.code, "CODE-XYZ");
    }

    #[test]
    fn parse_offer_url_rejects_wrong_scheme() {
        assert!(matches!(
            parse_offer_url("https://issuer/?credential_offer=%7B%7D"),
            Err(Oid4vciParseError::BadScheme(_))
        ));
    }
}
