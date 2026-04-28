use std::path::PathBuf;

pub fn cache_dir() -> PathBuf {
    if let Ok(p) = std::env::var("MIDNIGHT_PP") {
        return PathBuf::from(p);
    }
    PathBuf::from("/data/local/tmp/midnight-pp")
}
