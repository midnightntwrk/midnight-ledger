use std::sync::Once;

/// Idempotently install rustls' `ring` crypto provider as the default
/// for this process. rustls 0.23 panics at TLS-handshake time if
/// multiple providers are compiled in (dioxus-desktop pulls in
/// `aws-lc-rs` transitively while reqwest/tokio-tungstenite/jsonrpsee
/// pull `ring`) but none is marked default. Picking `ring` explicitly
/// keeps us pure-Rust on every target — no C / aws-lc compile.
///
/// Safe to call from any thread, any number of times: the underlying
/// `install_default()` would itself error on a second call.
pub fn ensure_default_crypto_provider() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}
