//! Platform-agnostic QR scanner surface. The native bridges
//! (Android CameraX in Task 37, iOS AVCaptureMetadataOutput
//! later) implement this trait. A pure-Rust "paste URL" stub
//! ships here so unit tests + dev affordance work without a
//! camera.

use std::future::Future;
use std::pin::Pin;

#[derive(Debug, thiserror::Error)]
pub enum QrScanError {
    #[error("user cancelled the scan")]
    Cancelled,
    #[error("scanner unavailable: {0}")]
    Unavailable(String),
}

pub trait QrScanner: Send + Sync {
    /// Open the scanner UI. Resolves with the decoded URL string
    /// on success.
    fn scan(&self) -> Pin<Box<dyn Future<Output = Result<String, QrScanError>> + Send + '_>>;
}

/// In-memory stub for unit tests + dev paste-URL flow. The
/// next `scan()` call returns whatever was set via `set_next`.
#[derive(Debug, Default)]
pub struct PasteUrlScanner {
    next: std::sync::Mutex<Option<Result<String, QrScanError>>>,
}

impl PasteUrlScanner {
    pub fn set_next(&self, value: Result<String, QrScanError>) {
        *self.next.lock().unwrap() = Some(value);
    }
}

impl QrScanner for PasteUrlScanner {
    fn scan(&self) -> Pin<Box<dyn Future<Output = Result<String, QrScanError>> + Send + '_>> {
        let v = self.next.lock().unwrap().take();
        Box::pin(async move {
            v.unwrap_or(Err(QrScanError::Cancelled))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn paste_url_scanner_returns_set_value() {
        let s = PasteUrlScanner::default();
        s.set_next(Ok("openid4vp://x".into()));
        assert_eq!(s.scan().await.unwrap(), "openid4vp://x");
        // Second call without re-setting returns Cancelled.
        assert!(matches!(s.scan().await, Err(QrScanError::Cancelled)));
    }
}
