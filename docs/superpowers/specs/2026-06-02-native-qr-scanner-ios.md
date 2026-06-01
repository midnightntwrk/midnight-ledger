# Native QR Scanner — iOS (Phase 2 of Mobile-Native Modules)

**Date:** 2026-06-02
**Status:** spec
**Scope:** iOS only. Reuses the `QrScanner` trait from Phase 1 (Android).
**Prereqs:** Phase 1 commit `fdba2182` (`AndroidQrScanner` + trait + fallback dispatch).

## Problem

After Phase 1 lands, the wallet has a native ML Kit scanner on
Android and a `eval_bridge` jsQR fallback for every other target.
The iOS WebView (WKWebView) has the same secure-context limitation
as Android Chromium WebView — `navigator.mediaDevices` is gated and
returns `undefined` for the `dioxus://` scheme. So the iOS demo
flow today depends on the paste fallback, exactly like the Android
demo did before Phase 1.

This spec replaces the WebView scanner on iOS with a native
`AVCaptureMetadataOutput`-backed scanner sitting behind the same
`QrScanner` trait, so the iOS demo can scan QR codes natively.

## Goals

1. **Tapping Scan QR in the iOS wallet opens a native full-screen viewfinder.** The decoded payload flows back into the existing dispatch logic — same `openid4vp://…` / `openid-credential-offer://…` routing as Android.
2. **No wallet-side code changes.** All iOS wiring is behind `crate::ActiveQrScanner` (cfg-resolved). `identity_centre.rs::ScanQrSection` already calls `wallet_core::QrScanner::scan()` — Phase 1's trait dispatch is the abstraction boundary.
3. **Graceful permission UX:** if the user denies camera permission, return `QrScanError::Unavailable` with the iOS settings hint; the wallet surfaces it recoverably.
4. **Cancellation:** dismissing the viewfinder returns `QrScanError::Cancelled`, treated as a silent no-op.
5. **No new dependencies.** AVFoundation is part of the iOS SDK — no CocoaPods, SPM, or Carthage needed.

## Non-Goals

