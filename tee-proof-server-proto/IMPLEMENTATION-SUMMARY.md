# PCR Endpoint Implementation Summary

## What Was Implemented

Added a PCR (Platform Configuration Register) measurements endpoint to the Midnight Proof Server to enable client-side attestation verification.

## Changes Made

### 1. New Endpoint: `/pcr` and `/.well-known/pcr-measurements.json`

**File**: `proof-server/src/lib.rs`

Added `pcr_measurements_handler()` function that:
- Serves PCR measurements as JSON
- Embeds the PCR file at compile time using `include_str!`
- Available at two URLs for flexibility:
  - `/.well-known/pcr-measurements.json` (standard location)
  - `/pcr` (convenience alias)

### 2. PCR Measurements File

**File**: `tee-proof-server-proto/pcr-measurements.json`

Contains:
- PCR0, PCR1, PCR2 values (SHA384 hashes)
- Build information (version, Docker image, CPU/memory config)
- Verification instructions for clients

### 3. Documentation

Created comprehensive guides:
- **PCR-UPDATE-GUIDE.md**: How to update PCR values when code changes
- **IMPLEMENTATION-SUMMARY.md**: This file
- Updated **DEPLOYMENT.md**: Added PCR endpoint testing

## How It Works

```
┌─────────────────────────────────────────────────┐
│ Client (Lace Wallet)                            │
│                                                 │
│ 1. Fetch expected PCRs from:                    │
│    GET /pcr                                     │
│                                                 │
│ 2. Request attestation document:                │
│    GET /attestation?nonce=xyz                   │
│                                                 │
│ 3. Verify attestation:                          │
│    - Certificate chain → AWS root cert          │
│    - PCRs in document → Expected PCRs           │
│    - Nonce matches                              │
│    - Timestamp is recent                        │
│                                                 │
│ 4. If verification passes:                      │
│    ✅ Trusted proof server                      │
│                                                 │
└─────────────────────────────────────────────────┘
```

## API Response Format

### GET `/pcr` or `/.well-known/pcr-measurements.json`

```json
{
  "version": "v6.3.1",
  "environment": "devnet",
  "description": "Midnight Proof Server - AWS Nitro Enclave PCR Measurements",
  "buildDate": "2026-01-07",
  "measurements": {
    "hashAlgorithm": "SHA384",
    "pcr0": {
      "value": "7a4a10...",
      "description": "Enclave image file - uniquely identifies the Docker image and application code"
    },
    "pcr1": {
      "value": "4b4d5b...",
      "description": "Kernel and boot ramfs - verifies the Linux kernel and initrd"
    },
    "pcr2": {
      "value": "f707d6...",
      "description": "Application vCPUs and memory - verifies CPU and memory configuration"
    }
  },
  "buildInfo": {
    "dockerImage": "midnight/proof-server:v6.3.1",
    "eifFile": "proof-server-v6.3.1.eif",
    "cpuCount": 2,
    "memoryMB": 4096,
    "reproducible": true
  },
  "publishedBy": "Midnight Foundation",
  "publishedAt": "2026-01-07T23:30:00Z"
}
```

## Testing

```bash
# Local testing (before deployment)
cargo build --package midnight-proof-server-prototype
cargo run --package midnight-proof-server-prototype -- --disable-auth --disable-tls

# Test PCR endpoint
curl http://localhost:6300/pcr | jq '.'

# After deployment to production
curl https://proof-test.devnet.midnight.network/pcr | jq '.'
curl https://proof-test.devnet.midnight.network/.well-known/pcr-measurements.json | jq '.'
```

## Client Integration

### For Lace Wallet

Configure Lace to:
1. Fetch PCRs from: `https://proof-test.devnet.midnight.network/pcr`
2. Compare PCRs in attestation document against fetched values
3. Show warning if PCRs don't match

Expected result: The "PCR verification skipped" warning disappears!

### For Custom Clients

