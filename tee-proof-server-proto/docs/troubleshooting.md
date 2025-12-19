# Troubleshooting Guide

**Midnight Proof Server - Comprehensive Troubleshooting**

## Document Control

| Version | Date       | Author               | Changes       |
| ------- | ---------- | -------------------- | ------------- |
| 1.0     | 2025-12-19 | Bob Blessing-Hartley | Initial draft |

---

## Table of Contents

1. [Quick Diagnostics](#quick-diagnostics)
2. [Deployment Issues](#deployment-issues)
3. [Runtime Issues](#runtime-issues)
4. [Attestation Issues](#attestation-issues)
5. [Performance Issues](#performance-issues)
6. [Network Issues](#network-issues)
7. [Security Issues](#security-issues)
8. [Cloud-Specific Issues](#cloud-specific-issues)
9. [Debugging Tools](#debugging-tools)
10. [Getting Help](#getting-help)

---

## Quick Diagnostics

### Health Check Script

Run this script first for a comprehensive diagnostic:

```bash
#!/bin/bash
# File: diagnose.sh
# Usage: ./diagnose.sh [SERVER_URL]
# Example: ./diagnose.sh https://proof.midnight.network
# Example: ./diagnose.sh http://localhost:6300

set -e

# Accept server URL as parameter, default to production server
SERVER_URL="${1:-https://proof.midnight.network}"

# Extract hostname and port for DNS/SSL checks
if [[ "$SERVER_URL" =~ ^https?://([^:/]+)(:([0-9]+))?$ ]]; then
    SERVER_HOST="${BASH_REMATCH[1]}"
    SERVER_PORT="${BASH_REMATCH[3]}"
    if [ -z "$SERVER_PORT" ]; then
        if [[ "$SERVER_URL" =~ ^https:// ]]; then
            SERVER_PORT="443"
        else
            SERVER_PORT="80"
        fi
    fi
else
    echo "Error: Invalid server URL format"
    echo "Usage: $0 [SERVER_URL]"
    echo "Example: $0 https://proof.midnight.network"
    exit 1
fi

echo "=== Midnight Proof Server Diagnostics ==="
echo "Server: $SERVER_URL"
echo "Timestamp: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
echo ""

# 1. Check if server is reachable
echo "1. Server Reachability:"
if curl -s -f -m 5 "$SERVER_URL/health" > /dev/null; then
    echo "   ✅ Server is reachable"
else
    echo "   ❌ Server is NOT reachable"
    echo "   Try: curl -v $SERVER_URL/health"
fi
echo ""

# 2. Check version
echo "2. Server Version:"
VERSION=$(curl -s "$SERVER_URL/version" 2>/dev/null || echo "ERROR")
if [ "$VERSION" != "ERROR" ]; then
    echo "   ✅ Version: $VERSION"
else
    echo "   ❌ Could not retrieve version"
fi
echo ""

# 3. Check readiness
echo "3. Server Readiness:"
READY=$(curl -s "$SERVER_URL/ready" 2>/dev/null || echo "ERROR")
if [ "$READY" != "ERROR" ]; then
    echo "   ✅ Ready: $READY"
    QUEUE_SIZE=$(echo "$READY" | jq -r '.queue_size // "unknown"')
    ACTIVE_WORKERS=$(echo "$READY" | jq -r '.active_workers // "unknown"')
    echo "   Queue size: $QUEUE_SIZE"
    echo "   Active workers: $ACTIVE_WORKERS"
else
    echo "   ❌ Server not ready"
fi
echo ""

# 4. Check SSL certificate (only for HTTPS)
if [[ "$SERVER_URL" =~ ^https:// ]]; then
    echo "4. SSL Certificate:"
    CERT_EXPIRY=$(echo | openssl s_client -servername "$SERVER_HOST" \
        -connect "$SERVER_HOST:$SERVER_PORT" 2>/dev/null | \
        openssl x509 -noout -dates 2>/dev/null || echo "ERROR")
    if [ "$CERT_EXPIRY" != "ERROR" ]; then
        echo "   ✅ Certificate valid"
        echo "   $CERT_EXPIRY"
    else
        echo "   ❌ Certificate error"
    fi
    echo ""
fi

# 5. Check DNS resolution
echo "5. DNS Resolution:"
DNS_IP=$(dig +short "$SERVER_HOST" @8.8.8.8 | head -1)
if [ -n "$DNS_IP" ]; then
    echo "   ✅ Resolves to: $DNS_IP"
else
    echo "   ⚠️  DNS resolution failed (might be localhost)"
fi
echo ""

# 6. Check response time
echo "6. Response Time:"
RESPONSE_TIME=$(curl -s -o /dev/null -w "%{time_total}" "$SERVER_URL/health" 2>/dev/null || echo "ERROR")
if [ "$RESPONSE_TIME" != "ERROR" ]; then
    echo "   ✅ Response time: ${RESPONSE_TIME}s"
    if (( $(echo "$RESPONSE_TIME > 2.0" | bc -l) )); then
        echo "   ⚠️  Slow response (>2s)"
    fi
else
    echo "   ❌ Could not measure response time"
fi
echo ""

# 7. Check proof-versions endpoint
echo "7. Supported Proof Versions:"
PROOF_VERSIONS=$(curl -s "$SERVER_URL/proof-versions" 2>/dev/null || echo "ERROR")
if [ "$PROOF_VERSIONS" != "ERROR" ]; then
    echo "   ✅ Supported: $PROOF_VERSIONS"
else
    echo "   ❌ Could not retrieve proof versions"
fi
echo ""

echo "=== Diagnostic Complete ==="
echo ""
echo "If any checks failed, see detailed troubleshooting below."
```

Save as `diagnose.sh`, make executable, and run:

```bash
chmod +x diagnose.sh

# Test production server (default)
./diagnose.sh

# Test specific server
./diagnose.sh https://proof.midnight.network

# Test local development server
./diagnose.sh http://localhost:6300

# Test custom server
./diagnose.sh https://proof-staging.midnight.network
```

❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌

## DANGER ZONE: All of the below is experimental, not tested ##

❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌

## Deployment Issues

### Issue 1: VM/Instance Fails to Start

#### AWS Nitro

**Symptom:** EC2 instance terminates immediately after launch

**Diagnosis:**
```bash
# Check instance status
aws ec2 describe-instances \
  --instance-ids i-1234567890abcdef0 \
  --query 'Reservations[0].Instances[0].StateReason'

# Check system log
aws ec2 get-console-output \
  --instance-id i-1234567890abcdef0
```

**Common Causes:**
1. ❌ Instance type doesn't support enclaves (must be .xlarge or larger)
2. ❌ Enclave options not enabled
3. ❌ Insufficient IAM permissions
4. ❌ AMI not compatible with enclaves

**Solutions:**
```bash
# Use correct instance type
aws ec2 run-instances \
  --instance-type c5.2xlarge \  # Must be xlarge or larger
  --enclave-options 'Enabled=true' \
  ...

# Verify enclave support
aws ec2 describe-instance-types \
  --instance-types c5.2xlarge \
  --query 'InstanceTypes[0].InstanceType' \
  --filters "Name=processor-info.supported-features,Values=amd-sev-snp"
```

#### GCP Confidential VM

**Symptom:** VM status shows "TERMINATED"

**Diagnosis:**
```bash
# Check VM status and error
gcloud compute instances describe midnight-proof-server \
  --zone=us-central1-a \
  --format="get(status, statusMessage)"

# Check serial console output
gcloud compute instances get-serial-port-output midnight-proof-server \
  --zone=us-central1-a
```

**Common Causes:**
1. ❌ Confidential Computing not available in zone
2. ❌ Maintenance policy not set to TERMINATE
3. ❌ Insufficient quota
4. ❌ n2d instance type not available

**Solutions:**
```bash
# Check zone support
gcloud compute zones describe us-central1-a | grep -i confidential

# Try different zone
gcloud compute instances create midnight-proof-server \
  --zone=us-central1-b \
  --confidential-compute \
  --maintenance-policy=TERMINATE

# Check quota
gcloud compute project-info describe \
  --project=your-project \
  --format="table(quotas.metric, quotas.limit, quotas.usage)"
```

#### Azure Confidential VM

**Symptom:** VM creation fails with "size not available"

**Diagnosis:**
```bash
# Check available sizes
az vm list-skus \
  --location eastus \
  --size Standard_DC \
  --all \
  --output table

# Check VM status
az vm get-instance-view \
  --resource-group midnight-proof-server-rg \
  --name midnight-proof-server \
  --query instanceView.statuses
```

**Common Causes:**
1. ❌ DCsv3-series not available in region
2. ❌ Quota exceeded
3. ❌ Wrong security-type parameter

**Solutions:**
```bash
# Try different region
az vm create \
  --resource-group midnight-proof-server-rg \
  --name midnight-proof-server \
  --location westus2 \  # Try different region
  --size Standard_DC4s_v3 \
  --security-type ConfidentialVM

# Check quota
az vm list-usage \
  --location eastus \
  --output table | grep "DC"
```

---

### Issue 2: Docker Container Won't Start

**Symptom:** Container exits immediately or restarts continuously

**Diagnosis:**
```bash
# Check container status
docker ps -a | grep midnight-proof-server

# Check logs
docker logs midnight-proof-server

# Check exit code
docker inspect midnight-proof-server \
  --format='{{.State.ExitCode}}: {{.State.Error}}'
```

**Common Exit Codes:**

| Code | Meaning | Common Cause |
|------|---------|--------------|
| 1 | General error | Missing environment variables, config error |
| 125 | Docker error | Invalid Docker run command |
| 126 | Command not executable | Binary not executable or wrong architecture |
| 127 | Command not found | Binary path wrong |
| 137 | Killed by SIGKILL | Out of memory |
| 139 | Segmentation fault | Binary corruption or incompatible architecture |

**Solutions by Exit Code:**

**Exit Code 1 (Config Error):**
```bash
# Check environment variables
docker inspect midnight-proof-server --format='{{json .Config.Env}}' | jq

# Common missing variables:
# - MIDNIGHT_PROOF_SERVER_API_KEY
# - MIDNIGHT_PROOF_SERVER_PORT

# Fix: Set environment variable
docker run -d \
  -e MIDNIGHT_PROOF_SERVER_API_KEY="your-key-here" \
  midnight-proof-server:latest
```

**Exit Code 137 (OOM Killed):**
```bash
# Check memory limits
docker inspect midnight-proof-server --format='{{.HostConfig.Memory}}'

# Increase memory
docker run -d \
  --memory=32g \
  midnight-proof-server:latest
```

**Exit Code 139 (Segmentation Fault):**
```bash
# Likely architecture mismatch or corruption

# Verify image architecture
docker inspect midnight-proof-server:latest \
  --format='{{.Architecture}}'

# Rebuild image
cd /Users/robertblessing-hartley/code/tee-prover-prototype/proof-server
docker build --no-cache -t midnight-proof-server:latest .
```

---

## Runtime Issues

### Issue 3: Proof Generation Fails

**Symptom:** `/prove` endpoint returns 500 error

**Diagnosis:**

```bash
# Test prove endpoint
curl -X POST https://proof.midnight.network/prove \
  -H "Content-Type: application/octet-stream" \
  -H "X-API-Key: your-api-key" \
  --data-binary @test-zkir.bin \
  -v

# Check server logs
docker logs midnight-proof-server | grep ERROR
```

**Common Causes:**

1. **Invalid ZKIR format**
   
   ```bash
   # Error: "Failed to deserialize ZKIR"
   # Solution: Verify ZKIR is correctly tagged and serialized
   
   # Test with known-good ZKIR
   curl -X POST https://proof.midnight.network/prove \
     -H "X-API-Key: your-key" \
     --data-binary @known-good-zkir.bin
   ```
   
2. **Timeout**
   ```bash
   # Error: "Proof generation timeout"
   # Solution: Increase timeout
   
   docker run -d \
     -e MIDNIGHT_PROOF_SERVER_JOB_TIMEOUT=1200 \  # 20 minutes
     midnight-proof-server:latest
   ```

3. **Out of Memory**
   ```bash
   # Check container memory
   docker stats midnight-proof-server --no-stream
   
   # Increase memory (AWS Nitro)
   nitro-cli terminate-enclave --enclave-id i-xxx-encYYY
   nitro-cli run-enclave \
     --eif-path midnight-proof-server.eif \
     --memory 32768 \  # 32GB
     --cpu-count 8
   
   # Increase memory (GCP/Azure)
   # Upgrade to larger instance type
   ```

4. **Worker Pool Exhausted**
   ```bash
   # Check worker status
   curl https://proof.midnight.network/ready
   # {"queue_size": 100, "active_workers": 16}
   
   # If queue_size is high, increase workers
   docker run -d \
     -e MIDNIGHT_PROOF_SERVER_NUM_WORKERS=32 \
     midnight-proof-server:latest
   ```

---

### Issue 4: High Latency

**Symptom:** Proof generation takes >10 minutes

**Diagnosis:**
```bash
# Measure end-to-end latency
time curl -X POST https://proof.midnight.network/prove \
  -H "X-API-Key: your-key" \
  --data-binary @zkir.bin \
  -o proof.bin

# Check server queue
curl https://proof.midnight.network/ready
```

**Performance Optimization:**

1. **Insufficient CPU**
   ```bash
   # AWS: Increase CPU cores
   nitro-cli terminate-enclave --enclave-id i-xxx-encYYY
   nitro-cli run-enclave \
     --eif-path midnight-proof-server.eif \
     --cpu-count 16  # Increase from 8
   
   # GCP: Upgrade instance
   gcloud compute instances stop midnight-proof-server --zone=us-central1-a
   gcloud compute instances set-machine-type midnight-proof-server \
     --machine-type=n2d-standard-16 \
     --zone=us-central1-a
   gcloud compute instances start midnight-proof-server --zone=us-central1-a
   
   # Azure: Upgrade instance
   az vm deallocate \
     --resource-group midnight-proof-server-rg \
     --name midnight-proof-server
   az vm resize \
     --resource-group midnight-proof-server-rg \
     --name midnight-proof-server \
     --size Standard_DC8s_v3
   az vm start \
     --resource-group midnight-proof-server-rg \
     --name midnight-proof-server
   ```

2. **Insufficient Worker Threads**
   ```bash
   # Increase workers (should match CPU cores)
   docker run -d \
     -e MIDNIGHT_PROOF_SERVER_NUM_WORKERS=16 \
     midnight-proof-server:latest
   ```

3. **Network Latency**
   ```bash
   # Measure network latency
   curl -w "@curl-format.txt" -o /dev/null -s https://proof.midnight.network/health
   
   # curl-format.txt:
   # time_namelookup: %{time_namelookup}\n
   # time_connect: %{time_connect}\n
   # time_appconnect: %{time_appconnect}\n
   # time_pretransfer: %{time_pretransfer}\n
   # time_redirect: %{time_redirect}\n
   # time_starttransfer: %{time_starttransfer}\n
   # time_total: %{time_total}\n
   
   # If network latency is high (>100ms), deploy closer to users
   ```

---

## Attestation Issues

### Issue 5: Attestation Verification Fails

**Symptom:** Wallets reject attestation documents

**Diagnosis:**

#### AWS Nitro

```bash
# Get attestation document
NONCE=$(openssl rand -hex 32)
curl "http://localhost:6300/attestation?nonce=$NONCE" > attestation.cbor

# Decode CBOR (requires cbor-diag tool)
cat attestation.cbor | cbor-diag

# Verify PCRs match published values
# Compare attestation PCRs with:
curl -L https://github.com/midnight/proof-server/releases/download/v1.0.0/aws-nitro-pcr-values.json
```

**Common Issues:**

1. **PCR Mismatch**
   ```bash
   # Published PCR0: abc123...
   # Attestation PCR0: def456...
   
   # Cause: Running different version than published
   
   # Solution: Redeploy with correct version
   docker pull midnight-proof-server:v1.0.0
   # Rebuild enclave with correct version
   ```

2. **Debug Mode Enabled**
   ```bash
   # Check debug mode in attestation
   cat attestation.cbor | cbor-diag | grep -i debug
   
   # If debug_mode: true, rebuild without debug
   nitro-cli run-enclave \
     --eif-path midnight-proof-server.eif \
     --debug-mode false  # MUST be false for production
   ```

3. **Certificate Chain Invalid**
   ```bash
   # AWS Nitro certificate chain must link to AWS root CA
   # Verify chain (requires specialized tool or library)
   
   # Common cause: System clock wrong
   ssh ec2-user@instance "timedatectl status"
   
   # Fix clock
   ssh ec2-user@instance "sudo timedatectl set-ntp true"
   ```

#### GCP Confidential VM

```bash
# Get TPM quote
NONCE=$(openssl rand -hex 32)
curl "http://localhost:6300/attestation?nonce=$NONCE" > quote.json

# Extract PCR values
cat quote.json | jq '.pcrs' | base64 -d | xxd

# Verify signature (requires tpm2-tools)
# This is complex - see PCR_PUBLICATION_GUIDE.md for full process
```

**Common Issues:**

1. **vTPM Not Enabled**
   ```bash
   # Check if vTPM is enabled
   gcloud compute instances describe midnight-proof-server \
     --zone=us-central1-a \
     --format="get(shieldedInstanceConfig.enableVtpm)"
   
   # If false, recreate with vTPM
   gcloud compute instances create midnight-proof-server \
     --shielded-vtpm \
     ...
   ```

2. **TPM Tools Not Installed**
   ```bash
   # SSH into VM
   gcloud compute ssh midnight-proof-server --zone=us-central1-a
   
   # Install tpm2-tools
   sudo apt-get update && sudo apt-get install -y tpm2-tools
   
   # Verify TPM works
   sudo tpm2_pcrread
   ```

#### Azure Confidential VM

```bash
# Get JWT attestation token
NONCE=$(openssl rand -hex 32)
curl "http://localhost:6300/attestation?nonce=$NONCE" > token.json

# Decode JWT (3 parts: header.payload.signature)
JWT=$(cat token.json | jq -r '.jwt_token')
echo $JWT | cut -d. -f2 | base64 -d 2>/dev/null | jq

# Verify signature against Azure Attestation Service public key
# (Requires jose library or similar)
```

**Common Issues:**

1. **Azure Attestation Service Not Configured**
   ```bash
   # Check if attestation provider exists
   az attestation show \
     --name midnight-attestation \
     --resource-group midnight-proof-server-rg
   
   # If not found, create it
   az attestation create \
     --name midnight-attestation \
     --resource-group midnight-proof-server-rg \
     --location eastus
   ```

2. **JWT Signature Invalid**
   ```bash
   # Common cause: System clock skew
   
   # Check VM time
   az vm run-command invoke \
     --resource-group midnight-proof-server-rg \
     --name midnight-proof-server \
     --command-id RunShellScript \
     --scripts "timedatectl status"
   
   # Enable NTP
   az vm run-command invoke \
     --resource-group midnight-proof-server-rg \
     --name midnight-proof-server \
     --command-id RunShellScript \
     --scripts "sudo timedatectl set-ntp true"
   ```

---

## Performance Issues

### Issue 6: High CPU Usage

**Symptom:** CPU usage consistently >80%

**Diagnosis:**
```bash
# AWS
ssh ec2-user@instance "top -bn1 | head -20"

# GCP
gcloud compute ssh midnight-proof-server --zone=us-central1-a \
  --command="top -bn1 | head -20"

# Azure
az vm run-command invoke \
  --resource-group midnight-proof-server-rg \
  --name midnight-proof-server \
  --command-id RunShellScript \
  --scripts "top -bn1 | head -20"
```

**Solutions:**

1. **Scale Vertically (More CPU)**
   ```bash
   # See "Issue 4: High Latency" for instance upgrade commands
   ```

2. **Scale Horizontally (Multiple Instances)**
   ```bash
   # Deploy multiple proof servers behind load balancer
   # Each handles a portion of the load
   
   # Example: 3 instances
   # proof-server-1.midnight.network
   # proof-server-2.midnight.network
   # proof-server-3.midnight.network
   # -> Load balancer: proof.midnight.network
   ```

3. **Optimize Worker Pool**
   ```bash
   # Set workers = number of CPU cores
   # For 16 vCPU:
   docker run -d \
     -e MIDNIGHT_PROOF_SERVER_NUM_WORKERS=16 \
     midnight-proof-server:latest
   ```

---

### Issue 7: Memory Leak

**Symptom:** Memory usage grows over time, eventually causing OOM

**Diagnosis:**
```bash
# Monitor memory over time
docker stats midnight-proof-server

# Check for memory leak indicators
docker exec midnight-proof-server cat /proc/meminfo

# Check OOM kills in system log
dmesg | grep -i "out of memory"
```

**Solutions:**

1. **Increase Memory Temporarily**
   ```bash
   # Buy time to investigate
   docker run -d \
     --memory=64g \
     midnight-proof-server:latest
   ```

2. **Restart Periodically (Workaround)**
   ```bash
   # Create cron job to restart daily
   0 2 * * * docker restart midnight-proof-server
   ```

3. **Report Bug**
   ```bash
   # Collect debug info
   docker stats midnight-proof-server --no-stream > memory-stats.txt
   docker logs midnight-proof-server > server-logs.txt
   
   # Report to GitHub
   gh issue create \
     --title "Memory leak in proof server v1.0.0" \
     --body "Attach memory-stats.txt and server-logs.txt"
   ```

---

## Network Issues

### Issue 8: Connection Refused

**Symptom:** `curl: (7) Failed to connect to proof.midnight.network port 443: Connection refused`

**Diagnosis:**
```bash
# 1. Check if server is listening
ssh into-vm "netstat -tulpn | grep 6300"

# 2. Check firewall rules
# AWS
aws ec2 describe-security-groups \
  --group-ids sg-12345678 \
  --query 'SecurityGroups[0].IpPermissions'

# GCP
gcloud compute firewall-rules list \
  --filter="name:midnight*"

# Azure
az network nsg rule list \
  --resource-group midnight-proof-server-rg \
  --nsg-name midnight-nsg \
  --output table

# 3. Check load balancer
# Verify backend health (cloud-specific commands)
```

**Solutions:**

1. **Server Not Listening**
   ```bash
   # Check Docker container
   docker ps | grep midnight-proof-server
   
   # If not running, start it
   docker start midnight-proof-server
   
   # Check port mapping
   docker port midnight-proof-server
   # Should show: 6300/tcp -> 0.0.0.0:6300
   ```

2. **Firewall Blocking**
   ```bash
   # AWS: Allow HTTPS (443)
   aws ec2 authorize-security-group-ingress \
     --group-id sg-12345678 \
     --protocol tcp \
     --port 443 \
     --cidr 0.0.0.0/0
   
   # GCP: Allow HTTPS (443)
   gcloud compute firewall-rules create allow-https \
     --network=midnight-vpc \
     --allow=tcp:443 \
     --source-ranges=0.0.0.0/0
   
   # Azure: Allow HTTPS (443)
   az network nsg rule create \
     --resource-group midnight-proof-server-rg \
     --nsg-name midnight-nsg \
     --name AllowHTTPS \
     --priority 100 \
     --source-address-prefixes '*' \
     --destination-port-ranges 443 \
     --access Allow \
     --protocol Tcp
   ```

3. **Load Balancer Unhealthy**
   ```bash
   # Check backend health
   # If unhealthy, verify health endpoint works
   ssh into-vm "curl http://localhost:6300/health"
   
   # If health endpoint fails, check server logs
   docker logs midnight-proof-server | grep "/health"
   ```

---

### Issue 9: SSL/TLS Errors

**Symptom:** `curl: (60) SSL certificate problem: certificate has expired`

**Diagnosis:**
```bash
# Check certificate expiration
echo | openssl s_client -servername proof.midnight.network \
  -connect proof.midnight.network:443 2>/dev/null | \
  openssl x509 -noout -dates

# Check certificate chain
echo | openssl s_client -servername proof.midnight.network \
  -connect proof.midnight.network:443 2>/dev/null | \
  openssl x509 -noout -issuer -subject
```

**Solutions:**

1. **Certificate Expired**
   ```bash
   # Renew Let's Encrypt certificate
   sudo certbot renew
   
   # Or regenerate self-signed (testing only)
   openssl req -x509 -nodes -days 365 -newkey rsa:2048 \
     -keyout selfsigned.key \
     -out selfsigned.crt \
     -subj "/CN=proof.midnight.network"
   
   # Upload to load balancer (cloud-specific)
   ```

2. **Certificate Chain Incomplete**
   ```bash
   # Ensure fullchain.pem is used, not just cert.pem
   sudo cat /etc/letsencrypt/live/proof.midnight.network/fullchain.pem
   
   # Should include:
   # 1. Server certificate
   # 2. Intermediate certificate(s)
   # 3. Root certificate (optional but recommended)
   ```

3. **Wrong Certificate Installed**
   ```bash
   # Verify certificate matches domain
   echo | openssl s_client -servername proof.midnight.network \
     -connect proof.midnight.network:443 2>/dev/null | \
     openssl x509 -noout -text | grep "Subject:"
   
   # Should show: CN=proof.midnight.network
   ```

---

## Security Issues

### Issue 10: Unauthorized Access

**Symptom:** API calls succeed without valid API key

**Diagnosis:**
```bash
# Test with no API key
curl -X POST https://proof.midnight.network/prove \
  -H "Content-Type: application/octet-stream" \
  --data-binary @zkir.bin

# Should return 401 Unauthorized
# If returns 200 OK, authentication is disabled!
```

**Solutions:**

1. **Authentication Disabled**
   ```bash
   # Check container environment
   docker inspect midnight-proof-server \
     --format='{{range .Config.Env}}{{println .}}{{end}}' | \
     grep DISABLE_AUTH
   
   # If MIDNIGHT_PROOF_SERVER_DISABLE_AUTH=true:
   # DANGER! Authentication is disabled!
   
   # Fix: Enable authentication
   docker run -d \
     -e MIDNIGHT_PROOF_SERVER_API_KEY="your-secure-key" \
     -e MIDNIGHT_PROOF_SERVER_DISABLE_AUTH=false \
     midnight-proof-server:latest
   ```

2. **Weak API Key**
   ```bash
   # Check API key strength
   # Should be at least 32 bytes of random data
   
   # Generate strong API key
   openssl rand -base64 32
   
   # Update server
   docker stop midnight-proof-server
   docker rm midnight-proof-server
   docker run -d \
     -e MIDNIGHT_PROOF_SERVER_API_KEY="$(openssl rand -base64 32)" \
     midnight-proof-server:latest
   ```

3. **API Key Leaked**
   ```bash
   # Rotate API key immediately
   NEW_KEY=$(openssl rand -base64 32)
   
   # Update server
   docker run -d \
     -e MIDNIGHT_PROOF_SERVER_API_KEY="$NEW_KEY" \
     midnight-proof-server:latest
   
   # Notify all wallet developers of new key
   ```

---

### Issue 11: Rate Limiting Not Working

**Symptom:** Single IP can send unlimited requests

**Diagnosis:**
```bash
# Send 100 requests rapidly
for i in {1..100}; do
  curl -s https://proof.midnight.network/health &
done
wait

# Should see 429 Too Many Requests after ~10 requests
# If all return 200, rate limiting is not working
```

**Solutions:**

1. **Rate Limit Not Configured**
   ```bash
   # Check rate limit setting
   docker inspect midnight-proof-server \
     --format='{{range .Config.Env}}{{println .}}{{end}}' | \
     grep RATE_LIMIT
   
   # Set rate limit (e.g., 10 req/s per IP)
   docker run -d \
     -e MIDNIGHT_PROOF_SERVER_RATE_LIMIT=10 \
     midnight-proof-server:latest
   ```

2. **Behind Load Balancer (Wrong IP)**
   ```bash
   # If behind load balancer, server sees LB IP, not client IP
   
   # Solution: Configure LB to pass X-Forwarded-For header
   # Then update server to use X-Forwarded-For for rate limiting
   
   # AWS ALB: Automatic
   # GCP LB: Automatic
   # Azure AppGW: Configure in HTTP settings
   ```

---

## Cloud-Specific Issues

### AWS Nitro Enclaves

#### Issue 12: Enclave Won't Start

**Symptom:** `nitro-cli run-enclave` fails

**Diagnosis:**
```bash
# Check enclave status
nitro-cli describe-enclaves

# Check enclave service
sudo systemctl status nitro-enclaves-allocator.service

# Check dmesg for errors
sudo dmesg | grep -i nitro
```

**Common Errors:**

1. **Insufficient Memory**
   ```
   Error: Insufficient memory available
   ```

   **Solution:**
   ```bash
   # Check available enclave memory
   cat /sys/module/nitro_enclaves/parameters/ne_enclave_memory_mb
   
   # Increase allocator memory (requires reboot)
   sudo sed -i 's/memory_mib: [0-9]*/memory_mib: 32768/' \
     /etc/nitro_enclaves/allocator.yaml
   sudo reboot
   ```

2. **Insufficient CPUs**
   ```
   Error: Insufficient CPUs available
   ```

   **Solution:**
   ```bash
   # Check available enclave CPUs
   cat /sys/module/nitro_enclaves/parameters/ne_enclave_cpu_pool
   
   # Increase allocator CPUs
   sudo sed -i 's/cpu_count: [0-9]*/cpu_count: 8/' \
     /etc/nitro_enclaves/allocator.yaml
   sudo reboot
   ```

3. **Invalid EIF File**
   ```
   Error: Invalid EIF file format
   ```

   **Solution:**
   ```bash
   # Verify EIF file
   nitro-cli describe-eif --eif-path midnight-proof-server.eif
   
   # If corrupted, rebuild
   nitro-cli build-enclave \
     --docker-uri midnight-proof-server:latest \
     --output-file midnight-proof-server.eif
   ```

---

### GCP Confidential VMs

#### Issue 13: Cloud-Init Fails

**Symptom:** Docker container doesn't start after VM boot

**Diagnosis:**
```bash
# Check cloud-init status
gcloud compute ssh midnight-proof-server --zone=us-central1-a \
  --command="cloud-init status --wait"

# Check cloud-init logs
gcloud compute ssh midnight-proof-server --zone=us-central1-a \
  --command="sudo cat /var/log/cloud-init-output.log"
```

**Common Issues:**

1. **Script Errors**
   ```bash
   # Check for errors in cloud-init script
   gcloud compute ssh midnight-proof-server --zone=us-central1-a \
     --command="sudo grep -i error /var/log/cloud-init-output.log"
   
   # Fix script and recreate VM
   ```

2. **Permissions**
   ```bash
   # Cloud-init runs as root, but Docker commands need permissions
   
   # Fix: Add azureuser to docker group in cloud-init
   # runcmd:
   #   - usermod -aG docker azureuser
   ```

---

### Azure Confidential VMs

#### Issue 14: Managed Identity Permissions

**Symptom:** VM can't access Key Vault or ACR

**Diagnosis:**
```bash
# Check managed identity
az vm identity show \
  --resource-group midnight-proof-server-rg \
  --name midnight-proof-server

# Test Key Vault access
az vm run-command invoke \
  --resource-group midnight-proof-server-rg \
  --name midnight-proof-server \
  --command-id RunShellScript \
  --scripts "az keyvault secret show --vault-name midnight-kv --name midnight-api-key"
```

**Solutions:**

1. **Identity Not Assigned**
   ```bash
   # Assign managed identity
   az vm identity assign \
     --resource-group midnight-proof-server-rg \
     --name midnight-proof-server
   
   # Get principal ID
   PRINCIPAL_ID=$(az vm identity show \
     --resource-group midnight-proof-server-rg \
     --name midnight-proof-server \
     --query principalId -o tsv)
   ```

2. **Missing Permissions**
   ```bash
   # Grant Key Vault access
   az keyvault set-policy \
     --name midnight-kv \
     --object-id $PRINCIPAL_ID \
     --secret-permissions get list
   
   # Grant ACR pull access
   ACR_ID=$(az acr show --name midnightacr --query id -o tsv)
   az role assignment create \
     --assignee $PRINCIPAL_ID \
     --role AcrPull \
     --scope $ACR_ID
   ```

---

## Debugging Tools

### Essential Commands

```bash
# 1. Check server health
curl https://proof.midnight.network/health

# 2. Check server readiness + queue stats
curl https://proof.midnight.network/ready

# 3. Check server version
curl https://proof.midnight.network/version

# 4. Check supported proof versions
curl https://proof.midnight.network/proof-versions

# 5. Test proof generation (requires API key and valid ZKIR)
curl -X POST https://proof.midnight.network/prove \
  -H "X-API-Key: your-api-key" \
  -H "Content-Type: application/octet-stream" \
  --data-binary @zkir.bin \
  -o proof.bin \
  -w "\nHTTP Status: %{http_code}\nTime: %{time_total}s\n"

# 6. Check Docker container status
docker ps -a | grep midnight-proof-server

# 7. View Docker logs
docker logs midnight-proof-server --tail=100 --follow

# 8. Check container resource usage
docker stats midnight-proof-server --no-stream

# 9. Execute command in container
docker exec -it midnight-proof-server bash

# 10. Inspect container config
docker inspect midnight-proof-server | jq
```

### Log Analysis

```bash
# Parse JSON logs
docker logs midnight-proof-server 2>&1 | \
  grep -v "GET /health" | \
  jq 'select(.level == "ERROR")'

# Count errors by type
docker logs midnight-proof-server 2>&1 | \
  jq -r '.message' | \
  sort | uniq -c | sort -rn

# Find slow requests (>5 seconds)
docker logs midnight-proof-server 2>&1 | \
  jq 'select(.duration_ms > 5000)'
```

### Performance Profiling

```bash
# Install perf tools (on VM)
sudo apt-get install linux-tools-generic

# Profile CPU usage
sudo perf record -p $(pgrep midnight-proof) -g -- sleep 30
sudo perf report

# Memory profiling (requires valgrind)
valgrind --tool=massif ./midnight-proof-server-prototype

# Network profiling
sudo tcpdump -i any -w capture.pcap 'port 6300'
wireshark capture.pcap
```

---

## Getting Help

### Before Opening an Issue

1. ✅ Run the diagnostic script at the top of this guide
2. ✅ Check this troubleshooting guide for your issue
3. ✅ Search existing GitHub issues
4. ✅ Collect relevant logs and diagnostic output

### Opening a GitHub Issue

```bash
# Use the GitHub CLI
gh issue create \
  --title "Brief description of issue" \
  --body "
## Environment

- Cloud Provider: AWS / GCP / Azure
- Instance Type: c5.2xlarge / n2d-standard-8 / DC4s_v3
- Proof Server Version: v1.0.0
- Docker Version: $(docker --version)
- OS: $(uname -a)

## Issue Description

Detailed description of the problem...

## Steps to Reproduce

1. Step 1
2. Step 2
3. Step 3

## Expected Behavior

What you expected to happen...

## Actual Behavior

What actually happened...

## Diagnostic Output

\`\`\`
# Output from ./diagnose.sh
...
\`\`\`

## Logs

\`\`\`
# Docker logs
$(docker logs midnight-proof-server --tail=100)
\`\`\`
"
```

### Community Support

- **GitHub Discussions**: https://github.com/midnight/proof-server/discussions
- **Discord**: https://discord.gg/midnight
- **Email**: support@midnight.network

### Priority Issues

For security-critical issues:
- **Email**: security@midnight.network
- **PGP Key**: [midnight-pgp-public.asc](https://github.com/midnight/proof-server/releases)
- **DO NOT** open public issues for security vulnerabilities

---

## Summary

### Most Common Issues

1. ❌ **Authentication disabled** → Enable with API key
2. ❌ **Firewall blocking** → Open port 443 in security group/NSG
3. ❌ **Debug mode enabled** → Rebuild with debug-mode=false
4. ❌ **PCR mismatch** → Deploy correct version
5. ❌ **Out of memory** → Increase memory allocation
6. ❌ **Worker pool exhausted** → Increase worker count
7. ❌ **SSL certificate expired** → Renew certificate
8. ❌ **Cloud-init failed** → Check startup script logs
9. ❌ **Managed identity missing** → Assign identity and grant permissions
10. ❌ **Rate limiting not working** → Configure X-Forwarded-For

### Quick Fixes

```bash
# Restart proof server
docker restart midnight-proof-server

# Increase memory
docker run -d --memory=32g midnight-proof-server:latest

# Increase workers
docker run -d -e MIDNIGHT_PROOF_SERVER_NUM_WORKERS=16 midnight-proof-server:latest

# Enable authentication
docker run -d -e MIDNIGHT_PROOF_SERVER_API_KEY="$(openssl rand -base64 32)" midnight-proof-server:latest

# Update to latest version
docker pull midnight-proof-server:latest
docker stop midnight-proof-server && docker rm midnight-proof-server
docker run -d midnight-proof-server:latest
```

---

