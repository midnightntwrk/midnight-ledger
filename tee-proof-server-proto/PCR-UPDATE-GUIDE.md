# PCR Measurements Update Guide

## Overview

PCR (Platform Configuration Register) measurements are cryptographic hashes that uniquely identify your enclave configuration. Clients verify these to ensure they're connecting to the correct, unmodified proof server.

## When PCR Values Change

- **PCR0**: Changes whenever the application code or Docker image changes
- **PCR1**: Changes if the Linux kernel or initrd changes (rare)
- **PCR2**: Changes if CPU count or memory allocation changes

## Updating PCR Measurements

### Step 1: Build New Version

```bash
# Build Docker image
docker build -f tee-proof-server-proto/Dockerfile -t midnight/proof-server:v6.3.2 .

# Build EIF and capture PCR values
nitro-cli build-enclave \
  --docker-uri midnight/proof-server:v6.3.2 \
  --output-file proof-server-v6.3.2.eif | tee build-output.txt

# Extract PCRs
grep -A 5 "Measurements" build-output.txt
```

### Step 2: Update PCR File

Edit `tee-proof-server-proto/pcr-measurements.json` with new values:

```json
{
  "version": "v6.3.2",
  "buildDate": "2026-01-08",
  "measurements": {
    "hashAlgorithm": "SHA384",
    "pcr0": {
      "value": "NEW_PCR0_VALUE_HERE",
      "description": "Enclave image file - uniquely identifies the Docker image and application code"
    },
    "pcr1": {
      "value": "NEW_PCR1_VALUE_HERE",
      "description": "Kernel and boot ramfs - verifies the Linux kernel and initrd"
    },
    "pcr2": {
      "value": "NEW_PCR2_VALUE_HERE",
      "description": "Application vCPUs and memory - verifies CPU and memory configuration"
    }
  },
  "buildInfo": {
    "dockerImage": "midnight/proof-server:v6.3.2",
    "eifFile": "proof-server-v6.3.2.eif",
    "cpuCount": 2,
    "memoryMB": 4096
  },
  "publishedAt": "2026-01-08T12:00:00Z"
}
```

### Step 3: Rebuild with New PCRs

The PCR file is embedded at compile time, so you must rebuild:

```bash
# Rebuild Docker image (PCR file is included)
docker build -f tee-proof-server-proto/Dockerfile -t midnight/proof-server:v6.3.2 .

# Rebuild EIF
nitro-cli build-enclave \
  --docker-uri midnight/proof-server:v6.3.2 \
  --output-file proof-server-v6.3.2.eif

# IMPORTANT: Verify PCRs match what you put in the JSON file!
```

### Step 4: Deploy

```bash
cd tee-proof-server-proto
VERSION=v6.3.2 ./deploy-nitro-enclave.sh
```

### Step 5: Verify

```bash
# Check PCR endpoint returns new values
curl https://proof-test.devnet.midnight.network/pcr | jq '.measurements'

# Test attestation
curl 'https://proof-test.devnet.midnight.network/attestation?nonce=test' | jq '.'
```

## Important Notes

### Reproducible Builds

For maximum trust, PCR values should be reproducible:
- Same source code → Same PCR0
- Anyone can rebuild and verify PCR values match

Document:
- Git commit SHA used for build
- Docker version
- Build commands
- Build date/time

### Client Migration Strategy

When updating PCRs:

1. **Announce update** - Give clients advance notice
2. **Deploy with grace period** - Accept both old and new PCRs temporarily
3. **Monitor** - Watch for clients still using old version
4. **Deprecate** - After grace period, reject old PCRs

### Versioning

Use semantic versioning:
- **Major** (v7.0.0): Breaking changes, incompatible API
- **Minor** (v6.4.0): New features, backward compatible
- **Patch** (v6.3.2): Bug fixes, backward compatible

PCR0 changes on any code update, but maintain API compatibility where possible.

## Automation

The deployment script automatically extracts and saves PCRs to:
```
pcr-measurements-<version>.json
```

You can use this to update the embedded file.

## Troubleshooting

### PCR Mismatch

If clients report PCR mismatch:
1. Verify deployed version matches published PCRs
2. Check if multiple versions are running
3. Ensure clients fetched latest PCR file

### PCR File Not Found

If `/pcr` endpoint returns 500:
1. Check PCR file exists: `tee-proof-server-proto/pcr-measurements.json`
2. Verify JSON is valid: `jq . pcr-measurements.json`
3. Rebuild Docker image (file is embedded at compile time)

### Wrong PCR Values

If PCR endpoint shows wrong values:
1. The PCR file is embedded at compile time
2. You must rebuild the Docker image after updating the JSON file
3. Rebuild the EIF from the new Docker image
4. Redeploy

## Security Best Practices

1. **Sign PCR publications** - Use GPG to sign the PCR JSON file
2. **Multiple channels** - Publish PCRs in multiple places (GitHub, website, etc.)
3. **Audit log** - Keep records of all PCR updates with justification
4. **Staged rollout** - Test on staging environment first
5. **Rollback plan** - Keep previous EIF file for quick rollback

## Example Workflow

```bash
# 1. Make code changes
git checkout -b feature/new-feature
# ... make changes ...
git commit -m "Add new feature"

# 2. Build and extract PCRs
docker build -f tee-proof-server-proto/Dockerfile -t midnight/proof-server:v6.3.2 .
nitro-cli build-enclave --docker-uri midnight/proof-server:v6.3.2 \
  --output-file proof-server-v6.3.2.eif | tee build-output.txt

# 3. Update PCR file with new values
nano tee-proof-server-proto/pcr-measurements.json

# 4. Rebuild (to embed new PCR file)
docker build -f tee-proof-server-proto/Dockerfile -t midnight/proof-server:v6.3.2 .

# 5. Build final EIF (PCRs should match JSON now)
nitro-cli build-enclave --docker-uri midnight/proof-server:v6.3.2 \
  --output-file proof-server-v6.3.2.eif

# 6. Commit PCR file
git add tee-proof-server-proto/pcr-measurements.json
git commit -m "Update PCR measurements for v6.3.2"
git push

# 7. Deploy
./deploy-nitro-enclave.sh

# 8. Verify
curl https://proof-test.devnet.midnight.network/pcr | jq '.'
```
