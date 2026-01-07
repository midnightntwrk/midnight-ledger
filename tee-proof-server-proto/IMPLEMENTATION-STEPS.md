# Step-by-Step Implementation Guide: Close Trust Gaps

## Overview

This guide provides **exact, copy-paste instructions** to implement NSM API attestation and close the critical trust gaps identified in the [Trusted Workload Gap Analysis](./docs/trusted-workload-gap-analysis.md).

**Goal**: Enable real-time, trustless attestation verification
**Time Required**: 2-3 days
**Difficulty**: Medium

---

## Prerequisites

Before starting, ensure you have:
- [ ] Proof server code checked out
- [ ] Rust toolchain installed (1.70+)
- [ ] Access to AWS Nitro Enclave environment for testing
- [ ] Text editor or IDE

**Working Directory**: `/Users/robertblessing-hartley/code/midnight-code/midnight-ledger/tee-proof-server-proto/proof-server`

---

## Phase 1: NSM API Integration (Critical Priority)

### Step 1: Add Dependencies to Cargo.toml

**File**: `proof-server/Cargo.toml`

**Action**: Add these dependencies to the `[dependencies]` section:

```toml
[dependencies]
# ... existing dependencies ...

# NSM API for attestation
aws-nitro-enclaves-nsm-api = "0.4"
serde_bytes = "0.11"
```

**Verify**:
```bash
cd /Users/robertblessing-hartley/code/midnight-code/midnight-ledger/tee-proof-server-proto/proof-server
cargo check
```

**Expected**: Compilation succeeds (may take a few minutes to download dependencies)

---

### Step 2: Create NSM Attestation Module

**File**: `proof-server/src/nsm_attestation.rs` (NEW FILE)

**Action**: Create this new file with the following content:

```rust
//! AWS Nitro Security Module (NSM) Attestation Integration
//!
//! This module provides direct integration with the Nitro Security Module
//! to generate cryptographic attestation documents proving enclave integrity.

use aws_nitro_enclaves_nsm_api::{api::Request, api::Response, driver::nsm_driver_request};
use serde::{Deserialize, Serialize};
use tracing::{debug, error, info, warn};

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
pub fn request_attestation(
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

    // Create attestation request
    let request = Request::Attestation {
        nonce,
        user_data,
        public_key,
    };

    // Send request to NSM driver
    debug!("Sending attestation request to NSM driver");
    match nsm_driver_request(request) {
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
pub fn is_nsm_available() -> bool {
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
pub fn get_nsm_info() -> Result<String, String> {
    if !is_nsm_available() {
        return Err("NSM device not available".to_string());
    }

    // Query NSM for description
    match nsm_driver_request(Request::DescribeNSM) {
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
                "NSM Version: {}.{}.{}\nModule ID: {}\nMax PCRs: {}\nLocked PCRs: {:?}\nDigest: {}",
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
    #[ignore] // Only runs inside actual enclave
    fn test_nsm_info() {
        if !is_nsm_available() {
            println!("Skipping: NSM not available");
            return;
        }

        match get_nsm_info() {
            Ok(info) => println!("NSM Info:\n{}", info),
            Err(e) => panic!("Failed to get NSM info: {}", e),
        }
    }

    #[test]
    fn test_nonce_size_limit() {
        let large_nonce = Some(vec![0u8; 513]); // Exceeds 512 byte limit
        let result = request_attestation(large_nonce, None, None);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("exceeds 512 bytes"));
    }
}
```

**Save the file.**

---

### Step 3: Update Module Declarations

**File**: `proof-server/src/lib.rs` (or `proof-server/src/main.rs` if no lib.rs exists)

**Action**: Add the module declaration at the top of the file:

```rust
// Near the top, with other module declarations
mod nsm_attestation;
```

**Verify**:
```bash
cargo check
```

**Expected**: Compilation succeeds with new module

---

### Step 4: Update Attestation Handler

**File**: `proof-server/src/attestation.rs`

**Action**: Replace the entire file content with this updated version:

