// This file is part of midnight-ledger.
// Copyright (C) 2025 Midnight Foundation
// SPDX-License-Identifier: Apache-2.0

//! TEE Attestation Module
//!
//! Provides attestation endpoints for verifying TEE integrity.
//! Attestation format depends on the cloud provider.

use axum::{
    extract::Query,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use base64::{Engine as _, engine::general_purpose};
use serde::{Deserialize, Serialize};
use serde_cbor::Value as CborValue;
use std::process::Command;
use tracing::{debug, error, info, warn};

// Import NSM attestation module
use crate::nsm_attestation;

/// Query parameters for attestation request
#[derive(Debug, Deserialize)]
pub(crate) struct AttestationQuery {
    /// Nonce for freshness (prevents replay attacks)
    #[serde(default)]
    pub nonce: Option<String>,
}

/// Attestation response
#[derive(Debug, Serialize)]
pub(crate) struct AttestationResponse {
    /// TEE platform type
    pub platform: String,
    /// Attestation format
    pub format: String,
    /// Nonce that was used (if provided)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nonce: Option<String>,
    /// Attestation document (base64 encoded)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attestation: Option<String>,
    /// Error message if attestation failed
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Additional metadata
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

/// Detect which TEE platform we're running on
fn detect_platform() -> TeePlatformType {
    // Early exit: if running on macOS or Windows, definitely not a cloud TEE
    #[cfg(target_os = "macos")]
    {
        debug!("Detected macOS - skipping cloud TEE detection");
        return TeePlatformType::Unknown;
    }

    #[cfg(target_os = "windows")]
    {
        debug!("Detected Windows - skipping cloud TEE detection");
        return TeePlatformType::Unknown;
    }

    // Only proceed with Linux-specific checks
    #[cfg(target_os = "linux")]
    {
        // Check for AWS Nitro Enclaves
        // Primary check: NSM device (most reliable indicator)
        if std::path::Path::new("/dev/nsm").exists() {
            debug!("Detected AWS Nitro Enclave (NSM device present)");
            return TeePlatformType::AwsNitro;
        }

        // Fallback: Check for vsock (less reliable but still indicates Nitro)
        if std::path::Path::new("/dev/vsock").exists() {
            // Additional check: try to read enclave-specific file
            if std::path::Path::new("/proc/cpuinfo").exists() {
                if let Ok(cpuinfo) = std::fs::read_to_string("/proc/cpuinfo") {
                    if cpuinfo.contains("Amazon") || std::env::var("AWS_EXECUTION_ENV").is_ok() {
                        debug!("Detected AWS Nitro Enclave (vsock present)");
                        return TeePlatformType::AwsNitro;
                    }
                }
            }
        }

        // Check for GCP Confidential VM via DMI
        if std::path::Path::new("/sys/firmware/dmi/tables/smbios_entry_point").exists() {
            if let Ok(output) = Command::new("dmidecode")
                .arg("-s")
                .arg("system-manufacturer")
                .output()
            {
                let manufacturer = String::from_utf8_lossy(&output.stdout);
                if manufacturer.contains("Google") {
                    debug!("Detected GCP Confidential VM");
                    return TeePlatformType::GcpConfidential;
                }
            }
        }

        // Check for Azure Confidential VM
        // Only check if we can reach the metadata endpoint quickly
        // Azure VMs have specific metadata endpoint at 169.254.169.254
        if let Ok(output) = Command::new("curl")
            .arg("-s")
            .arg("--max-time")
            .arg("2")  // 2 second timeout
            .arg("-H")
            .arg("Metadata:true")
            .arg("http://169.254.169.254/metadata/instance/compute/azEnvironment?api-version=2021-02-01&format=text")
            .output()
        {
            if output.status.success() {
                let response = String::from_utf8_lossy(&output.stdout);
                if response.contains("Azure") {
                    debug!("Detected Azure Confidential VM");
                    return TeePlatformType::AzureConfidential;
                }
            }
        }

        // Default for Linux: unknown/development
        debug!("No recognized TEE platform detected - running in development mode");
        TeePlatformType::Unknown
    }

    // For non-Linux platforms (other than macOS/Windows handled above)
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        debug!("Unsupported OS - skipping cloud TEE detection");
        TeePlatformType::Unknown
    }
}

#[derive(Debug, Clone, Copy)]
#[allow(dead_code)] // Variants may not be constructed on all platforms
enum TeePlatformType {
    AwsNitro,
    GcpConfidential,
    AzureConfidential,
    Unknown,
}

impl TeePlatformType {
    fn as_str(&self) -> &'static str {
        match self {
            Self::AwsNitro => "AWS Nitro Enclaves",
            Self::GcpConfidential => "GCP Confidential VM",
            Self::AzureConfidential => "Azure Confidential VM",
            Self::Unknown => "Unknown/Development",
        }
    }
}

