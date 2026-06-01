# Native QR Scanner — Android Implementation Plan

> **For agentic workers:** Execute each task in order. Each task ends with a signed commit (DCO + GPG via `git commit -S -s`). Per repo convention, `bash ~/iohk/git-iohk.sh` was run at session start.

**Goal:** Replace the WebView-side jsQR scanner on Android with a native ML Kit Code Scanner reached through a Rust `QrScanner` trait. Wallet code calls the trait; the implementation is swapped per platform at compile time. iOS lands in a follow-up.

**Architecture:** Rust trait in `wallet-core` defines the contract. Android impl in `dioxus-wallet` uses JNI + `ndk-context` to call a thin Kotlin shell (`QrScanBridge`) which wraps `com.google.android.gms.code.scanner.GmsBarcodeScanning`. A static `nativeOnQrResult` JNI callback delivers results to a process-global token → oneshot-channel map.

**Tech Stack:** Rust, JNI (`jni` crate 0.21), `ndk-context` 0.1, Kotlin, Google ML Kit `play-services-code-scanner` 16.1.0.

---

### Task 1: `QrScanner` trait + `ScanError` enum in wallet-core

**Files:**
- Create: `mobile-bench/wallet-core/src/qr_scanner.rs`
- Modify: `mobile-bench/wallet-core/src/lib.rs` (add `pub mod qr_scanner;` and re-export)

- [ ] **Step 1: Write the trait**

```rust
// mobile-bench/wallet-core/src/qr_scanner.rs
//! Camera-driven QR scanner trait. Implementations are platform-
//! specific (Android: ML Kit; iOS: AVFoundation; desktop: WebView
//! getUserMedia via the JS bridge). The wallet's identity-centre
//! flow calls `scan()` once and lets cargo's `#[cfg]` pick the
//! right impl at compile time.
//!
//! All errors are recoverable from the operator's point of view:
//! `Cancelled` → silent return; everything else → red banner with
//! the inner message + a hint to use the paste fallback.

use std::error::Error;
use std::fmt;

/// Outcome variants the wallet needs to distinguish.
#[derive(Debug, Clone)]
pub enum ScanError {
    /// User dismissed the viewfinder or denied permission. The
    /// wallet treats this as a no-op — no banner.
    Cancelled,
    /// Scanner can't run on this device (e.g. Play Services
    /// missing on Android, or the WebView denies getUserMedia
    /// for a secure-context reason on desktop). Inner string is a
    /// short human-readable explanation suitable for UI.
    Unavailable(String),
    /// Scanner ran but decoding failed or the platform layer
    /// returned an opaque error. Inner string is the underlying
    /// platform message.
    Decoder(String),
}

impl fmt::Display for ScanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled => write!(f, "cancelled"),
            Self::Unavailable(s) => write!(f, "scanner unavailable: {s}"),
            Self::Decoder(s) => write!(f, "scan failed: {s}"),
        }
    }
}

impl Error for ScanError {}

/// Platform-agnostic camera-driven QR scanner. The wallet holds a
/// `dyn QrScanner` and never knows which impl is running underneath.
#[async_trait::async_trait]
pub trait QrScanner: Send + Sync {
    /// Open a full-screen viewfinder, return the first decoded
    /// payload, or surface a `ScanError`. Single-shot — the
    /// viewfinder closes automatically on success.
    async fn scan(&self) -> Result<String, ScanError>;
}
```

- [ ] **Step 2: Export from lib.rs**

```rust
// mobile-bench/wallet-core/src/lib.rs — add near the other pub mods
pub mod qr_scanner;
pub use qr_scanner::{QrScanner, ScanError};
```

- [ ] **Step 3: Verify compile**

Run: `cd mobile-bench && cargo check -p wallet-core`
Expected: builds cleanly. `async_trait` is already a transitive dep through `chain.rs::NodeClient`; if not, add `async-trait = "0.1"` to `wallet-core/Cargo.toml`.

- [ ] **Step 4: Commit**

```bash
git add mobile-bench/wallet-core/src/qr_scanner.rs mobile-bench/wallet-core/src/lib.rs mobile-bench/wallet-core/Cargo.toml
git commit -S -s -m "feat(wallet-core): add QrScanner trait + ScanError enum

