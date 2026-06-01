# Native QR Scanner — Android (Phase 1 of Mobile-Native Modules)

**Date:** 2026-06-02
**Status:** spec → in-progress (autonomous execution)
**Scope:** Android only. iOS lands in a follow-up spec sharing the same `QrScanner` trait.

## Problem

The wallet's current Scan QR flow uses a WebView-side jsQR scanner reached
through the `eval_bridge` (JS bridge). On Android, `navigator.mediaDevices`
is `undefined` inside our `dioxus://` shell because Chromium's secure-
context check excludes custom schemes. The button has been hard-failing
with `getUserMedia not available` on every Android run; the demo currently
relies on the paste-URL fallback under Diagnostics → Bootstrap.

This spec replaces the WebView scanner on Android with a native scanner
behind a Rust trait, so the same wallet code path keeps working without
the WebView limitation. iOS will implement the same trait in a follow-up.

## Goals

1. **Tapping Scan QR on Android opens a native full-screen camera viewfinder.** The decoded payload (a string) flows back to the existing dispatch logic — same `openid4vp://…` / `openid-credential-offer://…` branching as before.
2. **Single Rust API the wallet uses everywhere:** `QrScanner::scan() -> Result<String, ScanError>`. The platform is invisible to the wallet code.
3. **Graceful permission UX:** if the user denies camera permission, the future resolves to `ScanError::PermissionDenied` and the wallet surfaces it like any other recoverable error.
4. **Cancellation:** dismissing the scanner returns `ScanError::Cancelled`, treated as a no-op (no banner, no error).
5. **Bridge unchanged on desktop:** `eval_bridge::scan_qr` keeps working on macOS/Linux/Windows WebView builds. We only swap the implementation behind a `#[cfg(target_os = "android")]` gate.

## Non-Goals

- iOS implementation (separate spec).
- Replacing `eval_bridge::paste_text`. The bridge stays for clipboard reads — Phase-3 work.
- Continuous scanning / multi-code support. One scan per tap.
- Image-picker fallback ("scan from photo"). Future enhancement.
- Custom viewfinder UI. ML Kit's built-in viewfinder is enough.

## Approach

Use **Google ML Kit's Code Scanner API** (`com.google.android.gms:play-services-code-scanner`). It is:

- Bundled with Google Play Services — no native libs to cross-compile.
- Built-in full-screen viewfinder, focus indicator, torch toggle.
- Built-in runtime permission flow — caller doesn't need to handle it.
- Supports QR + most 1D / 2D barcode formats. We restrict to `QR_CODE` for the wallet's scope.
- Cancellation surfaces as `MlKitException.CODE_SCANNER_CANCELLED`.

The wallet calls the scanner via a tiny Kotlin "bridge" class that exposes
`startScan(activity, token)`. Results come back through a native callback
registered as a `static external fun nativeOnQrResult(token: Long, url:
String?, error: String?)`. Each in-flight scan gets a `u64` token paired
with a Rust `oneshot::Sender<Result<String, ScanError>>` in a process-
global map, so the callback finds the right caller.

### Architecture

```
                ┌─────────────────────────────────────────┐
                │  Rust core (cross-platform)              │
                │                                          │
                │  trait QrScanner: Send + Sync {         │
                │    async fn scan(&self)                 │
                │      -> Result<String, ScanError>;      │
                │  }                                       │
                │                                          │
                │  enum ScanError {                       │
                │    Cancelled,                           │
                │    PermissionDenied,                    │
                │    Unavailable(String),                 │
                │    Decoder(String),                     │
                │  }                                       │
                │                                          │
                │  Wallet calls: scanner.scan().await      │
                └────────────────┬─────────────────────────┘
                                 │
                ┌────────────────┴─────────────────────────┐
                │                                          │
       Android impl                              Desktop / iOS fallback
       (this spec)                               (existing eval_bridge,
                                                  iOS = Phase-2 spec)
       ┌──────────────────────────┐              ┌────────────────────────┐
       │ AndroidQrScanner         │              │ JsBridgeQrScanner       │
       │   .scan()                │              │   .scan()               │
       │                          │              │     → eval_bridge::     │
       │ • Allocate token         │              │       scan_qr()         │
       │ • Register oneshot       │              └────────────────────────┘
       │ • JNI: QrScanBridge      │
       │     .startScan(act,tok)  │
       │ • Await rx               │
       └────────────┬─────────────┘
                    │ JNI
                    ▼
       ┌──────────────────────────┐
       │ QrScanBridge.kt (Kotlin) │
       │   GmsBarcodeScanning     │
       │     .getClient(ctx)      │
       │     .startScan()         │
       │   onSuccess/onFailure →  │
       │     JNI nativeOnQrResult │
       └──────────────────────────┘
```

