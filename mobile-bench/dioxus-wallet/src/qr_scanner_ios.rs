//! iOS implementation of [`wallet_core::QrScanner`]. Mirrors the
//! Android adapter (`qr_scanner_android.rs`) shape:
//!
//! - Stateless `IosQrScanner` struct, constructed on demand at
//!   each scan site.
//! - `scan()` returns `Pin<Box<dyn Future>>`. Inside, allocate a
//!   process-local `u64` token, register a `oneshot::Sender` in
//!   `PENDING`, hand the token to Swift via a C-ABI function the
//!   shell defines (`iosqr_present_scanner`), then await the
//!   matching `oneshot::Receiver`.
//! - Swift opens an `AVCaptureSession`-backed scanner view
//!   controller. When the user dismisses, cancels, or a barcode
//!   is detected, Swift calls back into Rust via
//!   `iosqr_deliver_result`, which finds the sender by token
//!   and forwards the [`Result`].
//!
//! ## Why a registered callback, not a Rust→Swift `extern`
//!
//! The Rust cdylib (`libdioxuswalletmain.dylib`) is built *before*
//! the Xcode pass that compiles Swift. If Rust declared
//! `extern "C" { fn iosqr_present_scanner(...) }`, the linker
//! would fail at cdylib-build time with an "undefined symbol"
//! because the Swift implementation isn't on the link line yet.
//!
//! Pattern instead: Swift's app delegate calls
//! [`iosqr_register_present_fn`] once at startup, handing Rust a
//! function pointer. Rust stores it; subsequent `IosQrScanner::scan`
//! calls invoke through the pointer. Same shape Android's JNI
//! uses (Kotlin reaches into Rust; Rust never reaches into
//! Kotlin directly).
//!
//! Threading: the Swift callback fires on the main UI thread
//! (where AVCaptureSession dispatches its metadata callback by
//! default). `iosqr_deliver_result` just locks `PENDING` and
//! `send`s; no await, no allocation that needs the tokio
//! runtime — safe to call from arbitrary contexts.
//!
//! Camera permission: handled by the system. The `Info.plist`
//! at `ios/App/Info.plist` declares `NSCameraUsageDescription`;
//! the first scan triggers the standard iOS permission alert.
//! Permission denial surfaces as
//! [`QrScanError::Unavailable`].
//!
//! Simulator behaviour: iOS Simulator's `AVCaptureSession`
//! returns a synthetic camera feed (a moving 3-D model) without
//! decodable barcodes. The scanner UI opens but never returns
//! a result — exercise the paste fallback under
//! `Diagnostics → Bootstrap` for simulator runs, or use a real
//! device.

use std::collections::HashMap;
use std::ffi::{c_char, CStr};
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use tokio::sync::oneshot;
use tracing::warn;

use wallet_core::{QrScanError, QrScanner};

/// Process-global scan-token allocator. Same shape as the
/// Android side — `u64` so wrap-around is theoretical; start at
/// 1 so all-zero is "uninitialised" in any future debug log.
static NEXT_TOKEN: AtomicU64 = AtomicU64::new(1);

type PendingMap = Mutex<HashMap<u64, oneshot::Sender<Result<String, QrScanError>>>>;

static PENDING: OnceLock<PendingMap> = OnceLock::new();

/// Function pointer the Swift shell registers at startup via
/// [`iosqr_register_present_fn`]. Set once, read on every scan.
/// `OnceLock` rather than `Mutex<Option<_>>` because the slot is
/// write-once + read-many.
static PRESENT_FN: OnceLock<PresentFn> = OnceLock::new();

/// Signature of the Swift function that presents the scanner.
/// Takes a token; returns 1 on dispatch success, 0 on failure
/// (no key window, etc.).
type PresentFn = extern "C" fn(u64) -> i32;

fn pending() -> &'static PendingMap {
    PENDING.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Stateless adapter. The Swift `iosqr_present_scanner` symbol
/// is declared via `extern "C"` below — at link time the iOS
/// build's `App.swift` (or a sibling Swift file added by the
/// QR-binding spec) provides the implementation.
pub struct IosQrScanner;

impl QrScanner for IosQrScanner {
    fn scan(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<String, QrScanError>> + Send + '_>> {
        Box::pin(async move {
            let token = NEXT_TOKEN.fetch_add(1, Ordering::Relaxed);
            let (tx, rx) = oneshot::channel();
            pending()
                .lock()
                .expect("iosqr PENDING mutex poisoned")
                .insert(token, tx);

            let Some(present) = PRESENT_FN.get() else {
                pending()
                    .lock()
                    .expect("iosqr PENDING mutex poisoned")
                    .remove(&token);
                return Err(QrScanError::Unavailable(
                    "iOS scanner-present fn not registered yet — Swift's \
                     app delegate must call iosqr_register_present_fn \
                     during boot"
                        .into(),
                ));
            };
            let dispatch_ok = present(token);
            if dispatch_ok == 0 {
                pending()
                    .lock()
                    .expect("iosqr PENDING mutex poisoned")
                    .remove(&token);
                return Err(QrScanError::Unavailable(
                    "iOS QR scanner failed to present — no key window? \
                     (Swift returned 0)"
                        .into(),
                ));
            }

            // Await the Swift side calling back. On user cancel /
            // scanner unavailable Swift sends the appropriate
            // QrScanError variant; on success the decoded payload
            // string.
            match rx.await {
                Ok(r) => r,
                Err(_) => {
                    // Receiver was dropped (Swift didn't call back).
                    // Shouldn't happen unless the scanner UI is
                    // forcibly killed; surface as Unavailable.
                    warn!(
                        target: "dioxus_wallet::qr_scanner_ios",
                        token,
                        "Swift never delivered a scan result",
                    );
                    Err(QrScanError::Unavailable(
                        "scanner never delivered a result".into(),
                    ))
                }
            }
        })
    }
}

