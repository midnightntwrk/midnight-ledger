# PCR Publication Guide

**Midnight Proof Server - PCR Value Publishing and Verification**

❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌

## DANGER ZONE: All of the below is experimental, not yet tested ##

❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌

## Document Control

| Version | Date       | Author               | Changes       |
| ------- | ---------- | -------------------- | ------------- |
| 1.0     | 2025-12-19 | Bob Blessing-Hartley | Initial draft |

---

## Table of Contents

1. [Overview](#overview)
2. [Understanding PCR Values](#understanding-pcr-values)
3. [Prerequisites](#prerequisites)
4. [Extracting PCR Values](#extracting-pcr-values)
5. [Signing PCR Values](#signing-pcr-values)
6. [Publishing to GitHub](#publishing-to-github)
7. [Wallet Verification](#wallet-verification)
8. [PCR Rotation and Updates](#pcr-rotation-and-updates)
9. [Multi-Cloud PCR Management](#multi-cloud-pcr-management)
10. [Security Best Practices](#security-best-practices)
11. [Troubleshooting](#troubleshooting)

---

## Overview

### What are PCR Values?

**PCR (Platform Configuration Register)** values are cryptographic measurements that prove the integrity of code running in a Trusted Execution Environment (TEE). Think of them as "fingerprints" of your deployed proof server.

### Why Publish PCR Values?

When wallets request proof generation, they need to verify:
1. ✅ The proof server is running **unmodified code**
2. ✅ The server is running in a **genuine TEE** (not simulated)
3. ✅ Debug mode is **disabled** (no backdoors)
4. ✅ The deployment matches the **official release**

**Publishing signed PCR values enables trustless verification.**

### Trust Model

```
Developer → Signs Code → Publishes PCR → Wallet Verifies
   (You)      (GPG)      (GitHub)         (Users)

Without PCRs: Users must trust the server operator
With PCRs: Users cryptographically verify server integrity
```

---

## Understanding PCR Values

### What Do PCR Values Measure?

Different PCR registers measure different components:

| PCR | Component | AWS Nitro | GCP/Azure |
|-----|-----------|-----------|-----------|
| **PCR0** | Firmware/BIOS | Enclave kernel | UEFI firmware |
| **PCR1** | Configuration | Enclave kernel config | UEFI config |
| **PCR2** | Boot components | Enclave application | Boot loader (GRUB) |
| **PCR3** | - | - | - |
| **PCR4** | Boot loader | - | Partition table |
| **PCR5** | GPT/Config | - | GPT |
| **PCR6** | - | - | - |
| **PCR7** | Secure Boot | - | Secure Boot policy |
| **PCR8** | Kernel | - | Linux kernel |
| **PCR9** | Initrd | - | Initial ramdisk |

### Critical PCRs for Verification

**AWS Nitro Enclaves:**
- **PCR0**: Enclave image root hash (most important)
- **PCR1**: Kernel configuration
- **PCR2**: Application (proof server binary)

**GCP/Azure Confidential VMs:**
- **PCR0**: UEFI firmware
- **PCR1**: UEFI configuration
- **PCR4**: Boot loader
- **PCR7**: Secure Boot policy
- **PCR8**: Linux kernel
- **PCR9**: Initrd

### When PCR Values Change

PCR values change when:
- ✅ **Code updated** (new proof server version)
- ✅ **Dependencies change** (Rust version, libraries)
- ✅ **Base image updated** (Debian, Ubuntu)
- ✅ **Configuration modified** (environment variables in image)
- ❌ **Runtime data** (does not affect PCRs)

**Important:** Even identical code on different clouds produces different PCR values!

---

## Prerequisites

### Required Tools

```bash
# 1. GPG (for signing)
# macOS
brew install gnupg

# Linux
sudo apt-get install gnupg

# 2. GitHub CLI (for publishing)
# macOS
brew install gh

# Linux
curl -fsSL https://cli.github.com/packages/githubcli-archive-keyring.gpg | sudo dd of=/usr/share/keyrings/githubcli-archive-keyring.gpg
echo "deb [arch=$(dpkg --print-architecture) signed-by=/usr/share/keyrings/githubcli-archive-keyring.gpg] https://cli.github.com/packages stable main" | sudo tee /etc/apt/sources.list.d/github-cli.list > /dev/null
sudo apt update
sudo apt install gh

# 3. jq (JSON processing)
brew install jq  # macOS
sudo apt install jq  # Linux

# Verify installations
gpg --version
gh --version
jq --version
```

### Generate GPG Key (First Time Only)

```bash
# Generate new GPG key for signing
gpg --full-generate-key

# Select:
# - Key type: (1) RSA and RSA
# - Key size: 4096
# - Expiration: 2y (2 years)
# - Real name: Midnight Foundation
# - Email: security@midnight.network
# - Comment: Proof Server PCR Signing Key

# List keys
gpg --list-secret-keys --keyid-format=long

# Export public key for distribution
gpg --armor --export security@midnight.network > midnight-pgp-public.asc

# Backup private key (KEEP SECURE!)
gpg --armor --export-secret-keys security@midnight.network > midnight-pgp-private.asc.BACKUP
chmod 600 midnight-pgp-private.asc.BACKUP

# Upload public key to keyserver
gpg --send-keys <KEY_ID>
```

### GitHub Authentication

```bash
# Login to GitHub CLI
gh auth login

# Verify access
gh repo view midnight/proof-server
```

---

## Extracting PCR Values

### AWS Nitro Enclaves

#### Method 1: From Running Enclave

```bash
# SSH into parent EC2 instance
ssh -i your-key.pem ec2-user@<instance-ip>

# Get enclave measurements
nitro-cli describe-enclaves

# Example output:
# {
#   "EnclaveID": "i-1234567890abcdef0-enc1234567890abcd",
#   "Measurements": {
#     "PCR0": "000000...",
#     "PCR1": "000000...",
#     "PCR2": "000000..."
#   },
#   ...
# }

# Save to file
nitro-cli describe-enclaves | jq '.[] | {
  enclave_id: .EnclaveID,
  measurements: .Measurements,
  debug_mode: .Flags.DebugMode
}' > aws-pcr-raw.json
```

#### Method 2: From .eif File

```bash
# Get measurements from enclave image file
nitro-cli describe-eif --eif-path midnight-proof-server.eif

# Save measurements
nitro-cli describe-eif --eif-path midnight-proof-server.eif | \
  jq '{
    image_file: "midnight-proof-server.eif",
    measurements: .Measurements,
    signature: .Signature
  }' > aws-eif-measurements.json
```

#### Create AWS PCR Publication File

```bash
cat > aws-nitro-pcr-values.json << EOF
{
  "version": "1.0.0",
  "cloud_provider": "AWS",
  "tee_type": "Nitro Enclaves",
  "timestamp": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "git_commit": "$(git rev-parse HEAD)",
  "docker_image": "midnight-proof-server:v1.0.0",
  "pcr_values": {
    "PCR0": "$(nitro-cli describe-enclaves | jq -r '.[0].Measurements.PCR0')",
    "PCR1": "$(nitro-cli describe-enclaves | jq -r '.[0].Measurements.PCR1')",
    "PCR2": "$(nitro-cli describe-enclaves | jq -r '.[0].Measurements.PCR2')"
  },
  "security": {
    "debug_mode": false,
    "production": true
  },
  "verification_steps": [
    "Verify GPG signature on this file",
    "Request attestation document from proof server",
    "Extract PCR values from attestation document",
    "Compare with published values above",
    "Verify debug_mode is false in attestation"
  ]
}
EOF
```

### GCP Confidential VMs

```bash
# SSH into Confidential VM
gcloud compute ssh midnight-proof-server --zone=us-central1-a

# Install tpm2-tools if not present
sudo apt-get update && sudo apt-get install -y tpm2-tools

# Read PCR values
sudo tpm2_pcrread sha256:0,1,4,5,7,8,9

# Save to structured JSON
cat > gcp-confidential-pcr-values.json << 'EOFGCP'
{
  "version": "1.0.0",
  "cloud_provider": "GCP",
  "tee_type": "Confidential VM (AMD SEV-SNP)",
  "timestamp": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "git_commit": "$(git rev-parse HEAD)",
  "docker_image": "midnight-proof-server:v1.0.0",
  "instance_type": "n2d-standard-8",
  "location": "$(curl -s -H Metadata:true http://169.254.169.254/computeMetadata/v1/instance/zone -H Metadata-Flavor:Google | cut -d/ -f4)",
  "pcr_values": {
    "PCR0": "$(sudo tpm2_pcrread sha256:0 | grep 'sha256: 0' | awk '{print $3}')",
    "PCR1": "$(sudo tpm2_pcrread sha256:1 | grep 'sha256: 1' | awk '{print $3}')",
    "PCR4": "$(sudo tpm2_pcrread sha256:4 | grep 'sha256: 4' | awk '{print $3}')",
    "PCR5": "$(sudo tpm2_pcrread sha256:5 | grep 'sha256: 5' | awk '{print $3}')",
    "PCR7": "$(sudo tpm2_pcrread sha256:7 | grep 'sha256: 7' | awk '{print $3}')",
    "PCR8": "$(sudo tpm2_pcrread sha256:8 | grep 'sha256: 8' | awk '{print $3}')",
    "PCR9": "$(sudo tpm2_pcrread sha256:9 | grep 'sha256: 9' | awk '{print $3}')"
  },
  "security": {
    "confidential_compute": true,
    "vtpm_enabled": true,
    "secure_boot": true,
    "production": true
  },
  "verification_steps": [
    "Verify GPG signature on this file",
    "Request TPM quote from proof server with random nonce",
    "Verify TPM quote signature against Google CA",
    "Extract PCR values from TPM quote",
    "Compare with published values above",
    "Verify quote nonce matches your request"
  ]
}
EOFGCP

# Download to local machine
gcloud compute scp midnight-proof-server:~/gcp-confidential-pcr-values.json . \
  --zone=us-central1-a
```

### Azure Confidential VMs

```bash
# SSH into Confidential VM
az ssh vm --resource-group midnight-proof-server-rg --name midnight-proof-server

# Install tpm2-tools
sudo apt-get update && sudo apt-get install -y tpm2-tools

# Read PCR values
sudo tpm2_pcrread sha256:0,1,2,3,4,5,6,7,8,9,10,11,12

# Create Azure PCR publication file
cat > azure-confidential-pcr-values.json << 'EOFAZURE'
{
  "version": "1.0.0",
  "cloud_provider": "Azure",
  "tee_type": "Confidential VM (AMD SEV-SNP)",
  "timestamp": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "git_commit": "$(git rev-parse HEAD)",
  "docker_image": "midnight-proof-server:v1.0.0",
  "instance_type": "Standard_DC4s_v3",
  "location": "$(curl -s -H Metadata:true http://169.254.169.254/metadata/instance/compute/location?api-version=2021-02-01&format=text)",
  "pcr_values": {
    "PCR0": "$(sudo tpm2_pcrread sha256:0 | grep 'sha256: 0' | awk '{print $3}')",
    "PCR1": "$(sudo tpm2_pcrread sha256:1 | grep 'sha256: 1' | awk '{print $3}')",
    "PCR2": "$(sudo tpm2_pcrread sha256:2 | grep 'sha256: 2' | awk '{print $3}')",
    "PCR3": "$(sudo tpm2_pcrread sha256:3 | grep 'sha256: 3' | awk '{print $3}')",
    "PCR4": "$(sudo tpm2_pcrread sha256:4 | grep 'sha256: 4' | awk '{print $3}')",
    "PCR5": "$(sudo tpm2_pcrread sha256:5 | grep 'sha256: 5' | awk '{print $3}')",
    "PCR6": "$(sudo tpm2_pcrread sha256:6 | grep 'sha256: 6' | awk '{print $3}')",
    "PCR7": "$(sudo tpm2_pcrread sha256:7 | grep 'sha256: 7' | awk '{print $3}')",
    "PCR8": "$(sudo tpm2_pcrread sha256:8 | grep 'sha256: 8' | awk '{print $3}')",
    "PCR9": "$(sudo tpm2_pcrread sha256:9 | grep 'sha256: 9' | awk '{print $3}')",
    "PCR10": "$(sudo tpm2_pcrread sha256:10 | grep 'sha256: 10' | awk '{print $3}')",
    "PCR11": "$(sudo tpm2_pcrread sha256:11 | grep 'sha256: 11' | awk '{print $3}')",
    "PCR12": "$(sudo tpm2_pcrread sha256:12 | grep 'sha256: 12' | awk '{print $3}')"
  },
  "security": {
    "security_type": "ConfidentialVM",
    "vtpm_enabled": true,
    "secure_boot": true,
    "production": true
  },
  "attestation": {
    "service": "Azure Attestation Service",
    "format": "JWT"
  },
  "verification_steps": [
    "Verify GPG signature on this file",
    "Request JWT attestation token from proof server with random nonce",
    "Decode JWT token (3 parts: header.payload.signature)",
    "Verify JWT signature against Azure Attestation Service public key",
    "Extract PCR values from JWT payload claims",
    "Compare with published values above",
    "Verify nonce in JWT matches your request"
  ]
}
EOFAZURE

# Download to local machine
az vm run-command invoke \
  --resource-group midnight-proof-server-rg \
  --name midnight-proof-server \
  --command-id RunShellScript \
  --scripts "cat ~/azure-confidential-pcr-values.json" \
  --query 'value[0].message' \
  --output tsv > azure-confidential-pcr-values.json
```

---

## Signing PCR Values

### Why Sign PCR Values?

Signing proves that the PCR values were published by the legitimate Midnight Foundation, not an attacker. Wallets verify the signature before trusting the PCR values.

### Sign Each Cloud's PCR File

```bash
# Navigate to directory with PCR files
cd pcr-releases/v1.0.0

# Sign AWS PCR values
gpg --armor --detach-sign aws-nitro-pcr-values.json

# Sign GCP PCR values
gpg --armor --detach-sign gcp-confidential-pcr-values.json

# Sign Azure PCR values
gpg --armor --detach-sign azure-confidential-pcr-values.json

# Verify signatures locally
gpg --verify aws-nitro-pcr-values.json.asc aws-nitro-pcr-values.json
gpg --verify gcp-confidential-pcr-values.json.asc gcp-confidential-pcr-values.json
gpg --verify azure-confidential-pcr-values.json.asc azure-confidential-pcr-values.json

# List files
ls -lh
# aws-nitro-pcr-values.json
# aws-nitro-pcr-values.json.asc
# gcp-confidential-pcr-values.json
# gcp-confidential-pcr-values.json.asc
# azure-confidential-pcr-values.json
# azure-confidential-pcr-values.json.asc
```

### Create Release README

```bash
cat > README.md << 'EOFREADME'
# Midnight Proof Server v1.0.0 - PCR Values

**Release Date:** 2025-12-18
**Git Commit:** $(git rev-parse HEAD)
**Docker Image:** midnight-proof-server:v1.0.0

## Files in This Release

- `aws-nitro-pcr-values.json` - PCR values for AWS Nitro Enclaves deployment
- `aws-nitro-pcr-values.json.asc` - GPG signature for AWS PCRs
- `gcp-confidential-pcr-values.json` - PCR values for GCP Confidential VM deployment
- `gcp-confidential-pcr-values.json.asc` - GPG signature for GCP PCRs
- `azure-confidential-pcr-values.json` - PCR values for Azure Confidential VM deployment
- `azure-confidential-pcr-values.json.asc` - GPG signature for Azure PCRs
- `midnight-pgp-public.asc` - Public GPG key for verification

## Verification Instructions

### 1. Import Public Key

```bash
curl -sL https://github.com/midnight/proof-server/releases/download/v1.0.0/midnight-pgp-public.asc | gpg --import
```

### 2. Verify Signature

```bash
# AWS
gpg --verify aws-nitro-pcr-values.json.asc aws-nitro-pcr-values.json

# GCP
gpg --verify gcp-confidential-pcr-values.json.asc gcp-confidential-pcr-values.json

# Azure
gpg --verify azure-confidential-pcr-values.json.asc azure-confidential-pcr-values.json
```

### 3. Request Attestation from Server

See individual PCR files for cloud-specific verification steps.

## Trust Model

1. ✅ Download PCR files and signatures from GitHub release
2. ✅ Verify GPG signatures (proves Midnight Foundation published them)
3. ✅ Request attestation from live proof server
4. ✅ Compare attestation PCRs with published PCRs (proves server matches release)
5. ✅ Verify security properties (debug mode off, production config)

## Support

- Issues: https://github.com/midnight/proof-server/issues
- Discord: https://discord.gg/midnight
- Email: security@midnight.network
EOFREADME
```

---

## Publishing to GitHub

### Create GitHub Release

```bash
# Navigate to repository root
cd /Users/robertblessing-hartley/code/tee-prover-prototype

# Create git tag
git tag -a v1.0.0 -m "Midnight Proof Server v1.0.0"
git push origin v1.0.0

# Create GitHub release with all files
gh release create v1.0.0 \
  --title "Midnight Proof Server v1.0.0" \
  --notes-file pcr-releases/v1.0.0/README.md \
  pcr-releases/v1.0.0/aws-nitro-pcr-values.json \
  pcr-releases/v1.0.0/aws-nitro-pcr-values.json.asc \
  pcr-releases/v1.0.0/gcp-confidential-pcr-values.json \
  pcr-releases/v1.0.0/gcp-confidential-pcr-values.json.asc \
  pcr-releases/v1.0.0/azure-confidential-pcr-values.json \
  pcr-releases/v1.0.0/azure-confidential-pcr-values.json.asc \
  midnight-pgp-public.asc

# Verify release
gh release view v1.0.0
```

### Release Checklist

Before publishing, verify:

- [ ] PCR files contain correct values from production deployments
- [ ] All PCR files are signed with GPG
- [ ] Signatures verify successfully
- [ ] README.md includes verification instructions
- [ ] Public GPG key is included
- [ ] Git tag matches version in PCR files
- [ ] Docker image tag matches version
- [ ] Debug mode is disabled in all deployments
- [ ] Production flag is set in all PCR files

---

## Wallet Verification

### Wallet Integration (Rust Example)

```rust
// File: wallets/pcr-verification/src/lib.rs

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Deserialize)]
pub struct PcrPublication {
    pub version: String,
    pub cloud_provider: String,
    pub tee_type: String,
    pub timestamp: String,
    pub git_commit: String,
    pub docker_image: String,
    pub pcr_values: HashMap<String, String>,
    pub security: SecurityConfig,
}

#[derive(Debug, Deserialize)]
pub struct SecurityConfig {
    pub debug_mode: Option<bool>,
    pub production: bool,
}

pub struct PcrVerifier {
    trusted_pcrs: HashMap<String, PcrPublication>,
}

impl PcrVerifier {
    /// Load published PCR values from GitHub release
    pub async fn new(version: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let base_url = format!(
            "https://github.com/midnight/proof-server/releases/download/{}",
            version
        );

        // Download and verify GPG signatures (not shown for brevity)
        let aws_pcrs = Self::fetch_and_verify_pcrs(
            &format!("{}/aws-nitro-pcr-values.json", base_url),
            &format!("{}/aws-nitro-pcr-values.json.asc", base_url),
        )
        .await?;

        let gcp_pcrs = Self::fetch_and_verify_pcrs(
            &format!("{}/gcp-confidential-pcr-values.json", base_url),
            &format!("{}/gcp-confidential-pcr-values.json.asc", base_url),
        )
        .await?;

        let azure_pcrs = Self::fetch_and_verify_pcrs(
            &format!("{}/azure-confidential-pcr-values.json", base_url),
            &format!("{}/azure-confidential-pcr-values.json.asc", base_url),
        )
        .await?;

        let mut trusted_pcrs = HashMap::new();
        trusted_pcrs.insert("AWS".to_string(), aws_pcrs);
        trusted_pcrs.insert("GCP".to_string(), gcp_pcrs);
        trusted_pcrs.insert("Azure".to_string(), azure_pcrs);

        Ok(Self { trusted_pcrs })
    }

    /// Verify attestation from proof server
    pub async fn verify_attestation(
        &self,
        proof_server_url: &str,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        // 1. Generate random nonce
        let nonce = Self::generate_nonce();

        // 2. Request attestation with nonce
        let attestation = self.request_attestation(proof_server_url, &nonce).await?;

        // 3. Detect cloud provider from attestation format
        let cloud_provider = self.detect_cloud_provider(&attestation)?;

        // 4. Get trusted PCRs for this cloud
        let trusted_pcrs = self
            .trusted_pcrs
            .get(&cloud_provider)
            .ok_or("Unknown cloud provider")?;

        // 5. Extract PCRs from attestation
        let attestation_pcrs = self.extract_pcrs_from_attestation(&attestation)?;

        // 6. Compare PCR values
        for (pcr_name, trusted_value) in &trusted_pcrs.pcr_values {
            let attestation_value = attestation_pcrs
                .get(pcr_name)
                .ok_or(format!("Missing PCR: {}", pcr_name))?;

            if trusted_value != attestation_value {
                return Err(format!(
                    "PCR mismatch: {} (expected: {}, got: {})",
                    pcr_name, trusted_value, attestation_value
                )
                .into());
            }
        }

        // 7. Verify security properties
        if let Some(debug_mode) = trusted_pcrs.security.debug_mode {
            if debug_mode {
                return Err("Debug mode enabled - not safe for production".into());
            }
        }

        if !trusted_pcrs.security.production {
            return Err("Not marked as production deployment".into());
        }

        // 8. Verify nonce (prevents replay attacks)
        let attestation_nonce = self.extract_nonce_from_attestation(&attestation)?;
        if nonce != attestation_nonce {
            return Err("Nonce mismatch - possible replay attack".into());
        }

        Ok(true)
    }

    fn generate_nonce() -> String {
        use rand::Rng;
        let mut rng = rand::thread_rng();
        let nonce: [u8; 32] = rng.gen();
        hex::encode(nonce)
    }

    async fn request_attestation(
        &self,
        url: &str,
        nonce: &str,
    ) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let client = reqwest::Client::new();
        let response = client
            .get(format!("{}/attestation?nonce={}", url, nonce))
            .send()
            .await?;

        Ok(response.bytes().await?.to_vec())
    }

    fn detect_cloud_provider(&self, attestation: &[u8]) -> Result<String, Box<dyn std::error::Error>> {
        // AWS: CBOR format
        if attestation.starts_with(&[0xd2, 0x84, 0x44]) {
            return Ok("AWS".to_string());
        }

        // GCP/Azure: TPM quote or JWT
        // (simplified - real implementation would be more robust)
        if attestation[0] == b'{' {
            let json: serde_json::Value = serde_json::from_slice(attestation)?;
            if json.get("jwt_token").is_some() {
                return Ok("Azure".to_string());
            } else {
                return Ok("GCP".to_string());
            }
        }

        Err("Unknown attestation format".into())
    }

    fn extract_pcrs_from_attestation(
        &self,
        attestation: &[u8],
    ) -> Result<HashMap<String, String>, Box<dyn std::error::Error>> {
        // Implementation depends on cloud provider
        // See cloud-specific attestation verification code
        todo!("Implement PCR extraction for each cloud")
    }

    fn extract_nonce_from_attestation(
        &self,
        attestation: &[u8],
    ) -> Result<String, Box<dyn std::error::Error>> {
        // Implementation depends on cloud provider
        todo!("Implement nonce extraction for each cloud")
    }

    async fn fetch_and_verify_pcrs(
        pcr_url: &str,
        sig_url: &str,
    ) -> Result<PcrPublication, Box<dyn std::error::Error>> {
        // 1. Download PCR file
        let pcr_data = reqwest::get(pcr_url).await?.text().await?;

        // 2. Download signature
        let sig_data = reqwest::get(sig_url).await?.text().await?;

        // 3. Verify GPG signature
        // (using gpgme or similar library)
        Self::verify_gpg_signature(&pcr_data, &sig_data)?;

        // 4. Parse PCR data
        let pcrs: PcrPublication = serde_json::from_str(&pcr_data)?;

        Ok(pcrs)
    }

    fn verify_gpg_signature(
        data: &str,
        signature: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Use gpgme crate or call gpg command
        // Verify against Midnight Foundation public key
        todo!("Implement GPG verification")
    }
}

// Usage in wallet:
// let verifier = PcrVerifier::new("v1.0.0").await?;
// if verifier.verify_attestation("https://proof.midnight.network").await? {
//     println!("✅ Proof server verified!");
// }
```

---

## PCR Rotation and Updates

### When to Publish New PCRs

Publish new PCR values when:

1. **Code Update**: New proof server version released
2. **Security Patch**: Critical vulnerability fixed
3. **Dependency Update**: Rust version or library updates
4. **Base Image Update**: New Debian/Ubuntu version
5. **Configuration Change**: Environment variables baked into image

### Update Process

```bash
# 1. Deploy new version to all clouds
# (follow deployment guides for each cloud)

# 2. Extract new PCR values
# (use extraction scripts above)

# 3. Test deployments
curl https://proof-aws.midnight.network/health
curl https://proof-gcp.midnight.network/health
curl https://proof-azure.midnight.network/health

# 4. Sign new PCR values
gpg --armor --detach-sign aws-nitro-pcr-values-v1.1.0.json
gpg --armor --detach-sign gcp-confidential-pcr-values-v1.1.0.json
gpg --armor --detach-sign azure-confidential-pcr-values-v1.1.0.json

# 5. Create new GitHub release
gh release create v1.1.0 \
  --title "Midnight Proof Server v1.1.0" \
  --notes "See CHANGELOG.md for changes" \
  ...

# 6. Announce to wallet developers
# Email, Discord, GitHub discussions

# 7. Grace period (recommended: 30 days)
# Allow time for wallets to update

# 8. Deprecate old version
gh release edit v1.0.0 --notes "⚠️ DEPRECATED: Use v1.1.0"
```

### Version Support Policy

**Recommended policy:**
- **Latest version**: Fully supported
- **Previous version**: Supported for 90 days after new release
- **Older versions**: Unsupported, wallets should reject

Example timeline:
```
v1.0.0 released: 2025-01-01
v1.1.0 released: 2025-04-01 (v1.0.0 enters 90-day grace period)
v1.0.0 deprecated: 2025-07-01 (90 days after v1.1.0)
```

---

## Multi-Cloud PCR Management

### Challenge: Different PCR Values Per Cloud

The same code produces different PCR values on each cloud:

```
midnight-proof-server:v1.0.0 (identical code)
  ├─ AWS Nitro   → PCR0: abc123...
  ├─ GCP Conf VM → PCR0: def456...
  └─ Azure Conf VM → PCR0: 789ghi...
```

### Solution: Publish All Cloud PCRs

```bash
# Directory structure
pcr-releases/
├── v1.0.0/
│   ├── aws-nitro-pcr-values.json
│   ├── aws-nitro-pcr-values.json.asc
│   ├── gcp-confidential-pcr-values.json
│   ├── gcp-confidential-pcr-values.json.asc
│   ├── azure-confidential-pcr-values.json
│   ├── azure-confidential-pcr-values.json.asc
│   └── README.md
├── v1.1.0/
│   └── ...
└── midnight-pgp-public.asc
```

### Wallet Strategy: Support All Clouds

```rust
// Wallet should accept attestations from any published cloud
let aws_valid = verifier.verify_attestation_aws(url).await?;
let gcp_valid = verifier.verify_attestation_gcp(url).await?;
let azure_valid = verifier.verify_attestation_azure(url).await?;

if aws_valid || gcp_valid || azure_valid {
    println!("✅ Proof server verified on at least one cloud");
}
```

---

## Security Best Practices

### 1. Secure Key Management

```bash
# Generate key on air-gapped machine (ideal)
# OR use hardware security module (HSM)

# Encrypt private key at rest
gpg --armor --export-secret-keys security@midnight.network | \
  gpg --symmetric --cipher-algo AES256 > midnight-private-key.gpg.enc

# Use key only for signing, never for encryption
```

### 2. Multi-Signature Verification (Advanced)

For maximum security, require N-of-M signatures:

```bash
# Generate 3 keys (held by 3 different people)
# Alice, Bob, Carol

# Require 2-of-3 signatures for valid PCR publication
gpg --sign aws-nitro-pcr-values.json  # Alice signs
gpg --sign aws-nitro-pcr-values.json.alice.asc  # Bob signs
gpg --sign aws-nitro-pcr-values.json.bob.asc  # Carol signs (optional)

# Wallets verify at least 2 signatures present
```

### 3. Automated PCR Monitoring

Set up alerts for unexpected PCR changes:

```bash
# Script to monitor PCRs
cat > monitor-pcrs.sh << 'EOFMON'
#!/bin/bash

EXPECTED_PCRS="pcr-releases/v1.0.0/aws-nitro-pcr-values.json"
CURRENT_PCRS=$(ssh ec2-user@production-server "nitro-cli describe-enclaves | jq '.[]Measurements'")

if [ "$(jq '.pcr_values' $EXPECTED_PCRS)" != "$CURRENT_PCRS" ]; then
    echo "⚠️ PCR MISMATCH DETECTED!"
    echo "Expected: $(cat $EXPECTED_PCRS)"
    echo "Current: $CURRENT_PCRS"
    # Send alert to security team
    curl -X POST https://alerts.midnight.network/pcr-mismatch \
      -d '{"alert": "PCR values changed unexpectedly"}'
fi
EOFMON

chmod +x monitor-pcrs.sh

# Run hourly via cron
echo "0 * * * * /path/to/monitor-pcrs.sh" | crontab -
```

### 4. Transparency Log (Optional)

For maximum transparency, consider publishing PCRs to a public ledger:

```bash
# Example: Certificate Transparency-style log
curl -X POST https://pcr-transparency-log.midnight.network/submit \
  -H "Content-Type: application/json" \
  -d @aws-nitro-pcr-values.json

# Returns:
# {
#   "timestamp": "2025-12-18T10:30:00Z",
#   "log_index": 12345,
#   "signature": "abc123..."
# }
```

---

## Troubleshooting

### Issue 1: GPG Signature Verification Fails

**Symptom:**
```
gpg: Can't check signature: No public key
```

**Solution:**
```bash
# Import public key
curl -sL https://github.com/midnight/proof-server/releases/download/v1.0.0/midnight-pgp-public.asc | gpg --import

# Or from keyserver
gpg --keyserver keys.openpgp.org --recv-keys <KEY_ID>

# Verify import
gpg --list-keys security@midnight.network
```

### Issue 2: PCR Values Don't Match

**Symptom:**
Attestation PCRs differ from published PCRs

**Diagnosis:**
```bash
# Check deployment version
curl https://proof.midnight.network/version

# Check published version
cat aws-nitro-pcr-values.json | jq '.version'

# Check git commit
curl https://proof.midnight.network/version | jq '.git_commit'
cat aws-nitro-pcr-values.json | jq '.git_commit'
```

**Possible Causes:**
1. Server running different version than published
2. Debug mode enabled (PCR0 differs)
3. Configuration baked into image differs
4. Using wrong cloud's PCR values

**Solution:**
```bash
# Redeploy with exact version
docker pull midnight-proof-server:v1.0.0
# Extract PCRs again
# Compare with running deployment
```

### Issue 3: Nonce Verification Fails

**Symptom:**
Nonce in attestation doesn't match requested nonce

**Cause:**
- Clock skew between client and server
- Replay attack attempt
- Bug in nonce handling

**Solution:**
```bash
# Check time synchronization
ssh ec2-user@proof-server "chronyc tracking"

# Test with fresh nonce
NONCE=$(openssl rand -hex 32)
curl "https://proof.midnight.network/attestation?nonce=$NONCE"
```

### Issue 4: GitHub Release Upload Fails

**Symptom:**
```
gh: release not found
```

**Solution:**
```bash
# Verify tag exists
git tag -l | grep v1.0.0

# Create tag if missing
git tag -a v1.0.0 -m "Release v1.0.0"
git push origin v1.0.0

# Verify GitHub authentication
gh auth status

# Retry release creation
gh release create v1.0.0 --title "..." ...
```

---

## Summary

### PCR Publication Workflow

1. ✅ **Extract** PCR values from production deployments (all clouds)
2. ✅ **Format** as structured JSON with metadata
3. ✅ **Sign** with GPG private key
4. ✅ **Verify** signatures locally
5. ✅ **Publish** to GitHub release with version tag
6. ✅ **Announce** to wallet developers
7. ✅ **Monitor** for unexpected changes

### Wallet Verification Workflow

1. ✅ **Download** published PCR values from GitHub release
2. ✅ **Verify** GPG signatures
3. ✅ **Request** attestation from proof server with random nonce
4. ✅ **Extract** PCR values from attestation
5. ✅ **Compare** with published values
6. ✅ **Verify** security properties (debug mode off)
7. ✅ **Verify** nonce matches request

### Key Takeaways

- 📌 PCR values are cryptographic proof of code integrity
- 📌 Different clouds produce different PCR values (publish all)
- 📌 GPG signatures prove PCRs came from Midnight Foundation
- 📌 Wallets MUST verify PCRs before trusting proof server
- 📌 Update PCRs whenever code changes
- 📌 Maintain 90-day grace period for version transitions

---

**Documentation Version:** 1.0
**Last Updated:** 2025-12-18

For questions:
- GitHub Issues: https://github.com/midnight/proof-server/issues
- Discord: https://discord.gg/midnight
- Email: security@midnight.network
