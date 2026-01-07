# Quick Fix: Enable vsock Communication in Nitro Enclave

## Problem

The proof server runs inside the Nitro Enclave but cannot be reached via vsock because it binds to TCP (0.0.0.0:6300) instead of vsock. Nitro Enclaves require explicit vsock listeners.

## Solution

Add `socat` inside the enclave to bridge vsock → TCP.

## Implementation Steps

### 1. Update Dockerfile

Edit `/Users/robertblessing-hartley/code/midnight-code/midnight-ledger/tee-proof-server-proto/Dockerfile`:

**Add socat to runtime dependencies** (line ~100):
```dockerfile
# Install runtime dependencies (curl for health checks)
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    curl \
    socat \
    && rm -rf /var/lib/apt/lists/*
```

**Create startup script** (add before CMD, around line 135):
```dockerfile
# Create startup script for vsock bridge
RUN cat > /app/start.sh << 'EOF'
#!/bin/bash
set -e

echo "[$(date)] Starting Midnight Proof Server with vsock bridge..."

# Start proof server in background
/app/proof-server --no-fetch-params &
PROOF_SERVER_PID=$!
echo "[$(date)] Proof server started (PID: $PROOF_SERVER_PID)"

# Give proof server time to start
sleep 5

# Start socat to bridge vsock → TCP
# Listen on vsock port 6300, forward to localhost:6300
echo "[$(date)] Starting socat bridge: vsock:6300 → localhost:6300"
socat VSOCK-LISTEN:6300,fork TCP:127.0.0.1:6300 &
SOCAT_PID=$!
echo "[$(date)] Socat bridge started (PID: $SOCAT_PID)"

echo "[$(date)] Proof server ready and accessible via vsock"

# Wait for either process to exit
wait -n
EXIT_CODE=$?

# If either exits, kill the other and exit
echo "[$(date)] Process exited with code $EXIT_CODE, shutting down..."
kill $PROOF_SERVER_PID $SOCAT_PID 2>/dev/null
exit $EXIT_CODE
EOF

RUN chmod +x /app/start.sh
```

**Update CMD** (replace existing CMD at line 159):
```dockerfile
# Run the startup script (includes proof server + vsock bridge)
CMD ["/app/start.sh"]
```

### 2. Build Updated Docker Image

```bash
cd /Users/robertblessing-hartley/code/midnight-code/midnight-ledger

docker build -f tee-proof-server-proto/Dockerfile -t midnight/proof-server:latest .
```

### 3. Test Locally (Optional but Recommended)

```bash
# Test that Docker image still works normally (without enclave)
docker run --rm -p 6300:6300 midnight/proof-server:latest

# In another terminal:
curl http://localhost:6300/health

# Should see proof server responding
# Note: socat will fail to start vsock (no vsock outside enclave), but that's expected
# The proof server itself should still work on TCP
```

### 4. Build Enclave Image File (EIF)

```bash
nitro-cli build-enclave \
  --docker-uri midnight/proof-server:latest \
  --output-file midnight-proof-server.eif

# Save the PCR measurements from the output!
```

### 5. Deploy to Nitro Enclave

```bash
# Stop any running enclave
nitro-cli describe-enclaves
nitro-cli terminate-enclave --enclave-id <id-if-running>

# Start new enclave
nitro-cli run-enclave \
  --eif-path midnight-proof-server.eif \
  --cpu-count 2 \
  --memory 4096 \
  --enclave-cid 16 \
  --debug-mode

# Check logs
nitro-cli console --enclave-id <new-id>

# Should see:
# [timestamp] Starting Midnight Proof Server with vsock bridge...
# [timestamp] Proof server started (PID: X)
# [timestamp] Starting socat bridge: vsock:6300 → localhost:6300
# [timestamp] Socat bridge started (PID: Y)
# [timestamp] Proof server ready and accessible via vsock
```

### 6. Configure Parent Instance Proxy