Introduces the platform-agnostic surface that Android, iOS, and
desktop QR-scanner implementations will share. The wallet's UI
code holds a \`dyn QrScanner\` and never knows which platform
backend is running. Each variant of \`ScanError\` maps to a
distinct UX path (silent return on Cancelled, banner with paste
hint on Unavailable/Decoder).

Co-Authored-By: Claude Sonnet 4.5 <noreply@anthropic.com>"
```

---

### Task 2: Fallback impl (desktop / iOS placeholder) delegating to eval_bridge

**Files:**
- Create: `mobile-bench/dioxus-wallet/src/qr_scanner_fallback.rs`

This impl preserves today's behaviour on every non-Android target — including iOS until its native impl lands.

- [ ] **Step 1: Write the fallback**

```rust
// mobile-bench/dioxus-wallet/src/qr_scanner_fallback.rs
//! Non-Android fallback: delegates to the existing JS bridge so
//! macOS / Linux / Windows desktop builds keep working unchanged.
//! iOS will eventually replace this with a native AVFoundation
//! impl, at which point this file becomes desktop-only.

use async_trait::async_trait;
use wallet_core::{QrScanner, ScanError};

use crate::eval_bridge;

pub struct FallbackQrScanner;

#[async_trait]
impl QrScanner for FallbackQrScanner {
    async fn scan(&self) -> Result<String, ScanError> {
        let Some(bridge) = eval_bridge::global_bridge() else {
            return Err(ScanError::Unavailable(
                "JS bridge not installed (js-bridge feature off?)".into(),
            ));
        };
        match eval_bridge::scan_qr(&*bridge).await {
            Ok(url) => Ok(url),
            Err(wallet_core::js_bridge::JsBridgeError::Transport(msg))
                if msg == "cancelled" =>
            {
                Err(ScanError::Cancelled)
            }
            Err(wallet_core::js_bridge::JsBridgeError::Transport(msg))
                if msg.contains("getUserMedia not available") =>
            {
                Err(ScanError::Unavailable(
                    "Camera unavailable in this WebView \
                     (secure-context limit). Paste the OID4VC URL \
                     in Diagnostics → Bootstrap instead."
                        .into(),
                ))
            }
            Err(e) => Err(ScanError::Decoder(e.to_string())),
        }
    }
}
```

- [ ] **Step 2: Verify compile** (no integration yet; covered by Task 5)

Skip — this file compiles standalone via the wallet-core re-exports. Real check lands when wired into lib.rs in Task 5.

- [ ] **Step 3: Commit (deferred — bundle with Task 5 wiring)**

Defer commit; Task 5 will commit `qr_scanner_fallback.rs` + lib.rs + identity_centre changes together as one coherent unit (the cfg-routed surface).

---

### Task 3: Kotlin bridge — `QrScanBridge.kt`

**Files:**
- Create: `mobile-bench/dioxus-wallet/android/app/src/main/kotlin/io/iohk/midnight/wallet/QrScanBridge.kt`

Verify the Kotlin source directory exists first; if not, create it. The Dioxus-generated `android/app/src/main/` layout typically has `java/dev/dioxus/main/` only; adding our own `kotlin/io/iohk/midnight/wallet/` is the convention.

- [ ] **Step 1: Confirm kotlin source dir support in build.gradle.kts**

```bash
grep -E "kotlin|srcDirs" mobile-bench/dioxus-wallet/android/app/build.gradle.kts | head -10
```
Expected: existing `kotlin { jvmToolchain(... ) }` block or `androidx.compose` plugin lines proving Kotlin compilation is already wired. If not, add the Kotlin plugin (Task 4).

