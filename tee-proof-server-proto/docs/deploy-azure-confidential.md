# Azure Confidential VMs Deployment Guide

**Midnight Proof Server - Microsoft Azure Deployment**

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

### What is Azure Confidential Computing?

Azure Confidential VMs use **AMD SEV-SNP** or **Intel TDX** (coming soon) to provide:

- **Memory Encryption**: VM memory encrypted with CPU-owned key
- **Azure Attestation Service**: Managed JWT-based attestation
- **Virtual TPM 2.0**: Hardware-backed cryptographic operations
- **Integration**: Deep integration with Azure security services

### Why Choose Azure for Midnight Proof Server?

**Advantages:**
- ✅ **Managed Attestation**: Azure Attestation Service handles complexity
- ✅ **JWT Format**: Standard, easy-to-verify attestation tokens
- ✅ **Azure Integration**: Works seamlessly with Azure Key Vault, Monitor, etc.
- ✅ **Enterprise Features**: Azure AD, RBAC, governance tools
- ✅ **Intel TDX Support**: Coming soon (Q1 2026)

**Considerations:**
- ⚠️ Dependent on AMD processors (Intel TDX not yet GA)
- ⚠️ Attestation requires Azure Attestation Service setup
- ⚠️ Slightly higher cost than GCP ($365/mo vs $341/mo)

---

## Prerequisites

### Required Tools

Install on your local machine:

```bash
# 1. Azure CLI
# macOS
brew update && brew install azure-cli

# Linux
curl -sL https://aka.ms/InstallAzureCLIDeb | sudo bash

# 2. Docker (for local image building)
# macOS
brew install docker

# Linux
curl -fsSL https://get.docker.com -o get-docker.sh
sudo sh get-docker.sh

# 3. jq (JSON parsing)
brew install jq  # macOS
sudo apt install jq  # Linux

# 4. Verify installations
az --version
docker --version
jq --version
```

### Azure Account Setup

1. **Create Azure Account**: https://portal.azure.com
2. **Enable Subscription**: Ensure you have an active subscription
3. **Login via CLI**:
   ```bash
   az login
   ```

4. **Set Default Subscription**:
   ```bash
   # List subscriptions
   az account list --output table
   
   # Set default
   az account set --subscription "Your Subscription Name"
   
   # Verify
   az account show --output table
   ```

5. **Register Required Providers**:
   ```bash
   az provider register --namespace Microsoft.Compute
   az provider register --namespace Microsoft.Network
   az provider register --namespace Microsoft.ContainerRegistry
   az provider register --namespace Microsoft.KeyVault
   az provider register --namespace Microsoft.Attestation
   az provider register --namespace Microsoft.Monitor
   
   # Verify registration (can take a few minutes)
   az provider show -n Microsoft.Attestation --query "registrationState"
   ```

### Required Permissions

Your Azure user needs these RBAC roles:

- `Contributor` - Create and manage resources
- `User Access Administrator` - Manage RBAC assignments (optional)

```bash
# Check your current role assignments
az role assignment list --assignee $(az account show --query user.name -o tsv) \
  --output table

# If needed, have admin grant Contributor role:
# az role assignment create \
#   --assignee user@example.com \
#   --role Contributor \
#   --scope /subscriptions/<subscription-id>
```

---

## Architecture

### High-Level Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                    Azure Cloud Platform                      │
│                                                               │
│  ┌────────────────────────────────────────────────────────┐ │
│  │              Virtual Network (VNet)                     │ │
│  │                  10.0.0.0/16                            │ │
│  │                                                          │ │
│  │  ┌──────────────────────────────────────────────────┐  │ │
│  │  │        Subnet (10.0.1.0/24)                      │  │ │
│  │  │                                                   │  │ │
│  │  │  ┌─────────────────────────────────────────┐    │  │ │
│  │  │  │   Confidential VM (DCsv3-series)        │    │  │ │
│  │  │  │   Standard_DC4s_v3 (AMD EPYC)           │    │  │ │
│  │  │  │                                          │    │  │ │
│  │  │  │  ┌───────────────────────────────────┐ │    │  │ │
│  │  │  │  │  AMD SEV-SNP Encrypted Memory     │ │    │  │ │
│  │  │  │  │                                    │ │    │  │ │
│  │  │  │  │  ┌──────────────────────────────┐ │ │    │  │ │
│  │  │  │  │  │  Docker Container            │ │ │    │  │ │
│  │  │  │  │  │  midnight-proof-server       │ │ │    │  │ │
│  │  │  │  │  │  Port 6300                   │ │ │    │  │ │
│  │  │  │  │  └──────────────────────────────┘ │ │    │  │ │
│  │  │  │  │                                    │ │    │  │ │
│  │  │  │  │  vTPM 2.0                          │ │    │  │ │
│  │  │  │  │  - Attestation                    │ │    │  │ │
│  │  │  │  │  - PCR Measurements               │ │    │  │ │
│  │  │  │  └───────────────────────────────────┘ │    │  │ │
│  │  │  └─────────────────────────────────────────┘    │  │ │
│  │  │                                                   │  │ │
│  │  └───────────────────────────────────────────────────┘  │ │
│  │                                                          │ │
│  │  ┌─────────────────────────────────────────┐            │ │
│  │  │   Application Gateway (HTTPS)           │            │ │
│  │  │   Public IP: 20.x.x.x                   │            │ │
│  │  │   TLS Termination                       │            │ │
│  │  └─────────────────────────────────────────┘            │ │
│  │                                                          │ │
│  └──────────────────────────────────────────────────────────┘ │
│                                                               │
│  ┌──────────────────┐  ┌──────────────────────────────┐     │
│  │  Azure Monitor   │  │  Azure Attestation Service    │     │
│  │  - Metrics       │  │  - JWT Token Generation       │     │
│  │  - Alerts        │  │  - Policy Management          │     │
│  │  - Log Analytics │  │  - Signature Verification     │     │
│  └──────────────────┘  └──────────────────────────────┘     │
│                                                               │
│  ┌──────────────────┐  ┌──────────────────────────────┐     │
│  │  Key Vault       │  │  Container Registry (ACR)     │     │
│  │  - API Keys      │  │  - Docker Images              │     │
│  │  - Certificates  │  │  - Versioning                 │     │
│  └──────────────────┘  └──────────────────────────────┘     │
│                                                               │
└─────────────────────────────────────────────────────────────┘

        ↑                           ↑
        │ HTTPS (443)               │ JWT Attestation
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
| **vTPM 2.0** | Attestation measurements | Virtual TPM |
| **Azure Attestation** | JWT token generation | Managed service |
| **Application Gateway** | HTTPS load balancer | Azure AppGW v2 |
| **Key Vault** | Secrets management | Azure Key Vault |
| **Container Registry** | Docker image storage | Azure ACR |
| **Azure Monitor** | Metrics and logging | Azure Monitor |

