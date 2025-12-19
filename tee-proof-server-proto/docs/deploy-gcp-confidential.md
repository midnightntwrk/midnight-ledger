# GCP Confidential VMs Deployment Guide

**Midnight Proof Server - Google Cloud Platform Deployment**

❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌

## DANGER ZONE: All of the below is experimental, not yet tested ##

❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌

---

## Table of Contents

1. [Overview](#overview)
2. [Prerequisites](#prerequisites)
3. [Architecture](#architecture)
4. [Cost Estimation](#cost-estimation)
5. [Infrastructure Setup](#infrastructure-setup)
6. [Building the Docker Image](#building-the-docker-image)
7. [Deploying Confidential VM](#deploying-confidential-vm)
8. [Attestation Setup](#attestation-setup)
9. [Load Balancer Configuration](#load-balancer-configuration)
10. [Monitoring and Logging](#monitoring-and-logging)
11. [Security Configuration](#security-configuration)
12. [PCR Publication](#pcr-publication)
13. [Troubleshooting](#troubleshooting)
14. [Maintenance](#maintenance)
15. [Cost Optimization](#cost-optimization)

---

## Overview

### What is GCP Confidential Computing?

GCP Confidential VMs use **AMD SEV-SNP (Secure Encrypted Virtualization - Secure Nested Paging)** technology to provide:

- **Memory Encryption**: All VM memory is encrypted with a key owned by the CPU
- **TPM 2.0 Attestation**: Cryptographic proof of VM integrity using TPM measurements
- **Integrity Protection**: Prevents unauthorized access even from Google cloud operators
- **Standard Docker Deployment**: No special enclave build process required

### Why Choose GCP for Midnight Proof Server?

**Advantages:**
- ✅ **Easiest deployment** - Standard Docker containers, no special builds
- ✅ **Lower cost** - Approximately $341/month (vs AWS $371/month)
- ✅ **Mature AMD SEV-SNP** - Battle-tested memory encryption
- ✅ **Simple attestation** - TPM 2.0 is industry standard
- ✅ **Integrated monitoring** - Cloud Monitoring and Logging built-in

**Considerations:**
- ⚠️ Dependent on AMD processor availability
- ⚠️ TPM attestation more complex than AWS CBOR format
- ⚠️ Less control than AWS Nitro custom silicon

---

## Prerequisites

### Required Tools

Install the following on your local machine:

```bash
# 1. Google Cloud SDK (gcloud CLI)
# macOS
brew install google-cloud-sdk

# Linux
curl https://sdk.cloud.google.com | bash
exec -l $SHELL

# 2. Docker (for local image building)
# macOS
brew install docker

# Linux
curl -fsSL https://get.docker.com -o get-docker.sh
sudo sh get-docker.sh

# 3. jq (JSON parsing)
brew install jq  # macOS
sudo apt install jq  # Linux
```

### GCP Account Setup

1. **Create GCP Account**: https://console.cloud.google.com
2. **Enable Billing**: Link a valid payment method
3. **Create Project**:
   ```bash
   gcloud projects create midnight-proof-server \
     --name="Midnight Proof Server" \
     --set-as-default
   ```

4. **Enable Required APIs**:
   ```bash
   gcloud services enable compute.googleapis.com
   gcloud services enable logging.googleapis.com
   gcloud services enable monitoring.googleapis.com
   gcloud services enable cloudkms.googleapis.com
   gcloud services enable secretmanager.googleapis.com
   ```

5. **Set Default Project**:
   ```bash
   gcloud config set project midnight-proof-server
   ```

### Required Permissions

Your GCP user needs these IAM roles:

- `roles/compute.admin` - Create and manage VMs
- `roles/compute.securityAdmin` - Manage firewall rules
- `roles/iam.serviceAccountAdmin` - Create service accounts
- `roles/logging.admin` - Configure logging
- `roles/monitoring.admin` - Configure monitoring

```bash
# Grant permissions to your user
export PROJECT_ID=midnight-proof-server
export USER_EMAIL=your-email@example.com

gcloud projects add-iam-policy-binding $PROJECT_ID \
  --member="user:$USER_EMAIL" \
  --role="roles/compute.admin"

gcloud projects add-iam-policy-binding $PROJECT_ID \
  --member="user:$USER_EMAIL" \
  --role="roles/compute.securityAdmin"
```

---

## Architecture

### High-Level Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                      GCP Cloud Platform                      │
│                                                               │
│  ┌────────────────────────────────────────────────────────┐ │
│  │                   VPC Network                           │ │
│  │                 10.0.0.0/16                             │ │
│  │                                                          │ │
│  │  ┌──────────────────────────────────────────────────┐  │ │
│  │  │           Public Subnet (10.0.1.0/24)            │  │ │
│  │  │                                                   │  │ │
│  │  │  ┌─────────────────────────────────────────┐    │  │ │
│  │  │  │   Confidential VM Instance              │    │  │ │
│  │  │  │   n2d-standard-8 (AMD EPYC)             │    │  │ │
│  │  │  │                                          │    │  │ │
│  │  │  │  ┌───────────────────────────────────┐ │    │  │ │
│  │  │  │  │  AMD SEV-SNP Encrypted Memory     │ │    │  │ │
│  │  │  │  │                                    │ │    │  │ │
│  │  │  │  │  ┌──────────────────────────────┐ │ │    │  │ │
│  │  │  │  │  │  Docker Container            │ │ │    │  │ │
│  │  │  │  │  │  midnight-proof-server:latest│ │ │    │  │ │
│  │  │  │  │  │  Port 6300                   │ │ │    │  │ │
│  │  │  │  │  └──────────────────────────────┘ │ │    │  │ │
│  │  │  │  │                                    │ │    │  │ │
│  │  │  │  │  TPM 2.0 (vTPM)                   │ │    │  │ │
│  │  │  │  │  - Attestation                    │ │    │  │ │
│  │  │  │  │  - PCR Measurements               │ │    │  │ │
│  │  │  │  └───────────────────────────────────┘ │    │  │ │
│  │  │  └─────────────────────────────────────────┘    │  │ │
│  │  │                                                   │  │ │
│  │  └───────────────────────────────────────────────────┘  │ │
│  │                                                          │ │
│  │  ┌─────────────────────────────────────────┐            │ │
│  │  │   Cloud Load Balancer (HTTPS)           │            │ │
│  │  │   External IP: 34.x.x.x                 │            │ │
│  │  │   TLS Termination                       │            │ │
│  │  └─────────────────────────────────────────┘            │ │
│  │                                                          │ │
│  └──────────────────────────────────────────────────────────┘ │
│                                                               │
│  ┌────────────────────┐  ┌────────────────────┐             │
│  │  Cloud Monitoring  │  │   Cloud Logging    │             │
│  │  - Metrics         │  │   - Application    │             │
│  │  - Alerts          │  │   - System Logs    │             │
│  └────────────────────┘  └────────────────────┘             │
│                                                               │
└─────────────────────────────────────────────────────────────┘

        ↑                           ↑
        │ HTTPS (6300)              │ TPM Attestation
        │                           │
   ┌────┴─────┐              ┌──────┴──────┐
   │  Wallet  │              │   Wallet    │
   │  Client  │              │  Verifier   │
   └──────────┘              └─────────────┘
```

### Components

| Component | Purpose | Technology |
|-----------|---------|------------|
| **Confidential VM** | Host for proof server | AMD SEV-SNP |
| **Docker Container** | Proof server runtime | Standard Docker |
| **TPM 2.0 (vTPM)** | Attestation provider | Virtual TPM |
| **Cloud Load Balancer** | HTTPS termination | GCP HTTPS LB |
| **VPC Network** | Network isolation | GCP VPC |
| **Cloud Monitoring** | Metrics and alerts | GCP Monitoring |
| **Cloud Logging** | Log aggregation | GCP Logging |

---

## Cost Estimation

### Monthly Costs (us-central1 region)

| Resource | Configuration | Monthly Cost |
|----------|--------------|--------------|
| **Confidential VM** | n2d-standard-8 (8 vCPU, 32GB) | ~$220 |
| **Persistent Disk** | 100GB SSD (pd-ssd) | ~$17 |
| **Static IP** | 1 external IP | ~$7 |
| **Load Balancer** | HTTPS forwarding rule + backend | ~$18 |
| **Network Egress** | 1TB/month | ~$85 |
| **Cloud Monitoring** | Custom metrics + alerts | ~$8 |
| **Cloud Logging** | 50GB/month retention | ~$3 |
| **Secrets Manager** | API key storage | ~$1 |
| **Total** | | **~$359/month** |

### Cost Optimization Tips

1. **Committed Use Discounts**: Save 37-55% with 1-3 year commitments
2. **Sustained Use Discounts**: Automatic discounts for >25% monthly usage
3. **Regional Selection**: Choose cheaper regions (asia-south1, europe-west4)
4. **Right-sizing**: Start with n2d-standard-4 (4 vCPU, 16GB) for $110/month
5. **Network Egress**: Use Cloud CDN for static content
6. **Preemptible VMs**: Not recommended for production (may be terminated)

**Example with optimizations:**
- n2d-standard-8 with 3-year commitment: $220 → $100/month (55% savings)
- Total optimized: **~$239/month**

---

## Infrastructure Setup

### Step 1: Create VPC Network

```bash
# Set variables
export PROJECT_ID=midnight-proof-server
export REGION=us-central1
export ZONE=us-central1-a

# Create VPC network
gcloud compute networks create midnight-vpc \
  --subnet-mode=custom \
  --bgp-routing-mode=regional \
  --project=$PROJECT_ID

# Create subnet
gcloud compute networks subnets create midnight-subnet \
  --network=midnight-vpc \
  --range=10.0.1.0/24 \
  --region=$REGION \
  --project=$PROJECT_ID
```

### Step 2: Configure Firewall Rules

```bash
# Allow SSH (port 22) from your IP only
export MY_IP=$(curl -s ifconfig.me)
gcloud compute firewall-rules create midnight-allow-ssh \
  --network=midnight-vpc \
  --allow=tcp:22 \
  --source-ranges=$MY_IP/32 \
  --target-tags=proof-server \
  --description="Allow SSH from admin IP"

# Allow HTTPS (port 443) from anywhere
gcloud compute firewall-rules create midnight-allow-https \
  --network=midnight-vpc \
  --allow=tcp:443 \
  --source-ranges=0.0.0.0/0 \
  --target-tags=proof-server \
  --description="Allow HTTPS from internet"

# Allow health checks from GCP load balancer
gcloud compute firewall-rules create midnight-allow-health-check \
  --network=midnight-vpc \
  --allow=tcp:6300 \
  --source-ranges=35.191.0.0/16,130.211.0.0/22 \
  --target-tags=proof-server \
  --description="Allow health checks from GCP LB"

# Deny all other inbound traffic (implicit, but explicit is better)
gcloud compute firewall-rules create midnight-deny-all \
  --network=midnight-vpc \
  --action=deny \
  --rules=all \
  --source-ranges=0.0.0.0/0 \
  --priority=65534 \
  --description="Deny all other inbound traffic"
```

### Step 3: Create Service Account

```bash
# Create service account for VM
gcloud iam service-accounts create midnight-proof-server-sa \
  --display-name="Midnight Proof Server Service Account" \
  --project=$PROJECT_ID

# Grant minimum required permissions
gcloud projects add-iam-policy-binding $PROJECT_ID \
  --member="serviceAccount:midnight-proof-server-sa@$PROJECT_ID.iam.gserviceaccount.com" \
  --role="roles/logging.logWriter"

gcloud projects add-iam-policy-binding $PROJECT_ID \
  --member="serviceAccount:midnight-proof-server-sa@$PROJECT_ID.iam.gserviceaccount.com" \
  --role="roles/monitoring.metricWriter"
```

### Step 4: Create Static IP Address

```bash
# Reserve static external IP
gcloud compute addresses create midnight-proof-server-ip \
  --region=$REGION \
  --project=$PROJECT_ID

# Get the reserved IP
export EXTERNAL_IP=$(gcloud compute addresses describe midnight-proof-server-ip \
  --region=$REGION \
  --format="get(address)")

echo "Reserved IP: $EXTERNAL_IP"
```

---

## Building the Docker Image

### Step 1: Prepare Dockerfile

Create a production-optimized Dockerfile:

```dockerfile
# File: /Users/robertblessing-hartley/code/tee-prover-prototype/proof-server/Dockerfile

# Build stage
FROM rust:1.75-slim as builder

WORKDIR /app

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Copy source code
COPY . .

# Build release binary
RUN cargo build --release --bin midnight-proof-server-prototype

# Runtime stage
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

# Create non-root user
RUN useradd -m -u 1000 proofserver && \
    mkdir -p /app/data && \
    chown -R proofserver:proofserver /app

USER proofserver
WORKDIR /app

# Copy binary from builder
COPY --from=builder --chown=proofserver:proofserver \
    /app/target/release/midnight-proof-server-prototype \
    /usr/local/bin/midnight-proof-server-prototype

# Health check
HEALTHCHECK --interval=30s --timeout=10s --start-period=60s --retries=3 \
    CMD ["/usr/local/bin/midnight-proof-server-prototype", "--help"] || exit 1

# Expose port
EXPOSE 6300

# Set environment defaults
ENV MIDNIGHT_PROOF_SERVER_PORT=6300
ENV MIDNIGHT_PROOF_SERVER_NUM_WORKERS=8
ENV MIDNIGHT_PROOF_SERVER_RATE_LIMIT=10
ENV RUST_LOG=info

# Start server
ENTRYPOINT ["/usr/local/bin/midnight-proof-server-prototype"]
CMD ["--port", "6300"]
```

### Step 2: Build and Push to Google Container Registry

```bash
# Enable Container Registry API
gcloud services enable containerregistry.googleapis.com

# Configure Docker to use gcloud as credential helper
gcloud auth configure-docker

# Build image
cd /Users/robertblessing-hartley/code/tee-prover-prototype/proof-server
docker build -t gcr.io/$PROJECT_ID/midnight-proof-server:latest .

# Tag with version
docker tag gcr.io/$PROJECT_ID/midnight-proof-server:latest \
  gcr.io/$PROJECT_ID/midnight-proof-server:v1.0.0

# Push to GCR
docker push gcr.io/$PROJECT_ID/midnight-proof-server:latest
docker push gcr.io/$PROJECT_ID/midnight-proof-server:v1.0.0
```

**Alternative: Artifact Registry (Recommended for new projects)**

```bash
# Enable Artifact Registry API
gcloud services enable artifactregistry.googleapis.com

# Create repository
gcloud artifacts repositories create midnight-proof-server \
  --repository-format=docker \
  --location=$REGION \
  --description="Midnight Proof Server Docker images"

# Configure Docker
gcloud auth configure-docker $REGION-docker.pkg.dev

# Build and push
docker build -t $REGION-docker.pkg.dev/$PROJECT_ID/midnight-proof-server/server:latest .
docker push $REGION-docker.pkg.dev/$PROJECT_ID/midnight-proof-server/server:latest
```

---

## Deploying Confidential VM

### Step 1: Store API Key in Secret Manager

```bash
# Generate secure API key
export API_KEY=$(openssl rand -base64 32)
echo "Generated API Key: $API_KEY"
echo "⚠️  SAVE THIS KEY SECURELY - IT WON'T BE SHOWN AGAIN"

# Store in Secret Manager
echo -n "$API_KEY" | gcloud secrets create midnight-api-key \
  --data-file=- \
  --replication-policy="automatic" \
  --project=$PROJECT_ID

# Grant VM service account access to secret
gcloud secrets add-iam-policy-binding midnight-api-key \
  --member="serviceAccount:midnight-proof-server-sa@$PROJECT_ID.iam.gserviceaccount.com" \
  --role="roles/secretmanager.secretAccessor" \
  --project=$PROJECT_ID
```

### Step 2: Create Startup Script

Create a startup script that will run when the VM boots:

```bash
cat > startup.sh << 'EOF'
#!/bin/bash
set -e

# Install Docker if not present
if ! command -v docker &> /dev/null; then
    curl -fsSL https://get.docker.com -o get-docker.sh
    sh get-docker.sh
    rm get-docker.sh
fi

# Configure Docker logging
cat > /etc/docker/daemon.json << 'DOCKER_EOF'
{
  "log-driver": "gcplogs",
  "log-opts": {
    "gcp-log-cmd": "true",
    "labels": "service"
  }
}
DOCKER_EOF

systemctl restart docker

# Fetch API key from Secret Manager
PROJECT_ID=$(curl -s "http://metadata.google.internal/computeMetadata/v1/project/project-id" -H "Metadata-Flavor: Google")
API_KEY=$(gcloud secrets versions access latest --secret="midnight-api-key" --project=$PROJECT_ID)

# Pull and run Docker container
docker pull gcr.io/$PROJECT_ID/midnight-proof-server:latest

docker run -d \
  --name midnight-proof-server \
  --restart always \
  --label service=midnight-proof-server \
  -p 6300:6300 \
  -e MIDNIGHT_PROOF_SERVER_PORT=6300 \
  -e MIDNIGHT_PROOF_SERVER_API_KEY="$API_KEY" \
  -e MIDNIGHT_PROOF_SERVER_NUM_WORKERS=8 \
  -e MIDNIGHT_PROOF_SERVER_RATE_LIMIT=10 \
  -e MIDNIGHT_PROOF_SERVER_JOB_TIMEOUT=600 \
  -e RUST_LOG=info \
  gcr.io/$PROJECT_ID/midnight-proof-server:latest

# Health check
sleep 10
curl -f http://localhost:6300/health || exit 1

echo "Midnight Proof Server started successfully"
EOF
```

### Step 3: Create Confidential VM Instance

```bash
# Create instance with Confidential Computing enabled
gcloud compute instances create midnight-proof-server \
  --project=$PROJECT_ID \
  --zone=$ZONE \
  --machine-type=n2d-standard-8 \
  --network-interface=subnet=midnight-subnet,address=$EXTERNAL_IP \
  --tags=proof-server \
  --service-account=midnight-proof-server-sa@$PROJECT_ID.iam.gserviceaccount.com \
  --scopes=cloud-platform \
  --confidential-compute \
  --maintenance-policy=TERMINATE \
  --shielded-secure-boot \
  --shielded-vtpm \
  --shielded-integrity-monitoring \
  --boot-disk-size=100GB \
  --boot-disk-type=pd-ssd \
  --image-family=ubuntu-2204-lts \
  --image-project=ubuntu-os-cloud \
  --metadata-from-file=startup-script=startup.sh

echo "Confidential VM created successfully!"
echo "External IP: $EXTERNAL_IP"
```

**Important flags explained:**

- `--confidential-compute`: Enables AMD SEV-SNP memory encryption
- `--maintenance-policy=TERMINATE`: Required for Confidential VMs (can't live migrate)
- `--shielded-secure-boot`: UEFI Secure Boot
- `--shielded-vtpm`: Virtual TPM 2.0 for attestation
- `--shielded-integrity-monitoring`: Baseline integrity measurements

### Step 4: Verify Deployment

```bash
# Wait for VM to start (2-3 minutes)
gcloud compute instances describe midnight-proof-server \
  --zone=$ZONE \
  --format="get(status)"

# Check startup script logs
gcloud compute instances get-serial-port-output midnight-proof-server \
  --zone=$ZONE \
  --port=1 | tail -20

# SSH into VM
gcloud compute ssh midnight-proof-server --zone=$ZONE

# Inside VM: Check Docker container
docker ps
docker logs midnight-proof-server

# Test health endpoint
curl http://localhost:6300/health

# Test from external (should timeout - not exposed yet)
curl http://$EXTERNAL_IP:6300/health
```

---

## Attestation Setup

### Understanding TPM 2.0 Attestation

GCP Confidential VMs use **Virtual TPM (vTPM) 2.0** for attestation:

- **PCR Registers**: Platform Configuration Registers store measurements
- **Quote**: Cryptographically signed attestation document
- **AIK**: Attestation Identity Key (private key in TPM)
- **EK**: Endorsement Key (certified by Google)

**Key PCR Registers:**

| PCR | Contents | Description |
|-----|----------|-------------|
| **0** | Firmware | UEFI firmware measurements |
| **1** | UEFI Config | UEFI configuration and variables |
| **4** | Boot Loader | GRUB bootloader |
| **5** | GPT | Partition table |
| **7** | Secure Boot | Secure Boot policy |
| **8-9** | Kernel/initrd | Linux kernel and initial ramdisk |

### Step 1: Install TPM Tools

```bash
# SSH into the Confidential VM
gcloud compute ssh midnight-proof-server --zone=$ZONE

# Install tpm2-tools
sudo apt-get update
sudo apt-get install -y tpm2-tools

# Verify TPM is available
sudo tpm2_pcrread
```

### Step 2: Extract PCR Values

Create a script to extract PCR measurements:

```bash
# On the Confidential VM
cat > /usr/local/bin/extract-pcrs.sh << 'EOF'
#!/bin/bash
set -e

OUTPUT_FILE=${1:-/tmp/pcr-values.json}

# Read PCR values
PCR_VALUES=$(sudo tpm2_pcrread -o /tmp/pcrs.bin sha256:0,1,4,5,7,8,9 2>&1)

# Parse and format as JSON
echo "{" > $OUTPUT_FILE
echo "  \"attestation_format\": \"TPM_2.0\"," >> $OUTPUT_FILE
echo "  \"cloud_provider\": \"GCP\"," >> $OUTPUT_FILE
echo "  \"vm_type\": \"Confidential VM (AMD SEV-SNP)\"," >> $OUTPUT_FILE
echo "  \"timestamp\": \"$(date -u +%Y-%m-%dT%H:%M:%SZ)\"," >> $OUTPUT_FILE
echo "  \"instance_id\": \"$(curl -s http://metadata.google.internal/computeMetadata/v1/instance/id -H 'Metadata-Flavor: Google')\"," >> $OUTPUT_FILE
echo "  \"zone\": \"$(curl -s http://metadata.google.internal/computeMetadata/v1/instance/zone -H 'Metadata-Flavor: Google' | cut -d/ -f4)\"," >> $OUTPUT_FILE
echo "  \"pcr_values\": {" >> $OUTPUT_FILE

# Extract individual PCR values
for pcr in 0 1 4 5 7 8 9; do
    value=$(sudo tpm2_pcrread sha256:$pcr | grep "sha256: $pcr" | awk '{print $3}')
    echo "    \"PCR$pcr\": \"$value\"," >> $OUTPUT_FILE
done

# Remove trailing comma and close JSON
sed -i '$ s/,$//' $OUTPUT_FILE
echo "  }" >> $OUTPUT_FILE
echo "}" >> $OUTPUT_FILE

echo "PCR values extracted to: $OUTPUT_FILE"
cat $OUTPUT_FILE
EOF

sudo chmod +x /usr/local/bin/extract-pcrs.sh
```

### Step 3: Generate TPM Quote

```bash
# On the Confidential VM
cat > /usr/local/bin/generate-quote.sh << 'EOF'
#!/bin/bash
set -e

NONCE=${1:-"$(openssl rand -hex 32)"}
OUTPUT_FILE=${2:-/tmp/attestation-quote.bin}
OUTPUT_JSON=${OUTPUT_FILE}.json

# Create attestation key if not exists
if [ ! -f /tmp/ak.ctx ]; then
    sudo tpm2_createek -c /tmp/ek.ctx -G rsa -u /tmp/ek.pub
    sudo tpm2_createak -C /tmp/ek.ctx -c /tmp/ak.ctx -G rsa -g sha256 -s rsassa -u /tmp/ak.pub
fi

# Generate quote with nonce
sudo tpm2_quote \
    -c /tmp/ak.ctx \
    -l sha256:0,1,4,5,7,8,9 \
    -q "$NONCE" \
    -m $OUTPUT_FILE \
    -s /tmp/quote.sig \
    -o /tmp/quote.pcrs \
    -g sha256

# Convert to JSON
echo "{" > $OUTPUT_JSON
echo "  \"nonce\": \"$NONCE\"," >> $OUTPUT_JSON
echo "  \"quote\": \"$(base64 -w0 $OUTPUT_FILE)\"," >> $OUTPUT_JSON
echo "  \"signature\": \"$(base64 -w0 /tmp/quote.sig)\"," >> $OUTPUT_JSON
echo "  \"pcrs\": \"$(base64 -w0 /tmp/quote.pcrs)\"," >> $OUTPUT_JSON
echo "  \"ak_pub\": \"$(base64 -w0 /tmp/ak.pub)\"," >> $OUTPUT_JSON
echo "  \"timestamp\": \"$(date -u +%Y-%m-%dT%H:%M:%SZ)\"" >> $OUTPUT_JSON
echo "}" >> $OUTPUT_JSON

echo "Quote generated: $OUTPUT_JSON"
cat $OUTPUT_JSON
EOF

sudo chmod +x /usr/local/bin/generate-quote.sh
```

### Step 4: Add Attestation Endpoint to Proof Server

Create a simple attestation endpoint:

```bash
# On your local machine, edit src/lib.rs
# Add this handler:
```

```rust
// In src/lib.rs
use std::process::Command;

async fn attestation_handler(
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<serde_json::Value>, AppError> {
    // Get nonce from query parameter
    let nonce = params.get("nonce")
        .ok_or_else(|| AppError::BadRequest("Missing nonce parameter".to_string()))?;

    // Execute TPM quote generation script
    let output = Command::new("/usr/local/bin/generate-quote.sh")
        .arg(nonce)
        .output()
        .map_err(|e| AppError::InternalError(format!("Failed to generate quote: {}", e)))?;

    if !output.status.success() {
        return Err(AppError::InternalError("Quote generation failed".to_string()));
    }

    // Read the generated JSON
    let quote_json = tokio::fs::read_to_string("/tmp/attestation-quote.bin.json")
        .await
        .map_err(|e| AppError::InternalError(format!("Failed to read quote: {}", e)))?;

    let attestation: serde_json::Value = serde_json::from_str(&quote_json)
        .map_err(|e| AppError::InternalError(format!("Failed to parse quote: {}", e)))?;

    Ok(Json(attestation))
}

// Add to router in create_app():
// .route("/attestation", get(attestation_handler))
```

**Rebuild and redeploy:**

```bash
# On your local machine
docker build -t gcr.io/$PROJECT_ID/midnight-proof-server:latest .
docker push gcr.io/$PROJECT_ID/midnight-proof-server:latest

# On the VM
gcloud compute ssh midnight-proof-server --zone=$ZONE
docker pull gcr.io/$PROJECT_ID/midnight-proof-server:latest
docker stop midnight-proof-server
docker rm midnight-proof-server
# Re-run docker run command from startup script
```

### Step 5: Test Attestation

```bash
# Generate nonce
NONCE=$(openssl rand -hex 32)

# Request attestation
curl "http://$EXTERNAL_IP:6300/attestation?nonce=$NONCE"
```

---

## Load Balancer Configuration

### Step 1: Create Health Check

```bash
gcloud compute health-checks create http midnight-proof-server-health \
  --port=6300 \
  --request-path=/health \
  --check-interval=30s \
  --timeout=10s \
  --unhealthy-threshold=3 \
  --healthy-threshold=2 \
  --project=$PROJECT_ID
```

### Step 2: Create Instance Group

```bash
# Create unmanaged instance group
gcloud compute instance-groups unmanaged create midnight-proof-server-ig \
  --zone=$ZONE \
  --project=$PROJECT_ID

# Add instance to group
gcloud compute instance-groups unmanaged add-instances midnight-proof-server-ig \
  --zone=$ZONE \
  --instances=midnight-proof-server \
  --project=$PROJECT_ID

# Set named port
gcloud compute instance-groups unmanaged set-named-ports midnight-proof-server-ig \
  --zone=$ZONE \
  --named-ports=http:6300 \
  --project=$PROJECT_ID
```

### Step 3: Create Backend Service

```bash
gcloud compute backend-services create midnight-proof-server-backend \
  --protocol=HTTP \
  --health-checks=midnight-proof-server-health \
  --global \
  --port-name=http \
  --timeout=600s \
  --enable-logging \
  --logging-sample-rate=1.0 \
  --project=$PROJECT_ID

# Add instance group as backend
gcloud compute backend-services add-backend midnight-proof-server-backend \
  --instance-group=midnight-proof-server-ig \
  --instance-group-zone=$ZONE \
  --balancing-mode=UTILIZATION \
  --max-utilization=0.8 \
  --capacity-scaler=1.0 \
  --global \
  --project=$PROJECT_ID
```

### Step 4: Create SSL Certificate

**Option A: Google-managed SSL certificate (recommended)**

```bash
# Create SSL certificate (requires domain)
gcloud compute ssl-certificates create midnight-proof-server-cert \
  --domains=proof.midnight.network \
  --global \
  --project=$PROJECT_ID

# Note: You must configure DNS first (see below)
```

**Option B: Self-signed certificate (testing only)**

```bash
# Generate self-signed certificate
openssl req -x509 -nodes -days 365 -newkey rsa:2048 \
  -keyout /tmp/selfsigned.key \
  -out /tmp/selfsigned.crt \
  -subj "/CN=midnight-proof-server"

# Create GCP SSL certificate from files
gcloud compute ssl-certificates create midnight-proof-server-cert \
  --certificate=/tmp/selfsigned.crt \
  --private-key=/tmp/selfsigned.key \
  --global \
  --project=$PROJECT_ID
```

### Step 5: Create URL Map and HTTPS Proxy

```bash
# Create URL map
gcloud compute url-maps create midnight-proof-server-lb \
  --default-service=midnight-proof-server-backend \
  --global \
  --project=$PROJECT_ID

# Create target HTTPS proxy
gcloud compute target-https-proxies create midnight-proof-server-https-proxy \
  --url-map=midnight-proof-server-lb \
  --ssl-certificates=midnight-proof-server-cert \
  --global \
  --project=$PROJECT_ID
```

### Step 6: Create Forwarding Rule

```bash
# Reserve static IP for load balancer
gcloud compute addresses create midnight-proof-server-lb-ip \
  --ip-version=IPV4 \
  --global \
  --project=$PROJECT_ID

# Get the IP
export LB_IP=$(gcloud compute addresses describe midnight-proof-server-lb-ip \
  --global \
  --format="get(address)")

echo "Load Balancer IP: $LB_IP"

# Create forwarding rule
gcloud compute forwarding-rules create midnight-proof-server-https-rule \
  --address=midnight-proof-server-lb-ip \
  --global \
  --target-https-proxy=midnight-proof-server-https-proxy \
  --ports=443 \
  --project=$PROJECT_ID
```

### Step 7: Configure DNS

Point your domain to the load balancer IP:

```bash
# Add DNS A record:
# proof.midnight.network -> $LB_IP

# If using Cloud DNS:
gcloud dns record-sets create proof.midnight.network. \
  --zone=midnight-zone \
  --type=A \
  --ttl=300 \
  --rrdatas=$LB_IP \
  --project=$PROJECT_ID
```

### Step 8: Test HTTPS Load Balancer

```bash
# Wait for certificate provisioning (5-15 minutes for managed certs)
gcloud compute ssl-certificates describe midnight-proof-server-cert \
  --global \
  --format="get(managed.status)"

# Test HTTPS endpoint
curl https://proof.midnight.network/health
curl https://proof.midnight.network/version
```

---

## Monitoring and Logging

### Step 1: Configure Cloud Logging

```bash
# Create log sink for errors
gcloud logging sinks create midnight-proof-server-errors \
  bigquery.googleapis.com/projects/$PROJECT_ID/datasets/proof_server_logs \
  --log-filter='resource.type="gce_instance" AND
                resource.labels.instance_id="midnight-proof-server" AND
                severity>=ERROR' \
  --project=$PROJECT_ID
```

### Step 2: Create Custom Metrics

Create a script to export custom metrics:

```bash
cat > /usr/local/bin/export-metrics.sh << 'EOF'
#!/bin/bash
set -e

PROJECT_ID=$(curl -s "http://metadata.google.internal/computeMetadata/v1/project/project-id" -H "Metadata-Flavor: Google")
INSTANCE_ID=$(curl -s "http://metadata.google.internal/computeMetadata/v1/instance/id" -H "Metadata-Flavor: Google")
ZONE=$(curl -s "http://metadata.google.internal/computeMetadata/v1/instance/zone" -H "Metadata-Flavor: Google" | cut -d/ -f4)

# Get metrics from proof server
METRICS=$(curl -s http://localhost:6300/ready)
QUEUE_SIZE=$(echo "$METRICS" | jq -r '.queue_size // 0')
ACTIVE_WORKERS=$(echo "$METRICS" | jq -r '.active_workers // 0')

# Send to Cloud Monitoring
gcloud monitoring time-series create --project=$PROJECT_ID << METRIC_EOF
{
  "timeSeries": [
    {
      "metric": {
        "type": "custom.googleapis.com/midnight/proof_server/queue_size",
        "labels": {
          "instance_id": "$INSTANCE_ID"
        }
      },
      "resource": {
        "type": "gce_instance",
        "labels": {
          "instance_id": "$INSTANCE_ID",
          "zone": "$ZONE",
          "project_id": "$PROJECT_ID"
        }
      },
      "points": [
        {
          "interval": {
            "endTime": "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
          },
          "value": {
            "int64Value": "$QUEUE_SIZE"
          }
        }
      ]
    },
    {
      "metric": {
        "type": "custom.googleapis.com/midnight/proof_server/active_workers",
        "labels": {
          "instance_id": "$INSTANCE_ID"
        }
      },
      "resource": {
        "type": "gce_instance",
        "labels": {
          "instance_id": "$INSTANCE_ID",
          "zone": "$ZONE",
          "project_id": "$PROJECT_ID"
        }
      },
      "points": [
        {
          "interval": {
            "endTime": "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
          },
          "value": {
            "int64Value": "$ACTIVE_WORKERS"
          }
        }
      ]
    }
  ]
}
METRIC_EOF
EOF

sudo chmod +x /usr/local/bin/export-metrics.sh

# Create cron job to run every minute
echo "* * * * * /usr/local/bin/export-metrics.sh >> /var/log/metrics-export.log 2>&1" | sudo crontab -
```

### Step 3: Create Alert Policies

```bash
# Alert on high error rate
gcloud alpha monitoring policies create \
  --notification-channels=CHANNEL_ID \
  --display-name="Proof Server High Error Rate" \
  --condition-display-name="Error rate > 5/min" \
  --condition-threshold-value=5 \
  --condition-threshold-duration=300s \
  --condition-filter='resource.type="gce_instance" AND
                      resource.labels.instance_id="midnight-proof-server" AND
                      severity>=ERROR' \
  --project=$PROJECT_ID

# Alert on high queue size
gcloud alpha monitoring policies create \
  --notification-channels=CHANNEL_ID \
  --display-name="Proof Server Queue Overload" \
  --condition-display-name="Queue size > 100" \
  --condition-threshold-value=100 \
  --condition-threshold-duration=300s \
  --condition-filter='metric.type="custom.googleapis.com/midnight/proof_server/queue_size"' \
  --project=$PROJECT_ID

# Alert on instance down
gcloud alpha monitoring policies create \
  --notification-channels=CHANNEL_ID \
  --display-name="Proof Server Instance Down" \
  --condition-display-name="Instance not running" \
  --condition-absent-duration=300s \
  --condition-filter='metric.type="compute.googleapis.com/instance/uptime" AND
                      resource.labels.instance_id="midnight-proof-server"' \
  --project=$PROJECT_ID
```

### Step 4: Create Dashboard

```bash
# Create monitoring dashboard JSON
cat > /tmp/dashboard.json << 'EOF'
{
  "displayName": "Midnight Proof Server",
  "mosaicLayout": {
    "columns": 12,
    "tiles": [
      {
        "width": 6,
        "height": 4,
        "widget": {
          "title": "Queue Size",
          "xyChart": {
            "dataSets": [{
              "timeSeriesQuery": {
                "timeSeriesFilter": {
                  "filter": "metric.type=\"custom.googleapis.com/midnight/proof_server/queue_size\""
                }
              }
            }]
          }
        }
      },
      {
        "width": 6,
        "height": 4,
        "widget": {
          "title": "Active Workers",
          "xyChart": {
            "dataSets": [{
              "timeSeriesQuery": {
                "timeSeriesFilter": {
                  "filter": "metric.type=\"custom.googleapis.com/midnight/proof_server/active_workers\""
                }
              }
            }]
          }
        }
      },
      {
        "width": 6,
        "height": 4,
        "yPos": 4,
        "widget": {
          "title": "CPU Utilization",
          "xyChart": {
            "dataSets": [{
              "timeSeriesQuery": {
                "timeSeriesFilter": {
                  "filter": "metric.type=\"compute.googleapis.com/instance/cpu/utilization\" AND resource.labels.instance_id=\"midnight-proof-server\""
                }
              }
            }]
          }
        }
      },
      {
        "width": 6,
        "height": 4,
        "xPos": 6,
        "yPos": 4,
        "widget": {
          "title": "Memory Usage",
          "xyChart": {
            "dataSets": [{
              "timeSeriesQuery": {
                "timeSeriesFilter": {
                  "filter": "metric.type=\"compute.googleapis.com/instance/memory/balloon/ram_used\" AND resource.labels.instance_id=\"midnight-proof-server\""
                }
              }
            }]
          }
        }
      }
    ]
  }
}
EOF

# Create dashboard
gcloud monitoring dashboards create --config-from-file=/tmp/dashboard.json --project=$PROJECT_ID
```

---

## Security Configuration

### Step 1: Enable OS Login

```bash
# Enable OS Login for the project
gcloud compute project-info add-metadata \
  --metadata enable-oslogin=TRUE \
  --project=$PROJECT_ID

# Grant IAM permissions
gcloud projects add-iam-policy-binding $PROJECT_ID \
  --member=user:admin@midnight.network \
  --role=roles/compute.osLogin

# SSH using OS Login
gcloud compute ssh midnight-proof-server \
  --zone=$ZONE \
  --tunnel-through-iap
```

### Step 2: Configure IAP for SSH

```bash
# Enable IAP API
gcloud services enable iap.googleapis.com

# Create firewall rule for IAP
gcloud compute firewall-rules create allow-ssh-from-iap \
  --network=midnight-vpc \
  --allow=tcp:22 \
  --source-ranges=35.235.240.0/20 \
  --target-tags=proof-server \
  --description="Allow SSH from Identity-Aware Proxy"

# Grant IAP permissions
gcloud projects add-iam-policy-binding $PROJECT_ID \
  --member=user:admin@midnight.network \
  --role=roles/iap.tunnelResourceAccessor

# SSH via IAP (no external IP needed)
gcloud compute ssh midnight-proof-server \
  --zone=$ZONE \
  --tunnel-through-iap
```

### Step 3: Enable Security Command Center (Optional)

```bash
# Enable Security Command Center
gcloud services enable securitycenter.googleapis.com

# Run security health analytics
gcloud scc findings list $PROJECT_ID \
  --filter="category=\"SECURITY_HEALTH_ANALYTICS\""
```

### Step 4: Configure Secrets Rotation

```bash
# Enable Secret Manager
gcloud services enable secretmanager.googleapis.com

# Add version with rotation
gcloud secrets versions add midnight-api-key \
  --data-file=- \
  --project=$PROJECT_ID

# Set up rotation reminder (manual process)
cat > /tmp/rotate-secrets.sh << 'EOF'
#!/bin/bash
# Run this monthly to rotate API keys
NEW_KEY=$(openssl rand -base64 32)
echo -n "$NEW_KEY" | gcloud secrets versions add midnight-api-key --data-file=-
echo "New API key created. Update clients within 7 days."
echo "Old key will be disabled on: $(date -d '+7 days' +%Y-%m-%d)"
EOF

chmod +x /tmp/rotate-secrets.sh
```

---

## PCR Publication

### Step 1: Extract Final PCR Values

```bash
# SSH into production VM
gcloud compute ssh midnight-proof-server --zone=$ZONE --tunnel-through-iap

# Run extraction script
sudo /usr/local/bin/extract-pcrs.sh /tmp/prod-pcr-values.json

# Download to local machine
gcloud compute scp midnight-proof-server:/tmp/prod-pcr-values.json \
  ./gcp-pcr-values.json \
  --zone=$ZONE \
  --tunnel-through-iap
```

### Step 2: Sign PCR Values

```bash
# On local machine with GPG key
gpg --detach-sign --armor gcp-pcr-values.json

# Verify signature
gpg --verify gcp-pcr-values.json.asc gcp-pcr-values.json
```

### Step 3: Publish to GitHub Release

```bash
# Create release with PCR values
gh release create v1.0.0-gcp \
  --title "Midnight Proof Server v1.0.0 - GCP Confidential VM" \
  --notes "Production PCR values for GCP Confidential VM deployment" \
  gcp-pcr-values.json \
  gcp-pcr-values.json.asc
```

### Step 4: Document Verification Process

Create `docs/VERIFY_GCP_ATTESTATION.md`:

```markdown
# Verifying GCP Confidential VM Attestation

## 1. Fetch Published PCR Values

Download from GitHub release:
- PCR values: gcp-pcr-values.json
- GPG signature: gcp-pcr-values.json.asc
- GPG public key: midnight-pgp-public.asc

## 2. Verify GPG Signature

\`\`\`bash
gpg --import midnight-pgp-public.asc
gpg --verify gcp-pcr-values.json.asc gcp-pcr-values.json
\`\`\`

## 3. Request Attestation from Server

\`\`\`bash
NONCE=$(openssl rand -hex 32)
curl "https://proof.midnight.network/attestation?nonce=$NONCE" > attestation.json
\`\`\`

## 4. Verify TPM Quote

See full verification code in `wallets/verify-tpm-quote.rs`
```

---

## Troubleshooting

### Common Issues

#### 1. VM Fails to Start

**Symptom**: VM status is "TERMINATED" immediately after creation

**Causes**:
- Confidential Computing not supported in zone
- Insufficient quota
- Maintenance policy not set to TERMINATE

**Solution**:
```bash
# Check zone support
gcloud compute zones describe $ZONE | grep -i confidential

# Check quota
gcloud compute project-info describe --project=$PROJECT_ID | grep -i quota

# Recreate with correct flags
gcloud compute instances create midnight-proof-server \
  --maintenance-policy=TERMINATE \
  --confidential-compute \
  ...
```

#### 2. Docker Container Not Starting

**Symptom**: `docker ps` shows container exited

**Diagnosis**:
```bash
# Check logs
docker logs midnight-proof-server

# Check startup script
sudo journalctl -u google-startup-scripts

# Common issues:
# - API key not found in Secret Manager
# - Docker image pull failed
# - Permission issues
```

**Solution**:
```bash
# Verify API key exists
gcloud secrets versions access latest --secret="midnight-api-key"

# Manually pull image
docker pull gcr.io/$PROJECT_ID/midnight-proof-server:latest

# Check service account permissions
gcloud projects get-iam-policy $PROJECT_ID \
  --flatten="bindings[].members" \
  --filter="bindings.members:serviceAccount:midnight-proof-server-sa*"
```

#### 3. Health Check Failing

**Symptom**: Load balancer shows backend as "UNHEALTHY"

**Diagnosis**:
```bash
# SSH into VM
gcloud compute ssh midnight-proof-server --zone=$ZONE

# Test health endpoint locally
curl http://localhost:6300/health

# Check firewall allows health check
gcloud compute firewall-rules describe midnight-allow-health-check

# Check Docker port binding
netstat -tulpn | grep 6300
```

**Solution**:
```bash
# Ensure firewall allows GCP health check IPs
gcloud compute firewall-rules update midnight-allow-health-check \
  --source-ranges=35.191.0.0/16,130.211.0.0/22

# Restart container with correct port
docker stop midnight-proof-server
docker run -d --name midnight-proof-server -p 6300:6300 ...
```

#### 4. TPM Attestation Failing

**Symptom**: `/attestation` endpoint returns errors

**Diagnosis**:
```bash
# Check vTPM is enabled
sudo tpm2_pcrread

# Check for TPM tools
which tpm2_quote

# Check script permissions
ls -la /usr/local/bin/generate-quote.sh
```

**Solution**:
```bash
# Install TPM tools
sudo apt-get install -y tpm2-tools

# Recreate VM with vTPM enabled
gcloud compute instances create midnight-proof-server \
  --shielded-vtpm \
  ...
```

#### 5. SSL Certificate Not Provisioning

**Symptom**: "PROVISIONING" status for > 15 minutes

**Diagnosis**:
```bash
# Check certificate status
gcloud compute ssl-certificates describe midnight-proof-server-cert \
  --global \
  --format="get(managed.status, managed.domainStatus)"

# Check DNS resolution
dig proof.midnight.network
```

**Solution**:
```bash
# Verify DNS points to load balancer IP
export LB_IP=$(gcloud compute addresses describe midnight-proof-server-lb-ip \
  --global --format="get(address)")

dig proof.midnight.network @8.8.8.8 | grep $LB_IP

# If DNS is correct, wait up to 60 minutes for provisioning
# Use self-signed cert temporarily for testing
```

### Debugging Commands

```bash
# Check VM serial console output
gcloud compute instances get-serial-port-output midnight-proof-server \
  --zone=$ZONE

# Check instance metadata
gcloud compute instances describe midnight-proof-server \
  --zone=$ZONE \
  --format=json

# Check confidential computing status
gcloud compute instances describe midnight-proof-server \
  --zone=$ZONE \
  --format="get(confidentialInstanceConfig)"

# View Cloud Logging
gcloud logging read "resource.type=gce_instance AND \
  resource.labels.instance_id=midnight-proof-server" \
  --limit=50 \
  --format=json

# Check backend health
gcloud compute backend-services get-health midnight-proof-server-backend \
  --global
```

---

## Maintenance

### Daily Tasks

```bash
# Check instance health
gcloud compute instances describe midnight-proof-server \
  --zone=$ZONE \
  --format="get(status)"

# Check container status
gcloud compute ssh midnight-proof-server --zone=$ZONE --command="docker ps"

# Check error logs
gcloud logging read "resource.type=gce_instance AND \
  resource.labels.instance_id=midnight-proof-server AND \
  severity>=ERROR" \
  --limit=10 \
  --format="table(timestamp, jsonPayload.message)"
```

### Weekly Tasks

```bash
# Check security updates
gcloud compute ssh midnight-proof-server --zone=$ZONE --command="\
  sudo apt-get update && \
  sudo apt-get --just-print upgrade | grep -i security"

# Review Cloud Monitoring alerts
gcloud alpha monitoring policies list --project=$PROJECT_ID

# Check disk usage
gcloud compute ssh midnight-proof-server --zone=$ZONE --command="df -h"
```

### Monthly Tasks

```bash
# Apply security updates (requires maintenance window)
gcloud compute ssh midnight-proof-server --zone=$ZONE --command="\
  sudo apt-get update && \
  sudo apt-get upgrade -y"

# Rotate API keys (see secrets rotation section)
/tmp/rotate-secrets.sh

# Review and optimize costs
gcloud billing accounts list
gcloud beta billing budgets list

# Backup configuration
gcloud compute instances describe midnight-proof-server \
  --zone=$ZONE \
  --format=json > backup-$(date +%Y%m%d).json
```

### Updating the Proof Server

```bash
# 1. Build new image
cd /Users/robertblessing-hartley/code/tee-prover-prototype/proof-server
docker build -t gcr.io/$PROJECT_ID/midnight-proof-server:v1.1.0 .
docker push gcr.io/$PROJECT_ID/midnight-proof-server:v1.1.0

# 2. SSH into VM
gcloud compute ssh midnight-proof-server --zone=$ZONE

# 3. Pull new image
docker pull gcr.io/$PROJECT_ID/midnight-proof-server:v1.1.0

# 4. Stop old container
docker stop midnight-proof-server
docker rm midnight-proof-server

# 5. Start new container
docker run -d \
  --name midnight-proof-server \
  --restart always \
  -p 6300:6300 \
  -e MIDNIGHT_PROOF_SERVER_API_KEY="$(gcloud secrets versions access latest --secret=midnight-api-key)" \
  gcr.io/$PROJECT_ID/midnight-proof-server:v1.1.0

# 6. Verify
docker logs midnight-proof-server
curl http://localhost:6300/health

# 7. Extract new PCR values (if code changed)
sudo /usr/local/bin/extract-pcrs.sh /tmp/pcr-v1.1.0.json
```

---

## Cost Optimization

### 1. Committed Use Discounts

```bash
# Purchase 1-year commitment (37% discount)
gcloud compute commitments create midnight-commitment-1y \
  --resources=vcpu=8,memory=32 \
  --plan=12-month \
  --region=$REGION

# Savings: $220/mo → $138/mo

# Purchase 3-year commitment (55% discount)
gcloud compute commitments create midnight-commitment-3y \
  --resources=vcpu=8,memory=32 \
  --plan=36-month \
  --region=$REGION

# Savings: $220/mo → $99/mo
```

### 2. Right-Sizing

```bash
# Start with smaller instance for testing
gcloud compute instances create midnight-proof-server \
  --machine-type=n2d-standard-4 \  # 4 vCPU, 16GB = $110/mo
  ...

# Monitor utilization
gcloud monitoring time-series list \
  --filter='metric.type="compute.googleapis.com/instance/cpu/utilization"'

# Resize if needed (requires VM stop)
gcloud compute instances stop midnight-proof-server --zone=$ZONE
gcloud compute instances set-machine-type midnight-proof-server \
  --machine-type=n2d-standard-8 \
  --zone=$ZONE
gcloud compute instances start midnight-proof-server --zone=$ZONE
```

### 3. Network Egress Optimization

```bash
# Use Cloud CDN for static content (not applicable for proof server)

# Use cheaper regions for low-latency requirements
# us-central1 (Iowa) is typically cheapest in US

# Monitor egress
gcloud monitoring time-series list \
  --filter='metric.type="compute.googleapis.com/instance/network/sent_bytes_count"'
```

### 4. Log Retention Optimization

```bash
# Reduce log retention (default is 30 days)
gcloud logging buckets update _Default \
  --location=global \
  --retention-days=7

# Savings: ~$3/mo → ~$0.70/mo
```

### 5. Remove Unused Resources

```bash
# List all resources
gcloud compute instances list
gcloud compute disks list
gcloud compute addresses list

# Delete test resources
gcloud compute instances delete test-instance --zone=$ZONE
gcloud compute disks delete test-disk --zone=$ZONE
gcloud compute addresses delete test-ip --region=$REGION
```

### Total Optimized Cost

| Resource | Standard | Optimized | Savings |
|----------|----------|-----------|---------|
| Compute (8 vCPU) | $220 | $99 (3yr CUD) | -55% |
| Compute (4 vCPU) | $110 | $50 (3yr CUD) | -55% |
| Disk | $17 | $17 | 0% |
| Network | $85 | $85 | 0% |
| Load Balancer | $18 | $18 | 0% |
| Monitoring | $8 | $8 | 0% |
| Logging | $3 | $1 | -67% |
| Other | $8 | $8 | 0% |
| **Total (8 vCPU)** | **$359** | **$236** | **-34%** |
| **Total (4 vCPU)** | **$249** | **$186** | **-25%** |

---

## Summary

### What We Deployed

- ✅ Confidential VM with AMD SEV-SNP memory encryption
- ✅ Docker-based proof server deployment
- ✅ TPM 2.0 attestation with PCR measurements
- ✅ HTTPS load balancer with SSL/TLS
- ✅ Cloud Monitoring and Logging
- ✅ Security hardening (IAP, OS Login, Shielded VM)
- ✅ Production-ready configuration

### Key GCP Resources

| Resource Type | Resource Name | Purpose |
|--------------|---------------|---------|
| Compute Instance | `midnight-proof-server` | Confidential VM |
| Instance Group | `midnight-proof-server-ig` | Load balancer backend |
| Health Check | `midnight-proof-server-health` | Backend health monitoring |
| Backend Service | `midnight-proof-server-backend` | LB backend config |
| SSL Certificate | `midnight-proof-server-cert` | HTTPS encryption |
| URL Map | `midnight-proof-server-lb` | LB routing |
| Forwarding Rule | `midnight-proof-server-https-rule` | HTTPS entry point |
| Static IP | `midnight-proof-server-lb-ip` | LB external IP |
| VPC Network | `midnight-vpc` | Network isolation |
| Firewall Rules | `midnight-allow-*` | Traffic control |
| Service Account | `midnight-proof-server-sa` | VM identity |
| Secret | `midnight-api-key` | API authentication |

### Access Information

```bash
# SSH Access
gcloud compute ssh midnight-proof-server \
  --zone=$ZONE \
  --tunnel-through-iap

# HTTPS Endpoint
https://proof.midnight.network

# Monitoring Dashboard
https://console.cloud.google.com/monitoring/dashboards

# Logs
https://console.cloud.google.com/logs
```

### Next Steps

1. **Configure DNS**: Point your domain to load balancer IP
2. **Test Attestation**: Verify TPM quotes from wallets
3. **Set Up Alerts**: Configure notification channels
4. **Document PCRs**: Publish PCR values to GitHub
5. **Plan Scaling**: Set up managed instance group for auto-scaling
6. **Implement Monitoring**: Add custom metrics export
7. **Schedule Maintenance**: Plan monthly update windows



For questions or issues, see [troubleshooting.md](troubleshooting.md) or open a GitHub issue.
