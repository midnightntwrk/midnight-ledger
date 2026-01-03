# AWS Nitro Instance Deployment Guide

This guide explains how to update your AWS Nitro enclave with the new proof server code that includes TLS support and attestation endpoint fixes.

---

## Prerequisites

Before starting, make sure you have:
- ✅ SSH or SSM access to your Nitro-enabled EC2 instance
- ✅ Docker installed on the EC2 instance
- ✅ Nitro CLI installed on the EC2 instance
- ✅ Your GitHub credentials or SSH key configured on the instance

---

## Step 1: Connect to Your EC2 Instance

```bash
# Using SSH
ssh -i ~/.ssh/your-key.pem ec2-user@your-instance-ip

# OR using SSM (no SSH key needed)
aws ssm start-session --target i-your-instance-id
```

---

## Step 2: Stop the Running Enclave (if any)

```bash
# List running enclaves
nitro-cli describe-enclaves

# Note the enclave-id from the output, then terminate it
nitro-cli terminate-enclave --enclave-id <enclave-id>

# Verify it's stopped
nitro-cli describe-enclaves
```

---

## Step 3: Navigate to Your Code Directory

```bash
# Navigate to where your midnight-ledger code is
cd /home/ec2-user/midnight-ledger

# Or wherever you cloned it, e.g.:
cd ~/code/midnight-ledger
```

---

## Step 4: Pull the Latest Code

```bash
# Fetch the latest changes
git fetch origin

# Switch to your new branch (after you've pushed it)
git checkout feature/proof-server-tls-and-attestation

# Or if you merged to main:
git checkout main
git pull origin main
```

---

## Step 5: Build the Proof Server Binary

```bash
# Navigate to the proof server directory
cd tee-proof-server-proto/proof-server

# Build the release binary
cargo build --release

# Verify the binary was created
ls -lh ../../target/release/midnight-proof-server-prototype
```

---

## Step 6: Build the Docker Image

```bash
# Navigate back to the workspace root
cd /home/ec2-user/midnight-ledger

# Build the Docker image for Nitro Enclaves
docker build -f tee-proof-server-proto/Dockerfile -t midnight/proof-server:latest .
```

**Note:** This build might take 10-20 minutes as it compiles the entire Rust workspace.

---

## Step 7: Convert Docker Image to Nitro Enclave Image

```bash
# Convert the Docker image to an Enclave Image File (.eif)
nitro-cli build-enclave \
  --docker-uri midnight/proof-server:latest \
  --output-file midnight-proof-server.eif

# This will output important information:
# - Enclave Image Format (EIF) measurements (PCR0, PCR1, PCR2)
# - Image size
# Save these PCR values! You'll need them for attestation verification
```

**Important:** Copy the PCR values from the output. They look like:
```json
{
  "Measurements": {
    "HashAlgorithm": "Sha384 { ... }",
    "PCR0": "abc123...",
    "PCR1": "def456...",
    "PCR2": "ghi789..."
  }
}
```

---

## Step 8: Configure the Enclave Resources

```bash
# Allocate CPU and memory for the enclave
# Adjust these values based on your EC2 instance size
# Example: 4 CPUs and 8GB RAM

# Create or edit the enclave config (optional - you can pass via CLI)
cat > enclave-config.json <<EOF
{
  "cpu_count": 2,
  "memory_mib": 4096
}
EOF
```

---

## Step 9: Start the Nitro Enclave

```bash
# Run the enclave with TLS enabled
nitro-cli run-enclave \
  --eif-path midnight-proof-server.eif \
  --cpu-count 2 \
  --memory 4096 \
  --enclave-cid 16 \
  --debug-mode

# The --debug-mode flag allows console output for debugging
# Remove it in production for better security

# You should see output like:
# Start allocating memory...
# Started enclave with enclave-id: i-abc123-enc456...
```

**Save the enclave-id!** You'll need it to manage the enclave.

---

## Step 10: Verify the Enclave is Running

```bash
# Check enclave status
nitro-cli describe-enclaves

# Should show:
# [
#   {
#     "EnclaveID": "i-...",
#     "ProcessID": 1234,
#     "EnclaveCID": 16,
#     "NumberOfCPUs": 4,
#     "CPUIDs": [...],
#     "MemoryMiB": 8192,
#     "State": "RUNNING",
#     "Flags": "DEBUG_MODE"
#   }
# ]
```

---

## Step 11: View Enclave Console Logs (Debug Mode Only)

```bash
# View the enclave console output
nitro-cli console --enclave-id <your-enclave-id>

# You should see:
# - Proof server startup logs
# - "Listening on: https://0.0.0.0:6300" (or http if TLS disabled)
# - "Detected platform: AWS Nitro Enclaves" (new!)

# Press Ctrl+C to exit console view
```