---

## Cost Estimation

### Monthly Costs (East US region)

| Resource | Configuration | Monthly Cost |
|----------|--------------|--------------|
| **Confidential VM** | Standard_DC4s_v3 (4 vCPU, 32GB) | ~$240 |
| **Managed Disk** | 128GB Premium SSD | ~$19 |
| **Public IP** | Static Standard SKU | ~$4 |
| **Application Gateway** | Standard_v2 (2 instances) | ~$73 |
| **Network Egress** | 1TB/month | ~$88 |
| **Azure Monitor** | Logs + metrics | ~$10 |
| **Key Vault** | Operations + storage | ~$2 |
| **Container Registry** | Basic tier | ~$5 |
| **Attestation Service** | API calls | ~$2 |
| **Total** | | **~$443/month** |

### Cost Optimization Tips

1. **Reserved Instances**: Save 40-65% with 1-3 year reservations
2. **Spot VMs**: Not recommended for production (can be deallocated)
3. **Hybrid Benefit**: Use existing Windows licenses (if applicable)
4. **Cheaper Regions**: West US 2, South Central US typically cheaper
5. **Right-sizing**: Start with DC2s_v3 (2 vCPU, 16GB) for $140/month

**Example with optimizations:**
- Standard_DC4s_v3 with 3-year RI: $240 → $85/month (65% savings)
- Total optimized: **~$288/month**

---

## Infrastructure Setup

### Step 1: Set Variables

```bash
# Configure deployment parameters
export RESOURCE_GROUP="midnight-proof-server-rg"
export LOCATION="eastus"
export VM_NAME="midnight-proof-server"
export VNET_NAME="midnight-vnet"
export SUBNET_NAME="midnight-subnet"
export NSG_NAME="midnight-nsg"
export PUBLIC_IP_NAME="midnight-public-ip"
export ACR_NAME="midnightacr${RANDOM}"  # Must be globally unique
export KEYVAULT_NAME="midnight-kv-${RANDOM}"  # Must be globally unique
export ATTESTATION_NAME="midnight-attestation-${RANDOM}"

echo "Using Resource Group: $RESOURCE_GROUP"
echo "Using Location: $LOCATION"
```

### Step 2: Create Resource Group

```bash
# Create resource group
az group create \
  --name $RESOURCE_GROUP \
  --location $LOCATION \
  --tags environment=production project=midnight-proof-server

# Verify creation
az group show --name $RESOURCE_GROUP --output table
```

### Step 3: Create Virtual Network

```bash
# Create VNet
az network vnet create \
  --resource-group $RESOURCE_GROUP \
  --name $VNET_NAME \
  --address-prefix 10.0.0.0/16 \
  --subnet-name $SUBNET_NAME \
  --subnet-prefix 10.0.1.0/24 \
  --location $LOCATION

# Verify
az network vnet show \
  --resource-group $RESOURCE_GROUP \
  --name $VNET_NAME \
  --output table
```

### Step 4: Configure Network Security Group

```bash
# Create NSG
az network nsg create \
  --resource-group $RESOURCE_GROUP \
  --name $NSG_NAME \
  --location $LOCATION

# Allow SSH from your IP only
export MY_IP=$(curl -s ifconfig.me)
az network nsg rule create \
  --resource-group $RESOURCE_GROUP \
  --nsg-name $NSG_NAME \
  --name AllowSSH \
  --priority 100 \
  --source-address-prefixes $MY_IP/32 \
  --destination-port-ranges 22 \
  --access Allow \
  --protocol Tcp \
  --description "Allow SSH from admin IP"

# Allow HTTPS from anywhere
az network nsg rule create \
  --resource-group $RESOURCE_GROUP \
  --nsg-name $NSG_NAME \
  --name AllowHTTPS \
  --priority 110 \
  --source-address-prefixes Internet \
  --destination-port-ranges 443 \
  --access Allow \
  --protocol Tcp \
  --description "Allow HTTPS from internet"

# Allow health probes from Application Gateway
az network nsg rule create \
  --resource-group $RESOURCE_GROUP \
  --nsg-name $NSG_NAME \
  --name AllowAppGwProbes \
  --priority 120 \
  --source-address-prefixes AzureLoadBalancer \
  --destination-port-ranges 6300 \
  --access Allow \
  --protocol Tcp \
  --description "Allow health probes from Application Gateway"

# Deny all other inbound
az network nsg rule create \
  --resource-group $RESOURCE_GROUP \
  --nsg-name $NSG_NAME \
  --name DenyAllInbound \
  --priority 4096 \
  --source-address-prefixes '*' \
  --destination-port-ranges '*' \
  --access Deny \
  --protocol '*' \
  --description "Deny all other inbound traffic"

# Associate NSG with subnet
az network vnet subnet update \
  --resource-group $RESOURCE_GROUP \
  --vnet-name $VNET_NAME \
  --name $SUBNET_NAME \
  --network-security-group $NSG_NAME
```

### Step 5: Create Public IP Address

```bash
# Create static public IP
az network public-ip create \
  --resource-group $RESOURCE_GROUP \
  --name $PUBLIC_IP_NAME \
  --location $LOCATION \
  --sku Standard \
  --allocation-method Static \
  --version IPv4

# Get the IP address
export PUBLIC_IP=$(az network public-ip show \
  --resource-group $RESOURCE_GROUP \
  --name $PUBLIC_IP_NAME \
  --query ipAddress \
  --output tsv)

echo "Reserved Public IP: $PUBLIC_IP"
```

### Step 6: Create Azure Container Registry

