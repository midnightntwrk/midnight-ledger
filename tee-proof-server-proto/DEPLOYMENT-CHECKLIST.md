# AWS Nitro Deployment Checklist ✅

Quick checklist to deploy the Midnight TEE Proof Server to AWS Nitro Enclaves.

## Pre-Deployment ✅

- [ ] Docker image built locally: `midnight/proof-server:latest`
- [ ] AWS account with EC2 permissions
- [ ] AWS CLI configured
- [ ] SSH key pair created

## EC2 Instance Setup ✅

- [ ] Launch Nitro-enabled instance (c6i.2xlarge or similar)
- [ ] **Enable Enclave support** when launching
- [ ] Configure Security Group:
  - [ ] Port 22 (SSH) from your IP
  - [ ] Port 6300 (Proof Server)
- [ ] Allocate at least 4 vCPUs and 8GB RAM
- [ ] Connect via SSH

## Software Installation ✅

```bash
# On EC2 instance
- [ ] Install Nitro CLI
  sudo amazon-linux-extras install aws-nitro-enclaves-cli -y
  sudo yum install aws-nitro-enclaves-cli-devel -y

- [ ] Install Docker
  sudo yum install docker -y
  sudo systemctl enable --now docker

- [ ] Configure enclave resources
  sudo nano /etc/nitro_enclaves/allocator.yaml
  # Set: cpu_count: 2, memory_mib: 4096

- [ ] Add user to groups
  sudo usermod -aG ne ec2-user
  sudo usermod -aG docker ec2-user

- [ ] Log out and back in
  exit
```

## Get Docker Image to EC2 ✅

Choose ONE method:

### Method A: Build on EC2 (Recommended)
```bash
- [ ] Clone repository
  git clone https://github.com/your-org/midnight-code.git
  cd midnight-code/midnight-ledger/tee-proof-server-proto

- [ ] Build and deploy
  ./scripts/aws-nitro-deploy.sh --build
```

### Method B: Transfer from Local
```bash
# On local machine
- [ ] Save image
  docker save midnight/proof-server:latest | gzip > midnight-proof-server.tar.gz

- [ ] Transfer to EC2
  scp -i your-key.pem midnight-proof-server.tar.gz ec2-user@YOUR_IP:~/

# On EC2
- [ ] Load image
  gunzip midnight-proof-server.tar.gz
  docker load < midnight-proof-server.tar
```

### Method C: Use Registry
```bash
# On local machine
- [ ] Push to registry
  docker tag midnight/proof-server:latest YOUR_REGISTRY/midnight-proof-server:latest
  docker push YOUR_REGISTRY/midnight-proof-server:latest

# On EC2
- [ ] Pull from registry
  docker pull YOUR_REGISTRY/midnight-proof-server:latest
  docker tag YOUR_REGISTRY/midnight-proof-server:latest midnight/proof-server:latest
```

## Deploy to Nitro ✅

```bash
- [ ] Run deployment script
  cd midnight-code/midnight-ledger/tee-proof-server-proto
  ./scripts/aws-nitro-deploy.sh

- [ ] Verify enclave is running
  nitro-cli describe-enclaves

- [ ] Note the Enclave ID
  ENCLAVE_ID=_______________
```

## Set Up Networking ✅

```bash
- [ ] Install vsock-proxy
  sudo yum install -y vsock-proxy

- [ ] Start vsock proxy
  vsock-proxy 6300 vsock://16:6300 &

- [ ] Create systemd service (optional)
  sudo tee /etc/systemd/system/proof-server-proxy.service << 'EOT'
[Unit]
Description=Midnight Proof Server vsock Proxy
After=network.target

[Service]
Type=simple
User=ec2-user
ExecStart=/usr/bin/vsock-proxy 6300 vsock://16:6300
Restart=always

[Install]
WantedBy=multi-user.target
EOT

  sudo systemctl daemon-reload
  sudo systemctl enable proof-server-proxy
  sudo systemctl start proof-server-proxy
```

## Verification ✅

```bash
# On EC2 instance
- [ ] Test locally
  curl http://localhost:6300/health
  curl http://localhost:6300/version
  curl http://localhost:6300/attestation

# From your local machine
- [ ] Test remotely
  curl http://YOUR_INSTANCE_IP:6300/health
  curl http://YOUR_INSTANCE_IP:6300/version

- [ ] View logs
  nitro-cli console --enclave-id $ENCLAVE_ID
```

## Production Configuration ✅

```bash
- [ ] Enable authentication
  # Edit before deployment:
  export MIDNIGHT_PROOF_SERVER_DISABLE_AUTH=false
  export MIDNIGHT_PROOF_SERVER_API_KEY=your-secure-key

- [ ] Configure monitoring
  # Set up CloudWatch logs

- [ ] Configure auto-restart
  # Create systemd service for enclave

- [ ] Document PCR values
  nitro-cli describe-eif --eif-path proof-server.eif | jq '.Measurements' > pcr-values.json

- [ ] Update Security Group
  # Restrict to known IPs only
```

## Post-Deployment ✅

- [ ] Test with real transactions
- [ ] Monitor CPU/memory usage
- [ ] Set up alerting for health checks
- [ ] Document connection details for clients
- [ ] Schedule regular updates
- [ ] Configure backups (if needed)

## Troubleshooting Reference

| Issue | Command |
|-------|---------|
| Check enclave status | `nitro-cli describe-enclaves` |
| View enclave logs | `nitro-cli console --enclave-id <ID>` |
| Check vsock proxy | `ps aux \| grep vsock-proxy` |
| Check port | `sudo netstat -tulpn \| grep 6300` |
| Restart enclave | `nitro-cli terminate-enclave --enclave-id <ID>` then redeploy |

## Quick Commands

```bash
# View logs in real-time
nitro-cli console --enclave-id $(nitro-cli describe-enclaves | jq -r '.[0].EnclaveID')

# Restart everything
ENCLAVE_ID=$(nitro-cli describe-enclaves | jq -r '.[0].EnclaveID')
nitro-cli terminate-enclave --enclave-id $ENCLAVE_ID
./scripts/aws-nitro-deploy.sh

# Test health
curl http://localhost:6300/health && echo "✅ Working" || echo "❌ Failed"
```

## Success Criteria ✅

Your deployment is successful when:

- ✅ `nitro-cli describe-enclaves` shows running enclave
- ✅ `curl http://localhost:6300/health` returns `{"status":"ok",...}`
- ✅ `curl http://localhost:6300/version` returns `6.2.0-alpha.1`
- ✅ `curl http://localhost:6300/attestation` returns attestation document
- ✅ Remote access works from your machine
- ✅ Enclave console shows no errors

---

**Current Status**: [ ] Not Started / [ ] In Progress / [ ] Complete

**Deployment Date**: _______________

**Instance ID**: _______________

**Instance IP**: _______________

**Enclave ID**: _______________

**Notes**:
