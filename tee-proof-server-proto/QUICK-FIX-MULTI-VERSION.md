# Quick Fix: Run Multiple Proof Server Versions

**Problem:** You need a proof server that works with the current preview network (6.1.0-alpha.6) but also want the latest version (6.2.0-alpha.1) for future use.

**Solution:** Build and run both versions as separate Docker containers!

## Quick Start (5 minutes)

### Option 1: Automated Build Script

```bash
cd ~/code/midnight-code/midnight-ledger/tee-proof-server-proto

# Build both versions automatically
./scripts/build-multi-version.sh
```

This script will:
1. ✅ Build **v1-legacy** (6.1.0-alpha.6 - works with current network)
2. ✅ Build **v1-current** (6.2.0-alpha.1 - for future network)
3. ✅ Test both versions
4. ✅ Show you which to use

### Option 2: Manual Build

```bash
cd ~/code/midnight-code/midnight-ledger

# Build legacy version (current network compatible)
git checkout 9955490  # Commit before domain separators
docker build -f tee-proof-server-proto/Dockerfile -t midnight/proof-server:v1-legacy .

# Build current version (future network)
git checkout main
docker build -f tee-proof-server-proto/Dockerfile -t midnight/proof-server:v1-current .
```

## Usage

### For Current Preview Network (RECOMMENDED NOW)

```bash
# Stop any running proof server
docker stop midnight-proof-server 2>/dev/null || true
docker rm midnight-proof-server 2>/dev/null || true

# Run legacy version (compatible with network 6.1.0-alpha.6)
docker run -d \
  --name midnight-proof-server \
  -p 6300:6300 \
  -e RUST_LOG=info \
  -e MIDNIGHT_PROOF_SERVER_DISABLE_AUTH=true \
  midnight/proof-server:v1-legacy

# Verify it works
curl http://localhost:6300/version
# Should show: 6.1.0-alpha.6 (or similar)

curl http://localhost:6300/health
# Should show: {"status":"ok",...}
```

### For Future Network (When Upgraded)

```bash
# Stop legacy server
docker stop midnight-proof-server
docker rm midnight-proof-server

# Run current version (with domain separators)
docker run -d \
  --name midnight-proof-server \
  -p 6300:6300 \
  -e RUST_LOG=info \
  -e MIDNIGHT_PROOF_SERVER_DISABLE_AUTH=true \
  midnight/proof-server:v1-current

# Verify
curl http://localhost:6300/version
# Should show: 6.2.0-alpha.1
```

### Run Both Simultaneously (Testing)

```bash
# Legacy on port 6300
docker run -d \
  --name proof-legacy \
  -p 6300:6300 \
  midnight/proof-server:v1-legacy

# Current on port 6301
docker run -d \
  --name proof-current \
  -p 6301:6300 \
  midnight/proof-server:v1-current

# Test both
curl http://localhost:6300/version  # → 6.1.0-alpha.6
curl http://localhost:6301/version  # → 6.2.0-alpha.1
```

## Update Lace Wallet

### For Legacy Server (Current Network)

```json
{
  "userNetworksConfigOverrides": "{\"preview\":{\"proofServerAddress\":\"http://localhost:6300\"}}"
}
```

### For Current Server (Future Network)

```json
{
  "userNetworksConfigOverrides": "{\"preview\":{\"proofServerAddress\":\"http://localhost:6301\"}}"
}
```

### Use Remote Server (No Configuration)

```json
{
  "userNetworksConfigOverrides": "{}"
}
```

## Monitoring Network Upgrades

Watch for when the preview network upgrades to 6.2.0+:

```bash
# Check every 5 minutes
watch -n 300 'curl -s https://lace-proof-pub.preview.midnight.network/version'

# Or one-time check
curl https://lace-proof-pub.preview.midnight.network/version

# When it shows 6.2.0+, switch to v1-current
```

## Quick Reference

| Image Tag | Version | Network Compatibility | Use When |
|-----------|---------|----------------------|----------|
| `v1-legacy` | 6.1.0-alpha.6 | ✅ Current preview | **NOW** |
| `v1-current` | 6.2.0-alpha.1 | ✅ Future (6.2.0+) | After network upgrade |
| `latest` | 6.2.0-alpha.1 | ✅ Future (6.2.0+) | Same as v1-current |

## Makefile Shortcuts

Add these to your `tee-proof-server-proto/Makefile`:

```makefile
# Build both versions
.PHONY: build-multi
build-multi:
	./scripts/build-multi-version.sh

# Run legacy (current network)
.PHONY: run-legacy
run-legacy:
	docker run -d --name midnight-proof-server \
		-p 6300:6300 \
		-e RUST_LOG=info \
		-e MIDNIGHT_PROOF_SERVER_DISABLE_AUTH=true \
		midnight/proof-server:v1-legacy

# Run current (future network)
.PHONY: run-current
run-current:
	docker run -d --name midnight-proof-server \
		-p 6300:6300 \
		-e RUST_LOG=info \
		-e MIDNIGHT_PROOF_SERVER_DISABLE_AUTH=true \
		midnight/proof-server:v1-current

# Switch from legacy to current
.PHONY: switch-to-current
switch-to-current: stop run-current

# Switch from current to legacy
.PHONY: switch-to-legacy
switch-to-legacy: stop run-legacy
```

Then use:
```bash
make build-multi        # Build both versions
make run-legacy         # Run legacy version
make switch-to-current  # Switch to current version
```

## Troubleshooting

### "Transaction failed with Custom error: 139"

**Cause:** Wrong proof server version for the network

**Fix:**
```bash
# Check network version
curl https://lace-proof-pub.preview.midnight.network/version

# If it shows 6.1.0-alpha.6, use legacy:
docker stop midnight-proof-server
docker rm midnight-proof-server
make run-legacy

# If it shows 6.2.0+, use current:
docker stop midnight-proof-server
docker rm midnight-proof-server
make run-current
```

### "Which version should I use?"

**Rule of thumb:**
- Network version starts with **6.1.x** → Use `v1-legacy`
- Network version starts with **6.2.x** or higher → Use `v1-current`

**Check with:**
```bash
./scripts/check-proof-server-config.sh
```

### "Can I run both at once?"

**Yes!** Use different ports:
```bash
docker run -d --name proof-legacy -p 6300:6300 midnight/proof-server:v1-legacy
docker run -d --name proof-current -p 6301:6300 midnight/proof-server:v1-current

# Configure Lace to use the appropriate one
# Legacy: http://localhost:6300
# Current: http://localhost:6301
```

## Next Steps

This is a **temporary workaround** until proper multi-version support is added to the codebase.

**Future enhancement:** See `MULTI-VERSION-SUPPORT.md` for the full solution that:
- ✅ Single binary supports both versions
- ✅ Runtime configuration via env var
- ✅ Versioned API endpoints
- ✅ Auto-detection of network requirements

**Estimated implementation:** 2-3 days of development

## Summary

**TODAY (Quick Fix):**
```bash
# Build both versions
./scripts/build-multi-version.sh

# Run legacy version (works with current network)
make run-legacy
```

**FUTURE (When Network Upgrades):**
```bash
# Switch to current version
make switch-to-current
```

**PERMANENT SOLUTION:**
- Implement runtime version selection (see MULTI-VERSION-SUPPORT.md)
- Single Docker image, configurable via env var
- Automatic network compatibility

---

**Created:** 2025-12-22
**Status:** ✅ Working solution
**Related:** MULTI-VERSION-SUPPORT.md, VERSION-MISMATCH.md
