# Quick Reference: Close Trust Gaps

## TL;DR - Copy-Paste Commands

### 1. Add Dependencies (30 seconds)

**File**: `proof-server/Cargo.toml`

```toml
# Add to [dependencies] section:
aws-nitro-enclaves-nsm-api = "0.4"
serde_bytes = "0.11"
```

### 2. Create NSM Module (2 minutes)

```bash
# Download the complete module
curl -o proof-server/src/nsm_attestation.rs \
  https://raw.githubusercontent.com/midnight/midnight-ledger/main/tee-proof-server-proto/proof-server/src/nsm_attestation.rs
```

**OR** create manually - see [IMPLEMENTATION-STEPS.md](./IMPLEMENTATION-STEPS.md#step-2-create-nsm-attestation-module)

### 3. Add Module Declaration (10 seconds)

**File**: `proof-server/src/lib.rs` (or `main.rs`)

```rust
mod nsm_attestation;
```

### 4. Update Attestation Handler (2 minutes)

Replace `proof-server/src/attestation.rs` - see [IMPLEMENTATION-STEPS.md](./IMPLEMENTATION-STEPS.md#step-4-update-attestation-handler)

### 5. Build and Test (5 minutes)

```bash
cd proof-server
cargo build --release
../../target/release/midnight-proof-server-prototype --disable-tls
```

Test:
```bash
curl "http://localhost:6300/attestation?nonce=test123"
```

### 6. Deploy to Enclave (10 minutes)

```bash
# Build Docker image
docker build -f tee-proof-server-proto/Dockerfile -t midnight/proof-server:v6.3.0 .

# Build EIF
nitro-cli build-enclave --docker-uri midnight/proof-server:v6.3.0 --output-file proof-server.eif

# Extract PCRs
nitro-cli build-enclave --docker-uri midnight/proof-server:v6.3.0 --output-file proof-server.eif | jq '.Measurements' > pcrs.json

# Deploy
nitro-cli run-enclave --eif-path proof-server.eif --cpu-count 4 --memory 8192 --enclave-cid 16
```

### 7. Test Real Attestation

```bash
curl "http://localhost:6300/attestation?nonce=$(date +%s)"
```

**Expected**: JSON with `"attestation": "base64-CBOR-document"`

---

## What Each File Does

| File | Purpose | Size |
|------|---------|------|
| `nsm_attestation.rs` | Calls NSM API to generate attestation | ~200 lines |
| `attestation.rs` | HTTP endpoint handler | ~150 lines |
| `Cargo.toml` | Dependencies | +2 lines |
| `lib.rs`/`main.rs` | Module declaration | +1 line |

**Total Changes**: ~350 lines of new code

---

## Decision Tree

**Q: Running locally?**
- Yes → Will return "NSM device not available" (correct behavior)
- No, in enclave → Will return real attestation document

**Q: Nonce in response?**
- Yes → ✅ Freshness protection working
- No → ❌ Check implementation

**Q: PCRs match published values?**
- Yes → ✅ Code integrity verified
- No → ⚠️ Different build or compromised

---

## Success Criteria

✅ **Local Test** (outside enclave):
```json
{
  "platform": "Development/Not in Enclave",
  "error": "NSM device not available"
}
```

✅ **Enclave Test** (inside enclave):
```json
{
  "platform": "AWS Nitro Enclaves",
  "attestation": "hEShATgioFkQ6q...",
  "nonce": "abc123"
}
```

✅ **Client Verification**:
```bash
python3 verify-attestation.py https://proof.devnet.midnight.network
# Output: ✅ ATTESTATION VERIFIED
```

---

## Common Issues

| Error | Cause | Fix |
|-------|-------|-----|
| `NSM device not available` | Not in enclave | Normal for local testing |
| `Compilation error` | Rust version | `rustup update` |
| `PCRs don't match` | Different build | Rebuild or check PCR publication |
| `Connection refused` | Socat proxy not running | `systemctl start proof-server-vsock-proxy` |

---

## Time Estimates

| Task | Time | Difficulty |
|------|------|------------|
| Add dependencies | 1 min | Easy |
| Create NSM module | 2 min | Easy (copy-paste) |
| Update handler | 5 min | Easy (replace file) |
| Local build & test | 10 min | Easy |
| Docker build | 15 min | Easy |
| Deploy to enclave | 10 min | Medium |
| Test & verify | 5 min | Easy |
| **TOTAL** | **~45-60 min** | **Medium** |

---

## Full Documentation

See [IMPLEMENTATION-STEPS.md](./IMPLEMENTATION-STEPS.md) for complete step-by-step guide.