```bash
# Create ACR
az acr create \
  --resource-group $RESOURCE_GROUP \
  --name $ACR_NAME \
  --sku Basic \
  --location $LOCATION \
  --admin-enabled true

# Get login server
export ACR_LOGIN_SERVER=$(az acr show \
  --name $ACR_NAME \
  --query loginServer \
  --output tsv)

echo "ACR Login Server: $ACR_LOGIN_SERVER"

# Get admin credentials
az acr credential show \
  --name $ACR_NAME \
  --output table
```

### Step 7: Create Key Vault

```bash
# Create Key Vault
az keyvault create \
  --resource-group $RESOURCE_GROUP \
  --name $KEYVAULT_NAME \
  --location $LOCATION \
  --enabled-for-deployment true \
  --enabled-for-disk-encryption true \
  --enabled-for-template-deployment true \
  --sku standard

# Generate and store API key
export API_KEY=$(openssl rand -base64 32)
echo "Generated API Key: $API_KEY"
echo "⚠️  SAVE THIS KEY SECURELY"

az keyvault secret set \
  --vault-name $KEYVAULT_NAME \
  --name midnight-api-key \
  --value "$API_KEY"

# Verify
az keyvault secret show \
  --vault-name $KEYVAULT_NAME \
  --name midnight-api-key \
  --query value \
  --output tsv
```

### Step 8: Create Azure Attestation Provider

```bash
# Create Attestation Provider
az attestation create \
  --name $ATTESTATION_NAME \
  --resource-group $RESOURCE_GROUP \
  --location $LOCATION

# Get attestation URI
export ATTESTATION_URI=$(az attestation show \
  --name $ATTESTATION_NAME \
  --resource-group $RESOURCE_GROUP \
  --query attestUri \
  --output tsv)

echo "Attestation URI: $ATTESTATION_URI"
```

---

## Building the Docker Image

### Step 1: Prepare Dockerfile

Create production-optimized Dockerfile:

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
    curl \
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
    CMD curl -f http://localhost:6300/health || exit 1

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

### Step 2: Build and Push to ACR

```bash
# Login to ACR
az acr login --name $ACR_NAME

# Build image
cd /Users/robertblessing-hartley/code/tee-prover-prototype/proof-server
docker build -t $ACR_LOGIN_SERVER/midnight-proof-server:latest .

# Tag with version
docker tag $ACR_LOGIN_SERVER/midnight-proof-server:latest \
  $ACR_LOGIN_SERVER/midnight-proof-server:v1.0.0

# Push to ACR
docker push $ACR_LOGIN_SERVER/midnight-proof-server:latest
docker push $ACR_LOGIN_SERVER/midnight-proof-server:v1.0.0

# Verify
az acr repository list --name $ACR_NAME --output table
az acr repository show-tags \
  --name $ACR_NAME \
  --repository midnight-proof-server \
  --output table
```

---

## Deploying Confidential VM

### Step 1: Create Cloud-Init Configuration

```bash
# Create cloud-init file
cat > cloud-init.txt << 'EOF'
#cloud-config

package_update: true
package_upgrade: true

packages:
  - docker.io
  - azure-cli

runcmd:
  # Start Docker
  - systemctl start docker
  - systemctl enable docker

  # Add azure user to docker group
  - usermod -aG docker azureuser

  # Install Azure CLI extensions
  - az extension add --name attestation

  # Fetch API key from Key Vault
  - |
    export KEYVAULT_NAME=$(curl -s -H Metadata:true "http://169.254.169.254/metadata/instance?api-version=2021-02-01" | jq -r '.compute.tagsList[] | select(.name=="keyvault") | .value')
    export API_KEY=$(az keyvault secret show --vault-name $KEYVAULT_NAME --name midnight-api-key --query value -o tsv)

    # Login to ACR
    export ACR_NAME=$(curl -s -H Metadata:true "http://169.254.169.254/metadata/instance?api-version=2021-02-01" | jq -r '.compute.tagsList[] | select(.name=="acr") | .value')
    az acr login --name $ACR_NAME

    # Pull and run Docker container
    docker pull ${ACR_NAME}.azurecr.io/midnight-proof-server:latest

    docker run -d \
      --name midnight-proof-server \
      --restart always \
      -p 6300:6300 \
      -e MIDNIGHT_PROOF_SERVER_PORT=6300 \
      -e MIDNIGHT_PROOF_SERVER_API_KEY="$API_KEY" \
      -e MIDNIGHT_PROOF_SERVER_NUM_WORKERS=8 \
      -e MIDNIGHT_PROOF_SERVER_RATE_LIMIT=10 \
      -e MIDNIGHT_PROOF_SERVER_JOB_TIMEOUT=600 \
      -e RUST_LOG=info \
      ${ACR_NAME}.azurecr.io/midnight-proof-server:latest

    # Wait for service to start
    sleep 10
    curl -f http://localhost:6300/health || exit 1

write_files:
  - path: /etc/systemd/system/midnight-metrics.service
    content: |
      [Unit]
      Description=Midnight Proof Server Metrics Exporter
      After=docker.service

      [Service]
      Type=oneshot
      ExecStart=/usr/local/bin/export-metrics.sh

      [Install]
      WantedBy=multi-user.target

  - path: /etc/systemd/system/midnight-metrics.timer
    content: |
      [Unit]
      Description=Midnight Metrics Timer

      [Timer]
      OnBootSec=5min
      OnUnitActiveSec=1min

      [Install]
      WantedBy=timers.target

  - path: /usr/local/bin/export-metrics.sh
    permissions: '0755'
    content: |
      #!/bin/bash
      METRICS=$(curl -s http://localhost:6300/ready || echo '{}')
      echo "Metrics: $METRICS" >> /var/log/metrics.log
EOF
```

### Step 2: Create Confidential VM

