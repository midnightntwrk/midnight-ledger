# Multi-Cloud Deployment Overview

## Quick Reference for Deploying Midnight Proof Server

**Last Updated:** 2025-12-18

---

❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌

## DANGER ZONE: All of the below is experimental, not yet tested ##

❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌

## Document Control

| Version | Date       | Author               | Changes       |
| ------- | ---------- | -------------------- | ------------- |
| 1.0     | 2025-12-19 | Bob Blessing-Hartley | Initial draft |

## Documentation Status

| Cloud Provider | Guide Status | Location | Difficulty |
|----------------|--------------|----------|------------|
| **AWS Nitro Enclaves** | Draft Completed | [deploy-aws-nitro.md](deploy-aws-nitro.md) | Intermediate |
| **GCP Confidential VMs** | Draft Completed | [deploy-gcp-confidential.md](deploy-gcp-confidential.md) | Intermediate |
| **Azure Confidential VMs** | Draft Completed | [deploy-azure-confidential.md](deploy-azure-confidential.md) | Intermediate |

---

## Cloud Provider Comparison

### TEE Technology

| Feature | AWS Nitro | GCP Confidential VM | Azure Confidential VM |
|---------|-----------|---------------------|----------------------|
| **TEE Type** | Custom Nitro silicon | AMD SEV-SNP | AMD SEV-SNP / Intel TDX |
| **Attestation Format** | CBOR | TPM 2.0 | JWT (via Attestation Service) |
| **Memory Encryption** | Hardware | AMD SEV | AMD SEV / Intel TDX |
| **Debug Mode** | Can disable | Can disable | Can disable |
| **Maturity** | ⭐⭐⭐⭐⭐ | ⭐⭐⭐⭐ | ⭐⭐⭐⭐ |

### Cost Comparison (Estimated Monthly)

| Component | AWS | GCP | Azure |
|-----------|-----|-----|-------|
| **Compute (8 vCPU, 32GB)** | $248 (c5.2xlarge) | $220 (n2d-standard-8) | $240 (DCsv3-series) |
| **Storage (100GB SSD)** | $8 (gp3) | $10 (SSD) | $9 (Premium SSD) |
| **Network (1TB egress)** | $90 | $85 | $88 |
| **Monitoring** | $5 | $8 | $6 |
| **Load Balancer** | $20 | $18 | $22 |
| **Total (Estimate)** | **~$371/mo** | **~$341/mo** | **~$365/mo** |

**Note:** Prices vary by region and commitment. All support 1-3 year reserved instance discounts (30-60% savings).

### Deployment Complexity

| Task | AWS Nitro | GCP Confidential | Azure Confidential |
|------|-----------|------------------|-------------------|
| **Infrastructure Setup** | Moderate | Easy | Moderate |
| **Enclave/VM Creation** | Complex (.eif build) | Easy (Docker-based) | Easy (Docker-based) |
| **Attestation Integration** | Moderate (CBOR) | Moderate (TPM 2.0) | Complex (JWT + Service) |
| **Networking** | Moderate (vsock) | Simple (standard) | Simple (standard) |
| **PCR Extraction** | Easy | Moderate | Moderate |
| **Overall Difficulty** | ⭐⭐⭐⭐ | ⭐⭐⭐ | ⭐⭐⭐⭐ |

### Instance Types Recommended

**AWS:**
- c5.xlarge (4 vCPU, 8GB) - Small deployments
- c5.2xlarge (8 vCPU, 16GB) - **Recommended**
- c5.4xlarge (16 vCPU, 32GB) - High traffic

**GCP:**
- n2d-standard-4 (4 vCPU, 16GB) - Small deployments
- n2d-standard-8 (8 vCPU, 32GB) - **Recommended**
- n2d-standard-16 (16 vCPU, 64GB) - High traffic

**Azure:**
- DC2s_v3 (2 vCPU, 16GB) - Small deployments
- DC4s_v3 (4 vCPU, 32GB) - **Recommended**
- DC8s_v3 (8 vCPU, 64GB) - High traffic

---

## Quick Start Guide

### AWS Nitro Enclaves

**Why Choose AWS:**
- Most mature TEE offering
- Custom silicon (not AMD/Intel dependent)
- Best documentation and tooling
- Easiest attestation verification

**Quick Start:**
```bash
# 1. Infrastructure
aws ec2 create-vpc --cidr-block 10.0.0.0/16
# ... (see full guide)

# 2. Launch EC2 with enclave support
aws ec2 run-instances \
  --instance-type c5.2xlarge \
  --enclave-options 'Enabled=true' \
  ...

# 3. Build enclave image
nitro-cli build-enclave \
  --docker-uri midnight-proof-server:latest \
  --output-file midnight-proof-server.eif

# 4. Run enclave
nitro-cli run-enclave \
  --eif-path midnight-proof-server.eif \
  --memory 24576 \
  --cpu-count 8 \
  --debug-mode false
```