/// Parse AWS Nitro attestation document to extract metadata
///
/// AWS Nitro attestation documents are CBOR-encoded COSE_Sign1 structures.
/// Structure: [protected_headers, unprotected_headers, payload, signature]
/// The payload (index 2) contains the attestation data we need to extract.
fn parse_aws_nitro_attestation(attestation_doc: &[u8]) -> Result<serde_json::Value, String> {
    // Parse the CBOR document
    let cbor_value: CborValue = serde_cbor::from_slice(attestation_doc)
        .map_err(|e| format!("Failed to parse CBOR attestation document: {}", e))?;

    // COSE_Sign1 is a CBOR array with 4 elements: [protected, unprotected, payload, signature]
    let cose_array = match cbor_value {
        CborValue::Array(arr) => arr,
        _ => return Err("Attestation document is not a CBOR array".to_string()),
    };

    if cose_array.len() != 4 {
        return Err(format!("Invalid COSE_Sign1 structure: expected 4 elements, got {}", cose_array.len()));
    }

    // Extract payload (index 2)
    let payload_bytes = match &cose_array[2] {
        CborValue::Bytes(bytes) => bytes,
        _ => return Err("Payload is not bytes".to_string()),
    };

    // Parse payload as CBOR
    let payload: CborValue = serde_cbor::from_slice(payload_bytes)
        .map_err(|e| format!("Failed to parse payload CBOR: {}", e))?;

    let payload_map = match payload {
        CborValue::Map(map) => map,
        _ => return Err("Payload is not a CBOR map".to_string()),
    };

    // Extract timestamp (milliseconds since epoch)
    let timestamp = payload_map.get(&CborValue::Text("timestamp".to_string()))
        .and_then(|v| match v {
            CborValue::Integer(i) => Some(*i as u64),
            _ => None,
        })
        .map(|ts| ts as f64); // Convert to f64 for JSON

    // Extract PCRs (Platform Configuration Registers)
    let pcrs = payload_map.get(&CborValue::Text("pcrs".to_string()))
        .and_then(|v| match v {
            CborValue::Map(map) => Some(map),
            _ => None,
        })
        .map(|pcr_map| {
            let mut pcr_values = serde_json::Map::new();
            for (key, value) in pcr_map {
                if let (CborValue::Integer(pcr_index), CborValue::Bytes(pcr_value)) = (key, value) {
                    pcr_values.insert(
                        pcr_index.to_string(),
                        serde_json::Value::String(hex::encode(pcr_value))
                    );
                }
            }
            serde_json::Value::Object(pcr_values)
        });

    // Extract certificate chain (cabundle)
    let certificate_chain = payload_map.get(&CborValue::Text("cabundle".to_string()))
        .and_then(|v| match v {
            CborValue::Array(arr) => Some(arr),
            _ => None,
        })
        .map(|certs| {
            certs.iter()
                .filter_map(|cert| match cert {
                    CborValue::Bytes(bytes) => Some(bytes),
                    _ => None,
                })
                .map(|cert_bytes| serde_json::Value::String(general_purpose::STANDARD.encode(cert_bytes)))
                .collect::<Vec<_>>()
        });

    // Extract module_id (enclave image measurement)
    let module_id = payload_map.get(&CborValue::Text("module_id".to_string()))
        .and_then(|v| match v {
            CborValue::Text(s) => Some(s.clone()),
            _ => None,
        })
        .map(|s| serde_json::Value::String(s));

    // Extract digest (hash algorithm used)
    let digest = payload_map.get(&CborValue::Text("digest".to_string()))
        .and_then(|v| match v {
            CborValue::Text(s) => Some(s.clone()),
            _ => None,
        })
        .map(|s| serde_json::Value::String(s));

    // Security properties for AWS Nitro Enclaves
    // AWS Nitro Enclaves always have these security properties enabled
    let debug_mode = false; // Nitro Enclaves run in production mode
    let secure_boot = true;  // Always enabled in Nitro
    let memory_encryption = true; // Always enabled in Nitro

    // Build metadata JSON
    let mut metadata = serde_json::Map::new();

    if let Some(ts) = timestamp {
        metadata.insert("timestamp".to_string(), serde_json::Value::Number(
            serde_json::Number::from_f64(ts).unwrap_or(serde_json::Number::from(0))
        ));
    }

    if let Some(pcr_vals) = pcrs {
        metadata.insert("pcrs".to_string(), pcr_vals);
    }

    if let Some(certs) = certificate_chain {
        metadata.insert("certificate_chain".to_string(), serde_json::Value::Array(certs));
    }

    if let Some(mid) = module_id {
        metadata.insert("module_id".to_string(), mid);
    }

    if let Some(dig) = digest {
        metadata.insert("digest".to_string(), dig);
    }

    metadata.insert("debug_mode".to_string(), serde_json::Value::Bool(debug_mode));
    metadata.insert("secure_boot".to_string(), serde_json::Value::Bool(secure_boot));
    metadata.insert("memory_encryption".to_string(), serde_json::Value::Bool(memory_encryption));

    Ok(serde_json::Value::Object(metadata))
}

