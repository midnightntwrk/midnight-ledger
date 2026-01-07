# Nitro Enclave Proof Server Deployment Guide

## Prerequisites

- Docker installed on build machine
- AWS account with Nitro Enclaves enabled EC2 instance
- Git access to the repository
- `nitro-cli` installed on EC2 instance

## Quick Deployment Workflow

### On Your Local Development Machine

1. **Commit and Push Changes to GitHub**

```bash
cd /path/to/midnight-ledger

# Check what files changed
git status

# Add the modified files
git add tee-proof-server-proto/proof-server/src/main.rs
git add tee-proof-server-proto/Dockerfile

# Commit with descriptive message
git commit -m "Fix Nitro Enclave networking: bind proof server to 127.0.0.1 for vsock bridge"

# Push to GitHub (replace 'main' with your branch name if different)
git push origin feature/proof-server-tls-and-attestation
```

### On Your AWS EC2 Instance (Remote System)

2. **Pull Latest Changes**

```bash
cd /home/ssm-user/midnight-ledger

# Fetch latest changes
git fetch origin

# Pull the updates (replace branch name if different)
git pull origin feature/proof-server-tls-and-attestation

# Verify the changes
git log -1 --stat
```

3. **Build Docker Image**

```bash
cd /home/ssm-user/midnight-ledger

# Build the Docker image
docker build -f tee-proof-server-proto/Dockerfile -t midnight/proof-server:v6.3.1 .
```

4. **Convert to Nitro Enclave Image Format (EIF)**

```bash
# Create EIF from Docker image
nitro-cli build-enclave \
  --docker-uri midnight/proof-server:v6.3.1 \
  --output-file proof-server-v6.3.1.eif

# This will output PCR measurements - save these for attestation verification!
```

5. **Deploy the Enclave**

```bash
# Stop any existing enclave
nitro-cli terminate-enclave --all

# Start the new enclave
nitro-cli run-enclave \
  --eif-path proof-server-v6.3.1.eif \
  --cpu-count 2 \
  --memory 4096 \
  --enclave-cid 16 \
  --debug-mode

# Note: Remove --debug-mode for production deployments
```

6. **Set Up Vsock Proxy on Parent Instance**

```bash
# Kill any existing socat processes
sudo pkill -f socat

# Start vsock proxy (bridges localhost:6300 to enclave vsock)
sudo socat TCP-LISTEN:6300,reuseaddr,fork VSOCK-CONNECT:16:6300 &

# Verify it's running
ps aux | grep socat
```

7. **Test the Deployment**

```bash
# Test health endpoint
curl http://localhost:6300/health

# Test attestation endpoint
curl "http://localhost:6300/attestation?nonce=test123"

# You should get a JSON response with attestation document!
```

## Automated Deployment Script

See `deploy-nitro-enclave.sh` for automated deployment.

## Troubleshooting

### Check Enclave Console (Debug Mode Only)

```bash
# Get enclave ID
nitro-cli describe-enclaves

# View console output
nitro-cli console --enclave-id <ENCLAVE_ID>
```

### Check Enclave Status

```bash
nitro-cli describe-enclaves
```

### Check Vsock Proxy

```bash
sudo ss -tlnp | grep 6300
ps aux | grep socat
```

### View Nitro Enclave Logs

```bash
sudo tail -f /var/log/nitro_enclaves/nitro_enclaves.log
```

## Production Deployment Notes

1. **Remove Debug Mode**: Don't use `--debug-mode` in production
2. **Enable Authentication**: Set API keys via environment variables
3. **Publish PCR Values**: Save and publish PCR measurements for client verification
4. **Monitor Enclave**: Set up CloudWatch monitoring for enclave health
5. **Automate Startup**: Create systemd service for vsock proxy

## PCR Measurements

When building the EIF, save the PCR measurements output:

```
PCR0: <hash> - Enclave Image
PCR1: <hash> - Kernel + Boot
PCR2: <hash> - Application + Runtime
```

Clients use these to verify they're connecting to the correct enclave.
