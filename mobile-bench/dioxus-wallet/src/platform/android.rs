use std::path::PathBuf;

#[allow(dead_code)] // used once iter-2 adds persistence
pub fn data_dir() -> PathBuf {
    if let Ok(p) = std::env::var("MIDNIGHT_WALLET_DATA") {
        return PathBuf::from(p);
    }
    PathBuf::from("/data/local/tmp/midnight-dx-wallet")
}
