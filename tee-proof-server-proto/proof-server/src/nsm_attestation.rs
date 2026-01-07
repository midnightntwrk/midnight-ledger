//! AWS Nitro Security Module (NSM) Attestation Integration
//!
//! This module provides direct integration with the Nitro Security Module
//! to generate cryptographic attestation documents proving enclave integrity.

use aws_nitro_enclaves_nsm_api::{api::Request, api::Response, driver::{nsm_init, nsm_process_request}};
use serde_bytes::ByteBuf;
use tracing::{debug, error, info, warn};
use std::sync::Mutex;

// Global NSM file descriptor (lazily initialized)
static NSM_FD: Mutex<Option<i32>> = Mutex::new(None);

/// Get or initialize NSM file descriptor
fn get_nsm_fd() -> Result<i32, String> {
    let mut fd_guard = NSM_FD.lock().map_err(|e| format!("Failed to lock NSM_FD: {}", e))?;

    if let Some(fd) = *fd_guard {
        Ok(fd)
    } else {
        // Initialize NSM device
        match nsm_init() {
            fd if fd >= 0 => {
                info!("NSM device initialized (fd: {})", fd);
                *fd_guard = Some(fd);
                Ok(fd)
            }
            fd => {
                error!("Failed to initialize NSM device (error code: {})", fd);
                Err(format!("NSM initialization failed with code: {}", fd))
            }
        }
    }
}

/// Request attestation document from NSM
///
/// # Arguments
/// * `nonce` - Optional nonce for replay protection (recommended)
/// * `user_data` - Optional application-specific data to include
/// * `public_key` - Optional public key for encrypted responses
///
/// # Returns
/// * `Ok(Vec<u8>)` - CBOR-encoded attestation document
/// * `Err(String)` - Error message if attestation fails
///
/// # Example
/// ```rust
/// let nonce = Some(b"client_nonce_123".to_vec());
/// let doc = request_attestation(nonce, None, None)?;
/// println!("Attestation document: {} bytes", doc.len());
/// ```
pub(crate) fn request_attestation(
    nonce: Option<Vec<u8>>,
    user_data: Option<Vec<u8>>,
    public_key: Option<Vec<u8>>,
) -> Result<Vec<u8>, String> {
    info!("Requesting attestation document from NSM");

    // Validate input sizes per NSM API spec
    if let Some(ref n) = nonce {
        if n.len() > 512 {
            return Err("Nonce exceeds 512 bytes".to_string());
        }
        debug!("Using nonce: {} bytes", n.len());
    }
    if let Some(ref u) = user_data {
        if u.len() > 512 {
            return Err("User data exceeds 512 bytes".to_string());
        }
        debug!("Using user data: {} bytes", u.len());
    }
    if let Some(ref p) = public_key {
        if p.len() > 1024 {
            return Err("Public key exceeds 1024 bytes".to_string());
        }
        debug!("Using public key: {} bytes", p.len());
    }

    // Convert Vec<u8> to ByteBuf for NSM API
    let nonce_buf = nonce.map(ByteBuf::from);
    let user_data_buf = user_data.map(ByteBuf::from);
    let public_key_buf = public_key.map(ByteBuf::from);

    // Create attestation request
    let request = Request::Attestation {
        nonce: nonce_buf,
        user_data: user_data_buf,
        public_key: public_key_buf,
    };

    // Get NSM file descriptor
    let fd = get_nsm_fd()?;

    // Send request to NSM driver
    debug!("Sending attestation request to NSM driver");
    match nsm_process_request(fd, request) {
        Response::Attestation { document } => {
            info!(
                "✅ Received attestation document from NSM ({} bytes)",
                document.len()
            );
            Ok(document)
        }
        Response::Error(error_code) => {
            error!("❌ NSM returned error: {:?}", error_code);
            Err(format!("NSM error: {:?}", error_code))
        }
        _ => {
            error!("❌ Unexpected NSM response type");
            Err("Unexpected NSM response".to_string())
        }
    }
}

/// Check if running inside a Nitro Enclave (NSM device available)
///
/// This function checks for the presence of the `/dev/nsm` device,
/// which is only available inside Nitro Enclaves.
///
/// # Returns
/// * `true` - Running inside Nitro Enclave
/// * `false` - Not in enclave (development environment)
pub(crate) fn is_nsm_available() -> bool {
    use std::path::Path;

    // NSM device path
    let nsm_device = Path::new("/dev/nsm");
    let available = nsm_device.exists();

    if available {
        info!("✅ NSM device detected at /dev/nsm");
    } else {
        warn!("⚠️ NSM device not found - not running in Nitro Enclave");
    }

    available
}

/// Get NSM device information (for debugging)
#[allow(dead_code)]
pub(crate) fn get_nsm_info() -> Result<String, String> {
    if !is_nsm_available() {
        return Err("NSM device not available".to_string());
    }

    // Get NSM file descriptor
    let fd = get_nsm_fd()?;

    // Query NSM for description
    match nsm_process_request(fd, Request::DescribeNSM) {
        Response::DescribeNSM {
            version_major,
            version_minor,
            version_patch,
            module_id,
            max_pcrs,
            locked_pcrs,
            digest,
        } => {
            let info = format!(
                "NSM Version: {}.{}.{}\nModule ID: {}\nMax PCRs: {}\nLocked PCRs: {:?}\nDigest: {:?}",
                version_major, version_minor, version_patch, module_id, max_pcrs, locked_pcrs, digest
            );
            Ok(info)
        }
        Response::Error(e) => Err(format!("NSM describe failed: {:?}", e)),
        _ => Err("Unexpected NSM response".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nsm_availability() {
        let available = is_nsm_available();
        println!("NSM available: {}", available);
        // Test passes regardless - just informational
    }

    #[test]
    #[ignore] // Only runs inside actual enclave with --ignored flag
    fn test_attestation_generation() {
        if !is_nsm_available() {
            println!("Skipping: NSM not available (not in enclave)");
            return;
        }

        let nonce = Some(b"test_nonce_12345".to_vec());
        let result = request_attestation(nonce, None, None);

        match result {
            Ok(doc) => {
                println!("✅ Attestation document generated: {} bytes", doc.len());
                assert!(doc.len() > 0);
            }
            Err(e) => {
                panic!("❌ Attestation generation failed: {}", e);
            }
        }
    }

    #[test]
    #[ignore]
    fn test_get_nsm_info() {
        match get_nsm_info() {
            Ok(info) => println!("NSM Info:\n{}", info),
            Err(e) => println!("NSM not available: {}", e),
        }
    }
}