- [ ] **Step 2: Create the directory + file**

```bash
mkdir -p mobile-bench/dioxus-wallet/android/app/src/main/kotlin/io/iohk/midnight/wallet
```

```kotlin
// mobile-bench/dioxus-wallet/android/app/src/main/kotlin/io/iohk/midnight/wallet/QrScanBridge.kt
package io.iohk.midnight.wallet

import android.app.Activity
import com.google.android.gms.common.ConnectionResult
import com.google.android.gms.common.GoogleApiAvailability
import com.google.android.gms.common.moduleinstall.ModuleInstall
import com.google.android.gms.common.moduleinstall.ModuleInstallRequest
import com.google.mlkit.common.MlKitException
import com.google.mlkit.vision.codescanner.GmsBarcodeScannerOptions
import com.google.mlkit.vision.codescanner.GmsBarcodeScanning
import com.google.mlkit.vision.barcode.common.Barcode

/**
 * Thin shell wrapping Google ML Kit's Code Scanner. Called from
 * Rust via JNI. The companion `nativeOnQrResult` symbol is
 * implemented in `qr_scanner_android.rs`; each in-flight scan
 * carries a `token: Long` so the Rust side can match the result
 * to its waiting oneshot::Sender.
 *
 * ML Kit handles the camera permission prompt + viewfinder UI
 * itself — we don't need a custom Activity. Restrict to QR codes
 * (FORMAT_QR_CODE) because the wallet only consumes OID4VC URLs;
 * anything else would be misleading.
 */
class QrScanBridge {
    companion object {
        /**
         * Implemented in libdioxuswalletmain.so via
         * `Java_io_iohk_midnight_wallet_QrScanBridge_nativeOnQrResult`.
         * `url` is non-null on success; `error` is non-null on
         * failure (or cancellation — ML Kit reports both via
         * MlKitException.CODE_SCANNER_CANCELLED, so the Rust side
         * maps `error == "cancelled"` to ScanError::Cancelled).
         */
        @JvmStatic
        external fun nativeOnQrResult(token: Long, url: String?, error: String?)

        /**
         * Entry point called from Rust. Verifies Play Services is
         * present, builds the QR-only client, and starts the scan.
         * Result flows back through `nativeOnQrResult` on either
         * the success or failure path.
         */
        @JvmStatic
        fun startScan(activity: Activity, token: Long) {
            val availability = GoogleApiAvailability.getInstance()
            val resultCode = availability.isGooglePlayServicesAvailable(activity)
            if (resultCode != ConnectionResult.SUCCESS) {
                nativeOnQrResult(
                    token,
                    null,
                    "Play Services unavailable (code=$resultCode)",
                )
                return
            }

            val options = GmsBarcodeScannerOptions.Builder()
                .setBarcodeFormats(Barcode.FORMAT_QR_CODE)
                .build()
            val scanner = GmsBarcodeScanning.getClient(activity, options)

            scanner.startScan()
                .addOnSuccessListener { barcode ->
                    val raw = barcode.rawValue
                    if (raw.isNullOrEmpty()) {
                        nativeOnQrResult(token, null, "empty QR payload")
                    } else {
                        nativeOnQrResult(token, raw, null)
                    }
                }
                .addOnFailureListener { e ->
                    val msg = when {
                        e is MlKitException && e.errorCode ==
                            MlKitException.CODE_SCANNER_CANCELLED -> "cancelled"
                        else -> e.message ?: "scan failed"
                    }
                    nativeOnQrResult(token, null, msg)
                }
        }
    }
}
```

- [ ] **Step 3: Defer compile check — covered by Task 4's Gradle build**

---

### Task 4: Gradle — add ML Kit dependency + (if needed) Kotlin plugin

