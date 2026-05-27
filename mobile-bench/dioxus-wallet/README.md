# dioxus-wallet

Rust-native Midnight wallet for iOS / Android, built on Dioxus 0.7 + a
WebView eval-bridge for the JS contract layer.

## Status (2026-05-27)

Working against the **standalone Midnight env** (`docker-compose` on
`:9944` / `:8088` / `:6300`) and against the **PreProd public network**.
The Identity Centre Phase 1 wallet-core slice is fully landed and tested;
UI integration is in progress — see
[`docs/superpowers/plans/2026-05-25-identity-centre-phase-1-PROGRESS.md`](../../docs/superpowers/plans/2026-05-25-identity-centre-phase-1-PROGRESS.md)
for the split between done / deferred.

## Tabs

- **Wallet** — NIGHT + DUST balances, address (bech32m), wallet sync.
- **DIDs** — Create DID, Resolve, Open detail (Methods / Relationships /
  Services / Operations / Sign / Resolver / Raw state), Update DID
  (Operation Builder with palette / form / queue), Deactivate.
- **Diagnostics** — Probes, Metrics, Benchmark, Test, Logs (5-page
  horizontal carousel).
- **About** — version, build info.

The Identity Centre (VCs tab) lands as part of the deferred Task 30-36
work; until then VC issuance / self-verify is exercisable from the Rust
side via the `wallet-core` test suite and the `did-bootstrap` CLI.

## Identity Centre (Phase 1)

The wallet-core surface is complete. The bringup recipe is in the
PROGRESS doc above. Key entry points:

| Surface | Where | Use |
|---|---|---|
| `bootstrap_did_with_keys` | `wallet-core::did::bootstrap` | One-shot create-DID + attach Ed25519/auth + Jubjub/assert. |
| `did-bootstrap` CLI | `target/debug/did-bootstrap` | Same as above, callable from shell / TS scripts. |
| `oid4vp_client::run_authentication` | `wallet-core::oid4vp_client` | Scan QR → SIOPv2 id_token JWS → POST. |
| `oid4vci_client::run_issuance` | `wallet-core::oid4vci_client` | Scan offer QR → token + c_nonce → credential → land in vc_store. |
| `vc_self_verify::self_verify_and_cache` | `wallet-core::vc_self_verify` | Re-resolve issuer DID, verify VC signature against `assertionMethod` key, cache outcome in vc_store metadata. |
| `VcStore` | `wallet-core::vc_store` | redb-backed VC + openings + metadata storage. |

## Build

### iOS Simulator

```bash
cd mobile-bench/dioxus-wallet
cargo build --target aarch64-apple-ios-sim --release -p dioxus-wallet \
  --lib --features "preprod-live js-bridge"
cp ../../target/aarch64-apple-ios-sim/release/libdioxuswalletmain.dylib ios/App/
install_name_tool -id "@rpath/libdioxuswalletmain.dylib" ios/App/libdioxuswalletmain.dylib
cd ios && xcodegen generate
xcodebuild -project DioxusWallet.xcodeproj -scheme DioxusWallet \
  -configuration Debug -sdk iphonesimulator \
  -destination 'platform=iOS Simulator,name=iPhone 17 Pro' build
xcrun simctl boot 'iPhone 17 Pro'
xcrun simctl install booted .../DioxusWallet.app
xcrun simctl launch --console-pty booted io.iohk.midnight.wallet
```

### Android Emulator

See `mobile-bench/architecture.md` §5b / §5c. Pipeline: `cargo ndk
build` → `gradlew assembleDebug` → `adb install` → `am start`.

## Standalone Midnight env

```bash
# At /tmp/midnight-standalone/ — files copied from
# ~/iohk/midnight-identity-workspace/arc-passport/experiments/redjubjub-wallet-rs/infra
cat > .env <<'EOF'
APP__INFRA__SECRET=303030303030303030303030303030303030303030303030303030303030303030303030303030303030303030303030303030303030303030303030303030
EOF
docker compose -f docker-compose.yml -f docker-compose.macos.yml up -d
```

The `.env` secret must be a hex string that doesn't parse as an integer
(the indexer rejects values that the config loader interprets as
numbers). The string above is `00` repeated 64× — same as PreProd test
secrets.

Then switch the wallet's network picker (top of the Wallet tab) to
**Undeployed**. The DUST syncer re-registers on switch (commit
`7b11d5e0` — `rehydrate_for_network` in `app.rs`).

## Tests

```bash
# wallet-core lib tests
cargo test -p wallet-core --features test-support --lib
# Expected: 208 passed (2026-05-27).

# Standalone integration (env must be up)
RUST_MIN_STACK=16777216 STANDALONE_RUN=1 cargo test \
  -p wallet-core --features test-support \
  --test did_bootstrap_standalone -- --ignored --nocapture

# dioxus-wallet compile-check
cargo build -p dioxus-wallet
```
