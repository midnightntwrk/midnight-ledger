//! Build-time sync of compiled contract artifacts.
//!
//! `src/did/artifacts.rs` and `src/vault/artifacts.rs` embed the DID and
//! passport-vault circuit blobs via `include_bytes!` from `contracts/`.
//! Rather than commit those (large, regenerated) binaries, this script copies
//! them from each contract's `managed/<contract>/{keys,zkir}` source tree into
//! the flat `contracts/<contract>/<circuit>.{prover,verifier,bzkir,zkir}`
//! layout the macros expect. `contracts/` is gitignored — so a plain
//! `cargo build` (no external script) regenerates it.
//!
//! Sources default to the sibling workspace submodules; override per contract
//! with `MIDNIGHT_DID_MANAGED_DIR` / `MIDNIGHT_VAULT_MANAGED_DIR`.

use std::path::{Path, PathBuf};

const ARTIFACT_EXTS: [&str; 4] = ["prover", "verifier", "bzkir", "zkir"];

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    // wallet-core -> mobile-bench -> midnight-ledger -> workspace root.
    let workspace = manifest
        .join("../../..")
        .canonicalize()
        .unwrap_or_else(|_| manifest.join("../../.."));

    let did_src = source_dir(
        "MIDNIGHT_DID_MANAGED_DIR",
        workspace.join("midnight-did/packages/contract/src/managed/did"),
    );
    let vault_src = source_dir(
        "MIDNIGHT_VAULT_MANAGED_DIR",
        workspace
            .join("midnight-identity-solution-examples/packages/contracts/vault/src/managed/passport-vault"),
    );

    sync_contract("DID", &did_src, &manifest.join("contracts/midnight-did"));
    sync_contract(
        "passport-vault",
        &vault_src,
        &manifest.join("contracts/passport-vault"),
    );

    println!("cargo:rerun-if-env-changed=MIDNIGHT_DID_MANAGED_DIR");
    println!("cargo:rerun-if-env-changed=MIDNIGHT_VAULT_MANAGED_DIR");
}

fn source_dir(env_var: &str, default: PathBuf) -> PathBuf {
    match std::env::var(env_var) {
        Ok(p) if !p.trim().is_empty() => PathBuf::from(p.trim()),
        _ => default,
    }
}

/// Flatten `<managed>/keys/<c>.{prover,verifier}` + `<managed>/zkir/<c>.{bzkir,zkir}`
/// into `<dest>/<c>.{prover,verifier,bzkir,zkir}`.
///
/// `rerun-if-changed` is declared on BOTH the upstream source dirs (so a
/// recompiled contract re-syncs) AND the destination dir (so deleting the
/// gitignored `contracts/` tree forces a regenerate on the next build —
/// otherwise Cargo would skip this script when only the output went missing).
fn sync_contract(label: &str, managed: &Path, dest: &Path) {
    println!("cargo:rerun-if-changed={}", dest.display());

    let keys = managed.join("keys");
    let zkir = managed.join("zkir");

    if !keys.is_dir() || !zkir.is_dir() {
        // Tolerate a missing source if the blobs were synced previously.
        if dest_is_populated(dest) {
            println!(
                "cargo:warning={label} contract source not found at {} — reusing already-synced contracts/ blobs",
                managed.display()
            );
            return;
        }
        panic!(
            "{label} contract artifacts not found: expected `keys/` + `zkir/` under {}. \
             Build the contract or set the corresponding *_MANAGED_DIR env var.",
            managed.display()
        );
    }

    println!("cargo:rerun-if-changed={}", keys.display());
    println!("cargo:rerun-if-changed={}", zkir.display());

    std::fs::create_dir_all(dest).unwrap_or_else(|e| panic!("create {}: {e}", dest.display()));

    // Drop stale flat blobs first so renamed/removed circuits don't linger.
    if let Ok(read) = std::fs::read_dir(dest) {
        for entry in read.flatten() {
            let p = entry.path();
            if p.extension()
                .and_then(|s| s.to_str())
                .is_some_and(|e| ARTIFACT_EXTS.contains(&e))
            {
                let _ = std::fs::remove_file(p);
            }
        }
    }

    let mut count = 0usize;
    for entry in std::fs::read_dir(&keys)
        .unwrap_or_else(|e| panic!("read {}: {e}", keys.display()))
        .flatten()
    {
        let p = entry.path();
        if p.extension().and_then(|s| s.to_str()) != Some("prover") {
            continue;
        }
        let circuit = p.file_stem().and_then(|s| s.to_str()).unwrap().to_string();
        copy(&keys.join(format!("{circuit}.prover")), &dest.join(format!("{circuit}.prover")));
        copy(&keys.join(format!("{circuit}.verifier")), &dest.join(format!("{circuit}.verifier")));
        copy(&zkir.join(format!("{circuit}.bzkir")), &dest.join(format!("{circuit}.bzkir")));
        copy(&zkir.join(format!("{circuit}.zkir")), &dest.join(format!("{circuit}.zkir")));
        count += 1;
    }

    if count == 0 {
        panic!(
            "{label}: no `*.prover` keys found under {} — nothing to embed",
            keys.display()
        );
    }
}

/// True when `dest` already holds at least one circuit's full 4-blob set.
fn dest_is_populated(dest: &Path) -> bool {
    let Ok(read) = std::fs::read_dir(dest) else {
        return false;
    };
    read.flatten().any(|e| {
        let p = e.path();
        p.extension().and_then(|s| s.to_str()) == Some("prover")
            && ARTIFACT_EXTS
                .iter()
                .all(|ext| p.with_extension(ext).is_file())
    })
}

fn copy(src: &Path, dst: &Path) {
    std::fs::copy(src, dst)
        .unwrap_or_else(|e| panic!("copy {} -> {}: {e}", src.display(), dst.display()));
}