### Token / callback flow

```
scan()  ──┐
          │ token = NEXT.fetch_add(1)
          │ (tx, rx) = oneshot::channel()
          │ PENDING.lock().insert(token, tx)
          │ JNI call into Kotlin with token
          ▼
   Kotlin / ML Kit               (UI: viewfinder)
          │
          │ on success → nativeOnQrResult(token, url, null)
          │ on failure → nativeOnQrResult(token, null, error)
          ▼
   Java_..._nativeOnQrResult     (JNI handler, Rust)
          │ tx = PENDING.lock().remove(&token).unwrap()
          │ tx.send(Ok(url) | Err(error))
          ▼
   rx.await resolves → wallet receives the result.
```

The PENDING map is the canonical pattern for FFI request/reply — the
token must outlive the JNI call but doesn't need to be cryptographically
unguessable (process-local).

## Files

### Created

- `mobile-bench/wallet-core/src/qr_scanner.rs` — the `QrScanner` trait + `ScanError`.
- `mobile-bench/dioxus-wallet/src/qr_scanner_android.rs` — Android impl (JNI + token map + `nativeOnQrResult` extern).
- `mobile-bench/dioxus-wallet/src/qr_scanner_fallback.rs` — desktop/iOS impl that delegates to `eval_bridge::scan_qr` (keeps existing behaviour).
- `mobile-bench/dioxus-wallet/android/app/src/main/kotlin/io/iohk/midnight/wallet/QrScanBridge.kt` — ML Kit launcher.

### Modified

- `mobile-bench/wallet-core/Cargo.toml` — add the `QrScanner` trait module to `lib.rs` exports (no new deps).
- `mobile-bench/dioxus-wallet/Cargo.toml` — add `jni = "0.21"`, `ndk-context = "0.1"` (Android-only via `[target.'cfg(target_os = "android")'.dependencies]`).
- `mobile-bench/dioxus-wallet/src/lib.rs` — wire the new modules under `#[cfg]`.
- `mobile-bench/dioxus-wallet/src/identity_centre.rs` — `scan_and_dispatch` now calls `QrScanner::scan()` (resolved at compile time to the right impl) instead of `eval_bridge::scan_qr`. The existing `getUserMedia not available` arm is preserved for desktop builds and as a fallback.
- `mobile-bench/dioxus-wallet/android/app/build.gradle.kts` — add the ML Kit dependency.
- `mobile-bench/dioxus-wallet/android/app/build.gradle.kts` — enable Kotlin plugin if not already (likely already is — wry generates Kotlin).
- `mobile-bench/dioxus-wallet/android/app/src/main/AndroidManifest.xml` — confirm `android.permission.CAMERA` is declared (already is per current manifest comment).
- `mobile-bench/dioxus-wallet/android/TAILSCALE.md` — add a "If you replace the APK" note: ML Kit needs Play Services, scanner will fail to start on AOSP-only devices (degrade-gracefully behaviour: surface error, point to paste fallback).

## Key risks & mitigations