- Replacing `paste_text` (Phase 3 — separate spec).
- macOS desktop scanner (continues to use the JS bridge fallback; macOS desktop builds aren't iOS).
- Continuous / multi-code scanning.
- Image-picker fallback ("scan from photo").
- Custom viewfinder UI beyond the system-standard look.

## Approach

Use **AVFoundation's `AVCaptureMetadataOutput`** with `.qr` as the
sole metadata-object type. Apple's API does everything we need:

- Built-in QR (and many other barcode) decoding directly from the camera buffer; no external decoder needed.
- Built-in `AVCapturePreviewLayer` for the viewfinder.
- Built-in `AVCaptureDevice.requestAccess(for: .video)` for runtime permission.
- Returns the decoded string via a delegate callback (`AVCaptureMetadataOutputObjectsDelegate`).

The wallet calls the scanner via a thin Swift "bridge" class
(`QrScanBridge.swift`) exposing a single `startScan(token:)`
class method. Results come back through a `@_cdecl`-exported Rust
function:

```rust
#[unsafe(no_mangle)]
pub extern "C" fn ios_qr_scan_result(token: u64, url: *const c_char, error: *const c_char);
```

Same `token` → `oneshot::Sender` mapping pattern as Android — the
Phase 1 token allocator in `qr_scanner_android.rs` becomes a
shared module to avoid duplication.

### Architecture

```
                ┌─────────────────────────────────────────┐
                │  Rust core (Phase 1)                     │
                │                                          │
                │  trait QrScanner { … }   (wallet-core)  │
                │  Token allocator + PENDING map           │
                │     (extracted to qr_scanner_common.rs)  │
                └────────────────┬─────────────────────────┘
                                 │
            ┌────────────────────┴────────────────────┐
            │                                          │
   Android impl                                  iOS impl
   (Phase 1, fdba2182)                          (this spec)
   ┌──────────────────────────┐                ┌──────────────────────────┐
   │ AndroidQrScanner         │                │ IosQrScanner             │
   │  + JNI                   │                │  + Swift FFI             │
   └──────────────────────────┘                └────────────┬─────────────┘
                                                            │ @_cdecl
                                                            ▼
                                              ┌──────────────────────────┐
                                              │ QrScanBridge.swift       │
                                              │  AVCaptureSession        │
                                              │  AVCapturePreviewLayer   │
                                              │  AVCaptureMetadataOutput │
                                              │  (delegate → Rust)       │
                                              └──────────────────────────┘
```

### Build wiring

The iOS build path goes through `cargo-bundle` / `xcodegen` — see
`mobile-bench/dioxus-wallet/ios/` for the project skeleton. The
Swift file lives at:

```
mobile-bench/dioxus-wallet/ios/Sources/QrScanBridge.swift
```

Xcode picks it up automatically (the project file globs `Sources/`).
The Rust `@_cdecl` symbol is exported from the static library
linked by Xcode; the Swift code declares it as:

```swift
@_silgen_name("ios_qr_scan_result")
private func ios_qr_scan_result(
    _ token: UInt64,
    _ url: UnsafePointer<CChar>?,
    _ error: UnsafePointer<CChar>?
)
```

## Files

### Created

- `mobile-bench/dioxus-wallet/src/qr_scanner_common.rs` — token allocator + `PENDING` map extracted from `qr_scanner_android.rs` so both impls share one set of statics.
- `mobile-bench/dioxus-wallet/src/qr_scanner_ios.rs` — iOS impl (Swift FFI + `ios_qr_scan_result` extern).
- `mobile-bench/dioxus-wallet/ios/Sources/QrScanBridge.swift` — AVFoundation launcher + delegate.

### Modified

- `mobile-bench/dioxus-wallet/src/qr_scanner_android.rs` — refactor to use `qr_scanner_common::{next_token, register, complete}`.
- `mobile-bench/dioxus-wallet/src/lib.rs` — add iOS branch of `ActiveQrScanner` alias.
- `mobile-bench/dioxus-wallet/Cargo.toml` — no new deps (libc + std::ffi are already present transitively).
- `mobile-bench/dioxus-wallet/ios/Info.plist` — add `NSCameraUsageDescription` (required by App Review for any app touching the camera).

## Permission flow

iOS requires:

1. `Info.plist` key `NSCameraUsageDescription` with a human-readable
   reason. Without it, calling `AVCaptureDevice.requestAccess(for:
   .video)` crashes the process with `Set NSCameraUsageDescription`.
2. Runtime call to `AVCaptureDevice.requestAccess(for: .video)`
   on first use. iOS shows a one-time system prompt; the answer is
   remembered. Subsequent calls return the cached answer
   synchronously.

`QrScanBridge.startScan` checks `AVCaptureDevice.authorizationStatus
(for: .video)` first:

- `.authorized` → proceed straight to camera setup
- `.notDetermined` → call `requestAccess`, await result, then
  proceed or surface `Unavailable("camera permission denied")`
- `.denied` / `.restricted` → surface `Unavailable("camera
  permission denied — enable in Settings → Privacy → Camera")`

## Key risks & mitigations

| Risk | Mitigation |
|---|---|
| iOS simulator has no camera | `AVCaptureDevice.default(for: .video) == nil` → surface `Unavailable("camera not available (simulator?)")`. The fallback path is the paste field. |
| `@_cdecl` symbol resolution requires linker flags | The Rust static lib already exports `extern "system"` symbols (JNI ones for Android) — the build pipeline keeps them. Just ensure `Cargo.toml` has `crate-type = ["staticlib", "cdylib"]` for iOS (which it should — check during impl). |
| AVCapturePreviewLayer Z-ordering vs WKWebView | Present the viewfinder as a full-screen modal `UIViewController` over the wallet's root VC, not inline. AVFoundation + WKWebView coexist fine as separate sibling layers. |
| Permission denial dead-ends the user | The Unavailable error message includes the Settings path; UI shows it verbatim. |
| Memory leak if Swift never calls Rust back | Same as Android — `oneshot::Receiver` drop closes the channel; subsequent send fails silently. Token map entry stays until next scan replaces it. Add a `tokio::time::timeout(60s, rx)` in Phase 4 if it becomes a problem. |

## Build sequence

1. Extract shared state into `qr_scanner_common.rs`. Refactor Android impl. `cargo check` both desktop and Android arm64.
2. Add `Info.plist` key.
3. Write `QrScanBridge.swift`.
4. Write `qr_scanner_ios.rs`.
5. Wire `ActiveQrScanner` for `target_os = "ios"` in `lib.rs`.
6. Build via existing iOS pipeline: `cargo build --target aarch64-apple-ios --release` + `xcodebuild` (or whatever the project uses).
7. Install on simulator (`xcrun simctl install`) → simulator should surface Unavailable. Install on physical device → tap Scan QR → viewfinder + permission prompt → scan → result.

## Test plan

End-to-end on iOS simulator (negative cases) + physical iPhone (positive cases):

Simulator (no camera):
- [ ] Tap Scan QR → wallet shows "scanner unavailable: camera not available (simulator?)" → paste fallback works.

Physical device:
- [ ] Cold launch + tap Scan QR → iOS system permission prompt appears once.
- [ ] Grant → viewfinder shows; corner brackets / focus indicator visible.
- [ ] Scan a printed/screen `openid-credential-offer://` QR → wallet runs OID4VCI → VC lands in Inventory.
- [ ] Scan `openid4vp://` → wallet runs OID4VP authentication.
- [ ] Press Cancel during viewfinder → silent return (no banner).
- [ ] Deny permission → wallet shows "camera permission denied — enable in Settings → Privacy → Camera". User opens Settings, flips it on, retry succeeds.
- [ ] Re-tap Scan QR after a successful scan → fresh viewfinder, no stale state.

## Rollback

Trivial: revert this commit. iOS reinstates the JS-bridge fallback
path (which is what runs on iOS today). No infrastructure changes
to undo.

## Follow-ups (not in this spec)

- Phase 3 — replace `paste_text` with native `ClipboardManager`
  (Android) + `UIPasteboard` (iOS). Same trait-+-impl pattern.
  Lets us drop the `eval_bridge` module entirely.
- Phase 4 — add `tokio::time::timeout` to both Android + iOS
  scanners so leaked tokens self-clean after 60s.
- Phase 5 — visualise the scan attempt in the wallet UI (subtle
  shimmer / overlay) so the operator knows scanning is in
  progress even before the OS-level viewfinder paints.
