// Build script for `midnight-prover-ffi`.
//
// Two responsibilities:
//
// 1. Run the UniFFI scaffolding generator against `src/midnight_prover.udl`
//    so the runtime symbols expected by the generated Swift / Kotlin
//    layers are present in the cdylib.
//
// 2. Surface a single rerun-if-changed entry so subsequent `cargo build`s
//    don't redundantly re-run the generator unless the UDL changes.
//
// The actual Swift / Kotlin / TypeScript binding generation is invoked
// separately via the `ubrn` (uniffi-bindgen-react-native) CLI from
// `scripts/build-{ios,android}.sh`.

fn main() {
    uniffi::generate_scaffolding("src/midnight_prover.udl")
        .expect("Failed to generate UniFFI scaffolding");

    // Rebuild the scaffolding only when the UDL changes. The Rust
    // source under `src/` is already tracked by cargo's default
    // file-watch behaviour.
    println!("cargo:rerun-if-changed=src/midnight_prover.udl");
}