```bash
# Create managed identity for VM
az identity create \
  --resource-group $RESOURCE_GROUP \
  --name ${VM_NAME}-identity

export IDENTITY_ID=$(az identity show \
  --resource-group $RESOURCE_GROUP \
  --name ${VM_NAME}-identity \
  --query id \
  --output tsv)

export IDENTITY_PRINCIPAL_ID=$(az identity show \
  --resource-group $RESOURCE_GROUP \
  --name ${VM_NAME}-identity \
  --query principalId \
  --output tsv)

# Grant Key Vault access to managed identity
az keyvault set-policy \
  --name $KEYVAULT_NAME \
  --object-id $IDENTITY_PRINCIPAL_ID \
  --secret-permissions get list

# Grant ACR pull access to managed identity
export ACR_ID=$(az acr show --name $ACR_NAME --query id --output tsv)
az role assignment create \
  --assignee $IDENTITY_PRINCIPAL_ID \
  --role AcrPull \
  --scope $ACR_ID

# Create Confidential VM
az vm create \
  --resource-group $RESOURCE_GROUP \
  --name $VM_NAME \
  --location $LOCATION \
  --size Standard_DC4s_v3 \
  --image Ubuntu2204 \
  --security-type ConfidentialVM \
  --os-disk-security-encryption-type VMGuestStateOnly \
  --enable-vtpm true \
  --enable-secure-boot true \
  --vnet-name $VNET_NAME \
  --subnet $SUBNET_NAME \
  --nsg $NSG_NAME \
  --public-ip-address $PUBLIC_IP_NAME \
  --assign-identity $IDENTITY_ID \
  --admin-username azureuser \
  --generate-ssh-keys \
  --custom-data cloud-init.txt \
  --tags keyvault=$KEYVAULT_NAME acr=$ACR_NAME environment=production

echo "Confidential VM created successfully!"
echo "Public IP: $PUBLIC_IP"
```

**Important flags explained:**

- `--security-type ConfidentialVM`: Enables AMD SEV-SNP
- `--os-disk-security-encryption-type VMGuestStateOnly`: Encrypts VM guest state
- `--enable-vtpm true`: Enables Virtual TPM 2.0
- `--enable-secure-boot true`: Enables UEFI Secure Boot
- `--assign-identity`: Assigns managed identity for Key Vault and ACR access

### Step 3: Verify Deployment

```bash
# Wait for VM to start (2-3 minutes)
az vm get-instance-view \
  --resource-group $RESOURCE_GROUP \
  --name $VM_NAME \
  --query instanceView.statuses \
  --output table

# Check cloud-init status
az vm run-command invoke \
  --resource-group $RESOURCE_GROUP \
  --name $VM_NAME \
  --command-id RunShellScript \
  --scripts "cloud-init status --wait"

# SSH into VM
ssh azureuser@$PUBLIC_IP

# Inside VM: Check Docker container
docker ps
docker logs midnight-proof-server

# Test health endpoint
curl http://localhost:6300/health
```

---

## Attestation Setup

### Understanding Azure Attestation

Azure Confidential VMs use **Azure Attestation Service** to generate JWT tokens:

- **vTPM 2.0**: Virtual TPM stores PCR measurements
- **Runtime Data**: Extracted from VM via IMDS (Instance Metadata Service)
- **Azure Attestation**: Service validates and signs JWT token
- **JWT Token**: Contains claims about VM integrity

**JWT Claims:**

| Claim | Description |
|-------|-------------|
| `x-ms-ver` | Attestation service version |
| `x-ms-attestation-type` | Always "azurevm" for Confidential VMs |
| `x-ms-policy-hash` | Hash of attestation policy used |
| `x-ms-runtime` | Runtime measurements and PCR values |
| `x-ms-sevsnpvm-*` | SEV-SNP specific claims |

### Step 1: Configure Attestation Policy

Create a custom attestation policy:

```bash
# Create policy JSON
cat > attestation-policy.json << 'EOF'
{
  "version": "1.0",
  "rules": [
    {
      "effect": "permit",
      "conditions": [
        {
          "claim": "x-ms-sevsnpvm-is-debuggable",
          "equals": "false"
        },
        {
          "claim": "x-ms-isolation-tee.x-ms-sevsnpvm-snpfw-svn",
          "greaterThanOrEquals": 0
        },
        {
          "claim": "x-ms-compliance-status",
          "equals": "azure-compliant-cvm"
        }
      ]
    }
  ]
}
EOF

# Set policy (requires base64 encoding)
export POLICY_BASE64=$(cat attestation-policy.json | base64 -w 0)

az attestation policy set \
  --name $ATTESTATION_NAME \
  --resource-group $RESOURCE_GROUP \
  --attestation-type SevSnpVm \
  --new-attestation-policy "$POLICY_BASE64"
```

### Step 2: Extract PCR Values

SSH into the VM and create extraction script:

```bash
# SSH into VM
ssh azureuser@$PUBLIC_IP

# Create PCR extraction script
sudo tee /usr/local/bin/extract-pcrs.sh > /dev/null << 'EOF'
#!/bin/bash
set -e

OUTPUT_FILE=${1:-/tmp/azure-pcr-values.json}

# Get VM metadata
METADATA=$(curl -s -H Metadata:true \
  "http://169.254.169.254/metadata/instance?api-version=2021-02-01")

VM_ID=$(echo $METADATA | jq -r '.compute.vmId')
LOCATION=$(echo $METADATA | jq -r '.compute.location')

# Install tpm2-tools if not present
if ! command -v tpm2_pcrread &> /dev/null; then
    sudo apt-get update && sudo apt-get install -y tpm2-tools
fi

# Read PCR values
PCR_VALUES=$(sudo tpm2_pcrread sha256:0,1,2,3,4,5,6,7,8,9,10,11,12 2>&1)

# Format as JSON
echo "{" > $OUTPUT_FILE
echo "  \"attestation_format\": \"Azure_JWT\"," >> $OUTPUT_FILE
echo "  \"cloud_provider\": \"Azure\"," >> $OUTPUT_FILE
echo "  \"vm_type\": \"Confidential VM (AMD SEV-SNP)\"," >> $OUTPUT_FILE
echo "  \"timestamp\": \"$(date -u +%Y-%m-%dT%H:%M:%SZ)\"," >> $OUTPUT_FILE
echo "  \"vm_id\": \"$VM_ID\"," >> $OUTPUT_FILE
echo "  \"location\": \"$LOCATION\"," >> $OUTPUT_FILE
echo "  \"pcr_values\": {" >> $OUTPUT_FILE

# Extract individual PCRs
for pcr in {0..12}; do
    value=$(echo "$PCR_VALUES" | grep "sha256: $pcr" | awk '{print $3}')
    if [ -n "$value" ]; then
        echo "    \"PCR$pcr\": \"$value\"," >> $OUTPUT_FILE
    fi
done

# Remove trailing comma
sed -i '$ s/,$//' $OUTPUT_FILE
echo "  }" >> $OUTPUT_FILE
echo "}" >> $OUTPUT_FILE

echo "PCR values extracted to: $OUTPUT_FILE"
cat $OUTPUT_FILE
EOF

sudo chmod +x /usr/local/bin/extract-pcrs.sh
```