```typescript
// 1. Fetch expected PCRs
const expectedPcrs = await fetch('https://proof-test.devnet.midnight.network/pcr')
  .then(r => r.json());

// 2. Get attestation
const attestation = await fetch('https://proof-test.devnet.midnight.network/attestation?nonce=xyz')
  .then(r => r.json());

// 3. Decode attestation document (CBOR)
const attestationDoc = decodeAttestationDocument(attestation.attestation);

// 4. Verify PCRs match
if (attestationDoc.pcrs.PCR0 === expectedPcrs.measurements.pcr0.value &&
    attestationDoc.pcrs.PCR1 === expectedPcrs.measurements.pcr1.value &&
    attestationDoc.pcrs.PCR2 === expectedPcrs.measurements.pcr2.value) {
  console.log('✅ PCR verification passed - trusted enclave');
} else {
  console.error('❌ PCR mismatch - untrusted or wrong version');
}
```

## Deployment Workflow

```bash
# 1. Update code
git checkout -b feature/my-changes
# make changes...
git commit -m "My changes"

# 2. Build and extract PCRs
docker build -f tee-proof-server-proto/Dockerfile -t midnight/proof-server:v6.3.2 .
nitro-cli build-enclave --docker-uri midnight/proof-server:v6.3.2 \
  --output-file proof-server-v6.3.2.eif | tee build-output.txt

# 3. Update pcr-measurements.json with new PCR values from build-output.txt

# 4. Rebuild (to embed updated PCR file)
docker build -f tee-proof-server-proto/Dockerfile -t midnight/proof-server:v6.3.2 .

# 5. Build final EIF
nitro-cli build-enclave --docker-uri midnight/proof-server:v6.3.2 \
  --output-file proof-server-v6.3.2.eif

# 6. Commit and deploy
git add tee-proof-server-proto/pcr-measurements.json
git commit -m "Update PCR measurements for v6.3.2"
git push

# 7. Deploy
cd tee-proof-server-proto
VERSION=v6.3.2 ./deploy-nitro-enclave.sh
```

## Security Considerations

### Why Embed PCRs in the Binary?

PCRs are embedded at compile time for several reasons:
1. **Self-documenting**: The binary knows its own expected measurements
2. **Tamper-proof**: PCRs are part of the enclave image itself
3. **No external dependencies**: Works even if external hosting fails

### Trust Model

Clients must trust:
1. **Source code**: Open source, auditable
2. **Build process**: Reproducible builds
3. **PCR publication**: Multiple channels (binary, GitHub, website)
4. **AWS infrastructure**: Nitro Enclave hardware + NSM

### Attack Scenarios

| Attack | Mitigation |
|--------|-----------|
| Modified code | PCR0 changes, clients detect mismatch |
| Modified kernel | PCR1 changes, clients detect mismatch |
| Different config | PCR2 changes, clients detect mismatch |
| Replay attack | Nonce in attestation prevents reuse |
| MITM | Certificate chain validated to AWS root |

## Troubleshooting

### PCR Endpoint Returns 500

**Cause**: PCR JSON file missing or invalid

**Fix**:
```bash
# Check file exists
ls -la tee-proof-server-proto/pcr-measurements.json

# Validate JSON
jq . tee-proof-server-proto/pcr-measurements.json

# Rebuild if needed
docker build -f tee-proof-server-proto/Dockerfile -t midnight/proof-server:latest .
```

### PCR Values Don't Match

**Cause**: PCR file not updated after code changes, or wrong version deployed

**Fix**:
1. Extract current PCRs: `nitro-cli describe-enclaves | jq '.[0].Measurements'`
2. Update `pcr-measurements.json`
3. Rebuild Docker image
4. Rebuild EIF
5. Redeploy

### Clients Still Show Warning

**Cause**: Clients not configured to fetch PCRs, or fetching from wrong URL

**Fix**:
1. Verify endpoint works: `curl https://proof-test.devnet.midnight.network/pcr`
2. Configure client to fetch from correct URL
3. Check client-side verification logic

## Future Enhancements

- [ ] Automatic PCR extraction during build
- [ ] Multiple PCR versions support (for rolling updates)
- [ ] PCR signing with GPG for additional verification
- [ ] PCR history tracking
- [ ] API for PCR version negotiation

## References

- [AWS Nitro Enclaves Documentation](https://docs.aws.amazon.com/enclaves/)
- [NSM API Specification](https://github.com/aws/aws-nitro-enclaves-nsm-api)
- [Attestation Document Format](https://github.com/aws/aws-nitro-enclaves-nsm-api/blob/main/docs/attestation_process.md)