**Files:**
- Modify: `mobile-bench/dioxus-wallet/android/app/build.gradle.kts`

- [ ] **Step 1: Read current build.gradle.kts**

```bash
cat mobile-bench/dioxus-wallet/android/app/build.gradle.kts
```

If Kotlin plugin is missing (only `id("com.android.application")` present), add `id("org.jetbrains.kotlin.android") version "1.9.22"` to the `plugins {}` block.

- [ ] **Step 2: Add ML Kit dep**

In the `dependencies {}` block, add:

```kotlin
implementation("com.google.android.gms:play-services-code-scanner:16.1.0")
```

- [ ] **Step 3: Run gradle build to confirm resolution**

```bash
cd mobile-bench/dioxus-wallet/android
JAVA_HOME=/Library/Java/JavaVirtualMachines/temurin-21.jdk/Contents/Home ./gradlew assembleDebug 2>&1 | tail -20
```

Expected: BUILD SUCCESSFUL (the Kotlin file references ML Kit imports which now resolve).

- [ ] **Step 4: Defer commit (bundle with Task 5)**

---

### Task 5: Android Rust impl — `qr_scanner_android.rs` + lib.rs wiring + Cargo deps

**Files:**
- Create: `mobile-bench/dioxus-wallet/src/qr_scanner_android.rs`
- Modify: `mobile-bench/dioxus-wallet/Cargo.toml`
- Modify: `mobile-bench/dioxus-wallet/src/lib.rs`

- [ ] **Step 1: Add Cargo target-gated deps**

Append to `mobile-bench/dioxus-wallet/Cargo.toml`:

```toml
[target.'cfg(target_os = "android")'.dependencies]
jni = "0.21"
ndk-context = "0.1"
```

- [ ] **Step 2: Write the Android impl**