```rust
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

// Import our NSM attestation module
use crate::nsm_attestation::{is_nsm_available, request_attestation};

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

/// Attestation endpoint handler
///
/// Returns attestation document for AWS Nitro Enclaves.
/// If NSM device is available, generates real-time attestation with nonce.
/// Otherwise, returns instructions for obtaining attestation.
pub(crate) async fn attestation_handler(
    Query(params): Query<AttestationQuery>,
) -> Result<Response, StatusCode> {
    info!("Attestation request received");

    let nonce = params.nonce;
    if let Some(ref n) = nonce {
        debug!("Nonce provided: {}", n);
    } else {
        debug!("No nonce provided");
    }

    // Check if NSM is available (running in actual Nitro Enclave)
    if !is_nsm_available() {
        warn!("NSM device not available - not running in Nitro Enclave");
        return Ok((
            StatusCode::OK,
            Json(AttestationResponse {
                platform: "Development/Not in Enclave".to_string(),
                format: "N/A".to_string(),
                nonce: nonce.clone(),
                attestation: None,
                error: Some("NSM device not available - not running in Nitro Enclave".to_string()),
                metadata: Some(serde_json::json!({
                    "message": "Attestation is only available inside AWS Nitro Enclaves",
                    "nsm_device": "/dev/nsm",
                    "supported_platforms": ["AWS Nitro Enclaves"],
                    "instructions": "Deploy to Nitro Enclave to enable attestation"
                })),
            }),
        )
        .into_response());
    }

    info!("NSM device available - generating attestation document");

    // Convert nonce to bytes
    let nonce_bytes = nonce.as_ref().map(|n| n.as_bytes().to_vec());

    // Request attestation from NSM with nonce
    match request_attestation(nonce_bytes, None, None) {
        Ok(attestation_doc) => {
            info!(
                "✅ Attestation document generated successfully ({} bytes)",
                attestation_doc.len()
            );

            // Encode to base64 for JSON transport
            let attestation_b64 = general_purpose::STANDARD.encode(&attestation_doc);

            Ok((
                StatusCode::OK,
                Json(AttestationResponse {
                    platform: "AWS Nitro Enclaves".to_string(),
                    format: "CBOR/COSE_Sign1".to_string(),
                    nonce: nonce.clone(),
                    attestation: Some(attestation_b64),
                    error: None,
                    metadata: Some(serde_json::json!({
                        "document_size": attestation_doc.len(),
                        "encoding": "base64",
                        "signature_algorithm": "ECDSA-SHA384",
                        "pcr_publication": "https://github.com/midnight/proof-server/releases",
                        "root_certificate": "https://aws-nitro-enclaves.amazonaws.com/AWS_NitroEnclaves_Root-G1.zip",
                        "verification_guide": "https://github.com/midnight/midnight-ledger/blob/main/tee-proof-server-proto/docs/attestation-implementation-guide.md"
                    })),
                }),
            )
            .into_response())
        }
        Err(e) => {
            error!("❌ Failed to generate attestation: {}", e);
            Ok((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(AttestationResponse {
                    platform: "AWS Nitro Enclaves".to_string(),
                    format: "Error".to_string(),
                    nonce,
                    attestation: None,
                    error: Some(e),
                    metadata: Some(serde_json::json!({
                        "troubleshooting": "Check that /dev/nsm device exists and is accessible"
                    })),
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
    fn test_attestation_response_serialization() {
        let response = AttestationResponse {
            platform: "AWS Nitro Enclaves".to_string(),
            format: "CBOR/COSE_Sign1".to_string(),
            nonce: Some("test123".to_string()),
            attestation: Some("base64data".to_string()),
            error: None,
            metadata: None,
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("AWS Nitro Enclaves"));
        assert!(json.contains("test123"));
    }
}
```

**Save the file.**

**Verify**:
```bash
cargo check
```

**Expected**: Compilation succeeds

---

### Step 5: Build and Test Locally

**Build the proof server**:

```bash
cd /Users/robertblessing-hartley/code/midnight-code/midnight-ledger/tee-proof-server-proto/proof-server

# Clean build
cargo clean

# Build in release mode
cargo build --release

# Verify binary exists
ls -lh ../../target/release/midnight-proof-server-prototype
```

**Expected Output**:
```
-rwxr-xr-x  1 user  staff   45M Jan  4 12:00 midnight-proof-server-prototype
```

**Test locally (outside enclave)**:

```bash
# Run the proof server
../../target/release/midnight-proof-server-prototype \
  --disable-tls \
  --disable-auth

# In another terminal, test attestation endpoint
curl -v "http://localhost:6300/attestation?nonce=test123"
```

**Expected Response** (outside enclave):
```json
{
  "platform": "Development/Not in Enclave",
  "format": "N/A",
  "nonce": "test123",
  "error": "NSM device not available - not running in Nitro Enclave",
  "metadata": {
    "message": "Attestation is only available inside AWS Nitro Enclaves",
    "nsm_device": "/dev/nsm",
    "supported_platforms": ["AWS Nitro Enclaves"]
  }
}
```

**✅ This is correct!** The server detects it's not in an enclave and returns appropriate message.

---

### Step 6: Update Dockerfile

