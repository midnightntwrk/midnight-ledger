#![deny(warnings)]

mod app;
mod platform;
mod runner;

fn main() {
    let _ = tracing_subscriber::fmt::try_init();
    dioxus::launch(app::App);
}
