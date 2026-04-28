#![deny(warnings)]

mod app;
mod platform;
mod runner;

pub fn run() {
    let _ = tracing_subscriber::fmt::try_init();
    dioxus::launch(app::App);
}

/// Android entry point. `dioxus-mobile`'s `JNI_OnLoad` looks up `main` via
/// `dlsym(RTLD_DEFAULT, "main")` and calls it before Java invokes
/// `start_app`. Exposing `main` from the cdylib makes that lookup succeed.
/// On desktop, the regular `main.rs` binary calls `run()` directly and this
/// symbol is unused.
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub extern "C" fn main() -> i32 {
    run();
    0
}