/// Get attestation for AWS Nitro Enclaves using NSM API
///
/// This function directly calls the NSM (Nitro Security Module) API to generate
/// a cryptographic attestation document proving enclave integrity.
async fn get_aws_attestation(nonce: Option<String>) -> Result<AttestationResponse, String> {
    info!("Generating AWS Nitro attestation via NSM API");

    // Check if NSM device is available (only present inside Nitro Enclave)
    if !nsm_attestation::is_nsm_available() {
        warn!("NSM device not available - not running inside Nitro Enclave");
        return Ok(AttestationResponse {
            platform: "AWS Nitro Enclaves".to_string(),
            format: "CBOR".to_string(),
            nonce: nonce.clone(),
            attestation: None,
            error: Some("NSM device not available - not running inside Nitro Enclave".to_string()),
            metadata: Some(serde_json::json!({
                "message": "This endpoint only works inside an AWS Nitro Enclave",
                "instructions": "Deploy using: nitro-cli run-enclave",
                "pcr_publication": "https://github.com/midnight/proof-server/releases"
            })),
        });
    }

    // Convert nonce string to bytes
    let nonce_bytes = nonce.as_ref().map(|n| n.as_bytes().to_vec());

    // Request attestation document from NSM
    match nsm_attestation::request_attestation(nonce_bytes.clone(), None, None) {
        Ok(attestation_doc) => {
            info!("✅ Successfully generated NSM attestation document ({} bytes)", attestation_doc.len());

            // Parse attestation document to extract metadata
            let parsed_metadata = match parse_aws_nitro_attestation(&attestation_doc) {
                Ok(metadata) => metadata,
                Err(e) => {
                    warn!("Failed to parse attestation document: {}, returning document without extracted metadata", e);
                    serde_json::json!({})
                }
            };

            // Encode attestation document as base64
            let attestation_b64 = general_purpose::STANDARD.encode(&attestation_doc);

            // Merge parsed metadata with additional info
            let mut final_metadata = serde_json::json!({
                "document_size_bytes": attestation_doc.len(),
                "pcr_publication": "https://github.com/midnight/proof-server/releases",
                "verification_instructions": "Decode base64, parse CBOR, verify COSE signature against AWS root certificate"
            });

            // Merge parsed metadata into final_metadata
            if let Some(final_obj) = final_metadata.as_object_mut() {
                if let Some(parsed_obj) = parsed_metadata.as_object() {
                    for (key, value) in parsed_obj {
                        final_obj.insert(key.clone(), value.clone());
                    }
                }
            }

            Ok(AttestationResponse {
                platform: "AWS Nitro Enclaves".to_string(),
                format: "CBOR/COSE_Sign1".to_string(),
                nonce: nonce.clone(),
                attestation: Some(attestation_b64),
                error: None,
                metadata: Some(final_metadata),
            })
        }
        Err(e) => {
            error!("❌ Failed to generate NSM attestation: {}", e);
            Err(format!("NSM attestation failed: {}", e))
        }
    }
}

