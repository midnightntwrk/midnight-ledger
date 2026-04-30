use std::path::PathBuf;

#[allow(dead_code)] // used once iter-2 adds persistence
pub fn data_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("midnight-dx-wallet")
}