```rust
// mobile-bench/dioxus-wallet/src/qr_scanner_android.rs
//! Android implementation of [`wallet_core::QrScanner`]. Wraps
//! Google ML Kit's Code Scanner via a thin Kotlin shell
//! (`QrScanBridge.kt`) reached through JNI. Each in-flight scan
//! gets a process-local `u64` token paired with a
//! `tokio::sync::oneshot::Sender` in `PENDING`; the Kotlin side
//! calls `Java_io_iohk_midnight_wallet_QrScanBridge_nativeOnQrResult`
//! when ML Kit resolves, and we route the result back through the
//! channel.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;

use async_trait::async_trait;
use jni::objects::{JClass, JObject, JString};
use jni::sys::{jlong, jstring};
use jni::{JNIEnv, JavaVM};
use tokio::sync::oneshot;

use wallet_core::{QrScanner, ScanError};

/// Process-global token allocator. Wraps around at u64::MAX,
/// which won't happen in any realistic session.
static NEXT_TOKEN: AtomicU64 = AtomicU64::new(1);

/// Maps in-flight scan tokens to the `oneshot::Sender` waiting for
/// their result. Pruned when `nativeOnQrResult` delivers, or when
/// the future is dropped (the receiver's drop closes the channel,
/// our `send` then errors and we drop the sender silently).
static PENDING: OnceLock<Mutex<HashMap<u64, oneshot::Sender<Result<String, ScanError>>>>> =
    OnceLock::new();

fn pending() -> &'static Mutex<HashMap<u64, oneshot::Sender<Result<String, ScanError>>>> {
    PENDING.get_or_init(|| Mutex::new(HashMap::new()))
}

pub struct AndroidQrScanner;

#[async_trait]
impl QrScanner for AndroidQrScanner {
    async fn scan(&self) -> Result<String, ScanError> {
        let token = NEXT_TOKEN.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        pending().lock().unwrap().insert(token, tx);

        // Drop any in-flight entry if launching the JNI call
        // fails — we still want the slot freed.
        if let Err(err) = launch_kotlin_scan(token) {
            pending().lock().unwrap().remove(&token);
            return Err(ScanError::Unavailable(format!(
                "could not launch native scanner: {err}",
            )));
        }

        // If the receiver errors (sender dropped without sending —
        // shouldn't happen unless Kotlin crashes), surface as a
        // decoder error so the user can retry.
        rx.await.unwrap_or_else(|_| {
            Err(ScanError::Decoder(
                "native scanner channel closed unexpectedly".into(),
            ))
        })
    }
}

fn launch_kotlin_scan(token: u64) -> Result<(), String> {
    let ctx = ndk_context::android_context();
    if ctx.vm().is_null() || ctx.context().is_null() {
        return Err("ndk-context not initialised (no Activity)".into());
    }

    // SAFETY: `ctx.vm()` is a valid `JavaVM*` for the lifetime of
    // the process — set by android_logger / wry at app start.
    let vm = unsafe { JavaVM::from_raw(ctx.vm().cast()) }
        .map_err(|e| format!("JavaVM::from_raw: {e}"))?;
    let mut env = vm
        .attach_current_thread()
        .map_err(|e| format!("attach_current_thread: {e}"))?;

    // SAFETY: `ctx.context()` is a valid jobject for the host
    // Activity, owned by wry.
    let activity = unsafe { JObject::from_raw(ctx.context().cast()) };

    env.call_static_method(
        "io/iohk/midnight/wallet/QrScanBridge",
        "startScan",
        "(Landroid/app/Activity;J)V",
        &[(&activity).into(), (token as jlong).into()],
    )
    .map_err(|e| format!("call_static_method: {e}"))?;

    Ok(())
}

/// JNI entry point — invoked by ML Kit's success/failure listener
/// on the main (Java-attached) thread. Resolves the matching
/// `oneshot::Sender` and removes it from `PENDING`. If `error` is
/// non-null we surface the right `ScanError` variant; `"cancelled"`
/// is special-cased to mean "user-initiated abort".
#[no_mangle]
pub extern "system" fn Java_io_iohk_midnight_wallet_QrScanBridge_nativeOnQrResult<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    token: jlong,
    url: JString<'local>,
    error: JString<'local>,
) {
    let token = token as u64;
    let sender = pending().lock().unwrap().remove(&token);
    let Some(sender) = sender else {
        // Late callback for a scan whose future was already
        // dropped (caller cancelled). Nothing to do.
        return;
    };

    let result: Result<String, ScanError> = if !error.is_null() {
        match env.get_string(&error) {
            Ok(s) => {
                let msg: String = s.into();
                if msg == "cancelled" {
                    Err(ScanError::Cancelled)
                } else if msg.starts_with("Play Services unavailable") {
                    Err(ScanError::Unavailable(msg))
                } else {
                    Err(ScanError::Decoder(msg))
                }
            }
            Err(e) => Err(ScanError::Decoder(format!(
                "could not decode JNI error string: {e}",
            ))),
        }
    } else if !url.is_null() {
        match env.get_string(&url) {
            Ok(s) => Ok(s.into()),
            Err(e) => Err(ScanError::Decoder(format!(
                "could not decode JNI url string: {e}",
            ))),
        }
    } else {
        Err(ScanError::Decoder("both url and error were null".into()))
    };

    // If the receiver is gone (future dropped between
    // `remove` and `send`), the send fails silently — fine.
    let _ = sender.send(result);
}
```

- [ ] **Step 3: Wire into lib.rs**

```rust
// mobile-bench/dioxus-wallet/src/lib.rs — add near other cfg-gated modules
#[cfg(target_os = "android")]
mod qr_scanner_android;
#[cfg(not(target_os = "android"))]
mod qr_scanner_fallback;

#[cfg(target_os = "android")]
pub use qr_scanner_android::AndroidQrScanner as ActiveQrScanner;
#[cfg(not(target_os = "android"))]
pub use qr_scanner_fallback::FallbackQrScanner as ActiveQrScanner;
```