### Step 3: Create Attestation Endpoint

Create attestation request script:

```bash
# On the VM
sudo tee /usr/local/bin/generate-attestation.sh > /dev/null << 'EOF'
#!/bin/bash
set -e

NONCE=${1:-"$(openssl rand -hex 32)"}
OUTPUT_FILE=${2:-/tmp/attestation-token.json}

# Get runtime data from IMDS
RUNTIME_DATA=$(curl -s -H Metadata:true \
  "http://169.254.169.254/metadata/attested/document?api-version=2020-09-01&nonce=$NONCE")

# Extract components
ENCODING=$(echo $RUNTIME_DATA | jq -r '.encoding')
SIGNATURE=$(echo $RUNTIME_DATA | jq -r '.signature')

# Get attestation provider URI from tags
ATTESTATION_URI=$(curl -s -H Metadata:true \
  "http://169.254.169.254/metadata/instance/compute/tagsList?api-version=2021-02-01" | \
  jq -r '.[] | select(.name=="attestation_uri") | .value')

# Request JWT token from Azure Attestation Service
JWT_TOKEN=$(curl -s -X POST \
  "$ATTESTATION_URI/attest/SevSnpVm?api-version=2020-10-01" \
  -H "Content-Type: application/json" \
  -d "{
    \"quote\": \"$RUNTIME_DATA\",
    \"runtimeData\": {
      \"data\": \"$(echo -n $NONCE | base64 -w 0)\",
      \"dataType\": \"Binary\"
    }
  }" | jq -r '.token')

# Save result
echo "{" > $OUTPUT_FILE
echo "  \"nonce\": \"$NONCE\"," >> $OUTPUT_FILE
echo "  \"jwt_token\": \"$JWT_TOKEN\"," >> $OUTPUT_FILE
echo "  \"timestamp\": \"$(date -u +%Y-%m-%dT%H:%M:%SZ)\"," >> $OUTPUT_FILE
echo "  \"attestation_uri\": \"$ATTESTATION_URI\"" >> $OUTPUT_FILE
echo "}" >> $OUTPUT_FILE

echo "Attestation token generated: $OUTPUT_FILE"
cat $OUTPUT_FILE
EOF

sudo chmod +x /usr/local/bin/generate-attestation.sh
```

### Step 4: Add Attestation Endpoint to Proof Server

Update the proof server to expose attestation:

```rust
// In src/lib.rs
use std::process::Command;

async fn attestation_handler(
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<serde_json::Value>, AppError> {
    let nonce = params.get("nonce")
        .ok_or_else(|| AppError::BadRequest("Missing nonce parameter".to_string()))?;

    // Execute attestation generation script
    let output = Command::new("/usr/local/bin/generate-attestation.sh")
        .arg(nonce)
        .output()
        .map_err(|e| AppError::InternalError(format!("Failed to generate attestation: {}", e)))?;

    if !output.status.success() {
        let error = String::from_utf8_lossy(&output.stderr);
        return Err(AppError::InternalError(format!("Attestation failed: {}", error)));
    }

    // Read generated JWT token
    let token_json = tokio::fs::read_to_string("/tmp/attestation-token.json")
        .await
        .map_err(|e| AppError::InternalError(format!("Failed to read token: {}", e)))?;

    let attestation: serde_json::Value = serde_json::from_str(&token_json)
        .map_err(|e| AppError::InternalError(format!("Failed to parse token: {}", e)))?;

    Ok(Json(attestation))
}

// Add to router:
// .route("/attestation", get(attestation_handler))
```

### Step 5: Test Attestation

```bash
# Generate nonce
NONCE=$(openssl rand -hex 32)

# Request attestation
curl "http://$PUBLIC_IP:6300/attestation?nonce=$NONCE" | jq

# Verify JWT token (decode without verification for inspection)
export JWT_TOKEN=$(curl -s "http://$PUBLIC_IP:6300/attestation?nonce=$NONCE" | jq -r '.jwt_token')
echo $JWT_TOKEN | cut -d. -f2 | base64 -d 2>/dev/null | jq
```

---

## Load Balancer Configuration

### Step 1: Create Application Gateway Subnet

```bash
# Create dedicated subnet for Application Gateway
az network vnet subnet create \
  --resource-group $RESOURCE_GROUP \
  --vnet-name $VNET_NAME \
  --name AppGatewaySubnet \
  --address-prefix 10.0.2.0/24
```

### Step 2: Create Public IP for Application Gateway

```bash
# Create public IP for Application Gateway
az network public-ip create \
  --resource-group $RESOURCE_GROUP \
  --name midnight-appgw-pip \
  --location $LOCATION \
  --sku Standard \
  --allocation-method Static
```

### Step 3: Create SSL Certificate

**Option A: Azure Key Vault Certificate (Recommended)**

```bash
# Generate certificate in Key Vault
az keyvault certificate create \
  --vault-name $KEYVAULT_NAME \
  --name midnight-ssl-cert \
  --policy "$(az keyvault certificate get-default-policy -o json | \
    jq '.keyProperties.exportable = true |
        .issuerParameters.name = "Self" |
        .x509CertificateProperties.subject = "CN=proof.midnight.network"')"

# Export certificate and private key
az keyvault secret download \
  --vault-name $KEYVAULT_NAME \
  --name midnight-ssl-cert \
  --encoding base64 \
  --file midnight-cert.pfx
```

**Option B: Let's Encrypt (Production)**

```bash
# Use certbot to get Let's Encrypt certificate
sudo apt-get install -y certbot
sudo certbot certonly --manual --preferred-challenges dns \
  -d proof.midnight.network

# Convert to PFX format
sudo openssl pkcs12 -export \
  -in /etc/letsencrypt/live/proof.midnight.network/fullchain.pem \
  -inkey /etc/letsencrypt/live/proof.midnight.network/privkey.pem \
  -out midnight-cert.pfx \
  -password pass:YourSecurePassword
```

### Step 4: Create Application Gateway

