# Version Mismatch Issue - RESOLVED

**Date:** 2025-12-22
**Status:** ⚠️ LOCAL SERVER INCOMPATIBLE WITH PREVIEW NETWORK

## Problem

Transaction errors when using local proof server:
```
1010: Invalid Transaction: Custom error: 139
```

## Root Cause

**Version mismatch between local proof server and preview network:**

| Component | Version | Domain Separators |
|-----------|---------|-------------------|
| Local proof server | 6.2.0-alpha.1 | ✅ Yes (added Nov 30) |
| Preview network | 6.1.0-alpha.6 | ❌ No |
| Remote proof server | 6.1.0-alpha.6 | ❌ No |

### What Changed

Commit `b77a5fa` (Nov 30, 2025) added domain separators to:
- `dust:proof`
- `zswap:ciphertext`

This is a **breaking cryptographic change** - proofs generated with domain separators are rejected by older network versions.

## Solution 1: Use Remote Proof Server (RECOMMENDED)

Update Lace wallet to use the official remote proof server:

1. Open Lace → DevTools (F12)
2. Go to: Application → Storage → Extension Storage
3. Find: `redux:persist:midnightContext`
4. Update JSON to **remove** the local proof server override:

```json
{
  "userNetworksConfigOverrides": "{}"
}
```

Or explicitly set it to the remote server:

```json
{
  "userNetworksConfigOverrides": "{\"preview\":{\"proofServerAddress\":\"https://lace-proof-pub.preview.midnight.network\"}}"
}
```

5. Reload Lace extension
6. Transactions should now work

### Verify Remote Server

```bash
$ curl https://lace-proof-pub.preview.midnight.network/version
6.1.0-alpha.6

$ curl https://lace-proof-pub.preview.midnight.network/health
{"status":"ok","timestamp":"..."}
```

## Solution 2: Downgrade Local Server (ALTERNATIVE)

If you need to use a local proof server, downgrade to match the network:

```bash
cd ~/code/midnight-code/midnight-ledger

# Check out the version tag that matches the network
git tag -l | grep "6.1.0"
git checkout v6.1.0-alpha.6  # Or whatever tag matches

# Rebuild Docker container
cd tee-proof-server-proto
make build-local
make run
```

**Note:** You'll need to find the correct git tag/commit for version 6.1.0-alpha.6.

## Solution 3: Wait for Network Upgrade

The preview network will eventually upgrade to 6.2.0-alpha.x, at which point your local server will be compatible again.

**Check network version:**
```bash
# Monitor when the remote proof server updates
watch -n 300 'curl -s https://lace-proof-pub.preview.midnight.network/version'
```

When it shows `6.2.0-alpha.x`, you can switch back to your local server.

## Why This Happened

1. You built your local proof server from the latest `main` branch
2. The `main` branch includes the domain separator changes (6.2.0-alpha.1)
3. The preview network hasn't been upgraded yet (still on 6.1.0-alpha.6)
4. Proofs generated locally use new format → network rejects them

## Technical Details

### Domain Separators

Domain separators prevent cross-protocol attacks by ensuring cryptographic operations are bound to specific contexts:

```rust
// Before (6.1.0-alpha.6): No domain separator
let hash = PersistentHash(seed);

// After (6.2.0-alpha.1): With domain separator
let mut writer = PersistentHashWriter::new();
writer.write(b"dust:proof");  // Domain separator
writer.write(seed);
let hash = writer.finalize();
```

This changes the proof outputs, making them incompatible between versions.

### Error Code 139

In Substrate/Polkadot runtimes:
- Error `1010` = Invalid Transaction
- Custom error `139` = Runtime-specific validation failure

In this case, it means the proof verification failed because the proof was generated with a different cryptographic protocol (domain separators) than the network expects.

## Verification

After switching to the remote proof server:

1. **Clear any cached state** in Lace
2. **Test a small transaction** to verify it works
3. **Check browser console** for errors - should be clean
4. **Monitor proof server logs** (if using remote, you won't see them)

## Future Prevention

To avoid this issue in the future:

1. **Always check network version** before building local proof server:
   ```bash
   curl https://lace-proof-pub.preview.midnight.network/version
   ```

2. **Use git tags** instead of latest main:
   ```bash
   git checkout v$(curl -s https://lace-proof-pub.preview.midnight.network/version)
   ```

3. **Subscribe to network upgrade announcements** from Midnight team

4. **Test transactions** after building/updating proof server

## References

- **Breaking commit:** `b77a5fa` - "added domain seperators for dust:proof and zswap:ciphertext"
- **Changed files:**
  - `ledger/src/dust.rs` (proof domain separators)
  - `zswap/src/lib.rs` (ciphertext domain separators)
- **Remote proof server:** https://lace-proof-pub.preview.midnight.network
- **Network:** Midnight preview

## Status Check

✅ **Local proof server works** - generates proofs successfully
❌ **Network rejects proofs** - version mismatch
✅ **Remote proof server works** - matches network version

**Action Required:** Switch to remote proof server OR downgrade local server

---

**Updated:** 2025-12-22
**Issue:** PM-20172 domain separators
**Affected Versions:** 6.2.0-alpha.1+ (local) vs 6.1.0-alpha.6 (network)
