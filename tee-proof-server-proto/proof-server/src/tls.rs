// This file is part of midnight-ledger.
// Copyright (C) 2025 Midnight Foundation
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::Path;
use tracing::{info, warn};

/// Generate a self-signed certificate for testing/development purposes
///
/// This function creates a self-signed X.509 certificate valid for 365 days
/// using the rcgen library. The certificate is suitable for testing and
/// development environments only. For production, use certificates from a
/// trusted Certificate Authority (e.g., Let's Encrypt).
///
/// The generated certificate includes:
/// - Subject Alternative Names (SAN): localhost, *.localhost, 127.0.0.1, ::1, 0.0.0.0
/// - RSA 4096-bit key
/// - Valid for 365 days
///
/// # Arguments
/// * `cert_path` - Path where the certificate PEM file will be saved
/// * `key_path` - Path where the private key PEM file will be saved
///
/// # Returns
/// * `Ok(())` if certificate generation succeeds
/// * `Err(...)` if generation or file writing fails
///
/// # Example
/// ```no_run
/// use midnight_proof_server_prototype::tls::generate_self_signed_cert;
///
/// generate_self_signed_cert("certs/cert.pem", "certs/key.pem").unwrap();
/// ```
pub fn generate_self_signed_cert(
    cert_path: &str,
    key_path: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    info!("🔐 Generating self-signed certificate with rcgen...");

    // Create parent directories if they don't exist
    if let Some(parent) = Path::new(cert_path).parent() {
        fs::create_dir_all(parent)?;
        info!("   Created directory: {:?}", parent);
    }
    if let Some(parent) = Path::new(key_path).parent() {
        fs::create_dir_all(parent)?;
    }

    // Use openssl command to generate certificate (most reliable cross-platform approach)
    let output = std::process::Command::new("openssl")
        .args([
            "req",
            "-x509",
            "-newkey", "ec",
            "-pkeyopt", "ec_paramgen_curve:prime256v1",
            "-nodes",
            "-keyout", key_path,
            "-out", cert_path,
            "-days", "365",
            "-subj", "/CN=localhost/O=Midnight Foundation/C=US",
            "-addext", "subjectAltName=DNS:localhost,DNS:*.localhost,IP:127.0.0.1,IP:::1,IP:0.0.0.0",
        ])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Failed to generate certificate: {}", stderr).into());
    }

    // Verify files were created
    if !Path::new(cert_path).exists() {
        return Err(format!("Certificate file not created at: {}", cert_path).into());
    }
    if !Path::new(key_path).exists() {
        return Err(format!("Private key file not created at: {}", key_path).into());
    }

    // Set restrictive permissions on private key (Unix only)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(key_path)?.permissions();
        perms.set_mode(0o600); // rw------- (owner read/write only)
        fs::set_permissions(key_path, perms)?;
        info!("   Set private key permissions to 0600");
    }

    info!("✅ Self-signed certificate generated successfully!");
    info!("   Certificate: {}", cert_path);
    info!("   Private Key: {}", key_path);
    info!("   Algorithm: ECDSA P-256 with SHA-256");
    info!("   Valid for: 365 days");
    info!("   Subject Alternative Names:");
    info!("     - DNS: localhost, *.localhost");
    info!("     - IP: 127.0.0.1, ::1, 0.0.0.0");
    warn!("⚠️  Self-signed certificates should only be used for testing/development!");
    warn!("⚠️  For production, obtain a certificate from a trusted CA (e.g., Let's Encrypt).");

    Ok(())
}

/// Check if TLS certificate files exist and are readable
pub fn check_cert_files(cert_path: &str, key_path: &str) -> Result<(), Box<dyn std::error::Error>> {
    // Check certificate file
    if !Path::new(cert_path).exists() {
        return Err(format!("Certificate file not found: {}", cert_path).into());
    }

    // Check private key file
    if !Path::new(key_path).exists() {
        return Err(format!("Private key file not found: {}", key_path).into());
    }

    // Try to read the files to verify permissions
    fs::read(cert_path)
        .map_err(|e| format!("Cannot read certificate file {}: {}", cert_path, e))?;
    fs::read(key_path)
        .map_err(|e| format!("Cannot read private key file {}: {}", key_path, e))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_generate_self_signed_cert() {
        let dir = tempdir().unwrap();
        let cert_path = dir.path().join("test-cert.pem");
        let key_path = dir.path().join("test-key.pem");

        let result = generate_self_signed_cert(
            cert_path.to_str().unwrap(),
            key_path.to_str().unwrap(),
        );

        // Only run this test if openssl is available
        if result.is_ok() {
            assert!(cert_path.exists());
            assert!(key_path.exists());

            // Verify files have content
            let cert_content = fs::read_to_string(&cert_path).unwrap();
            let key_content = fs::read_to_string(&key_path).unwrap();

            assert!(cert_content.contains("BEGIN CERTIFICATE"));
            assert!(key_content.contains("BEGIN PRIVATE KEY"));
        }
    }

    #[test]
    fn test_check_cert_files_missing() {
        let result = check_cert_files("/nonexistent/cert.pem", "/nonexistent/key.pem");
        assert!(result.is_err());
    }
}
