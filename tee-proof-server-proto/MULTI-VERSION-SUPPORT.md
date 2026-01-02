# Multi-Version Proof Server Support

**Question:** Can we build a proof server that supports multiple proof versions via startup parameter?

**Answer:** ✅ Yes, but it requires code changes to the ledger library.

## Current Situation

### Version Enum Already Exists

The codebase has version support infrastructure:

```rust
// ledger/src/structure.rs:243
#[tag = "proof-versioned"]
#[non_exhaustive]  // ← Can add more versions!
pub enum ProofVersioned {
    V1(Proof),
    // Future: V2, V3, etc.
}
```

### The Challenge

The domain separator change is **hardcoded in the proof generation logic**:

```rust
// ledger/src/dust.rs (commit b77a5fa)
// OLD (6.1.0-alpha.6): No domain separator
transient_hash(&(segment_id, binding).field_vec())

// NEW (6.2.0-alpha.1): With domain separator
transient_hash(&(
    Fr::from_le_bytes(b"midnight:dust:proof"),  // ← Added here
    segment_id,
    binding
).field_vec())
```

The same change appears in `zswap/src/lib.rs`:
```rust
// OLD: Simple hash
transient_hash(&[c.c.x(), c.c.y()])

// NEW: Domain-separated hash
transient_hash(&[
    Fr::from_le_bytes(b"midnight:zswap:ciphertext"),  // ← Added
    c.c.x(),
    c.c.y()
])
```

## Solutions

### Option 1: Runtime Version Selection (RECOMMENDED)

Add a configuration parameter and branch on proof version at runtime.

#### Implementation Steps:

**1. Add version variants to `ProofVersioned`:**

```rust
// ledger/src/structure.rs
#[tag = "proof-versioned"]
#[non_exhaustive]
pub enum ProofVersioned {
    V1Legacy(Proof),  // Without domain separators (6.1.0 compatible)
    V1(Proof),        // With domain separators (6.2.0+)
    // Future: V2, V3...
}
```

**2. Make domain separators conditional:**

```rust
// ledger/src/dust.rs
pub enum ProofVersion {
    V1Legacy,  // 6.1.0-alpha.6 compatible
    V1,        // 6.2.0-alpha.1+ with domain separators
}

impl<D: DB> DustSpend<ProofPreimageMarker, D> {
    pub async fn prove<Zkir>(
        &self,
        prover: impl ProvingProvider<Zkir>,
        segment_id: u16,
        binding: Pedersen,
        version: ProofVersion,  // ← New parameter
    ) -> Result<DustSpend<ProofMarker, D>, ProvingError> {
        let binding_hash = match version {
            ProofVersion::V1Legacy => {
                // Old format (6.1.0 compatible)
                transient_hash(&(segment_id, binding).field_vec())
            }
            ProofVersion::V1 => {
                // New format with domain separator
                transient_hash(&(
                    Fr::from_le_bytes(b"midnight:dust:proof"),
                    segment_id,
                    binding
                ).field_vec())
            }
        };

        let proof = prover.prove(&self.proof, Some(binding_hash)).await?;
        // ... rest of implementation
    }
}
```

**3. Add version parameter to proof server:**

```rust
// tee-proof-server-proto/proof-server/src/main.rs
#[derive(Parser, Debug)]
struct Args {
    // ... existing args ...

    /// Proof version to generate (v1-legacy, v1)
    #[arg(long, env = "MIDNIGHT_PROOF_SERVER_VERSION", default_value = "v1")]
    proof_version: String,
}

// In prove handler:
let proof_version = match state.config.proof_version.as_str() {
    "v1-legacy" => ProofVersion::V1Legacy,
    "v1" => ProofVersion::V1,
    _ => return Err(AppError::BadRequest("Unknown proof version".into())),
};
```

**4. Docker startup parameter:**

```bash
docker run -p 6300:6300 \
  -e MIDNIGHT_PROOF_SERVER_VERSION=v1-legacy \
  midnight/proof-server:latest
```

**Benefits:**
- ✅ Single binary supports both versions
- ✅ Runtime configuration via env var
- ✅ Can switch without rebuilding
- ✅ Network upgrades don't require new builds

**Drawbacks:**
- ⚠️ Requires changes to `ledger` crate (not just proof server)
- ⚠️ Need to maintain both code paths
- ⚠️ Larger binary size (includes both implementations)

---

### Option 2: Compile-Time Feature Flags

Use Rust feature flags to compile different versions.

```rust
// ledger/Cargo.toml
[features]
default = ["domain-separators"]
domain-separators = []
legacy-proofs = []

// ledger/src/dust.rs
#[cfg(feature = "domain-separators")]
const USE_DOMAIN_SEPARATOR: bool = true;

#[cfg(feature = "legacy-proofs")]
const USE_DOMAIN_SEPARATOR: bool = false;
```

Build different images:
```bash
# Legacy version (6.1.0 compatible)
docker build --build-arg FEATURES="legacy-proofs" -t midnight/proof-server:v1-legacy .

# Current version (6.2.0+)
docker build --build-arg FEATURES="domain-separators" -t midnight/proof-server:v1 .
```

**Benefits:**
- ✅ Smaller binary size (only includes one version)
- ✅ No runtime overhead

**Drawbacks:**
- ❌ Need to build and maintain multiple images
- ❌ Cannot switch versions without rebuilding
- ❌ More complex CI/CD pipeline

---

### Option 3: Dynamic Version Auto-Detection