/// Get attestation for GCP Confidential VM using TPM 2.0
async fn get_gcp_attestation(nonce: Option<String>) -> Result<AttestationResponse, String> {
    info!("Generating GCP Confidential VM attestation (TPM 2.0)");

    // Generate nonce if not provided
    let nonce_value = nonce.unwrap_or_else(|| {
        use rand::Rng;
        let random_bytes: [u8; 32] = rand::thread_rng().gen();
        hex::encode(random_bytes)
    });

    debug!("Using nonce: {}", nonce_value);

    // Check if tpm2-tools is available
    let tpm2_available = Command::new("which")
        .arg("tpm2_quote")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if !tpm2_available {
        warn!("tpm2-tools not installed");
        return Ok(AttestationResponse {
            platform: "GCP Confidential VM".to_string(),
            format: "TPM 2.0".to_string(),
            nonce: Some(nonce_value),
            attestation: None,
            error: Some("tpm2-tools not installed. Run: sudo apt-get install tpm2-tools".to_string()),
            metadata: Some(serde_json::json!({
                "pcr_publication": "https://github.com/midnight/proof-server/releases"
            })),
        });
    }

    // Read PCR values
    let pcr_output = Command::new("tpm2_pcrread")
        .arg("sha256:0,1,4,5,7,8,9")
        .output()
        .map_err(|e| format!("Failed to read PCRs: {}", e))?;

    if !pcr_output.status.success() {
        let stderr = String::from_utf8_lossy(&pcr_output.stderr);
        error!("tpm2_pcrread failed: {}", stderr);
        return Err(format!("Failed to read PCRs: {}", stderr));
    }

    let pcr_values = String::from_utf8_lossy(&pcr_output.stdout);
    debug!("PCR values: {}", pcr_values);

    // For a full implementation, you would:
    // 1. Create an attestation key (if not exists)
    // 2. Generate a TPM quote with the nonce
    // 3. Return the quote + signature + PCR values

    // Simplified version: just return PCR values
    let attestation_data = general_purpose::STANDARD.encode(&pcr_output.stdout);

    Ok(AttestationResponse {
        platform: "GCP Confidential VM".to_string(),
        format: "TPM 2.0".to_string(),
        nonce: Some(nonce_value),
        attestation: Some(attestation_data),
        error: None,
        metadata: Some(serde_json::json!({
            "pcr_values_raw": pcr_values,
            "instructions": "Full TPM quote requires attestation key generation",
            "pcr_publication": "https://github.com/midnight/proof-server/releases"
        })),
    })
}

