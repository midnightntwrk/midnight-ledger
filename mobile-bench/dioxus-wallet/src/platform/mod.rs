#[cfg(target_os = "android")]
mod android;
#[cfg(target_os = "ios")]
mod ios;
#[cfg(all(not(target_os = "android"), not(target_os = "ios")))]
mod desktop;

// `data_dir` lights up in iter-2 when persistence lands; re-exported now
// so the platform module shape is set.
#[allow(unused_imports)]
#[cfg(target_os = "android")]
pub use android::data_dir;
#[allow(unused_imports)]
#[cfg(target_os = "ios")]
pub use ios::data_dir;
#[allow(unused_imports)]
#[cfg(all(not(target_os = "android"), not(target_os = "ios")))]
pub use desktop::data_dir;
