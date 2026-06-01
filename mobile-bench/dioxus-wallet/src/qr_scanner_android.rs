//! Android implementation of [`wallet_core::QrScanner`]. Wraps
//! Google ML Kit's Code Scanner via a thin Kotlin shell
//! (`QrScanBridge.kt`) reached through JNI. Each in-flight scan
//! gets a process-local `u64` token paired with a
//! [`tokio::sync::oneshot::Sender`] in [`PENDING`]; the Kotlin
//! side calls
//! `Java_io_iohk_midnight_wallet_QrScanBridge_nativeOnQrResult`
//! when ML Kit resolves, and we route the result back through the
//! channel.
//!
//! Threading: `scan()` runs on whatever Dioxus task it was
//! spawned from (typically Tokio worker). It attaches the JNI
//! thread, makes a static call into Kotlin, and drops the
//! `JNIEnv` *before* awaiting — `JNIEnv` is `!Send` so it must
//! not survive across `.await`. The callback (`nativeOnQrResult`)
//! is invoked by ML Kit on the Android main thread, which is
//! Java-attached by default.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Mutex;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};

use jni::JNIEnv;
use jni::JavaVM;
use jni::objects::{JClass, JObject, JString};
use jni::sys::jlong;
use tokio::sync::oneshot;
use tracing::warn;

use wallet_core::{QrScanError, QrScanner};

/// Process-global token allocator. `u64` so wrap-around is purely
/// theoretical; we start at 1 so the all-zero `token == 0` is
/// recognisable as "uninitialised" in any future debugging.
static NEXT_TOKEN: AtomicU64 = AtomicU64::new(1);

type PendingMap = Mutex<HashMap<u64, oneshot::Sender<Result<String, QrScanError>>>>;

/// Maps in-flight scan tokens to the [`oneshot::Sender`] waiting
/// for their result. Pruned when [`nativeOnQrResult`] delivers,
/// or implicitly when the future is dropped (the receiver's drop
/// closes the channel; our subsequent `send` returns `Err` and we
/// silently discard).
static PENDING: OnceLock<PendingMap> = OnceLock::new();

fn pending() -> &'static PendingMap {
    PENDING.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Stateless impl — construct on demand at each call site.
pub struct AndroidQrScanner;

impl QrScanner for AndroidQrScanner {
    fn scan(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<String, QrScanError>> + Send + '_>> {
        Box::pin(async move {
            let token = NEXT_TOKEN.fetch_add(1, Ordering::Relaxed);
            let (tx, rx) = oneshot::channel();
            pending()
                .lock()
                .expect("PENDING mutex poisoned")
                .insert(token, tx);

            // The JNI handshake is synchronous + cheap (it just
            // posts a call into ML Kit). If it fails we must free
            // the PENDING slot ourselves; the callback won't fire.
            if let Err(err) = launch_kotlin_scan(token) {
                pending()
                    .lock()
                    .expect("PENDING mutex poisoned")
                    .remove(&token);
                return Err(QrScanError::Unavailable(format!(
                    "could not launch native scanner: {err}",
                )));
            }

            // Receiver error happens iff the sender was dropped
            // without sending. That shouldn't occur on ML Kit's
            // happy path; if it does, surface as Decoder so the
            // user can retry.
            rx.await.unwrap_or_else(|_| {
                Err(QrScanError::Decoder(
                    "native scanner channel closed unexpectedly".into(),
                ))
            })
        })
    }
}

/// Attach the current thread to the JVM, look up the
/// `QrScanBridge` companion's `startScan(Activity, Long)` method,
/// and invoke it. The `JNIEnv` is dropped before the function
/// returns — caller may await freely after.
fn launch_kotlin_scan(token: u64) -> Result<(), String> {
    let ctx = ndk_context::android_context();
    if ctx.vm().is_null() || ctx.context().is_null() {
        return Err("ndk-context not initialised (no Activity)".into());
    }

    // SAFETY: `ctx.vm()` is a valid `JavaVM*` for the lifetime of
    // the process — set by wry's `WryActivity` JNI init at app
    // start, and ndk-context owns the pointer thereafter.
    let vm = unsafe { JavaVM::from_raw(ctx.vm().cast()) }
        .map_err(|e| format!("JavaVM::from_raw: {e}"))?;
    let mut env = vm
        .attach_current_thread()
        .map_err(|e| format!("attach_current_thread: {e}"))?;

    // SAFETY: `ctx.context()` is a valid jobject for the host
    // Activity, owned by wry for the process lifetime.
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
/// on the main Java-attached thread. Resolves the matching
/// [`oneshot::Sender`] and removes it from [`PENDING`].
///
/// Error-string contract (matches `QrScanBridge.kt`):
/// - `"cancelled"`                 → [`QrScanError::Cancelled`]
/// - starts with `"Play Services"` → [`QrScanError::Unavailable`]
/// - anything else                 → [`QrScanError::Decoder`]
///
/// # Safety
///
/// Called by the JVM on a Java-attached thread with valid
/// JNIEnv + jobject parameters. `token` is whatever we passed to
/// `QrScanBridge.startScan`.
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_iohk_midnight_wallet_QrScanBridge_nativeOnQrResult<
    'local,
>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    token: jlong,
    url: JString<'local>,
    error: JString<'local>,
) {
    let token = token as u64;
    let sender = pending()
        .lock()
        .expect("PENDING mutex poisoned")
        .remove(&token);
    let Some(sender) = sender else {
        // Late callback for a scan whose future was already
        // dropped (caller cancelled / future cancelled at the
        // wallet side). Nothing to deliver to — drop silently
        // and warn so we can spot misuse in logs.
        warn!(
            target: "dioxus_wallet::qr_scanner_android",
            token, "nativeOnQrResult called with no matching PENDING entry",
        );
        return;
    };

    let result: Result<String, QrScanError> = if !error.is_null() {
        match env.get_string(&error) {
            Ok(s) => {
                let msg: String = s.into();
                if msg == "cancelled" {
                    Err(QrScanError::Cancelled)
                } else if msg.starts_with("Play Services") {
                    Err(QrScanError::Unavailable(msg))
                } else {
                    Err(QrScanError::Decoder(msg))
                }
            }
            Err(e) => Err(QrScanError::Decoder(format!(
                "could not decode JNI error string: {e}",
            ))),
        }
    } else if !url.is_null() {
        match env.get_string(&url) {
            Ok(s) => Ok(s.into()),
            Err(e) => Err(QrScanError::Decoder(format!(
                "could not decode JNI url string: {e}",
            ))),
        }
    } else {
        Err(QrScanError::Decoder(
            "both url and error JStrings were null".into(),
        ))
    };

    // If the receiver is gone (future dropped between
    // `remove` and `send`), the send fails silently — fine.
    let _ = sender.send(result);
}