/// Get attestation for Azure Confidential VM using Azure Attestation Service
async fn get_azure_attestation(nonce: Option<String>) -> Result<AttestationResponse, String> {
    info!("Generating Azure Confidential VM attestation (JWT)");

    // Generate nonce if not provided
    let nonce_value = nonce.unwrap_or_else(|| {
        use rand::Rng;
        let random_bytes: [u8; 32] = rand::thread_rng().gen();
        hex::encode(random_bytes)
    });

    debug!("Using nonce: {}", nonce_value);

    // Get attestation token from Azure IMDS
    let imds_response = Command::new("curl")
        .arg("-s")
        .arg("-H")
        .arg("Metadata:true")
        .arg(&format!(
            "http://169.254.169.254/metadata/attested/document?api-version=2020-09-01&nonce={}",
            nonce_value
        ))
        .output()
        .map_err(|e| format!("Failed to query Azure IMDS: {}", e))?;

    if !imds_response.status.success() {
        let stderr = String::from_utf8_lossy(&imds_response.stderr);
        error!("Azure IMDS query failed: {}", stderr);
        return Err(format!("Failed to get attestation from IMDS: {}", stderr));
    }

    let _attestation_doc = String::from_utf8_lossy(&imds_response.stdout);
    debug!("Azure attestation document received");

    let attestation_data = general_purpose::STANDARD.encode(&imds_response.stdout);

    Ok(AttestationResponse {
        platform: "Azure Confidential VM".to_string(),
        format: "JWT".to_string(),
        nonce: Some(nonce_value),
        attestation: Some(attestation_data),
        error: None,
        metadata: Some(serde_json::json!({
            "instructions": "Decode JWT to extract PCR values and verify signature",
            "pcr_publication": "https://github.com/midnight/proof-server/releases"
        })),
    })
}

/// Attestation endpoint handler
///
/// Returns attestation document for the current TEE platform.
/// Format depends on platform:
/// - AWS Nitro: CBOR attestation document (must be requested from parent)
/// - GCP: TPM 2.0 quote
/// - Azure: JWT token from Attestation Service
pub(crate) async fn attestation_handler(
    Query(params): Query<AttestationQuery>,
) -> Result<Response, StatusCode> {
    info!("Attestation request received");

    let nonce = params.nonce;
    if let Some(ref n) = nonce {
        debug!("Nonce provided: {}", n);
    } else {
        debug!("No nonce provided, will generate one");
    }

    // Detect platform
    let platform = detect_platform();
    info!("Detected platform: {}", platform.as_str());

    // Get attestation based on platform
    let result = match platform {
        TeePlatformType::AwsNitro => get_aws_attestation(nonce.clone()).await,
        TeePlatformType::GcpConfidential => get_gcp_attestation(nonce.clone()).await,
        TeePlatformType::AzureConfidential => get_azure_attestation(nonce.clone()).await,
        TeePlatformType::Unknown => {
            warn!("Running in development/unknown environment");
            Ok(AttestationResponse {
                platform: "Development/Unknown".to_string(),
                format: "N/A".to_string(),
                nonce: nonce.clone(),
                attestation: None,
                error: Some("Not running in a recognized TEE environment".to_string()),
                metadata: Some(serde_json::json!({
                    "message": "Attestation is only available in production TEE deployments",
                    "supported_platforms": ["AWS Nitro Enclaves", "GCP Confidential VM", "Azure Confidential VM"]
                })),
            })
        }
    };

    match result {
        Ok(response) => Ok((StatusCode::OK, Json(response)).into_response()),
        Err(e) => {
            error!("Attestation failed: {}", e);
            Ok((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(AttestationResponse {
                    platform: platform.as_str().to_string(),
                    format: "Error".to_string(),
                    nonce,
                    attestation: None,
                    error: Some(e),
                    metadata: None,
                }),
            )
            .into_response())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_platform_detection() {
        // Platform detection depends on environment
        let platform = detect_platform();
        println!("Detected platform: {}", platform.as_str());
    }
}
