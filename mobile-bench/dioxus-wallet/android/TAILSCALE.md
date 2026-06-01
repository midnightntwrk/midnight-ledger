# Driving the Android wallet against your laptop's standalone env

The Android APK ships with the wallet-core's `Network::Undeployed`
endpoint URLs **baked in at build time**. By default those URLs
point at `localhost` (= the phone's own loopback), so an APK built
on the laptop and installed on the phone can't reach the
laptop-hosted standalone env without a tunnel.

Tailscale solves this with one shared `100.x.y.z` IP that both
devices use to reach each other directly — no proxy hop, full-
duplex WebSocket, free for personal use.

## One-time setup

1. **Install Tailscale**:
   - Laptop: `brew install --cask tailscale` (then launch the app
     from `/Applications/Tailscale.app` and sign in)
   - Phone: install the *Tailscale* app from Play Store, sign in
     to the **same account**
2. **Note your laptop's tailnet IP** — `tailscale status | head -1`,
   or look under *Devices* in the Tailscale tray. It looks like
   `100.64.0.42`.
3. **Verify reachability from the phone** — open a browser on the
   phone, visit `http://100.64.0.42:18088/api/v3/graphql` (or any
   other standalone-env port). You should see a JSON 405 / 400
   response, not a connection refused.

## Build the APK pointed at your tailnet

The four endpoint URLs travel via `option_env!()` in
`wallet-core/src/network.rs::Network::Undeployed`. Set them at
`cargo ndk` invocation time:

```bash
cd mobile-bench/dioxus-wallet

# Replace 100.64.0.42 with YOUR laptop's tailnet IP.
TAIL_IP=100.64.0.42

MIDNIGHT_INDEXER_HTTP_URL="http://${TAIL_IP}:18088/api/v3/graphql" \
MIDNIGHT_INDEXER_WS_URL="ws://${TAIL_IP}:18088/api/v3/graphql/ws" \
MIDNIGHT_NODE_WS_URL="ws://${TAIL_IP}:19944" \
MIDNIGHT_PROOF_SERVER_URL="http://${TAIL_IP}:16300" \
ANDROID_NDK_HOME=$HOME/Library/Android/sdk/ndk/27.0.12077973 \
  cargo ndk -t arm64-v8a -o android/app/src/main/jniLibs \
  build --release -p dioxus-wallet --lib

cd android
JAVA_HOME=/Library/Java/JavaVirtualMachines/temurin-21.jdk/Contents/Home \
  ./gradlew assembleDebug
# APK lands at: app/build/outputs/apk/debug/app-debug.apk
```

`cargo` records env-var values into the `.fingerprint`, so a
follow-up build with a different `TAIL_IP` rebuilds the wallet-core
crate cleanly without manual `cargo clean`.

## Install + launch

```bash
adb install -r android/app/build/outputs/apk/debug/app-debug.apk
adb shell am start -n io.iohk.midnight.wallet/dev.dioxus.main.MainActivity

# Stream Rust logs:
adb logcat -v brief RustStdoutStderr:V '*:S' &

# Tail the wallet's own tracing output:
adb logcat -v brief Dioxus:V '*:S'
```

## The issuer side

The issuer (`IssuerDIDIT-mock`) lives on the laptop too. By default
it emits QR codes that embed `http://localhost:3001/...` URLs —
the phone can't reach those. Two options:

1. **Set `BASE_URL` when starting the issuer** so the QR codes
   embed the tailnet IP:

   ```bash
   cd /Users/ysh/iohk/midnight-identity-workspace/midnight-identity-solution-examples/IssuerDIDIT-mock
   source ~/.nvm/nvm.sh && nvm use 24
   COREPACK_ENABLE_STRICT=0 \
   INDEXER_URL=http://localhost:18088/api/v3/graphql \
   BASE_URL="http://${TAIL_IP}:3001" \
     nohup ./node_modules/.bin/tsx src/server.ts > /tmp/midnight-ssi-issuer.log 2>&1 &
   ```

   (The issuer's `config.ts` reads `BASE_URL` — check there if the
   variable name has drifted.)

2. **Scan the QR in your laptop browser, copy the URL by hand**,
   then paste it into the wallet's *Diagnostics → Bootstrap* tab
   on the phone, edit `localhost` → tailnet IP, and submit.

Option 1 scales better past the first scan.

## Troubleshooting

- **Phone says "endpoint unreachable" on the Wallet tab** — Tailscale
  isn't routing. Toggle the Tailscale switch off + on on the phone,
  and verify `tailscale status` shows both nodes as online on the
  laptop.
- **Logcat shows `Connection refused` to port 18088 / 19944 /
  16300** — the docker services are bound but the laptop's firewall
  might be blocking inbound on the tailnet interface. On macOS
  check *System Settings → Network → Firewall*; either disable it
  for the demo or add a per-app exception for Docker.
- **TLS handshake error on first network call** — Android-side
  cert verifier didn't initialise. Reboot the app; this is a known
  race in `try_init_android_tls()` on cold start (see comment on
  `App::run()` in `lib.rs`).
- **Switch back to `localhost` for desktop runs** — just rebuild
  without the env vars. The defaults kick back in via `option_env!`.
