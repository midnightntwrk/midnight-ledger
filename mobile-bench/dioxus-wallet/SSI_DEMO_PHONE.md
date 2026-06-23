# Phone-driven SSI demo runbook

End-to-end recipe for the SSI verifier demo with the wallet running on
a real Android phone over Tailscale, talking to a laptop-hosted
standalone Midnight chain + issuer + verifier dApp.

> Companion to [`PASSPORT_VAULT_DAPP.md`](PASSPORT_VAULT_DAPP.md)
> (architecture) and the workspace's
> [`MIDNIGHT-SSI-DEMO.md`](../../../midnight-ssi-demo/midnight-identity-workspace/MIDNIGHT-SSI-DEMO.md)
> (desktop/iOS flow + sequence diagrams).

## Topology

```
Phone (Android, R5CX82NAS0P)              Laptop (100.110.241.102 on tailnet)
─────────────────────────────             ────────────────────────────────
WebView                                   midnight-node           :9944
└─ verifier dApp (iframe)                 indexer                 :18088
   ├─ window.midnight.<conn>              proof-server (Rust)     :6300
   └─ DApp Connector ──postMessage──▶     passport-issuer (Docker):8080
      Wallet bridge (Rust)                smocker (mock Didit)    :9090
      ├─ LocalProver (in-process)         dApp (Next.js dev)      :3000
      └─ Chain RPC over tailnet              └─ NEXT_PUBLIC_VAULT_CONTRACT_ADDRESS
                                                 → vault on chain
```

All chain/issuer/dApp traffic from the phone flows over the laptop's
Tailscale IP (`100.110.241.102` in this run). Localhost from the
phone's POV is the phone itself — never use `localhost` URLs in any
config the phone has to reach.

## Repos in play

| Repo | Branch | Used for |
|---|---|---|
| `midnight-ledger` (this checkout) | `dioxus-vc-demo` | wallet + wallet-core |
| `midnight-identity-solution-examples` | `develop` (or feat branch) | dApp + verifier vault contract + `apps/cli` |
| `midnight-ssi-demo/midnight-identity-workspace` | `midnight-ssi-demo` | `run-demo.sh` orchestrator + issuer Docker |

## Nuke + bootstrap (fresh chain)

The standalone chain doesn't have a "rewind" — to demo from a known
state, wipe and start over.

### 1. Stop everything

```bash
docker stop fixtures-node-1 fixtures-indexer-1 fixtures-proof-server-1 kyc-server smocker
docker rm   fixtures-node-1 fixtures-indexer-1 fixtures-proof-server-1 kyc-server smocker
docker volume rm fixtures_node-data    # WIPES chain state
pkill -f 'target/release/midnight-proof-server'   # local proof-server
rm -f apps/cli/.midnight-wallet-cache/*.json      # CLI wallet snapshot
adb -s <serial> shell pm clear io.iohk.midnight.wallet   # phone wallet
```

### 2. Restart standalone (node + indexer + proof-server)

```bash
cd midnight-workspace-vc-test/midnight-identity-solution-examples/apps/issuer-mock/e2e/fixtures
docker compose \
  -f docker-compose.yml \
  -f ../../../../../scripts/docker-compose.demo.yml \
  up -d
```

Health: `docker ps` should show `fixtures-{node,indexer,proof-server}-1` healthy
within ~30 s.

### 3. Wait for dust

**This is non-negotiable.** Dust generation is asynchronous after node
start — the first contract write will fail with `Insufficient Funds:
could not balance dust` or `MalformedError::InputsSignaturesLengthMismatch`
on a freshly-restarted chain. **Wait ~10 minutes** before any
`deploy` / `create-lock` / `deposit` / `claim`. See
[[project_standalone_funded_seed]] memory for why.

```bash
# block until tDUST > 0 via the JS show-state runner, or just sleep 600
```

### 4. Bootstrap the issuer + start passport-issuer

From the demo orchestrator workspace:

```bash
cd midnight-ssi-demo/midnight-identity-workspace
./scripts/run-demo.sh bootstrap-issuer        # mint issuer DID on chain
./scripts/run-demo.sh start-passport-issuer   # boot kyc-server + smocker
```