| Risk | Mitigation |
|---|---|
| `dev.dioxus.main.MainActivity` is wry-generated; we can't subclass without forking wry | We don't need to subclass. ML Kit's Code Scanner takes the Activity context from `ndk_context::android_context()` — no Activity changes needed. |
| JNI `FindClass` from a non-Java thread fails | Acquire the class via `env.find_class()` on the main thread via `AttachCurrentThread`. ML Kit's `addOnSuccessListener` runs on the main thread, so our `nativeOnQrResult` callback originates on a Java-attached thread automatically. |
| ML Kit not present (GrapheneOS / non-Play devices) | Detect via `GoogleApiAvailability.isGooglePlayServicesAvailable` before calling `startScan`. If unavailable, the impl resolves with `ScanError::Unavailable("Play Services not available — use paste fallback")`. |
| Token map leak if Kotlin never calls back | `oneshot::Receiver` is dropped when the wallet code drops the future. If the caller times out, the entry stays until the next scan replaces it. Acceptable for now; if it becomes a problem add a `tokio::time::timeout` in the trait impl. |
| Build complexity — Cargo + Kotlin glue | Three concrete changes: add ML Kit Gradle dep, add `QrScanBridge.kt`, add JNI extern. No new Gradle plugins. |
| Re-entrant scans (user double-taps) | The `busy` signal in `scan_and_dispatch` already guards this. |

## Permission flow

ML Kit's Code Scanner **handles the runtime permission prompt itself**.
Our app only needs to:

1. Declare `android.permission.CAMERA` in the manifest (already present).
2. Call `startScan()`. ML Kit checks permission, prompts if needed, returns through the listener.

If the user denies, `addOnFailureListener` fires with
`MlKitException.CODE_SCANNER_CANCELLED` (yes — same as cancellation; the
docs note this is intentional, and there's no separate "denied" code).
We treat it as `ScanError::Cancelled`. The user retries by tapping
Scan QR again; ML Kit will prompt once more.

## Build sequence

1. Modify Cargo manifests + create Rust modules.
2. `cargo ndk -t arm64-v8a build --release -p dioxus-wallet --lib` — confirms Rust compiles (uses `jni` + `ndk-context`).
3. Add Kotlin file under `android/app/src/main/kotlin/io/iohk/midnight/wallet/`.
4. Add ML Kit dep to `build.gradle.kts`.
5. `./gradlew assembleDebug` — confirms Kotlin compiles + ML Kit resolves.
6. `adb install -r app-debug.apk` + relaunch.
7. Tap Scan QR on the phone → viewfinder appears → scan a known-good QR → URL flows back to wallet → existing OID4VP/OID4VCI dispatch runs.

## Test plan

End-to-end on physical phone (no automated tests for this — UI + Play Services involved):

- [ ] Cold launch + tap Scan QR → permission prompt appears the first time.
- [ ] Grant permission → viewfinder shows.
- [ ] Scan a printed/screen QR with an `openid-credential-offer://…` URL → wallet runs OID4VCI issuance flow → VC lands in Inventory.
- [ ] Scan an `openid4vp://…` URL → wallet runs OID4VP authentication flow.
- [ ] Scan a malformed payload → wallet shows "Unsupported QR payload" error.
- [ ] Press back during viewfinder → `ScanError::Cancelled` → wallet silently returns to idle (no error banner).
- [ ] Deny permission → wallet shows recoverable error.
- [ ] Repeat scan back-to-back → both succeed (token map is correctly cleared).

## Rollback

Trivial: revert the commit. The Kotlin file and `qr_scanner_android.rs`
sit behind `#[cfg(target_os = "android")]`; reverting reinstates the
JS-bridge path on Android, which still produces the recoverable "Camera
unavailable" error introduced in commit `f1dffe5e`.

## Follow-ups (not in this spec)

- **iOS implementation** — `QrScannerViewController` + `AVCaptureMetadataOutput` + Swift FFI. Separate spec.
- **Clipboard replacement** — `JsBridgeQrScanner`'s sibling `pasteText` can be moved to `ClipboardManager` (Android) / `UIPasteboard` (iOS) using the same trait pattern.
- **AOSP camera path** — if we ever ship to non-Play devices, swap `GmsBarcodeScanning` for CameraX + `rqrr` (~200 lines more). Same `QrScanner` trait, only the impl changes.
- **Spec → SDK upstream** — if Dioxus SDK eventually ships a `camera` module, swap our impl for theirs. Wallet code doesn't change.