**Option A: Quick Test (Foreground)**
```bash
# On parent EC2 instance
socat TCP-LISTEN:6300,reuseaddr,fork VSOCK-CONNECT:16:6300
```

**Option B: Production (systemd service)**
```bash
sudo tee /etc/systemd/system/proof-server-vsock-proxy.service <<EOF
[Unit]
Description=vsock proxy for Nitro Enclave proof server
After=network.target

[Service]
Type=simple
User=ec2-user
ExecStart=/usr/bin/socat TCP-LISTEN:6300,reuseaddr,fork VSOCK-CONNECT:16:6300
Restart=always
RestartSec=5

[Install]
WantedBy=multi-user.target
EOF

sudo systemctl daemon-reload
sudo systemctl enable proof-server-vsock-proxy
sudo systemctl start proof-server-vsock-proxy
sudo systemctl status proof-server-vsock-proxy
```

### 7. Test End-to-End

```bash
# From parent EC2 instance:
curl http://localhost:6300/health

# Expected: {"status":"healthy"} or similar

# Test attestation endpoint:
curl "http://localhost:6300/attestation?nonce=test123"

# Expected: JSON response with platform detection

# Check that the proxy is listening:
sudo ss -tulpn | grep 6300
# Should show socat listening on TCP 0.0.0.0:6300
```

### 8. Verify Public Endpoint (If Configured)

```bash
# From anywhere:
curl https://proof-test.devnet.midnight.network/health

# Should now return 200 OK instead of 502 Bad Gateway
```

---

## Troubleshooting

### Enclave starts but no console output
- Check that enclave was started with `--debug-mode`
- Some output may be delayed - wait 10-15 seconds

### "Connection refused" from parent
- Verify enclave is RUNNING: `nitro-cli describe-enclaves`
- Check enclave CID matches (should be 16)
- Verify socat started inside enclave: `nitro-cli console --enclave-id <id>`

### Proof server not starting inside enclave
- Check if parameters are causing issues
- Verify `--no-fetch-params` is being used
- Check memory allocation (4096 MB should be sufficient)

### Parent proxy not working
- Check if port 6300 is already in use: `sudo ss -tulpn | grep 6300`
- Verify socat is installed: `which socat`
- Check systemd service logs: `sudo journalctl -u proof-server-vsock-proxy -f`

---

## What This Does

**Before**:
```
Parent tries to connect → [vsock CID 16:6300] → ❌ Nothing listening on vsock
                                                   (proof-server only on TCP)
```

**After**:
```
Parent TCP:6300 → [vsock CID 16:6300] → socat (vsock listener) → proof-server (TCP localhost:6300) ✅
```

The key insight: **Something inside the enclave must listen on vsock**. Our proof server listens on TCP, so we add `socat` to bridge vsock → TCP.

---

## Files Modified

1. `/Users/robertblessing-hartley/code/midnight-code/midnight-ledger/tee-proof-server-proto/Dockerfile`
   - Added `socat` to runtime dependencies
   - Created `/app/start.sh` startup script
   - Changed CMD to use startup script

---

## Next Steps After This Fix

Once this is working, you may want to:

1. **Remove debug mode** for production:
   ```bash
   nitro-cli run-enclave --eif-path midnight-proof-server.eif --cpu-count 2 --memory 4096 --enclave-cid 16
   # (removed --debug-mode)
   ```

2. **Configure ALB/TLS termination** at the parent or load balancer level

3. **Set up monitoring** for the enclave and proxy services

4. **Document PCR measurements** for attestation verification

5. **Automate deployment** with infrastructure as code (Terraform/CloudFormation)

---

## Related Documentation

- `tee-proof-server-proto/docs/nitro-enclave-networking-solutions.md` - Full research and options
- `tee-proof-server-proto/docs/nitro-enclave-learnings.md` - Troubleshooting history
- `tee-proof-server-proto/docs/update-nitro-deployment.md` - Deployment guide