**Full Guide:** [deploy-aws-nitro.md](deploy-aws-nitro.md)

---

### GCP Confidential VMs

**Why Choose GCP:**
- Simplest deployment process
- Standard Docker containers
- No special enclave build process
- Good integration with GCP ecosystem

**Quick Start:**
```bash
# 1. Create Confidential VM
gcloud compute instances create midnight-proof-server \
  --zone=us-central1-a \
  --machine-type=n2d-standard-8 \
  --confidential-compute \
  --maintenance-policy=TERMINATE \
  ...

# 2. SSH and run Docker
gcloud compute ssh midnight-proof-server
docker run -p 6300:6300 midnight-proof-server:latest

# 3. Extract attestation (TPM 2.0)
tpm2_quote \
  --key-context 0x81000003 \
  --pcr-list sha256:0,1,4,5,7 \
  ...
```

**Full Guide:** [deploy-gcp-confidential.md](deploy-gcp-confidential.md)

---

### Azure Confidential VMs

**Why Choose Azure:**
- Good if already using Azure
- Managed Attestation Service
- AMD SEV-SNP support
- Growing ecosystem

**Quick Start:**
```bash
# 1. Create resource group
az group create \
  --name midnight-proof-server-rg \
  --location eastus

# 2. Create Confidential VM
az vm create \
  --resource-group midnight-proof-server-rg \
  --name midnight-proof-server \
  --size Standard_DC4s_v3 \
  --security-type ConfidentialVM \
  ...

# 3. SSH and run Docker
az vm run-command invoke \
  --resource-group midnight-proof-server-rg \
  --name midnight-proof-server \
  --command-id RunShellScript \
  --scripts "docker run -p 6300:6300 midnight-proof-server:latest"

# 4. Get attestation (Azure Attestation Service)
curl -H Metadata:true \
  "http://169.254.169.254/metadata/attested/document?api-version=2020-09-01&nonce=$NONCE"
```

**Full Guide:** [deploy-azure-confidential.md](deploy-azure-confidential.md)

---

## Common Concepts Across All Clouds

### 1. Docker Image Build

**Same across all clouds:**

```dockerfile
FROM rust:1.75-slim as builder
WORKDIR /app
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim
COPY --from=builder /app/target/release/midnight-proof-server-prototype /usr/local/bin/
EXPOSE 6300
CMD ["midnight-proof-server-prototype", "--port", "6300"]
```

### 2. Attestation Verification (Wallet Side)

**Wallets must verify regardless of cloud:**
1. Certificate chain validation
2. PCR measurement verification
3. Timestamp freshness (<5 min)
4. Security properties (debug mode OFF)
5. Signature validation

### 3. PCR Publication

**Same process for all clouds:**
1. Extract PCR values from deployment
2. Format as JSON with metadata
3. Sign with GPG key
4. Publish to GitHub releases
5. Include public key for verification

### 4. Monitoring

**All support:**
- Native cloud monitoring (CloudWatch/Cloud Monitoring/Azure Monitor)
- Custom metrics via API
- Log aggregation
- Alerting
- Health checks

---

## Migration Between Clouds

### Considerations

**PCR Values:**
- ⚠️ PCR values **will differ** between clouds
- Must publish separate PCR values for each cloud
- Wallets must support multiple PCR sets

**Code Changes:**
- ✅ Proof server code is **identical**
- Only attestation endpoint differs (cloud-specific)
- Unified interface abstracts differences

**Data:**
- ✅ Proof server is **stateless**
- No data to migrate
- Can run multi-cloud simultaneously

### Multi-Cloud Strategy

**Active-Active:**
```
Users → Load Balancer
         ├─> AWS Nitro (us-east-1)
         ├─> GCP Confidential (us-central1)
         └─> Azure Confidential (East US)
```

**Benefits:**
- Geographic distribution
- Cloud provider redundancy
- Cost optimization (use cheapest)
- A/B testing attestation formats

---

## Support and Next Steps

### Documentation
- ✅ **AWS Guide Complete:** [deploy-aws-nitro.md](deploy-aws-nitro.md) 
- ✅ **GCP Guide Complete:** [deploy-gcp-confidential.md](deploy-gcp-confidential.md)
- ✅ **Azure Guide Complete:** [deploy-azure-confidential.md](deploy-azure-confidential.md) 

### Additional Resources
- ✅ **Operations and Monitoring Guide:** [operations_monitoring.md](operations_monitoring.md)
- ✅ **PCR Publication Guide Complete:** [pcr_publication_guide.md](pcr_publication_guide.md)
- ✅ **Troubleshooting Guide Complete:** [troubleshooting.mdl](troubleshooting.md)
- Terraform Modules (coming)