### 5. Deploy the verifier vault + create both locks

```bash
cd midnight-workspace-vc-test/midnight-identity-solution-examples/apps/cli
nvm use 24    # ledger-v8 needs Node 24+; see [[node24_ledger_v8_wasm]]

npm run deploy                                                    # → emits VAULT_CONTRACT_ADDRESS into .env
npm run create-lock -- --min-age 18 --amount 0 --max-claim 1000000   # lock #0 (age ≥ 18)
npm run deposit     -- --lock-id 0 --amount 100000000                # 100 NIGHT into lock #0
npm run create-lock -- --min-age 21 --amount 0 --max-claim 1000000   # lock #1 (age ≥ 21)
npm run deposit     -- --lock-id 1 --amount 100000000                # 100 NIGHT into lock #1
npm run show-state                                                   # sanity check
```

**Why `--amount 0` then a separate `deposit`?** Bundling the initial
deposit into `createLock` exercises a code path that hits
`MalformedError::InputsSignaturesLengthMismatch` on the wallet SDK's
unshielded-input/signature pairing. Splitting the contract write from
the value transfer sidesteps the bug. Verified live 2026-06-24.

### 6. Update dApp + wallet configs with the new vault address

The contract address changes every redeploy. Two places to update:

```bash
# apps/dapp/.env.local — dApp's connection target
NEXT_PUBLIC_MIDNIGHT_NETWORK=standalone
NEXT_PUBLIC_VAULT_CONTRACT_ADDRESS=<new address from `show-state`>
NEXT_PUBLIC_INDEXER_HTTP_URL=http://100.110.241.102:18088/api/v3/graphql
NEXT_PUBLIC_INDEXER_WS_URL=ws://100.110.241.102:18088/api/v3/graphql/ws
NEXT_PUBLIC_PROOF_SERVER_URL=http://100.110.241.102:16300
```

```rust
// mobile-bench/dioxus-wallet/src/bridge.rs — Android-arm DEFAULT_VAULT_CONTRACT_ADDRESS
#[cfg(target_os = "android")]
const DEFAULT_VAULT_CONTRACT_ADDRESS: &str =
    "<new address>";
```

> See [[wallet-vault-default-contract-address]] for the rationale —
> today's dApp doesn't thread `contractAddress` through connector
> params, so this const is the de-facto address every phone-driven
> claim hits.

### 7. Rebuild dApp + APK

```bash
# dApp (static export rebuild — env is baked in)
cd midnight-workspace-vc-test/midnight-identity-solution-examples/apps/dapp
pnpm build         # OR: pnpm dev with allowedDevOrigins from next.config.mjs

# JS bundle for the wallet's WebView
cd midnight-ledger/mobile-bench/dioxus-wallet/web
MIDNIGHT_DID_SRC=$WORKSPACE/midnight-did \
MIDNIGHT_VC_SRC=$WORKSPACE/midnight-verifiable-credentials \
MIDNIGHT_SOLUTION_SRC=$WORKSPACE/midnight-identity-solution-examples \
  npm run build

# Android APK
cd ..
MIDNIGHT_DID_MANAGED_DIR=$WORKSPACE/midnight-did/packages/contract/src/managed/did \
MIDNIGHT_VAULT_MANAGED_DIR=$WORKSPACE/midnight-identity-solution-examples/packages/contracts/vault/src/managed/passport-vault \
ANDROID_NDK_HOME=$HOME/Library/Android/sdk/ndk/27.0.12077973 \
  cargo ndk -t arm64-v8a -o android/app/src/main/jniLibs build --release -p dioxus-wallet --lib
cd android
JAVA_HOME=/Library/Java/JavaVirtualMachines/temurin-21.jdk/Contents/Home \
  ./gradlew clean assembleDebug
adb -s <serial> install -r app/build/outputs/apk/debug/app-debug.apk
```

> See [[gradle-so-stale-apk]] — `clean assembleDebug` is required;
> Gradle skips repackaging when only the .so changed.

### 8. Run the demo

On the phone:

1. **Bootstrap DID** — Identity Centre tab → Bootstrap. ~10 s with
   LocalProver (vs ~3 min on PreProd HttpProver).
2. **Issue VC** — Scan QR-2 from `http://100.110.241.102:8080/issue/`
   (KYC: pick `19900115` as DOB so age ≥ 21 passes lock #1's policy).
3. **Open verifier dApp tab** — DApp Connector connects automatically.
4. **Claim from lock #0** (age ≥ 18) — pick the credential, tap
   Claim. Watch for `Claimed 1.000000 NIGHT from lock #0 — tx <hash>`.
5. **Claim from lock #1** (age ≥ 21) — same VC reproves age in ZK
   against the tighter policy. Different nullifier, so the contract
   accepts it.
6. **Try claiming from lock #0 again** — fails with `This credential
   has already claimed from this lock`. Demonstrates the nullifier
   security property.

## Known shapes

| Demo shape | DEFAULT vault | Issuer | Network | Notes |
|---|---|---|---|---|
| Phone over tailnet | `#[cfg(target_os = "android")]` const | Docker passport-issuer on laptop:8080 | `UndeployedYurii` | This runbook |
| Desktop (mac) | `#[cfg(not(target_os = "android"))]` const → PreProd address | Lab-shared | `PreProd` | See `MIDNIGHT-SSI-DEMO.md` |
| iOS sim | Same as desktop | Lab-shared | `PreProd` | See `MIDNIGHT-SSI-DEMO.md` |

The `runtime-configurable trusted issuer` PR
(`midnightntwrk/midnight-identity-solution-examples#53`) decouples
issuer DID from the vault binary — once merged, redeploying the issuer
no longer requires redeploying the vault contract.

## Troubleshooting

| Symptom | Likely cause | Fix |
|---|---|---|
| `Claim failed: no on-chain state for <hex>` | Wrong `DEFAULT_VAULT_CONTRACT_ADDRESS` after redeploy | Update `bridge.rs` Android arm + rebuild APK |
| `"asMap is not a function"` in dApp | `for ... of view.locks` in `entry.ts` | Use `.member(i)` / `.lookup(i)` indexed by `lockCount` |
| `Insufficient Funds: could not balance dust` | Dust hasn't accrued yet | Wait ~10 min after fresh chain start |
| `MalformedError::InputsSignaturesLengthMismatch` | Bundled `createLock + initial deposit` | Split: `--amount 0` then separate `deposit` |
| `block_in_place ... not allowed within a current_thread runtime` | Tokio current-thread + `block_in_place` | Branch on `runtime_flavor()` (fixed in `wallet-core`) |
| Phone sees SSR HTML but no JS | Next.js 15 cross-origin block | `allowedDevOrigins: ["<tailnet-ip>"]` in `apps/dapp/next.config.mjs` |
| `Wallet defaults to PreProd` | `startup_network()` missing Android cfg | `#[cfg(target_os = "android")] let fallback = Network::UndeployedYurii;` |
| Wallet redb won't open | Schema downgrade (older APK on newer DB) | `adb shell pm clear io.iohk.midnight.wallet` |

## Files touched by this demo's enablement

- `mobile-bench/wallet-core/src/tx/prove.rs` — runtime-flavor branch
- `mobile-bench/dioxus-wallet/src/app.rs` — Android default network + LocalProver
- `mobile-bench/dioxus-wallet/src/bridge.rs` — Android default vault + live vault readers
- `mobile-bench/dioxus-wallet/src/eval_bridge.rs` — `EvalError::Finished` retry
- `mobile-bench/dioxus-wallet/src/lib.rs` — dApp relay tracing
- `mobile-bench/dioxus-wallet/web/src/entry.ts` — `readVaultLocks` indexed iteration
- `apps/dapp/components/VaultCard.tsx` (workspace repo) — per-call `.catch`
- `apps/dapp/next.config.mjs` (workspace repo) — `allowedDevOrigins`

PRs:
- Wallet: <https://github.com/yshyn-iohk/midnight-ledger/pull/4>
- dApp: <https://github.com/midnightntwrk/midnight-identity-solution-examples/pull/54>