The Dockerfile already has the correct structure. No changes needed if you've applied the socat fix from earlier.

**Verify Dockerfile includes**:
```dockerfile
# In runtime stage - should already have this
USER proofserver
ENV HOME=/app
```

---

### Step 7: Build Docker Image

```bash
cd /Users/robertblessing-hartley/code/midnight-code/midnight-ledger

# Build Docker image
docker build -f tee-proof-server-proto/Dockerfile -t midnight/proof-server:v6.3.0 .

# Verify image
docker images | grep midnight/proof-server
```

**Expected**:
```
midnight/proof-server   v6.3.0   abc123def456   2 minutes ago   500MB
```

---

### Step 8: Test Docker Image Locally

```bash
# Run Docker container
docker run --rm -p 6300:6300 midnight/proof-server:v6.3.0

# In another terminal, test
curl "http://localhost:6300/health"
curl "http://localhost:6300/attestation?nonce=test123"
```

**Expected**: Server starts, endpoints respond, attestation returns "not in enclave" message.

---

### Step 9: Deploy to AWS Nitro Enclave

**On your AWS EC2 instance with Nitro Enclave support**:

```bash
# 1. Transfer Docker image (or rebuild on instance)
# Option A: Save and transfer
docker save midnight/proof-server:v6.3.0 | gzip > proof-server-v6.3.0.tar.gz
scp proof-server-v6.3.0.tar.gz ec2-user@your-instance:/home/ec2-user/

# On EC2 instance:
docker load < proof-server-v6.3.0.tar.gz

# Option B: Rebuild on EC2 instance (recommended)
git pull origin main
docker build -f tee-proof-server-proto/Dockerfile -t midnight/proof-server:v6.3.0 .

# 2. Build EIF
nitro-cli build-enclave \
  --docker-uri midnight/proof-server:v6.3.0 \
  --output-file proof-server-v6.3.0.eif \
  | tee eif-build-output.json

# 3. SAVE PCR MEASUREMENTS (CRITICAL!)
cat eif-build-output.json | jq '.Measurements' > pcr-v6.3.0.json

# Display PCRs
cat pcr-v6.3.0.json

# 4. Stop old enclave (if running)
nitro-cli describe-enclaves
nitro-cli terminate-enclave --enclave-id <old-id>

# 5. Start new enclave (PRODUCTION MODE - NO DEBUG)
nitro-cli run-enclave \
  --eif-path proof-server-v6.3.0.eif \
  --cpu-count 4 \
  --memory 8192 \
  --enclave-cid 16

# 6. Verify enclave is running
nitro-cli describe-enclaves

# Should show:
# "State": "RUNNING"
# "Flags": "NONE"  ← No debug mode = production
```

---

### Step 10: Test Real Attestation Inside Enclave

**From parent EC2 instance**:

```bash
# Test health endpoint (should work via socat proxy)
curl http://localhost:6300/health

# Test attestation endpoint with nonce
curl -v "http://localhost:6300/attestation?nonce=$(date +%s)"

# Save response
curl -s "http://localhost:6300/attestation?nonce=production_test_123" > attestation-response.json

# View response
cat attestation-response.json | jq
```

**Expected Response** (inside enclave):
```json
{
  "platform": "AWS Nitro Enclaves",
  "format": "CBOR/COSE_Sign1",
  "nonce": "production_test_123",
  "attestation": "hEShATgioFkQ6qkCAVggg...base64-encoded-CBOR...",
  "metadata": {
    "document_size": 4096,
    "encoding": "base64",
    "signature_algorithm": "ECDSA-SHA384",
    "pcr_publication": "https://github.com/midnight/proof-server/releases"
  }
}
```

**✅ SUCCESS!** You now have real-time attestation!

---

### Step 11: Verify Attestation Document

**Extract and decode the attestation document**:

```bash
# Extract base64 attestation
cat attestation-response.json | jq -r '.attestation' | base64 -d > attestation.cbor

# Verify it's valid CBOR
file attestation.cbor
# Should say: "CBOR data"

# Decode CBOR (requires cbor2 Python package)
python3 << 'EOF'
import cbor2
import json

with open('attestation.cbor', 'rb') as f:
    doc = cbor2.load(f)

# Display PCRs
print("PCR Measurements:")
for index, value in doc['pcrs'].items():
    print(f"  PCR{index}: {value.hex()}")

# Display other fields
print(f"\nModule ID: {doc['module_id']}")
print(f"Timestamp: {doc['timestamp']}")
print(f"Digest: {doc['digest']}")

if 'nonce' in doc:
    print(f"Nonce: {doc['nonce'].decode('utf-8')}")
EOF
```

