//! Non-Android fallback: delegates to the existing JS bridge so
//! macOS / Linux / Windows desktop builds keep working unchanged.
//! iOS will eventually replace this with a native AVFoundation
//! impl, at which point this file becomes desktop-only.
//!
//! The mapping below mirrors what the legacy inline
//! `scan_and_dispatch` arm did before the `QrScanner` trait
//! existed — see commit f1dffe5e ("paste-fallback for unavailable
//! camera"). Behaviour on desktop is untouched; this just relocates
//! the dispatch into a trait impl so the Android variant can stand
//! beside it.
//!
//! Hot path: `eval_bridge::global_bridge() → eval_bridge::scan_qr →
//! match the `JsBridgeError::Transport` payload`.

use std::future::Future;
use std::pin::Pin;

use wallet_core::{QrScanError, QrScanner};

use crate::eval_bridge;

/// Desktop / iOS-today implementation. Stateless — construct on
/// demand at each call site, no shared resources to thread.
pub struct FallbackQrScanner;

impl QrScanner for FallbackQrScanner {
    fn scan(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<String, QrScanError>> + Send + '_>> {
        Box::pin(async move {
            let Some(bridge) = eval_bridge::global_bridge() else {
                return Err(QrScanError::Unavailable(
                    "JS bridge not installed (js-bridge feature off?)".into(),
                ));
            };
            match eval_bridge::scan_qr(&*bridge).await {
                Ok(url) => Ok(url),
                Err(wallet_core::js_bridge::JsBridgeError::Transport(msg))
                    if msg == "cancelled" =>
                {
                    Err(QrScanError::Cancelled)
                }
                // Android WebView (and any other Chromium-derived
                // shell on a non-secure scheme) reports
                // `getUserMedia not available` because
                // `navigator.mediaDevices` is gated by the
                // secure-context check. Surface that as
                // `Unavailable` with a hint pointing at the paste
                // field in Diagnostics → Bootstrap, mirroring the
                // UX from commit f1dffe5e.
                Err(wallet_core::js_bridge::JsBridgeError::Transport(msg))
                    if msg.contains("getUserMedia not available") =>
                {
                    Err(QrScanError::Unavailable(
                        "Camera unavailable in this WebView \
                         (secure-context limit). Paste the OID4VC URL \
                         in Diagnostics → Bootstrap instead."
                            .into(),
                    ))
                }
                Err(e) => Err(QrScanError::Decoder(e.to_string())),
            }
        })
    }
}