// ─── Swift → Rust callback ─────────────────────────────────────

/// Called by the iOS shell when an in-flight scan completes.
/// `outcome` selects which variant the Rust side surfaces:
///
/// | `outcome`        | Meaning                          |
/// |------------------|----------------------------------|
/// | 1 (`OUTCOME_OK`) | `Ok(payload)` — `payload` is the decoded barcode text, NUL-terminated UTF-8. |
/// | 2 (`OUTCOME_CANCELLED`) | `Err(QrScanError::Cancelled)`. |
/// | 3 (`OUTCOME_UNAVAILABLE`) | `Err(QrScanError::Unavailable(msg))` — `payload` carries the reason. |
/// | _                | `Err(QrScanError::Decoder(msg))` — same. |
///
/// SAFETY: caller must ensure `payload` is either NULL (only valid
/// for `OUTCOME_CANCELLED`) or a valid NUL-terminated UTF-8 string
/// pointer that lives at least until this call returns. The
/// pointer is NOT retained — we copy the bytes into a Rust `String`
/// before the function ends. Swift's `String.withCString` gives
/// the right lifetime guarantee.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn iosqr_deliver_result(
    token: u64,
    outcome: u8,
    payload: *const c_char,
) {
    const OUTCOME_OK: u8 = 1;
    const OUTCOME_CANCELLED: u8 = 2;
    const OUTCOME_UNAVAILABLE: u8 = 3;

    let sender = pending()
        .lock()
        .expect("iosqr PENDING mutex poisoned")
        .remove(&token);
    let Some(sender) = sender else {
        warn!(
            target: "dioxus_wallet::qr_scanner_ios",
            token,
            outcome,
            "iosqr_deliver_result for unknown token (cancelled future?)",
        );
        return;
    };

    let result: Result<String, QrScanError> = match outcome {
        OUTCOME_OK => {
            if payload.is_null() {
                Err(QrScanError::Decoder(
                    "OUTCOME_OK with NULL payload".into(),
                ))
            } else {
                // SAFETY: caller contract — see fn-level safety note.
                let cstr = unsafe { CStr::from_ptr(payload) };
                match cstr.to_str() {
                    Ok(s) => Ok(s.to_owned()),
                    Err(e) => Err(QrScanError::Decoder(format!(
                        "non-UTF-8 payload: {e}"
                    ))),
                }
            }
        }
        OUTCOME_CANCELLED => Err(QrScanError::Cancelled),
        OUTCOME_UNAVAILABLE => {
            let msg = c_string_or(payload, "scanner unavailable");
            Err(QrScanError::Unavailable(msg))
        }
        _ => {
            let msg = c_string_or(payload, "unknown scanner error");
            Err(QrScanError::Decoder(msg))
        }
    };
    // `send` returning Err means the future was dropped before
    // the callback fired. Nothing to do — same fate as Android's
    // PENDING-already-removed branch.
    let _ = sender.send(result);
}

/// Copy a NUL-terminated UTF-8 C string into a Rust `String`,
/// substituting a fallback when the pointer is NULL or the bytes
/// aren't UTF-8.
fn c_string_or(payload: *const c_char, fallback: &str) -> String {
    if payload.is_null() {
        return fallback.to_string();
    }
    // SAFETY: caller contract — pointer must be NUL-terminated.
    let cstr = unsafe { CStr::from_ptr(payload) };
    cstr.to_str()
        .map(|s| s.to_owned())
        .unwrap_or_else(|_| fallback.to_string())
}

// ─── Rust → Swift ─────────────────────────────────────────────

/// Swift calls this once during app startup to hand Rust the
/// scanner-present function pointer. Idempotent — first
/// registration wins; subsequent calls are ignored (the
/// `OnceLock` semantics). Returns `1` on first registration,
/// `0` if already registered (debug-only signal).
///
/// SAFETY: `present` must be a valid function pointer whose
/// lifetime exceeds the app's runtime (i.e. either `static` or
/// effectively-static). Swift's `@_cdecl` symbols satisfy this
/// trivially.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn iosqr_register_present_fn(present: PresentFn) -> i32 {
    if PRESENT_FN.set(present).is_ok() {
        1
    } else {
        0
    }
}
