use std::path::PathBuf;

pub fn cache_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("midnight-bench")
        .join("params")
}