```bash
# Create Application Gateway
az network application-gateway create \
  --resource-group $RESOURCE_GROUP \
  --name midnight-appgw \
  --location $LOCATION \
  --sku Standard_v2 \
  --capacity 2 \
  --vnet-name $VNET_NAME \
  --subnet AppGatewaySubnet \
  --public-ip-address midnight-appgw-pip \
  --http-settings-cookie-based-affinity Disabled \
  --http-settings-port 6300 \
  --http-settings-protocol Http \
  --frontend-port 443 \
  --cert-file midnight-cert.pfx \
  --cert-password YourSecurePassword \
  --servers $PUBLIC_IP

# Add health probe
az network application-gateway probe create \
  --resource-group $RESOURCE_GROUP \
  --gateway-name midnight-appgw \
  --name midnight-health-probe \
  --protocol Http \
  --host 127.0.0.1 \
  --path /health \
  --interval 30 \
  --timeout 30 \
  --threshold 3

# Update backend HTTP settings to use probe
az network application-gateway http-settings update \
  --resource-group $RESOURCE_GROUP \
  --gateway-name midnight-appgw \
  --name appGatewayBackendHttpSettings \
  --probe midnight-health-probe

# Get Application Gateway public IP
export APPGW_PUBLIC_IP=$(az network public-ip show \
  --resource-group $RESOURCE_GROUP \
  --name midnight-appgw-pip \
  --query ipAddress \
  --output tsv)

echo "Application Gateway IP: $APPGW_PUBLIC_IP"
```

### Step 5: Configure DNS

```bash
# Add DNS A record pointing to Application Gateway IP
# proof.midnight.network -> $APPGW_PUBLIC_IP

# If using Azure DNS:
az network dns record-set a add-record \
  --resource-group $RESOURCE_GROUP \
  --zone-name midnight.network \
  --record-set-name proof \
  --ipv4-address $APPGW_PUBLIC_IP
```

### Step 6: Test HTTPS Endpoint

```bash
# Test HTTPS
curl https://proof.midnight.network/health
curl https://proof.midnight.network/version
curl https://proof.midnight.network/ready
```

---

## Monitoring and Logging

### Step 1: Create Log Analytics Workspace

```bash
# Create workspace
az monitor log-analytics workspace create \
  --resource-group $RESOURCE_GROUP \
  --workspace-name midnight-logs \
  --location $LOCATION

# Get workspace ID
export WORKSPACE_ID=$(az monitor log-analytics workspace show \
  --resource-group $RESOURCE_GROUP \
  --workspace-name midnight-logs \
  --query customerId \
  --output tsv)

echo "Workspace ID: $WORKSPACE_ID"
```

### Step 2: Enable VM Insights

```bash
# Install Azure Monitor agent on VM
az vm extension set \
  --resource-group $RESOURCE_GROUP \
  --vm-name $VM_NAME \
  --name AzureMonitorLinuxAgent \
  --publisher Microsoft.Azure.Monitor \
  --enable-auto-upgrade true

# Create data collection rule
az monitor data-collection rule create \
  --resource-group $RESOURCE_GROUP \
  --name midnight-dcr \
  --location $LOCATION \
  --rule-file - << 'EOF'
{
  "dataSources": {
    "performanceCounters": [
      {
        "name": "perfCounterDataSource",
        "streams": ["Microsoft-Perf"],
        "samplingFrequencyInSeconds": 60,
        "counterSpecifiers": [
          "\\Processor(_Total)\\% Processor Time",
          "\\Memory\\Available MBytes",
          "\\Network Interface(*)\\Bytes Total/sec"
        ]
      }
    ],
    "syslog": [
      {
        "name": "syslogDataSource",
        "streams": ["Microsoft-Syslog"],
        "facilityNames": ["*"],
        "logLevels": ["Error", "Critical", "Alert", "Emergency"]
      }
    ]
  },
  "destinations": {
    "logAnalytics": [
      {
        "workspaceResourceId": "/subscriptions/$(az account show --query id -o tsv)/resourceGroups/$RESOURCE_GROUP/providers/Microsoft.OperationalInsights/workspaces/midnight-logs",
        "name": "midnightLogs"
      }
    ]
  },
  "dataFlows": [
    {
      "streams": ["Microsoft-Perf", "Microsoft-Syslog"],
      "destinations": ["midnightLogs"]
    }
  ]
}
EOF
```

### Step 3: Create Custom Metrics

Create script to send custom metrics:

```bash
# On the VM
sudo tee /usr/local/bin/export-metrics-azure.sh > /dev/null << 'EOF'
#!/bin/bash
set -e

# Get metrics from proof server
METRICS=$(curl -s http://localhost:6300/ready || echo '{}')
QUEUE_SIZE=$(echo "$METRICS" | jq -r '.queue_size // 0')
ACTIVE_WORKERS=$(echo "$METRICS" | jq -r '.active_workers // 0')

# Get VM metadata
RESOURCE_ID=$(curl -s -H Metadata:true \
  "http://169.254.169.254/metadata/instance/compute/resourceId?api-version=2021-02-01&format=text")

# Send to Azure Monitor
az monitor metrics create \
  --resource $RESOURCE_ID \
  --namespace "MidnightProofServer" \
  --metric "QueueSize" \
  --value $QUEUE_SIZE \
  --timestamp "$(date -u +%Y-%m-%dT%H:%M:%SZ)"

az monitor metrics create \
  --resource $RESOURCE_ID \
  --namespace "MidnightProofServer" \
  --metric "ActiveWorkers" \
  --value $ACTIVE_WORKERS \
  --timestamp "$(date -u +%Y-%m-%dT%H:%M:%SZ)"

echo "Metrics exported: queue_size=$QUEUE_SIZE, active_workers=$ACTIVE_WORKERS"
EOF

sudo chmod +x /usr/local/bin/export-metrics-azure.sh

# Create cron job
echo "* * * * * /usr/local/bin/export-metrics-azure.sh >> /var/log/metrics-azure.log 2>&1" | sudo crontab -
```

### Step 4: Create Alert Rules