- [ ] **Step 4: Cross-compile check**

```bash
ANDROID_NDK_HOME=$HOME/Library/Android/sdk/ndk/27.0.12077973 \
  cargo ndk -t arm64-v8a -o mobile-bench/dioxus-wallet/android/app/src/main/jniLibs \
  build --release -p dioxus-wallet --lib 2>&1 | tail -10
```
Expected: BUILD SUCCESSFUL. If `ndk-context` complains about a missing init, the wry shell initialises it for us via its own JNI entry point — no extra work needed.

- [ ] **Step 5: Defer commit (bundle with Task 6)**

---

### Task 6: Wallet UI integration — swap eval_bridge::scan_qr for ActiveQrScanner

**Files:**
- Modify: `mobile-bench/dioxus-wallet/src/identity_centre.rs`

- [ ] **Step 1: Replace the scan call**

Find the existing `eval_bridge::scan_qr(&*bridge).await` call inside `ScanQrSection::scan_and_dispatch` (search for `let url = match eval_bridge::scan_qr`). Replace the entire `match` arm body so the call flows through the trait:

```rust
use crate::ActiveQrScanner;
use wallet_core::{QrScanner as _, ScanError};

let scanner = ActiveQrScanner;
let url = match scanner.scan().await {
    Ok(u) => u,
    Err(ScanError::Cancelled) => {
        busy.set(false);
        return;
    }
    Err(ScanError::Unavailable(msg)) => {
        err_msg.set(Some(msg));
        busy.set(false);
        return;
    }
    Err(ScanError::Decoder(msg)) => {
        err_msg.set(Some(format!("scan failed: {msg}")));
        busy.set(false);
        return;
    }
};
```

The previous `Some(bridge) = eval_bridge::global_bridge() else {…}` guard is no longer needed at this call site — `ActiveQrScanner` carries no per-instance state and is `Send + Sync`.

- [ ] **Step 2: Full cross-build**

```bash
cd mobile-bench/dioxus-wallet
ANDROID_NDK_HOME=$HOME/Library/Android/sdk/ndk/27.0.12077973 \
MIDNIGHT_INDEXER_HTTP_URL="http://100.110.241.102:18088/api/v4/graphql" \
MIDNIGHT_INDEXER_WS_URL="ws://100.110.241.102:18088/api/v4/graphql/ws" \
MIDNIGHT_NODE_WS_URL="ws://100.110.241.102:19944" \
MIDNIGHT_PROOF_SERVER_URL="http://100.110.241.102:16300" \
  cargo ndk -t arm64-v8a -o android/app/src/main/jniLibs \
  build --release -p dioxus-wallet --lib
```
Expected: builds cleanly.

```bash
cd mobile-bench/dioxus-wallet/android
JAVA_HOME=/Library/Java/JavaVirtualMachines/temurin-21.jdk/Contents/Home \
  ./gradlew assembleDebug
```
Expected: BUILD SUCCESSFUL — the Kotlin file compiles against ML Kit + the JNI symbol from libdioxuswalletmain.so links cleanly.

- [ ] **Step 3: Install on the phone + smoke test**

```bash
adb install -r mobile-bench/dioxus-wallet/android/app/build/outputs/apk/debug/app-debug.apk
adb shell am force-stop io.iohk.midnight.wallet
adb shell am start -n io.iohk.midnight.wallet/dev.dioxus.main.MainActivity
```

Tap Scan QR. Expected: full-screen ML Kit viewfinder appears (asks for camera permission the first time). Scan an `openid-credential-offer://` QR from the issuer page → returns to wallet, runs OID4VCI flow, lands a new VC in the Inventory.

- [ ] **Step 4: Commit (the bundle)**