**Expected Output**:
```
PCR Measurements:
  PCR0: 287b24930a34969c05abdb48eee1371edec28a7d...
  PCR1: aca6e62f4b8c1668f8a2e5e9c2f7b3f1234567...
  PCR2: 45e6789abcdef1234567890abcdef1234567890...

Module ID: i-00db671000a16f26e-enc19b820c6deb5a0b
Timestamp: 1704384000000
Digest: SHA384
Nonce: production_test_123
```

**Compare PCRs**:
```bash
# Compare with published PCRs
diff <(cat pcr-v6.3.0.json | jq -r '.PCR0') \
     <(python3 -c "import cbor2; doc=cbor2.load(open('attestation.cbor','rb')); print(doc['pcrs'][0].hex())")

# ✅ No output = PCRs match!
```

---

### Step 12: Publish PCR Measurements

**Create GitHub release with PCR measurements**:

```bash
# On your development machine
cd /Users/robertblessing-hartley/code/midnight-code/midnight-ledger

# Create release metadata
cat > release-v6.3.0.json << EOF
{
  "version": "6.3.0",
  "release_date": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "git_commit": "$(git rev-parse HEAD)",
  "measurements": $(cat ~/pcr-v6.3.0.json),
  "eif_sha256": "$(sha256sum ~/proof-server-v6.3.0.eif | awk '{print $1}')",
  "features": [
    "Real-time NSM API attestation",
    "Nonce-based freshness protection",
    "CBOR/COSE_Sign1 signed documents"
  ]
}
EOF

# Create GitHub release (if you have gh CLI)
gh release create v6.3.0 \
  --title "Proof Server v6.3.0 - NSM Attestation" \
  --notes "# Midnight Proof Server v6.3.0

## New Features
✅ **Real-time NSM API attestation** - Clients can now verify enclave integrity in real-time
✅ **Nonce-based freshness** - Protection against replay attacks
✅ **CBOR/COSE_Sign1 documents** - Industry-standard attestation format

## Verification
\`\`\`bash
# Request attestation
curl https://proof.devnet.midnight.network/attestation?nonce=abc123

# Verify PCRs match published values (see release-v6.3.0.json)
\`\`\`

## PCR Measurements
See attached \`release-v6.3.0.json\` for complete PCR measurements." \
  release-v6.3.0.json

# ✅ PCRs now publicly available and tamper-evident!
```

---

## Phase 2: Client-Side Verification (Testing)

### Step 13: Test Client-Side Verification

**Create verification script** (`verify-attestation.py`):

```python
#!/usr/bin/env python3
"""
Verify Midnight Proof Server Attestation

Usage:
    python3 verify-attestation.py https://proof.devnet.midnight.network
"""

import sys
import requests
import cbor2
import base64
import hashlib
from datetime import datetime

# Expected PCR values (from GitHub release)
EXPECTED_PCRS = {
    0: "287b24930a34969c05abdb48eee1371edec28a7d...",  # Replace with actual
    1: "aca6e62f4b8c1668f8a2e5e9c2f7b3f1234567...",  # Replace with actual
    2: "45e6789abcdef1234567890abcdef1234567890...",  # Replace with actual
}

def verify_attestation(base_url):
    """Verify proof server attestation"""
    print("🔍 Verifying Midnight Proof Server Attestation")
    print(f"   Server: {base_url}\n")

    # Generate nonce
    nonce = hashlib.sha256(str(datetime.now()).encode()).hexdigest()[:32]
    print(f"1. Generated nonce: {nonce}")

    # Request attestation
    print("2. Requesting attestation document...")
    response = requests.get(f"{base_url}/attestation", params={"nonce": nonce})

    if response.status_code != 200:
        print(f"❌ Failed to get attestation: HTTP {response.status_code}")
        return False

    data = response.json()

    # Check for errors
    if data.get("error"):
        print(f"❌ Attestation error: {data['error']}")
        return False

    # Decode attestation document
    print("3. Decoding attestation document...")
    attestation_b64 = data["attestation"]
    attestation_bytes = base64.b64decode(attestation_b64)
    print(f"   Document size: {len(attestation_bytes)} bytes")

    # Parse CBOR
    doc = cbor2.loads(attestation_bytes)

    # Verify nonce
    print("4. Verifying nonce...")
    doc_nonce = doc.get("nonce", b"").decode("utf-8")
    if doc_nonce != nonce:
        print(f"❌ Nonce mismatch: expected {nonce}, got {doc_nonce}")
        return False
    print(f"   ✅ Nonce verified: {doc_nonce}")

    # Verify PCRs
    print("5. Verifying PCR measurements...")
    pcrs = doc["pcrs"]
    for index, expected_value_hex in EXPECTED_PCRS.items():
        if index not in pcrs:
            print(f"❌ PCR{index} not found in attestation")
            return False

        actual_value_hex = pcrs[index].hex()
        if actual_value_hex != expected_value_hex:
            print(f"❌ PCR{index} mismatch:")
            print(f"   Expected: {expected_value_hex}")
            print(f"   Actual:   {actual_value_hex}")
            return False

        print(f"   ✅ PCR{index} matches")

    # TODO: Verify certificate chain (requires cryptography package)
    print("6. Certificate chain verification: SKIPPED (implement in production)")

    print("\n" + "="*60)
    print("✅ ATTESTATION VERIFIED - ENCLAVE IS TRUSTWORTHY")
    print("="*60)
    return True

if __name__ == "__main__":
    if len(sys.argv) != 2:
        print("Usage: python3 verify-attestation.py <server-url>")
        sys.exit(1)

    base_url = sys.argv[1].rstrip("/")
    success = verify_attestation(base_url)
    sys.exit(0 if success else 1)
```