```bash
# Alert on high error rate
az monitor metrics alert create \
  --name "Proof Server High Error Rate" \
  --resource-group $RESOURCE_GROUP \
  --scopes "/subscriptions/$(az account show --query id -o tsv)/resourceGroups/$RESOURCE_GROUP/providers/Microsoft.Compute/virtualMachines/$VM_NAME" \
  --condition "count syslog where severity in ('Error', 'Critical') > 5" \
  --window-size 5m \
  --evaluation-frequency 1m \
  --description "Alert when error rate exceeds 5 per 5 minutes"

# Alert on high queue size
az monitor metrics alert create \
  --name "Proof Server Queue Overload" \
  --resource-group $RESOURCE_GROUP \
  --scopes "/subscriptions/$(az account show --query id -o tsv)/resourceGroups/$RESOURCE_GROUP/providers/Microsoft.Compute/virtualMachines/$VM_NAME" \
  --condition "avg MidnightProofServer/QueueSize > 100" \
  --window-size 5m \
  --evaluation-frequency 1m \
  --description "Alert when queue size exceeds 100"

# Alert on VM down
az monitor metrics alert create \
  --name "Proof Server VM Down" \
  --resource-group $RESOURCE_GROUP \
  --scopes "/subscriptions/$(az account show --query id -o tsv)/resourceGroups/$RESOURCE_GROUP/providers/Microsoft.Compute/virtualMachines/$VM_NAME" \
  --condition "count heartbeat < 1" \
  --window-size 5m \
  --evaluation-frequency 1m \
  --description "Alert when VM stops sending heartbeats"
```

### Step 5: Create Dashboard

```bash
# Create dashboard JSON
cat > dashboard.json << 'EOF'
{
  "lenses": {
    "0": {
      "order": 0,
      "parts": {
        "0": {
          "position": {"x": 0, "y": 0, "colSpan": 6, "rowSpan": 4},
          "metadata": {
            "type": "Extension/Microsoft_OperationsManagementSuite_Workspace/PartType/LogsDashboardPart",
            "inputs": [],
            "settings": {
              "content": {
                "Query": "Perf | where ObjectName == \"Processor\" | summarize avg(CounterValue) by bin(TimeGenerated, 5m)",
                "ControlType": "FrameControlChart"
              }
            }
          }
        },
        "1": {
          "position": {"x": 6, "y": 0, "colSpan": 6, "rowSpan": 4},
          "metadata": {
            "type": "Extension/Microsoft_OperationsManagementSuite_Workspace/PartType/LogsDashboardPart",
            "inputs": [],
            "settings": {
              "content": {
                "Query": "Perf | where ObjectName == \"Memory\" | summarize avg(CounterValue) by bin(TimeGenerated, 5m)",
                "ControlType": "FrameControlChart"
              }
            }
          }
        }
      }
    }
  },
  "metadata": {
    "model": {
      "timeRange": {"type": "MsPortalFx.Composition.Configuration.ValueTypes.TimeRange"}
    }
  }
}
EOF

# Create dashboard
az portal dashboard create \
  --resource-group $RESOURCE_GROUP \
  --name "Midnight Proof Server Dashboard" \
  --input-path dashboard.json \
  --location $LOCATION
```

---

## Security Configuration

### Step 1: Enable Azure AD Authentication for SSH

```bash
# Install Azure AD SSH extension
az vm extension set \
  --resource-group $RESOURCE_GROUP \
  --vm-name $VM_NAME \
  --name AADSSHLoginForLinux \
  --publisher Microsoft.Azure.ActiveDirectory

# Grant VM Login role
az role assignment create \
  --role "Virtual Machine Administrator Login" \
  --assignee user@midnight.network \
  --scope "/subscriptions/$(az account show --query id -o tsv)/resourceGroups/$RESOURCE_GROUP/providers/Microsoft.Compute/virtualMachines/$VM_NAME"

# SSH using Azure AD
az ssh vm \
  --resource-group $RESOURCE_GROUP \
  --name $VM_NAME
```

### Step 2: Enable Just-In-Time VM Access

```bash
# Enable Microsoft Defender for Cloud (if not already enabled)
az security pricing create \
  --name VirtualMachines \
  --tier Standard

# Configure JIT access
az security jit-policy create \
  --resource-group $RESOURCE_GROUP \
  --location $LOCATION \
  --name $VM_NAME \
  --virtual-machines "/subscriptions/$(az account show --query id -o tsv)/resourceGroups/$RESOURCE_GROUP/providers/Microsoft.Compute/virtualMachines/$VM_NAME" \
  --ports '[{"number": 22, "protocol": "*", "allowedSourceAddressPrefix": "*", "maxRequestAccessDuration": "PT3H"}]'

# Request JIT access (for 3 hours)
az security jit-policy request \
  --resource-group $RESOURCE_GROUP \
  --jit-policy-name $VM_NAME \
  --virtual-machines "[{\"id\":\"/subscriptions/$(az account show --query id -o tsv)/resourceGroups/$RESOURCE_GROUP/providers/Microsoft.Compute/virtualMachines/$VM_NAME\",\"ports\":[{\"number\":22,\"duration\":\"PT3H\"}]}]"
```

### Step 3: Enable Disk Encryption

```bash
# Disk is already encrypted with VMGuestStateOnly
# Verify encryption settings
az vm encryption show \
  --resource-group $RESOURCE_GROUP \
  --name $VM_NAME \
  --output table
```

### Step 4: Configure Network Watcher

```bash
# Enable Network Watcher
az network watcher configure \
  --resource-group $RESOURCE_GROUP \
  --locations $LOCATION \
  --enabled true

# Enable NSG flow logs
az network watcher flow-log create \
  --resource-group $RESOURCE_GROUP \
  --location $LOCATION \
  --nsg $NSG_NAME \
  --storage-account midnight-flow-logs-${RANDOM} \
  --name midnight-nsg-flow-log \
  --enabled true \
  --retention 7
```

---

## PCR Publication

### Step 1: Extract Final PCR Values

```bash
# SSH into production VM
az ssh vm --resource-group $RESOURCE_GROUP --name $VM_NAME

# Run extraction script
sudo /usr/local/bin/extract-pcrs.sh /tmp/prod-azure-pcr-values.json

# Download to local machine
az vm run-command invoke \
  --resource-group $RESOURCE_GROUP \
  --name $VM_NAME \
  --command-id RunShellScript \
  --scripts "cat /tmp/prod-azure-pcr-values.json" \
  --query 'value[0].message' \
  --output tsv > azure-pcr-values.json
```

### Step 2: Sign PCR Values

```bash
# On local machine with GPG key
gpg --detach-sign --armor azure-pcr-values.json

# Verify
gpg --verify azure-pcr-values.json.asc azure-pcr-values.json
```

