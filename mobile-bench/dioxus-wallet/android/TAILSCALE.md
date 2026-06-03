# Driving the Android wallet against a laptop-hosted standalone env

The Android APK ships with two flavours of the "no-deploy" chain in
the network picker:

- **`Undeployed`** — points at `http://localhost:1{8088,9944,6300}`.
  Useful when the wallet runs alongside docker on the same host
  (desktop dev, emulator on the dev machine). The phone can't
  reach `localhost` over the network, so this variant is wrong
  on a real device.
- **`Undeployed (Yurii's)`** — same chain, reached over Yurii's
  tailnet (`http://100.110.241.102:1{8088,9944,6300}`). This is
  the one the demo APK uses on the phone.

Both variants share `network_id`, the funded genesis seed, and
the on-disk wallet shard. Switching the picker swaps endpoints
without "ghost wallet" weirdness.

If you're not Yurii, you'll want either:

- a build with a sibling variant pointing at *your* tailnet (add
  a new `Network::UndeployedAlice` arm next to `UndeployedYurii`
  in `wallet-core/src/network.rs`), or
- to drive the emulator instead of a real device, in which case
  the default `Undeployed` (localhost) works directly.

## One-time tailnet setup (real-device path)

1. **Install Tailscale**:
   - Laptop: `brew install --cask tailscale` (launch from
     `/Applications/Tailscale.app` and sign in)
   - Phone: install the *Tailscale* app from Play Store, sign in
     to the **same account**
2. **Confirm the laptop's tailnet IP** — `tailscale status` shows it
   on the first line. For Yurii's setup it's `100.110.241.102`.
   If you're adapting this doc for your own machine, replace
   that IP throughout (and in `Network::UndeployedYurii` in
   `wallet-core/src/network.rs`).
3. **Verify reachability from the phone** — open a browser on the
   phone, visit `http://<TAIL_IP>:18088/api/v4/graphql` (or any
   other standalone-env port). You should see a JSON 405 / 400
   response, not a connection refused.

## Build the APK

No env vars required. The endpoint URLs live in the network
config, so a plain debug build is enough:

```bash
cd mobile-bench/dioxus-wallet

ANDROID_NDK_HOME=$HOME/Library/Android/sdk/ndk/27.0.12077973 \
  cargo ndk -t arm64-v8a -o android/app/src/main/jniLibs \
  build --release -p dioxus-wallet --lib

cd android
JAVA_HOME=/Library/Java/JavaVirtualMachines/temurin-21.jdk/Contents/Home \
  ./gradlew assembleDebug
# APK lands at: app/build/outputs/apk/debug/app-debug.apk
```

## Install + launch

```bash
adb install -r android/app/build/outputs/apk/debug/app-debug.apk
adb shell am start -n io.iohk.midnight.wallet/dev.dioxus.main.MainActivity

# Stream Rust logs:
adb logcat -v brief RustStdoutStderr:V '*:S' &

# Tail the wallet's own tracing output:
adb logcat -v brief Dioxus:V '*:S'
```

When the wallet boots, open **Settings → Network** (or whatever
the current build calls it — the picker iterates `Network::ALL`)
and pick **Undeployed (Yurii's)** to use the tailnet endpoints.

## The issuer side

The issuer (`IssuerDIDIT-mock`) lives on the laptop too. By default
it emits QR codes that embed `http://localhost:3001/...` URLs —
the phone can't reach those. Two options:

1. **Set `PUBLIC_BASE_URL` when starting the issuer** so the QR
   codes (and every URL the wallet fetches indirectly — the
   `request_uri` in the OID4VP request, the `credential_issuer`
   in the OID4VCI offer, the `redirect_to` after KYC) embed the
   tailnet IP:

   ```bash
   cd /Users/ysh/iohk/midnight-identity-workspace/midnight-identity-solution-examples/IssuerDIDIT-mock
   source ~/.nvm/nvm.sh && nvm use 24
   COREPACK_ENABLE_STRICT=0 \
   INDEXER_URL=http://localhost:18088/api/v4/graphql \
   NODE_WS_URL=ws://localhost:19944 \
   PROOF_SERVER_URL=http://localhost:6300 \
   PUBLIC_BASE_URL="http://100.110.241.102:3001" \
     nohup ./node_modules/.bin/tsx src/server.ts > /tmp/midnight-ssi-issuer.log 2>&1 &
   ```

   **Important:** the variable is `PUBLIC_BASE_URL`, NOT
   `BASE_URL` — the latter looks tempting but the issuer's
   `config.ts` only honours the public-prefixed name (see its
   zod schema). Setting `BASE_URL` alone makes the issuer's
   own startup log look right (it prints whatever was passed)
   but the indirect-request URLs the wallet fetches still point
   at `localhost:3001`, which is unreachable from the phone and
   surfaces as `authenticate failed: http error fetching
   request_uri`. The login route is also mounted at
   `/authorize`, not `/login`.

2. **Scan the QR in your laptop browser, copy the URL by hand**,
   then paste it into the wallet's *Diagnostics → Bootstrap* tab
   on the phone, edit `localhost` → tailnet IP, and submit.

Option 1 scales better past the first scan.

## Troubleshooting

- **Phone says "endpoint unreachable" on the Wallet tab** — confirm
  the network picker is on **Undeployed (Yurii's)**, not the
  localhost `Undeployed`. Then verify Tailscale: toggle the switch
  off + on on the phone, and `tailscale status` on the laptop
  should show both nodes as online.
- **Logcat shows `Connection refused` to port 18088 / 19944 /
  16300** — the docker services are bound but the laptop's firewall
  might be blocking inbound on the tailnet interface. On macOS
  check *System Settings → Network → Firewall*; either disable it
  for the demo or add a per-app exception for Docker.
- **TLS handshake error on first network call** — Android-side
  cert verifier didn't initialise. Reboot the app; this is a known
  race in `try_init_android_tls()` on cold start (see comment on
  `App::run()` in `lib.rs`).
- **Want to flip back to localhost for an emulator run** — just
  switch the network picker to `Undeployed`. Same APK, no rebuild.
- **Scan QR opens then immediately shows "scanner unavailable: Play
  Services unavailable: …"** — your phone doesn't have Google Play
  Services available (GrapheneOS, Huawei post-2020, emulator
  without Play). The native scanner from commit `fdba2182` relies
  on ML Kit's `GmsBarcodeScanning`, which is a Play-Services-only
  API. Use the paste field under Diagnostics → Bootstrap instead.
  A CameraX + `rqrr` fallback for AOSP-only devices fits behind
  the same `QrScanner` trait — file an issue if you need it.
