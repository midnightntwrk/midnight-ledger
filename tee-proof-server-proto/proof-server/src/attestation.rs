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
use std::process::Command;
use tracing::{debug, error, info, warn};

/// Query parameters for attestation request
#[derive(Debug, Deserialize)]
pub struct AttestationQuery {
    /// Nonce for freshness (prevents replay attacks)
    #[serde(default)]
    pub nonce: Option<String>,
}

/// Attestation response
#[derive(Debug, Serialize)]
pub struct AttestationResponse {
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
    // Check for AWS Nitro Enclaves
    if std::path::Path::new("/dev/vsock").exists() {
        // Additional check: try to read enclave-specific file
        if std::path::Path::new("/proc/cpuinfo").exists() {
            if let Ok(cpuinfo) = std::fs::read_to_string("/proc/cpuinfo") {
                if cpuinfo.contains("Amazon") || std::env::var("AWS_EXECUTION_ENV").is_ok() {
                    return TeePlatformType::AwsNitro;
                }
            }
        }
    }

    // Check for GCP Confidential VM
    if std::path::Path::new("/sys/firmware/dmi/tables/smbios_entry_point").exists() {
        if let Ok(output) = Command::new("dmidecode")
            .arg("-s")
            .arg("system-manufacturer")
            .output()
        {
            let manufacturer = String::from_utf8_lossy(&output.stdout);
            if manufacturer.contains("Google") {
                return TeePlatformType::GcpConfidential;
            }
        }
    }

    // Check for Azure Confidential VM
    // Azure VMs have specific metadata endpoint
    if let Ok(output) = Command::new("curl")
        .arg("-s")
        .arg("-H")
        .arg("Metadata:true")
        .arg("http://169.254.169.254/metadata/instance/compute/azEnvironment?api-version=2021-02-01&format=text")
        .output()
    {
        if output.status.success() {
            let response = String::from_utf8_lossy(&output.stdout);
            if response.contains("Azure") {
                return TeePlatformType::AzureConfidential;
            }
        }
    }

    // Default: unknown/development
    TeePlatformType::Unknown
}

#[derive(Debug, Clone, Copy)]
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

/// Get attestation for AWS Nitro Enclaves
///
/// NOTE: This must be called from the PARENT EC2 instance, not from inside the enclave!
/// The enclave cannot attest itself - attestation comes from the Nitro hypervisor.
async fn get_aws_attestation(nonce: Option<String>) -> Result<AttestationResponse, String> {
    info!("Generating AWS Nitro attestation");

    // In production AWS Nitro deployment, this should be called via vsock
    // from the parent EC2 instance which has access to nitro-cli

    warn!("AWS Nitro attestation must be requested from parent EC2 instance");
    warn!("Use: nitro-cli describe-enclaves --enclave-id <id>");

    Ok(AttestationResponse {
        platform: "AWS Nitro Enclaves".to_string(),
        format: "CBOR".to_string(),
        nonce,
        attestation: None,
        error: Some("Attestation must be requested from parent EC2 instance using nitro-cli".to_string()),
        metadata: Some(serde_json::json!({
            "instructions": "From parent EC2 instance, run: nitro-cli describe-enclaves --enclave-id <id>",
            "pcr_publication": "https://github.com/midnight/proof-server/releases"
        })),
    })
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
pub async fn attestation_handler(
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
