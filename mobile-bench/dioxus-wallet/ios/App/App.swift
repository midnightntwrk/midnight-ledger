// SwiftUI shell for the iOS Simulator build of dioxus-wallet.
//
// The actual UI is rendered by Dioxus / Wry inside a `UIViewController`
// the Rust side mounts onto the key window from inside `start_app()`.
// We just need a SwiftUI Scene that survives until that view controller
// takes over, then yields. `Color.clear` keeps the splash transparent
// while Rust spins up the Wry WebView.
//
// `@_silgen_name("start_app")` tells the Swift compiler to look up the
// C-ABI symbol `start_app` (exported by the Rust `cdylib`) at link
// time, without going through the usual Swift name mangling.

import SwiftUI

// `@_silgen_name("start_app")` tells the Swift compiler to look up
// the C-ABI symbol `start_app` (exported by the Rust `cdylib`) at
// link time, without going through Swift's name mangling. We expose
// it from inside the App type so the file has no other top-level
// declarations beyond the `@main` struct — Swift forbids `@main` in
// a file that also has top-level code, which is what tripped the
// SourceKit `'main' attribute cannot be used in a module that
// contains top-level code` diagnostic when the FFI decl sat at file
// scope.

@main
struct DioxusWalletApp: App {
    @_silgen_name("start_app") static func start_app()

    init() {
        // Register the iOS-side QR scanner with the Rust core
        // BEFORE handing control over. The Rust side stores the
        // function pointer in a `OnceLock`; subsequent
        // `IosQrScanner::scan` calls invoke through it. Registration
        // before `start_app()` guarantees the pointer is in place
        // before any UI surface that could call it appears.
        iosqrInstall()
        // Hand control to Rust. `start_app()` calls `run()` which
        // launches the Dioxus mobile App. From here on the
        // SwiftUI scene is irrelevant — Rust owns the window.
        Self.start_app()
    }

    var body: some Scene {
        WindowGroup {
            Color.clear
        }
    }
}
