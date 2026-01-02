# Debugging Intermittent Transaction Failures

## Check Which Proof Server Is Being Used

### Method 1: Browser DevTools Network Tab

1. Open Lace wallet
2. Open DevTools (F12) → Network tab
3. Filter by "6300" or "proof"
4. Attempt a transaction
5. Look for requests to:
   - `http://localhost:6300/prove` → Using LOCAL server (6.2.0-alpha.1) ❌
   - `https://lace-proof-pub.preview.midnight.network/prove` → Using REMOTE server (6.1.0-alpha.6) ✅

### Method 2: Check Extension Storage

1. DevTools → Application → Storage → Extension Storage
2. Find key: `redux:persist:midnightContext`
3. Check `userNetworksConfigOverrides`:
   - Contains `localhost:6300` → Using LOCAL server ❌
   - Empty `{}` or missing → Using REMOTE server ✅

### Method 3: Watch Proof Server Logs

```bash
# In one terminal, watch local server logs
docker logs -f midnight-proof-server

# In another terminal, attempt transaction in Lace

# If you see "Prove request received" → Using local server
# If you see nothing → Using remote server
```

## Check Transaction Type

Different transaction types require different proofs:

| Transaction Type | Requires ZK Proof? | Affected by Version Mismatch? |
|------------------|-------------------|------------------------------|
| Public send | ❌ No | ✅ Works always |
| Shielded send | ✅ Yes (zswap:ciphertext) | ❌ Fails with local server |
| Contract deploy | ✅ Yes | ❌ May fail |
| Contract call | ✅ Yes (depends on contract) | ❌ May fail |
| Note creation | ❌ No proof | ✅ Works always |
| Note spending | ✅ Yes (dust:proof) | ❌ Fails with local server |

**To test:**
1. Try a simple public transaction → Should work
2. Try a shielded transaction → Fails with local, works with remote

## Common Scenarios

### Scenario 1: Config Keeps Resetting
**Symptom:** Sometimes works, sometimes doesn't - seems random

**Cause:** Browser or extension is resetting your storage

**Fix:**
- Pin the remote server explicitly (don't rely on defaults)
- Or keep local server stopped to force remote usage

### Scenario 2: Different Transaction Types
**Symptom:** Send to public addresses works, shielded transfers fail

**Cause:** Public transactions don't need proofs

**Fix:**
- Understand which transactions need proofs
- Use remote server for all shielded operations

### Scenario 3: Cached Proofs
**Symptom:** First transaction of a type works, subsequent similar ones work, but new types fail

**Cause:** Wallet caching successful proofs

**Fix:**
- Clear wallet cache
- Or understand that cached proofs will work regardless of server

### Scenario 4: Fallback Logic
**Symptom:** Transactions eventually work after delay or retry

**Cause:** Wallet tries local, fails, falls back to remote

**Check:**
- Network tab shows two requests (one to localhost, one to remote)
- Transaction takes longer than usual (failed attempt + retry)

## Definitive Test

**To confirm it's the version mismatch issue:**

### Test 1: Force Local Server
```bash
# Ensure local server is running
docker ps | grep midnight-proof-server

# Ensure Lace is configured for local
# DevTools → Storage → Extension → redux:persist:midnightContext
# Set: {"userNetworksConfigOverrides": "{\"preview\":{\"proofServerAddress\":\"http://localhost:6300\"}}"}

# Attempt SHIELDED transaction
# Expected: FAILS with "Custom error: 139"
```

### Test 2: Force Remote Server
```bash
# Stop local server
docker stop midnight-proof-server

# Ensure Lace is configured for remote (or default)
# DevTools → Storage → Extension → redux:persist:midnightContext
# Set: {"userNetworksConfigOverrides": "{}"}

# Attempt same SHIELDED transaction
# Expected: SUCCEEDS
```

### Test 3: Public Transaction
```bash
# With local server running (6.2.0-alpha.1)
# Attempt PUBLIC (non-shielded) transaction
# Expected: SUCCEEDS (no proof needed)
```

## Network Tab Example

When using **local server** (fails):
```
POST http://localhost:6300/prove
Status: 200 OK
Response: [binary proof data]

[Later...]
POST [blockchain node]/submit
Status: 400 Bad Request
Error: "1010: Invalid Transaction: Custom error: 139"
```

When using **remote server** (works):
```
POST https://lace-proof-pub.preview.midnight.network/prove
Status: 200 OK
Response: [binary proof data]

[Later...]
POST [blockchain node]/submit
Status: 200 OK
Transaction: [tx hash]
```

## Recommendations

1. **For now:** Use remote proof server exclusively
   ```json
   {"userNetworksConfigOverrides": "{}"}
   ```

2. **Stop local server** to avoid confusion:
   ```bash
   docker stop midnight-proof-server
   ```

3. **Monitor network version** for when it upgrades:
   ```bash
   # When this shows 6.2.0-alpha.x, you can use local again
   curl https://lace-proof-pub.preview.midnight.network/version
   ```

4. **Future builds:** Always check network version first:
   ```bash
   NETWORK_VERSION=$(curl -s https://lace-proof-pub.preview.midnight.network/version)
   echo "Network is on: $NETWORK_VERSION"

   # Build matching version if needed
   cd ~/code/midnight-code/midnight-ledger
   git checkout "v${NETWORK_VERSION}" 2>/dev/null || git checkout main
   ```

## Summary

**Most likely cause of "works sometimes":**
- ✅ You're switching between local and remote proof servers (config resets)
- ✅ Different transaction types have different proof requirements
- ✅ Wallet fallback/retry logic eventually uses remote server
- ✅ Cached proofs from earlier successful attempts

**To fix permanently:**
- Use remote proof server until network upgrades to 6.2.0+
- OR downgrade local server to match network (6.1.0-alpha.6)

---

**Created:** 2025-12-22
**Related:** VERSION-MISMATCH.md