```bash
git add mobile-bench/dioxus-wallet/Cargo.toml \
        mobile-bench/dioxus-wallet/src/qr_scanner_fallback.rs \
        mobile-bench/dioxus-wallet/src/qr_scanner_android.rs \
        mobile-bench/dioxus-wallet/src/lib.rs \
        mobile-bench/dioxus-wallet/src/identity_centre.rs \
        mobile-bench/dioxus-wallet/android/app/build.gradle.kts \
        mobile-bench/dioxus-wallet/android/app/src/main/kotlin/io/iohk/midnight/wallet/QrScanBridge.kt
git commit -S -s -m "feat(android): native QR scanner via ML Kit Code Scanner

Replaces the WebView-side jsQR scanner with Google ML Kit's
Code Scanner on Android. The wallet's identity-centre flow now
goes through a platform-neutral \`QrScanner\` trait
(wallet-core) backed by an Android impl that JNIs into a thin
Kotlin shell (\`QrScanBridge\`). Other platforms keep using the
existing JS bridge via the fallback impl, so desktop builds are
unchanged.

The previous secure-context limitation
(\`getUserMedia not available\` from the dioxus:// scheme)
disappears entirely — ML Kit owns the camera + viewfinder UI
and handles the runtime permission prompt itself.

ScanError variants are wired:
- Cancelled (user dismiss / permission deny) → silent no-op
- Unavailable (Play Services missing, etc.) → recoverable banner
- Decoder (anything else)                   → recoverable banner

iOS implementation lands in a follow-up spec sharing the same
trait. No wallet code outside the trait dispatch will change
when that arrives.

Co-Authored-By: Claude Sonnet 4.5 <noreply@anthropic.com>"
```

- [ ] **Step 5: Verify signature**

```bash
git log --format="%h %G? %s" -1
```
Expected: leading `G` (good GPG signature).

---

### Task 7: TAILSCALE.md update — note ML Kit dependency

**Files:**
- Modify: `mobile-bench/dioxus-wallet/android/TAILSCALE.md`

- [ ] **Step 1: Append a short note**

Add to the Troubleshooting section:

```markdown
- **Scan QR opens then immediately closes with "scanner unavailable:
  Play Services unavailable (code=…)"** — your phone doesn't have
  Google Play Services available (GrapheneOS, Huawei post-2020,
  emulator without Play). Use the paste field under
  Diagnostics → Bootstrap instead. ML Kit's standalone-Android
  fallback (CameraX + rqrr) lives behind the same `QrScanner`
  trait — file an issue if you need it.
```

- [ ] **Step 2: Commit (small, on its own)**

```bash
git add mobile-bench/dioxus-wallet/android/TAILSCALE.md
git commit -S -s -m "docs(android): note Play Services requirement for native QR scanner

Co-Authored-By: Claude Sonnet 4.5 <noreply@anthropic.com>"
```

---

## Acceptance

All of these must hold on a physical Android phone (Samsung Galaxy
S24 Ultra, model SM-S928B, ID R5CX82NAS0P) before the plan is
considered done:

- [ ] Tap Scan QR → ML Kit viewfinder appears full-screen.
- [ ] First-run camera permission prompt appears once; grant it.
- [ ] Scan a real `openid-credential-offer://` from the issuer page → wallet receives the URL → existing OID4VCI path runs → VC lands in Inventory.
- [ ] Press back during viewfinder → silent return (no banner).
- [ ] Re-tap Scan QR after a successful scan → viewfinder re-opens cleanly (token map free of stale entries).
- [ ] Existing desktop build (`cargo run -p dioxus-wallet`) still scans via the JS bridge — no regression.
- [ ] No new compile warnings (-Werror is not on but warnings should stay quiet).

## Out of scope (do not bundle into this plan)

- iOS native scanner.
- Replacing `paste_text` with native ClipboardManager.
- Removing `eval_bridge` entirely.
- Continuous / multi-code scanning.
- CameraX + rqrr fallback for AOSP-only devices.

Each lands in its own follow-up plan.