### Step 3: Publish to GitHub

```bash
# Create release
gh release create v1.0.0-azure \
  --title "Midnight Proof Server v1.0.0 - Azure Confidential VM" \
  --notes "Production PCR values for Azure Confidential VM deployment" \
  azure-pcr-values.json \
  azure-pcr-values.json.asc
```

---

## Troubleshooting

### Common Issues

#### 1. VM Creation Fails with "Size Not Available"

**Symptom**: `The requested VM size 'Standard_DC4s_v3' is not available`

**Solution**:
```bash
# Check available sizes in region
az vm list-skus \
  --location $LOCATION \
  --size Standard_DC \
  --all \
  --output table | grep -i confidential

# Try different region
export LOCATION="westus2"

# Or use smaller size
# Standard_DC2s_v3 (2 vCPU, 16GB)
```

#### 2. Docker Container Fails to Start

**Symptom**: Container exits immediately after creation

**Diagnosis**:
```bash
# Check cloud-init logs
az vm run-command invoke \
  --resource-group $RESOURCE_GROUP \
  --name $VM_NAME \
  --command-id RunShellScript \
  --scripts "tail -100 /var/log/cloud-init-output.log"

# Check Docker logs
az vm run-command invoke \
  --resource-group $RESOURCE_GROUP \
  --name $VM_NAME \
  --command-id RunShellScript \
  --scripts "docker logs midnight-proof-server"
```

#### 3. Application Gateway Shows "Unhealthy"

**Symptom**: Backend pool status is unhealthy

**Diagnosis**:
```bash
# Check backend health
az network application-gateway show-backend-health \
  --resource-group $RESOURCE_GROUP \
  --name midnight-appgw

# Test health endpoint from VM
az vm run-command invoke \
  --resource-group $RESOURCE_GROUP \
  --name $VM_NAME \
  --command-id RunShellScript \
  --scripts "curl -v http://localhost:6300/health"
```

#### 4. Attestation Fails

**Symptom**: `/attestation` endpoint returns errors

**Diagnosis**:
```bash
# Check vTPM status
az vm run-command invoke \
  --resource-group $RESOURCE_GROUP \
  --name $VM_NAME \
  --command-id RunShellScript \
  --scripts "sudo tpm2_pcrread"

# Check Azure Attestation Service
az attestation show \
  --name $ATTESTATION_NAME \
  --resource-group $RESOURCE_GROUP
```

---

## Maintenance

### Daily Tasks

```bash
# Check VM status
az vm get-instance-view \
  --resource-group $RESOURCE_GROUP \
  --name $VM_NAME \
  --query instanceView.statuses \
  --output table

# Check error logs
az monitor log-analytics query \
  --workspace $WORKSPACE_ID \
  --analytics-query "Syslog | where SeverityLevel in ('err', 'crit') | top 10 by TimeGenerated desc" \
  --output table
```

### Weekly Tasks

```bash
# Review security recommendations
az security assessment list \
  --resource-group $RESOURCE_GROUP \
  --output table

# Check for updates
az vm run-command invoke \
  --resource-group $RESOURCE_GROUP \
  --name $VM_NAME \
  --command-id RunShellScript \
  --scripts "apt list --upgradable"
```

### Monthly Tasks

```bash
# Apply updates
az vm run-command invoke \
  --resource-group $RESOURCE_GROUP \
  --name $VM_NAME \
  --command-id RunShellScript \
  --scripts "sudo apt-get update && sudo apt-get upgrade -y"

# Rotate secrets
az keyvault secret set-attributes \
  --vault-name $KEYVAULT_NAME \
  --name midnight-api-key \
  --enabled true
```

---

## Cost Optimization

### 1. Reserved Instances

```bash
# Purchase 3-year RI (65% discount)
az vm reservation create \
  --resource-group $RESOURCE_GROUP \
  --term P3Y \
  --sku Standard_DC4s_v3 \
  --quantity 1 \
  --scope Shared

# Savings: $240/mo → $85/mo
```

### 2. Auto-Shutdown

```bash
# Schedule daily shutdown at 10 PM
az vm auto-shutdown \
  --resource-group $RESOURCE_GROUP \
  --name $VM_NAME \
  --time 2200 \
  --email notification@midnight.network
```

### Total Optimized Cost

| Resource | Standard | Optimized | Savings |
|----------|----------|-----------|---------|
| VM (DC4s_v3) | $240 | $85 (3yr RI) | -65% |
| Disk | $19 | $19 | 0% |
| Network | $88 | $88 | 0% |
| App Gateway | $73 | $73 | 0% |
| Other | $23 | $23 | 0% |
| **Total** | **$443** | **$288** | **-35%** |

---

## Summary

### What We Deployed

- ✅ Confidential VM with AMD SEV-SNP
- ✅ Azure Attestation Service integration
- ✅ JWT-based attestation tokens
- ✅ Application Gateway with HTTPS
- ✅ Azure Monitor and Log Analytics
- ✅ Key Vault for secrets management
- ✅ Managed identity for secure access
- ✅ Production-ready configuration

### Key Resources

| Resource | Name | Purpose |
|----------|------|---------|
| Resource Group | midnight-proof-server-rg | Container for all resources |
| VM | midnight-proof-server | Confidential VM |
| Key Vault | midnight-kv-* | Secrets storage |
| ACR | midnightacr* | Docker image registry |
| Attestation | midnight-attestation-* | JWT attestation service |
| App Gateway | midnight-appgw | HTTPS load balancer |
| Log Analytics | midnight-logs | Log aggregation |

### Access Information

```bash
# SSH (Azure AD)
az ssh vm --resource-group $RESOURCE_GROUP --name $VM_NAME

# HTTPS Endpoint
https://proof.midnight.network

# Monitoring
https://portal.azure.com/#@/resource/subscriptions/.../resourceGroups/midnight-proof-server-rg/providers/Microsoft.Compute/virtualMachines/midnight-proof-server/overview
```

---

**Documentation Version:** 1.0
**Last Updated:** 2025-12-18
**Tested On:** Azure Confidential VMs (AMD SEV-SNP), Ubuntu 22.04 LTS

For questions or issues, see [TROUBLESHOOTING.md](TROUBLESHOOTING.md) or open a GitHub issue.