---

## Step 12: Configure Port Forwarding (Parent EC2 Instance)

The enclave runs isolated, so you need to proxy traffic from the parent EC2 instance:

```bash
# Install vsock-proxy (if not already installed)
# This is usually included with nitro-cli

# Create a systemd service for the proxy
sudo tee /etc/systemd/system/nitro-vsock-proxy.service <<EOF
[Unit]
Description=Nitro Enclave vsock proxy for proof server
After=network.target

[Service]
Type=simple
User=ec2-user
ExecStart=/usr/bin/vsock-proxy 6300 127.0.0.1:6300 16
Restart=always
RestartSec=5

[Install]
WantedBy=multi-user.target
EOF

# Reload systemd and start the proxy
sudo systemctl daemon-reload
sudo systemctl enable nitro-vsock-proxy
sudo systemctl start nitro-vsock-proxy
sudo systemctl status nitro-vsock-proxy
```

**Explanation:**
- `6300` = parent EC2 port
- `127.0.0.1:6300` = where to forward traffic
- `16` = enclave CID (must match what you used in run-enclave)

---

## Step 13: Test the Attestation Endpoint

```bash
# From the parent EC2 instance, test the attestation endpoint
curl "http://localhost:6300/attestation?nonce=test123"

# Expected response (on AWS Nitro):
# {
#   "platform": "AWS Nitro Enclaves",
#   "format": "CBOR",
#   "nonce": "test123",
#   "error": "Attestation must be requested from parent EC2 instance using nitro-cli",
#   "metadata": {
#     "instructions": "From parent EC2 instance, run: nitro-cli describe-enclaves --enclave-id <id>",
#     "pcr_publication": "https://github.com/midnight/proof-server/releases"
#   }
# }
```

**Note:** The platform detection should now correctly identify AWS Nitro Enclaves! The Azure check won't hang because it's running on Linux in a proper cloud environment.

---

## Step 14: Get Actual Attestation Document

```bash
# To get the real attestation document, use nitro-cli from parent
nitro-cli describe-enclaves --enclave-id <your-enclave-id>

# This returns the attestation document with:
# - PCR measurements
# - Enclave state
# - Cryptographic proof of TEE integrity
```

---

## Step 15: Configure Security Group / Firewall

Make sure your EC2 instance security group allows:

```bash
# Allow HTTPS traffic on port 6300
aws ec2 authorize-security-group-ingress \
  --group-id sg-your-security-group \
  --protocol tcp \
  --port 6300 \
  --cidr 0.0.0.0/0  # Adjust this to restrict access
```

---

## Troubleshooting

### If the enclave fails to start:

```bash
# Check console logs
nitro-cli console --enclave-id <enclave-id>

# Check parent instance logs
sudo journalctl -u nitro-vsock-proxy -f
```

### If attestation still hangs:

The new code should fix this, but if it still happens:
- Verify you're running the latest code with `git log -1`
- Check the attestation.rs file has the macOS/Windows early exit code
- Verify the enclave console shows "Detected platform: AWS Nitro Enclaves"

### If you need to rebuild:

```bash
# Stop enclave
nitro-cli terminate-enclave --enclave-id <enclave-id>

# Rebuild from Step 6
```

---

## Summary Checklist

- [ ] Connect to EC2 instance
- [ ] Stop old enclave
- [ ] Pull latest code
- [ ] Build Rust binary
- [ ] Build Docker image
- [ ] Convert to EIF (save PCR values!)
- [ ] Start new enclave
- [ ] Verify enclave is running
- [ ] Configure vsock proxy
- [ ] Test attestation endpoint
- [ ] Update security group

---

## What Changed in This Update

### Attestation Improvements
- **Fixed platform detection hanging** on non-cloud environments
- Added macOS/Windows early exit (doesn't affect Linux/Nitro, but improves code quality)
- Added 2-second timeout to Azure metadata endpoint check
- Platform detection now correctly identifies AWS Nitro Enclaves

### TLS/HTTPS Support
- Added full HTTPS/TLS support using rustls
- Command-line flags for certificate configuration:
  - `--tls-cert` and `--tls-key` for custom certificates
  - `--auto-generate-cert` for self-signed certificates
  - `--disable-tls` for testing (not recommended for production)

### Performance & Monitoring
- Added memory tracking during proof generation
- Improved logging with timing information
- Added parameter pre-fetching at startup

### Stability
- Added graceful shutdown handling (responds to SIGTERM/SIGINT)
- Updated dependencies (Axum 0.7 → 0.8, tokio 1.35 → 1.48)

The new code is production-ready for AWS Nitro Enclaves and will properly handle attestation requests!