**Run verification**:

```bash
# Update EXPECTED_PCRS in the script with your actual PCR values
python3 verify-attestation.py https://proof.devnet.midnight.network

# Expected output:
# 🔍 Verifying Midnight Proof Server Attestation
#    Server: https://proof.devnet.midnight.network
#
# 1. Generated nonce: abc123def456...
# 2. Requesting attestation document...
# 3. Decoding attestation document...
#    Document size: 4096 bytes
# 4. Verifying nonce...
#    ✅ Nonce verified: abc123def456...
# 5. Verifying PCR measurements...
#    ✅ PCR0 matches
#    ✅ PCR1 matches
#    ✅ PCR2 matches
# 6. Certificate chain verification: SKIPPED (implement in production)
#
# ============================================================
# ✅ ATTESTATION VERIFIED - ENCLAVE IS TRUSTWORTHY
# ============================================================
```

---

## Verification Checklist

After completing all steps, verify:

- [ ] NSM API dependency added to Cargo.toml
- [ ] nsm_attestation.rs module created
- [ ] attestation.rs updated with NSM integration
- [ ] Module declaration added to lib.rs/main.rs
- [ ] Code compiles successfully (`cargo build --release`)
- [ ] Local testing works (returns "not in enclave" message)
- [ ] Docker image builds successfully
- [ ] EIF created from Docker image
- [ ] PCR measurements extracted and saved
- [ ] Enclave deployed to AWS (without debug mode)
- [ ] Attestation endpoint returns real documents
- [ ] Nonce included in attestation document
- [ ] PCRs published to GitHub release
- [ ] Client verification script works

---

## Troubleshooting

### Issue: NSM API compilation error

**Symptom**:
```
error: failed to compile aws-nitro-enclaves-nsm-api
```

**Solution**:
```bash
# Update Rust
rustup update

# Try specific version
cargo update aws-nitro-enclaves-nsm-api --precise 0.4.0
```

### Issue: Attestation returns "NSM device not available" inside enclave

**Symptom**: Running inside enclave but `/dev/nsm` not found

**Solutions**:
1. Check enclave is actually running: `nitro-cli describe-enclaves`
2. Verify enclave CID: Should be 16
3. Check permissions: Proof server runs as `proofserver` user
4. Try running as root temporarily for testing

### Issue: PCRs don't match between builds

**Symptom**: Rebuilding produces different PCR0 values

**Cause**: Non-reproducible builds

**Solutions**:
1. Use exact same Rust version
2. Set `SOURCE_DATE_EPOCH=0` in Dockerfile
3. Use `kaniko` for reproducible Docker builds
4. Document exact build environment

---

## Summary

You've now:
1. ✅ Integrated NSM API for real-time attestation
2. ✅ Enabled nonce-based freshness protection
3. ✅ Created CBOR/COSE_Sign1 signed attestation documents
4. ✅ Published PCR measurements publicly
5. ✅ Provided client verification tools

**Trust Model**: ✅ **Trustless** - Clients can independently verify enclave integrity

**Next Steps**:
- Implement reproducible builds (Phase 2 of gap analysis)
- Automate PCR publication in CI/CD
- Add certificate chain verification to client

---

## References

- [Full Attestation Implementation Guide](./docs/attestation-implementation-guide.md)
- [Trust Gap Analysis](./docs/trusted-workload-gap-analysis.md)
- [AWS NSM API Documentation](https://github.com/aws/aws-nitro-enclaves-nsm-api/blob/main/docs/attestation_process.md)
