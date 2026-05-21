use std::path::PathBuf;

/// iOS data directory — lives inside the app's private sandbox
/// (`$HOME/Library/Application Support/midnight-dx-wallet/` per
/// Apple's "user-visible non-document data" guidance). Falls back
/// to `$TMPDIR` if the sandbox env vars are unavailable (shouldn't
/// happen in practice on a real device or simulator, but we never
/// want to panic at startup).
#[allow(dead_code)] // used once iter-2 adds persistence
pub fn data_dir() -> PathBuf {
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home)
            .join("Library")
            .join("Application Support")
            .join("midnight-dx-wallet");
    }
    std::env::temp_dir().join("midnight-dx-wallet")
}
