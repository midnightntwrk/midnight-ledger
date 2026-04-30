#[cfg(target_os = "android")]
mod android;
#[cfg(not(target_os = "android"))]
mod desktop;

// `data_dir` lights up in iter-2 when persistence lands; re-exported now
// so the platform module shape is set.
#[allow(unused_imports)]
#[cfg(target_os = "android")]
pub use android::data_dir;
#[allow(unused_imports)]
#[cfg(not(target_os = "android"))]
pub use desktop::data_dir;