The proof server accepts the **network version** as input and automatically uses the matching proof format.

```rust
// Proof server accepts network version from client
#[derive(Deserialize)]
struct ProveRequest {
    proof_preimage: ProofPreimageVersioned,
    network_version: String,  // e.g., "6.1.0-alpha.6"
}

// Map network version to proof format
fn determine_proof_version(network_version: &str) -> ProofVersion {
    let ver = semver::Version::parse(network_version).unwrap();
    if ver < semver::Version::new(6, 2, 0) {
        ProofVersion::V1Legacy  // No domain separators
    } else {
        ProofVersion::V1        // With domain separators
    }
}
```

**Benefits:**
- ✅ Automatically compatible with any network
- ✅ Single binary, runtime decision
- ✅ Client specifies requirements

**Drawbacks:**
- ⚠️ Requires client changes (Lace wallet must send network version)
- ⚠️ More complex protocol

---

### Option 4: Multi-Endpoint Approach

Expose different endpoints for different versions:

```rust
// Public endpoints
GET  /version              → "6.2.0-alpha.1"
GET  /supported-versions   → ["v1-legacy", "v1"]

// Versioned proof endpoints
POST /v1-legacy/prove      → Proofs without domain separators
POST /v1/prove             → Proofs with domain separators
POST /prove                → Default (latest version)
```

**Benefits:**
- ✅ Clear API versioning
- ✅ Backward compatible
- ✅ Easy for clients to choose

**Drawbacks:**
- ⚠️ Need to duplicate handler code
- ⚠️ API surface area increases

---

## Recommended Approach

**Hybrid: Option 1 + Option 4**

1. **Add runtime version selection** to ledger library (Option 1)
2. **Expose versioned endpoints** in proof server (Option 4)
3. **Add startup parameter** for default version

### Implementation Priority:

#### Phase 1: Quick Fix (No Code Changes)
```bash
# Build two separate Docker images from different git commits
git checkout v6.1.0-alpha.6
docker build -t midnight/proof-server:v1-legacy .

git checkout main  # 6.2.0-alpha.1
docker build -t midnight/proof-server:v1 .

# Run the appropriate version
docker run -p 6300:6300 midnight/proof-server:v1-legacy  # For current network
docker run -p 6301:6300 midnight/proof-server:v1         # For future network
```

#### Phase 2: Add Runtime Version Support (Best Solution)
1. Modify `ledger/src/dust.rs` and `zswap/src/lib.rs` to accept version parameter
2. Update `ProofVersioned` enum with `V1Legacy` variant
3. Add `MIDNIGHT_PROOF_SERVER_VERSION` env var to proof server
4. Add `/v1-legacy/prove` and `/v1/prove` endpoints
5. Document version compatibility in API docs

#### Phase 3: Auto-Detection (Future Enhancement)
- Add network version detection
- Automatically select proof format based on network
- Remove need for manual configuration

---

## Example Usage

### Current Workaround (No Changes Needed)
```bash
# Terminal 1: Run legacy server for current network
docker run -d -p 6300:6300 --name proof-v1-legacy \\
  midnight/proof-server:v1-legacy

# Terminal 2: Run new server for testing
docker run -d -p 6301:6300 --name proof-v1 \\
  midnight/proof-server:v1
```

### With Multi-Version Support (After Implementation)
```bash
# Single server, switch versions via env var
docker run -p 6300:6300 \\
  -e MIDNIGHT_PROOF_SERVER_VERSION=v1-legacy \\
  midnight/proof-server:latest

# Or use versioned endpoint
curl -X POST http://localhost:6300/v1-legacy/prove -d @proof.bin

# Check supported versions
curl http://localhost:6300/supported-versions
# → ["v1-legacy", "v1", "v2"]
```

---

## Code Changes Required

### Minimal Changes (ledger library):
- **3 files**: `ledger/src/dust.rs`, `zswap/src/lib.rs`, `ledger/src/structure.rs`
- **~100 lines** of code
- **Low risk**: Isolated to proof generation logic

### Proof Server Changes:
- **2 files**: `main.rs`, `lib.rs`
- **~50 lines** of code
- **Low risk**: Just routing and config

---

## Compatibility Matrix

| Server Version | Network 6.1.0 | Network 6.2.0+ |
|----------------|---------------|----------------|
| v1-legacy only | ✅ Works      | ❌ Fails       |
| v1 only        | ❌ Fails      | ✅ Works       |
| Multi-version  | ✅ Works      | ✅ Works       |

---

## Timeline Estimate

| Phase | Effort | Timeline |
|-------|--------|----------|
| Phase 1 (Separate images) | 1 hour | Immediate |
| Phase 2 (Runtime selection) | 2-3 days | 1 week |
| Phase 3 (Auto-detection) | 1 week | 2-3 weeks |

---

## Conclusion

**Yes, multi-version support is definitely possible!**

The quickest path forward:
1. ✅ **Today:** Build two separate Docker images (10 min)
2. ⏭️ **This week:** Add runtime version selection to ledger lib (2-3 days)
3. ⏭️ **Next sprint:** Add auto-detection and versioned endpoints

The architecture is already set up for this with the `ProofVersioned` enum - we just need to:
- Add the version variants
- Make domain separators conditional
- Expose configuration parameter

**Would you like me to create a proof-of-concept implementation?**

---

**Created:** 2025-12-22
**Author:** Technical Analysis
**Related:** VERSION-MISMATCH.md, BUILD-STATUS.md
