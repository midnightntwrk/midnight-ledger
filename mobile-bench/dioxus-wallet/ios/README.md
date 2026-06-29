# iOS simulator notes

Working notes for running the dioxus-wallet on the iOS Simulator. Most of this content is *not* a property of our app — it's standing iOS-sim quirks that bite anyone trying to drive a WKWebView app through scripted input.

## One-time per fresh install

Granting clipboard permission on iOS 16+ requires either tapping a system banner the first time the app reads the pasteboard, or pre-granting via `simctl`:

```sh
SIM_UDID=$(xcrun simctl list devices booted --json \
  | python3 -c 'import json,sys; d=json.load(sys.stdin); [print(dev["udid"]) for r,ds in d["devices"].items() for dev in ds if dev["state"]=="Booted"][0]')
BUNDLE=io.iohk.midnight.wallet

xcrun simctl privacy "$SIM_UDID" grant pasteboard "$BUNDLE"
```

The simulator usually swallows the banner instead of showing it, so the default state is effectively "Deny". Both `navigator.clipboard.readText()` *and* a regular `⌘V` into a `<textarea>` go through the same permission gate — both fail silently without this grant.

## Every time: Mac → sim clipboard

`com.apple.iphonesimulator.PasteboardAutomaticSync = 1` is the default and reads as "ON", but **does not actually sync reliably**. Confirmed empirically (Xcode 16 + iOS 17.5 sim): Mac's pasteboard has 569 chars, sim's `pbpaste` returns 0 chars.

Workaround — force-push after every Mac-side copy:

```sh
pbpaste | xcrun simctl pbcopy "$SIM_UDID"
```

Verify with:

```sh
diff <(pbpaste) <(xcrun simctl pbpaste "$SIM_UDID") && echo OK || echo "STILL OUT OF SYNC"
```

Then in the sim: tap into the textarea, **⌘V** (Mac keyboard forwards to sim if Hardware Keyboard is connected — see below). Long-press → Paste from the iOS edit menu also works once the permission grant is in place.

## Hardware keyboard (so ⌘V reaches the sim)

The Simulator's I/O menu controls this per-window. To pin it ON globally:

```sh
defaults write com.apple.iphonesimulator ConnectHardwareKeyboard -bool true
# Then quit + reopen Simulator.app so the new default takes effect.
osascript -e 'tell application "Simulator" to quit'
open -a Simulator
```

If `⌘V` still doesn't reach the sim, toggle the per-window setting: Sim menu bar → **I/O → Keyboard → Connect Hardware Keyboard** (or `⌘K`).

## Network — wallet defaults to `Undeployed` (localhost)

Since [commit `e89e4a26`](../../../docs/superpowers/) (drop platform-conditional network defaults), every build of the wallet starts on `Network::Undeployed`. iOS sim shares the Mac's loopback, so the local docker chain at `localhost:18088/19944/16300` is reachable directly — no manual network switch needed for the demo flow.

If you need a different network (e.g. `Undeployed (Tailscale)` for cross-host testing, or `PreProd` for mainnet-ish work), either tap to switch in the Wallet tab's network picker, or set `MIDNIGHT_WALLET_NETWORK` before launching:

```sh
xcrun simctl launch --terminate-running-process \
  --console-pty "$SIM_UDID" "$BUNDLE" \
  MIDNIGHT_WALLET_NETWORK=preprod
```

## Build + install loop

The shortest fresh build + reinstall cycle:

```sh
cd mobile-bench/dioxus-wallet
cargo build --target aarch64-apple-ios-sim --release --lib
cp ../../target/aarch64-apple-ios-sim/release/libdioxuswalletmain.dylib ios/App/libdioxuswalletmain.dylib
xcodebuild -project ios/DioxusWallet.xcodeproj -scheme DioxusWallet \
  -configuration Debug -destination "id=$SIM_UDID" \
  -derivedDataPath ios/build build
xcrun simctl terminate "$SIM_UDID" "$BUNDLE"
xcrun simctl install "$SIM_UDID" ios/build/Build/Products/Debug-iphonesimulator/DioxusWallet.app
xcrun simctl launch "$SIM_UDID" "$BUNDLE"
```

`adb reverse`-style port forwards aren't needed on iOS sim — the sim shares the Mac's network stack directly. `localhost:8080` from inside the sim reaches the issuer running on the Mac.

## QR scanner — no camera in sim

`QrScanBridge.swift` opens an `AVCaptureSession` against the sim's synthetic camera feed, which never produces a decodable barcode. The demo flow expects scanning the issuer's credential-offer QR code; in the sim, use the paste-fallback instead:

1. On the issuer's `/issue/complete.html`, long-press the QR-URL `<pre>` element → Select All → Copy (or use the "Copy link" button)
2. Force-push to sim pasteboard: `pbpaste | xcrun simctl pbcopy "$SIM_UDID"`
3. Wallet → Identity Centre → paste-URL textarea → ⌘V → tap **Request VC**

## Known wins / fixes

- **`feedback_ios_sim_pasteboard_sync_unreliable`** (auto-memory) — the `PasteboardAutomaticSync` is unreliable; documented above.
- **commit `e89e4a26`** — wallet's network default is now platform-agnostic; iOS gets `Undeployed` by default and the demo works without flipping the picker.
- **commit `e569835a`** — `worker::dispatch_action` is the canonical UI→worker entry point. Prevents the thread-affinity bug that hung the OID4VCI flow on the iOS sim mid-ingestion.
