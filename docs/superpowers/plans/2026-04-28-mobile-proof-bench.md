# Mobile Proof Bench — Iteration 1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a Rust workspace `mobile-bench/` containing a `prover-core` library and a `dioxus-bench` UI app that can run Midnight ZK proofs (Dust spend + a small zkir circuit) natively on macOS desktop, an Android arm64 emulator, and a Samsung S24 Ultra, recording prove/verify latency.

**Architecture:** `prover-core` wraps `transient-crypto`, `ledger`, and `zkir` proving paths and exposes two async fns. `dioxus-bench` calls those fns from a single-screen UI. A `proof-server-http` cargo feature spawns the existing `midnight-proof-server::server()` in-process for desktop-only HTTP/library latency comparison. KZG public params and circuit keys are downloaded on first run via the existing `MidnightDataProvider`; small zkir test fixtures are bundled into the binary (and into Android assets).

**Tech Stack:** Rust 2024 edition; Tokio; Dioxus 0.6; `cargo-ndk` 3.x; Android NDK r27; Compact-compiled zkir fixtures from the existing Nix `test-artifacts` derivation; macOS arm64 host; Samsung S24 Ultra (aarch64) target.

**Spec:** [docs/superpowers/specs/2026-04-28-mobile-proof-bench-design.md](../specs/2026-04-28-mobile-proof-bench-design.md)

**Working branch:** `mobile-bench/iteration-1` (already created and pushed; spec already committed).

---

## File structure

```
mobile-bench/
├── README.md                          # how to install, run, test
├── RESULTS.md                         # latency snapshots (manually appended)
├── scripts/
│   └── setup-android-toolchain.sh     # already exists
├── fixtures/                          # checked-in bzkir + keys for zkir example
│   ├── fallible.bzkir
│   ├── fallible.prover
│   └── fallible.verifier
├── prover-core/
│   ├── Cargo.toml
│   ├── build.rs                       # embeds fixtures via include_bytes!
│   └── src/
│       ├── lib.rs                     # public API, error types
│       ├── params.rs                  # ParamsCache
│       ├── dust.rs                    # prove_dust_spend
│       ├── zkir_example.rs            # prove_zkir_example
│       ├── http.rs                    # prove_via_http (feature: proof-server-http)
│       └── server.rs                  # spawn_local_server (feature: proof-server-http)
│   ├── tests/
│   │   ├── library_path.rs
│   │   └── http_path.rs               # feature: proof-server-http
│   └── benches/
│       └── proofs.rs                  # feature: bench
└── dioxus-bench/
    ├── Cargo.toml
    ├── Dioxus.toml
    ├── assets/
    │   ├── styles.css
    │   └── fixtures/                  # symlink/copy of mobile-bench/fixtures (Android assets)
    └── src/
        ├── main.rs
        ├── app.rs
        ├── runner.rs
        └── platform/
            ├── mod.rs
            ├── desktop.rs
            └── android.rs
```

`Cargo.toml` (root): `members` gains `mobile-bench/prover-core` and `mobile-bench/dioxus-bench`. **`default-members` is NOT changed.**

---

## Task 0: Branch sanity, baseline build

**Files:**
- Verify: `Cargo.toml` (root), `mobile-bench/scripts/setup-android-toolchain.sh`

- [ ] **Step 0.1: Confirm branch and clean tree**

Run:
```bash
git branch --show-current
git status --short
```
Expected: branch is `mobile-bench/iteration-1`; status is clean (the spec + setup script were committed in `ac45e9f4`).

- [ ] **Step 0.2: Confirm host toolchain**

Run:
```bash
rustc --version
cargo --version
rustup target list --installed | grep aarch64-apple-darwin
```
Expected: any 1.85+ stable with edition2024 support; `aarch64-apple-darwin` installed.

- [ ] **Step 0.3: Confirm baseline workspace builds**

Run:
```bash
cargo check --workspace --all-targets
```
Expected: builds succeed (this is our baseline before adding new crates). If it fails on this branch, fix the existing failure before continuing — do not start adding new crates on top of a broken tree.

- [ ] **Step 0.4: Confirm Nix is present and the test-artifacts derivation builds**

Run:
```bash
which nix
nix build .#test-artifacts --print-out-paths
```
Expected: prints a `/nix/store/...-test-artifacts/` path. Save it: we'll need it in Task 2.

If `nix` is unavailable on this host: skip step 0.4 and instead add `xfail-on-no-nix` to Task 2's notes — we'll source fixtures by hand from the test-artifacts directory if a teammate has one.

- [ ] **Step 0.5: Commit nothing — this task is verification only**

No commit.

---

## Task 1: Create the `prover-core` crate skeleton

**Files:**
- Create: `mobile-bench/prover-core/Cargo.toml`
- Create: `mobile-bench/prover-core/src/lib.rs`
- Modify: `Cargo.toml` (root) — add `mobile-bench/prover-core` to `members` only.

- [ ] **Step 1.1: Add the new member to the root workspace**

Edit `Cargo.toml` (root). Inside `members = [ ... ]`, add the line `"mobile-bench/prover-core",` between `"storage-core"` and the closing `]`. Do **not** modify `default-members`.

- [ ] **Step 1.2: Create the crate manifest**

Create `mobile-bench/prover-core/Cargo.toml`:

```toml
[package]
name = "prover-core"
version = "0.1.0"
edition = "2024"
license.workspace = true
publish = false

[features]
default = []
# When enabled, also embeds the in-process midnight-proof-server. Desktop only;
# the Cargo target gating happens via cfg in the source files, not here.
proof-server-http = ["dep:midnight-proof-server", "dep:reqwest"]
# Enables criterion benches.
bench = ["dep:criterion"]

[dependencies]
# Existing workspace members (path deps so we always track HEAD).
ledger = { path = "../../ledger", package = "midnight-ledger", default-features = false, features = ["proving", "test-utilities"] }
zswap = { path = "../../zswap", package = "midnight-zswap" }
base-crypto = { path = "../../base-crypto", package = "midnight-base-crypto" }
transient-crypto = { path = "../../transient-crypto", package = "midnight-transient-crypto" }
storage = { path = "../../storage", package = "midnight-storage" }
serialize = { path = "../../serialize", package = "midnight-serialize" }
zkir = { path = "../../zkir", package = "midnight-zkir" }
coin-structure = { path = "../../coin-structure", package = "midnight-coin-structure" }
onchain-runtime = { path = "../../onchain-runtime", package = "midnight-onchain-runtime" }

# Async + RNG.
tokio = { version = "1.46.1", features = ["rt", "rt-multi-thread", "macros", "sync"] }
rand = { version = "0.8.4", features = ["getrandom"] }
rand_chacha = "0.3.1"
futures = "0.3"

# Errors / serde / utility.
thiserror = "2.0"
anyhow = "1.0"
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
hex = "0.4"
tracing = "0.1"
once_cell = "1.20"

# Optional deps for features.
midnight-proof-server = { path = "../../proof-server", optional = true }
reqwest = { version = "0.13", default-features = false, features = ["rustls-tls", "json"], optional = true }
criterion = { version = "0.5", optional = true }

[dev-dependencies]
tokio = { version = "1.46.1", features = ["full"] }
tracing-subscriber = "0.3"

[[bench]]
name = "proofs"
harness = false
required-features = ["bench"]
```

- [ ] **Step 1.3: Create the lib root**

Create `mobile-bench/prover-core/src/lib.rs`:

```rust
//! prover-core: a thin embeddable wrapper around Midnight's proving primitives
//! used by both the dioxus-bench app and `cargo test`/`cargo bench`.
//!
//! See `docs/superpowers/specs/2026-04-28-mobile-proof-bench-design.md`.

#![deny(unreachable_pub)]
#![deny(warnings)]

use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("anyhow: {0}")]
    Anyhow(#[from] anyhow::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone)]
pub struct BenchOpts {
    pub verify_after: bool,
    pub seed: Option<u64>,
}

impl Default for BenchOpts {
    fn default() -> Self {
        Self { verify_after: true, seed: Some(0x42) }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ProofRun {
    pub label: &'static str,
    pub k: u8,
    pub elapsed: Duration,
    pub verify_elapsed: Option<Duration>,
    pub verified: Option<bool>,
    pub proof_bytes: Vec<u8>,
}

pub struct ProverCore {
    cache_dir: PathBuf,
}

impl ProverCore {
    pub async fn new(cache_dir: PathBuf) -> Result<Self> {
        std::fs::create_dir_all(&cache_dir)?;
        Ok(Self { cache_dir })
    }

    /// Returns the on-disk directory used for cached KZG params and circuit
    /// keys.
    pub fn cache_dir(&self) -> &std::path::Path {
        &self.cache_dir
    }
}
```

- [ ] **Step 1.4: Run cargo check on just the new crate**

Run:
```bash
cargo check -p prover-core
```
Expected: succeeds. If it fails because `serde`'s derive feature is missing or similar, copy whatever `transient-crypto` does in `Cargo.toml` for the same dep.

- [ ] **Step 1.5: Commit**

```bash
git add Cargo.toml Cargo.lock mobile-bench/prover-core
git commit -S -s -m "$(cat <<'EOF'
feat(prover-core): add empty prover-core crate scaffold

Adds the workspace member, public skeleton (Error, ProverCore, ProofRun,
BenchOpts), and pulls in the dependency surface we will need across the
rest of the iteration. No proving logic yet.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

Verify:
```bash
git log --format="%h %G? %s" -1
```
Expected: `G` signature, "feat(prover-core)..." subject.

---

## Task 2: Source the zkir test fixtures into the repo

We need a small bzkir + prover key + verifier key triple checked into the repo so the zkir example runs anywhere (desktop dev box, CI, Android — no Nix needed at runtime).

**Files:**
- Create (3 files): `mobile-bench/fixtures/fallible.bzkir`, `fallible.prover`, `fallible.verifier`
- Create: `mobile-bench/fixtures/README.md`

- [ ] **Step 2.1: Locate the test-artifacts output**

Run:
```bash
TA="$(nix build .#test-artifacts --no-link --print-out-paths)"
echo "$TA"
ls "$TA"
```
Expected: a directory containing per-test subdirs (`""`, `fallible/`, `simple-merkle-tree/`, ...). The empty-string subdir is named `_` or just `""`; check what's there.

- [ ] **Step 2.2: Identify the smallest circuit**

Run:
```bash
find "$TA" -name "*.bzkir" -exec ls -la {} \; | sort -k5 -n | head
find "$TA" -name "*.prover" -exec ls -la {} \; | sort -k5 -n | head
```
Expected: a list of bzkir files. We want the smallest one paired with a prover key. The "fallible" test's `count` circuit is the canonical "smallest example" used in `proof-server`'s tests; pick that one.

If `fallible/zkir/count.bzkir` exists with paired `fallible/keys/count.prover` and `count.verifier`, those are our three fixtures.

- [ ] **Step 2.3: Copy fixtures into the repo**

Run:
```bash
mkdir -p mobile-bench/fixtures
cp "$TA/fallible/zkir/count.bzkir" mobile-bench/fixtures/fallible.bzkir
cp "$TA/fallible/keys/count.prover" mobile-bench/fixtures/fallible.prover
cp "$TA/fallible/keys/count.verifier" mobile-bench/fixtures/fallible.verifier
ls -la mobile-bench/fixtures/
```
Expected: three files. The prover key may be on the order of a few MB; that's OK to commit to the repo for a spike.

If `count.bzkir` doesn't exist under that exact path, run `find "$TA" -name 'count*'` and adapt. The names follow `<test_dir>/zkir/<circuit_name>.bzkir` and `<test_dir>/keys/<circuit_name>.{prover,verifier}` from `ledger/src/test_utilities.rs:646`.

- [ ] **Step 2.4: Document provenance**

Create `mobile-bench/fixtures/README.md`:

```markdown
# zkir fixtures

These three files are the smallest checked-in zkir example used by the
mobile-bench prover. They are a copy of the `fallible/count` artifacts
produced by `nix build .#test-artifacts`.

To regenerate:

```bash
TA="$(nix build .#test-artifacts --no-link --print-out-paths)"
cp "$TA/fallible/zkir/count.bzkir"     fallible.bzkir
cp "$TA/fallible/keys/count.prover"    fallible.prover
cp "$TA/fallible/keys/count.verifier"  fallible.verifier
```

These are checked in (small enough — ~few MB total) so the prover-core
crate works on any host without invoking Nix at runtime, and so the
Dioxus app can bundle them as Android assets.
```

- [ ] **Step 2.5: Verify the bzkir is loadable**

Run a one-off:
```bash
cargo run -p midnight-zkir --features binary -- mock-compile mobile-bench/fixtures/fallible.bzkir
```
Expected: prints something like "Mock compilation succeeded" or similar (the zkir CLI's `mock-compile` deserializes and validates without producing a proof). If it errors, the bzkir wasn't a tagged-serialized IR — re-check the source path in step 2.3.

- [ ] **Step 2.6: Commit**

```bash
git add mobile-bench/fixtures
git commit -S -s -m "$(cat <<'EOF'
feat(mobile-bench): vendor fallible/count zkir fixtures

These are copied verbatim from the test-artifacts Nix derivation so the
prover-core crate has a runnable zkir example without requiring Nix at
runtime. Provenance documented in fixtures/README.md.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
git log --format="%h %G? %s" -1
```
Expected: `G` signature.

---

## Task 3: `params.rs` — ParamsCache (KZG + Dust prover key download)

**Files:**
- Create: `mobile-bench/prover-core/src/params.rs`
- Modify: `mobile-bench/prover-core/src/lib.rs`

- [ ] **Step 3.1: Write the failing integration test**

Create `mobile-bench/prover-core/tests/params_smoke.rs`:

```rust
use prover_core::ProverCore;
use std::path::PathBuf;

#[tokio::test]
async fn params_cache_initialises_in_isolated_dir() {
    let dir = tempdir_for_test("params_cache_initialises");
    let pc = ProverCore::new(dir.clone()).await.expect("init");

    // Cache dir exists and is the one we asked for.
    assert!(pc.cache_dir().exists());
    assert_eq!(pc.cache_dir(), dir);
}

fn tempdir_for_test(name: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!("prover-core-{}-{}", name, std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    p
}
```

- [ ] **Step 3.2: Run it and watch it pass**

Run:
```bash
cargo test -p prover-core --test params_smoke
```
Expected: PASS (this just exercises `ProverCore::new` from Task 1).

- [ ] **Step 3.3: Implement `ParamsCache` skeleton**

Create `mobile-bench/prover-core/src/params.rs`:

```rust
use std::path::{Path, PathBuf};
use std::sync::Arc;

use base_crypto::data_provider::{self, FetchMode, MidnightDataProvider, OutputMode};
use ledger::dust::{DUST_EXPECTED_FILES, DustResolver};
use zswap::{ZSWAP_EXPECTED_FILES, prove::ZswapResolver};

/// Wraps the existing MidnightDataProvider machinery. On first call, files
/// listed in DUST_EXPECTED_FILES / ZSWAP_EXPECTED_FILES are downloaded into
/// `dir`. Subsequent calls hit the cache.
pub(crate) struct ParamsCache {
    dir: PathBuf,
    pub(crate) zswap: Arc<ZswapResolver>,
    pub(crate) dust: Arc<DustResolver>,
}

impl ParamsCache {
    pub(crate) fn new(dir: PathBuf) -> std::io::Result<Self> {
        std::fs::create_dir_all(&dir)?;

        // The MidnightDataProvider currently uses XDG_DATA_HOME / dirs::data_dir
        // by default. To force it into our `dir`, set MIDNIGHT_PARAM_CACHE_DIR
        // before constructing it; this is the env var honoured by
        // base_crypto::data_provider.
        // SAFETY: we set this once per process; if the var is already set to a
        // different value, we honour the caller — useful for tests.
        if std::env::var_os("MIDNIGHT_PARAM_CACHE_DIR").is_none() {
            unsafe {
                std::env::set_var("MIDNIGHT_PARAM_CACHE_DIR", &dir);
            }
        }

        let zswap = ZswapResolver(MidnightDataProvider::new(
            FetchMode::OnDemand,
            OutputMode::Log,
            ZSWAP_EXPECTED_FILES.to_vec(),
        )?);
        let dust = DustResolver(MidnightDataProvider::new(
            FetchMode::OnDemand,
            OutputMode::Log,
            DUST_EXPECTED_FILES.to_owned(),
        )?);

        Ok(Self { dir, zswap: Arc::new(zswap), dust: Arc::new(dust) })
    }

    pub(crate) fn dir(&self) -> &Path {
        &self.dir
    }
}
```

Then add the module to `lib.rs`. Edit `mobile-bench/prover-core/src/lib.rs`, add right after the `pub type Result` line:

```rust
mod params;
```

- [ ] **Step 3.4: Validate the env-var path actually works**

Re-read `base-crypto/src/data_provider.rs` lines mentioning `MIDNIGHT_PARAM_CACHE_DIR` (or whichever env var it reads — search for `var_os`, `XDG_DATA_HOME`, or `data_dir` in that file). If the var name differs or there's no env-var hook, **stop** and:

1. Pick the actual hook used (likely a constructor arg, or a different env var).
2. Replace the `set_var` block with whatever the right plumbing is — e.g. construct `MidnightDataProvider::new_with_dir(dir, ...)` if such a constructor exists.

Run:
```bash
grep -n "var_os\|env::var\|data_dir\|XDG\|cache_dir\|\"MIDNIGHT" base-crypto/src/data_provider.rs
```
Adapt `params.rs` accordingly. Do **not** invent an env var name that isn't actually read by `base-crypto`.

- [ ] **Step 3.5: Wire the ParamsCache into ProverCore**

Edit `mobile-bench/prover-core/src/lib.rs`. Replace the existing `ProverCore` struct + impl with:

```rust
pub struct ProverCore {
    cache_dir: PathBuf,
    pub(crate) params: params::ParamsCache,
}

impl ProverCore {
    pub async fn new(cache_dir: PathBuf) -> Result<Self> {
        std::fs::create_dir_all(&cache_dir)?;
        let params = params::ParamsCache::new(cache_dir.clone())?;
        Ok(Self { cache_dir, params })
    }

    pub fn cache_dir(&self) -> &std::path::Path {
        &self.cache_dir
    }
}
```

- [ ] **Step 3.6: Re-run the smoke test**

Run:
```bash
cargo test -p prover-core --test params_smoke
```
Expected: PASS.

- [ ] **Step 3.7: Commit**

```bash
git add mobile-bench/prover-core
git commit -S -s -m "$(cat <<'EOF'
feat(prover-core): add ParamsCache wrapping MidnightDataProvider

Wraps Zswap and Dust data providers with on-demand fetching anchored to a
caller-supplied cache dir. Tested via a simple smoke test that confirms
ProverCore can be constructed in an isolated tempdir.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: `zkir_example.rs` — prove the embedded fallible/count circuit

**Files:**
- Create: `mobile-bench/prover-core/build.rs`
- Create: `mobile-bench/prover-core/src/zkir_example.rs`
- Create: `mobile-bench/prover-core/tests/library_path.rs`
- Modify: `mobile-bench/prover-core/src/lib.rs`

This is the **simplest** end-to-end proof and so the first one we get green.

- [ ] **Step 4.1: Embed the fixtures via build.rs**

Create `mobile-bench/prover-core/build.rs`:

```rust
use std::path::PathBuf;

fn main() {
    // Re-run if any fixture changes.
    let dir = PathBuf::from("../fixtures");
    println!("cargo:rerun-if-changed={}", dir.display());
    for f in ["fallible.bzkir", "fallible.prover", "fallible.verifier"] {
        println!("cargo:rerun-if-changed=../fixtures/{f}");
    }
    println!("cargo:rustc-env=PROVER_CORE_FIXTURES_DIR={}", dir.canonicalize().unwrap().display());
}
```

- [ ] **Step 4.2: Write the failing zkir-example test**

Create `mobile-bench/prover-core/tests/library_path.rs`:

```rust
use prover_core::{BenchOpts, ProverCore};

#[tokio::test]
async fn prove_zkir_example_succeeds_and_verifies() {
    let _ = tracing_subscriber::fmt::try_init();
    let cache = std::env::temp_dir().join(format!("prover-core-zkir-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&cache);

    let pc = ProverCore::new(cache).await.expect("init");
    let run = pc
        .prove_zkir_example(BenchOpts::default())
        .await
        .expect("prove_zkir_example");

    assert!(!run.proof_bytes.is_empty(), "proof should have bytes");
    assert_eq!(run.verified, Some(true), "verify must succeed");
    assert!(run.elapsed.as_millis() > 0);
    eprintln!(
        "zkir-example: prove={:?} verify={:?} bytes={}",
        run.elapsed,
        run.verify_elapsed,
        run.proof_bytes.len()
    );
}
```

- [ ] **Step 4.3: Run the test and watch it fail with "method not found"**

Run:
```bash
cargo test -p prover-core --test library_path
```
Expected: FAILS with `error[E0599]: no method named 'prove_zkir_example' found for struct 'ProverCore'`.

- [ ] **Step 4.4: Implement `zkir_example.rs`**

Create `mobile-bench/prover-core/src/zkir_example.rs`:

```rust
use std::time::Instant;

use rand::SeedableRng;
use rand_chacha::ChaCha20Rng;
use serialize::tagged_deserialize;
use transient_crypto::proofs::{
    KeyLocation, ParamsProverProvider, ProofPreimage, ProverKey, Resolver as ResolverT,
    ProvingKeyMaterial, VerifierKey,
};
use zkir::IrSource;

use crate::{BenchOpts, Error, ProofRun, ProverCore, Result};

const LABEL: &str = "zkir-fallible-count";

pub(crate) const IR_BYTES:       &[u8] = include_bytes!(concat!(env!("PROVER_CORE_FIXTURES_DIR"), "/fallible.bzkir"));
pub(crate) const PROVER_BYTES:   &[u8] = include_bytes!(concat!(env!("PROVER_CORE_FIXTURES_DIR"), "/fallible.prover"));
pub(crate) const VERIFIER_BYTES: &[u8] = include_bytes!(concat!(env!("PROVER_CORE_FIXTURES_DIR"), "/fallible.verifier"));

/// A minimal Resolver implementation that hands back the embedded fixtures
/// for any KeyLocation. Used only by `prove_zkir_example` since we have one
/// and only one circuit here.
struct EmbeddedResolver;

#[allow(async_fn_in_trait)]
impl ResolverT for EmbeddedResolver {
    async fn resolve_key(
        &self,
        _key: KeyLocation,
    ) -> std::io::Result<Option<ProvingKeyMaterial>> {
        Ok(Some(ProvingKeyMaterial {
            ir_source: IR_BYTES.to_vec(),
            prover_key: PROVER_BYTES.to_vec(),
            verifier_key: VERIFIER_BYTES.to_vec(),
        }))
    }
}

impl ProverCore {
    pub async fn prove_zkir_example(&self, opts: BenchOpts) -> Result<ProofRun> {
        let seed = opts.seed.unwrap_or(0x42);
        let mut rng = ChaCha20Rng::seed_from_u64(seed);

        // Build a trivial preimage. The fallible/count circuit takes no inputs.
        let preimage = ProofPreimage {
            inputs: vec![],
            private_transcript: vec![],
            public_transcript_inputs: vec![],
            public_transcript_outputs: vec![],
            binding_input: Default::default(),
            communications_commitment: None,
            key_location: KeyLocation(std::borrow::Cow::Borrowed("count")),
        };

        let resolver = EmbeddedResolver;

        // Prove.
        let started = Instant::now();
        let (proof, _pi_skips) = preimage
            .prove::<IrSource>(&mut rng, &self.params.zswap.0, &resolver)
            .await
            .map_err(|e| Error::Anyhow(anyhow::anyhow!("prove: {e}")))?;
        let elapsed = started.elapsed();

        // Encode.
        let proof_bytes = {
            let mut buf = Vec::new();
            serialize::tagged_serialize(&proof, &mut buf)
                .map_err(|e| Error::Anyhow(anyhow::anyhow!("serialize proof: {e}")))?;
            buf
        };

        // Optionally re-verify.
        let (verified, verify_elapsed) = if opts.verify_after {
            let v_started = Instant::now();
            let vk: VerifierKey = tagged_deserialize(&mut &VERIFIER_BYTES[..])
                .map_err(|e| Error::Anyhow(anyhow::anyhow!("deserialize vk: {e}")))?;
            let k = vk.force_init().map_err(|e| Error::Anyhow(anyhow::anyhow!(e)))?.k();
            let params_v = self
                .params
                .zswap
                .0
                .get_params(k)
                .await
                .map_err(|e| Error::Anyhow(anyhow::anyhow!("get_params: {e}")))?
                .as_verifier();
            // The fallible/count circuit has no public inputs.
            let ok = vk.verify(&params_v, &proof, std::iter::empty()).is_ok();
            (Some(ok), Some(v_started.elapsed()))
        } else {
            (None, None)
        };

        Ok(ProofRun {
            label: LABEL,
            k: 13, // fallible/count was generated against k=13; we record this for results
            elapsed,
            verify_elapsed,
            verified,
            proof_bytes,
        })
    }
}
```

Wire it in. Edit `mobile-bench/prover-core/src/lib.rs`, add a line right under `mod params;`:

```rust
mod zkir_example;
```

- [ ] **Step 4.5: Run the test and resolve compile errors**

Run:
```bash
cargo test -p prover-core --test library_path
```

Expected: it builds, but may fail at runtime if the `count` circuit's public-input handling differs from "no public inputs". Read the actual error.

If verify fails because public inputs are non-empty, look at how `proof-server/src/endpoints.rs:check` calls verification (around the `verify_key.verify(...)` call) and copy the public-input plumbing here.

If `prove` fails with "failed to find proving key for 'count'" the `KeyLocation` string is wrong — check the actual circuit name encoded in the bzkir by looking at `fallible.compact`'s exported circuit name, then update the `Cow::Borrowed("count")` line.

If serialization tag mismatch, the bytes in the `.prover` file are not a `ProverKey<IrSource>` directly — look at how `MidnightDataProvider` deserializes them in `transient_crypto/src/proofs.rs:678` and replicate.

When the test passes you'll see something like:
```
zkir-example: prove=Duration { ... } verify=Some(Duration { ... }) bytes=4096
```

- [ ] **Step 4.6: Commit**

```bash
git add mobile-bench/prover-core
git commit -S -s -m "$(cat <<'EOF'
feat(prover-core): implement prove_zkir_example end-to-end

First green proof: builds a ProofPreimage for the fallible/count circuit,
proves it via transient_crypto's ProofPreimage::prove using an embedded
Resolver that returns bytes baked in at compile time, optionally
re-verifies, and reports timing.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 5: `dust.rs` — prove a Dust spend, downloading the prover key on first call

**Files:**
- Create: `mobile-bench/prover-core/src/dust.rs`
- Modify: `mobile-bench/prover-core/src/lib.rs`
- Modify: `mobile-bench/prover-core/tests/library_path.rs`

This task downloads ~256 MB of KZG params on first run. Make sure you're on Wi-Fi.

- [ ] **Step 5.1: Add the failing test for the Dust path**

Append to `mobile-bench/prover-core/tests/library_path.rs`:

```rust
#[tokio::test]
#[ignore = "downloads ~256 MB of KZG params on first run; run with `--ignored`"]
async fn prove_dust_spend_succeeds_and_verifies() {
    let _ = tracing_subscriber::fmt::try_init();
    let cache = std::env::temp_dir().join("prover-core-dust-shared");
    let pc = ProverCore::new(cache).await.expect("init");
    let run = pc
        .prove_dust_spend(BenchOpts::default())
        .await
        .expect("prove_dust_spend");

    assert!(!run.proof_bytes.is_empty());
    assert_eq!(run.verified, Some(true));
    eprintln!(
        "dust-spend: prove={:?} verify={:?} bytes={}",
        run.elapsed, run.verify_elapsed, run.proof_bytes.len()
    );
}
```

- [ ] **Step 5.2: Run and watch it fail to compile**

Run:
```bash
cargo test -p prover-core --test library_path -- --ignored prove_dust_spend
```
Expected: `no method named 'prove_dust_spend'`.

- [ ] **Step 5.3: Find the canonical Dust spend builder**

Read `ledger/src/dust.rs` between lines 1683 and 1780 (the `pub fn spend(...)` section we found earlier; line 1685). This is where a `ProofPreimage` for a spend is constructed. Note the function signature and its inputs (which DustState fields, RNG, value, etc).

If `dust::spend()` produces a `ProofPreimage` directly we'll call it. If instead it returns a higher-level structure, follow that structure into `ledger::prove`'s code path until we find the place a single Dust-spend `ProofPreimage` is produced.

Save the path here as a comment in `dust.rs` for future readers.

- [ ] **Step 5.4: Implement `dust.rs`**

Create `mobile-bench/prover-core/src/dust.rs`:

```rust
use std::time::Instant;

use rand::SeedableRng;
use rand_chacha::ChaCha20Rng;
use serialize::tagged_deserialize;
use transient_crypto::proofs::{ParamsProverProvider, VerifierKey};

use crate::{BenchOpts, Error, ProofRun, ProverCore, Result};

const LABEL: &str = "dust-spend";

impl ProverCore {
    pub async fn prove_dust_spend(&self, opts: BenchOpts) -> Result<ProofRun> {
        let seed = opts.seed.unwrap_or(0x42);
        let mut rng = ChaCha20Rng::seed_from_u64(seed);

        // STEP A: build a Dust-spend ProofPreimage from canned inputs.
        // We mirror what `ledger::test_utilities::tx_prove_bind` does for a
        // single dust spend, minus the surrounding Transaction wrapping.
        // The exact construction depends on what's accessible from
        // `ledger::dust::*` — adapt this block once you've read
        // `ledger/src/dust.rs` per Step 5.3.
        let preimage = build_canned_dust_spend_preimage(&mut rng)?;

        // STEP B: prove via the ProverCore's DustResolver. This will
        // download `spend.prover` on first call and cache it under our dir.
        let started = Instant::now();
        let (proof, _) = preimage
            .prove::<zkir::IrSource>(&mut rng, &self.params.zswap.0, &*self.params.dust)
            .await
            .map_err(|e| Error::Anyhow(anyhow::anyhow!("prove: {e}")))?;
        let elapsed = started.elapsed();

        // STEP C: serialize and (optionally) verify.
        let proof_bytes = {
            let mut buf = Vec::new();
            serialize::tagged_serialize(&proof, &mut buf)
                .map_err(|e| Error::Anyhow(anyhow::anyhow!("serialize: {e}")))?;
            buf
        };

        let (verified, verify_elapsed) = if opts.verify_after {
            let v_started = Instant::now();
            // The Dust spend verifier key is bundled in `ledger/static/dust/spend.verifier`.
            let vk_bytes = include_bytes!("../../../ledger/static/dust/spend.verifier");
            let vk: VerifierKey = tagged_deserialize(&mut &vk_bytes[..])
                .map_err(|e| Error::Anyhow(anyhow::anyhow!("deserialize vk: {e}")))?;
            let k = vk.force_init().map_err(|e| Error::Anyhow(anyhow::anyhow!(e)))?.k();
            let params_v = self
                .params
                .zswap
                .0
                .get_params(k)
                .await
                .map_err(|e| Error::Anyhow(anyhow::anyhow!("get_params: {e}")))?
                .as_verifier();
            // The Dust-spend public inputs come from the preimage; mirror what
            // proof-server's `check` endpoint does for verification.
            let pis = canned_dust_spend_pis(&preimage);
            let ok = vk.verify(&params_v, &proof, pis.iter().copied()).is_ok();
            (Some(ok), Some(v_started.elapsed()))
        } else {
            (None, None)
        };

        Ok(ProofRun {
            label: LABEL,
            k: 13,
            elapsed,
            verify_elapsed,
            verified,
            proof_bytes,
        })
    }
}

fn build_canned_dust_spend_preimage(
    _rng: &mut ChaCha20Rng,
) -> Result<transient_crypto::proofs::ProofPreimage> {
    // FILL IN per Step 5.3:
    // - construct a DustState (or use ledger::test_utilities::TestState if
    //   that's the simplest entry).
    // - call ledger::dust::DustState::spend() to produce a ProofPreimage.
    // - return it.
    todo!("populate from ledger::dust::DustState::spend / test_utilities patterns")
}

fn canned_dust_spend_pis(_preimage: &transient_crypto::proofs::ProofPreimage)
    -> Vec<transient_crypto::curve::Fr> {
    // FILL IN: derive the public-input vector from the preimage's
    // public_transcript_inputs/outputs in the same way proof-server does.
    todo!("populate after the preimage builder is in place")
}
```

Wire it in. Edit `mobile-bench/prover-core/src/lib.rs`, add:

```rust
mod dust;
```

- [ ] **Step 5.5: Resolve the two `todo!()`s**

Read `proof-server/src/endpoints.rs` lines 175-end to find the exact place that builds and calls a `prove` for a Dust spend. Mirror that construction in `build_canned_dust_spend_preimage`.

For `canned_dust_spend_pis`, look at how `transient_crypto::proofs::ProofPreimage::prove` exposes `pis` — the function we read earlier returns `(proof, pis, pi_skips)` from `ir.prove(...)`, but the public version only returns `(proof, pi_skips)`. Verification needs `pis` separately; check whether `proof-server`'s verifier path threads them through `WrappedIr` or recomputes them. Mirror the same logic.

Iterate: `cargo test -p prover-core --test library_path -- --ignored prove_dust_spend`. If your first run fails to download params, ensure you have working internet and the data-provider URL is reachable. Once the download succeeds it caches.

- [ ] **Step 5.6: Run with --ignored once it compiles**

Run:
```bash
cargo test -p prover-core --test library_path -- --ignored prove_dust_spend
```
Expected: passes. The first run will print download progress for ~256 MB; subsequent runs are fast.

- [ ] **Step 5.7: Commit**

```bash
git add mobile-bench/prover-core
git commit -S -s -m "$(cat <<'EOF'
feat(prover-core): implement prove_dust_spend with on-demand key download

Builds a canned Dust-spend ProofPreimage following the same construction
proof-server uses internally, proves it via transient_crypto, optionally
verifies against the bundled spend.verifier, and reports timing.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 6: Cross-compile gate — does `prover-core` build for `aarch64-linux-android`?

This is the highest-risk task. It must run **before** any UI work; if a transitive dep needs C/CMake we want to know now.

**Files:** No source changes; this is a build verification task.

- [ ] **Step 6.1: Confirm toolchain prerequisites**

Run:
```bash
which dx
which cargo-ndk
rustup target list --installed | grep aarch64-linux-android
echo "$ANDROID_NDK_HOME"
ls "$ANDROID_NDK_HOME"
```
Expected: all four return populated values. If they don't, finish running `mobile-bench/scripts/setup-android-toolchain.sh` and `source ~/.zshrc` first.

- [ ] **Step 6.2: Attempt the cross-compile**

Run:
```bash
cargo ndk -t arm64-v8a build -p prover-core --release 2>&1 | tee /tmp/ndk-build.log
```
Expected: builds successfully with no link errors.

- [ ] **Step 6.3: Triage failures, if any**

If build fails, the most likely culprits and fixes:

| Symptom | Likely cause | Fix |
|---------|--------------|-----|
| `cc: error: …blst…` | `blst` C dep needs Android NDK linker | Look at `Cargo.lock` for `blst`; if it's optional (e.g., a feature on `midnight-curves`), disable it; if not, add `[target.'cfg(target_os = "android")'.dependencies]` overrides or a no-blst feature flag. |
| `linking with 'cc' failed: linker not found` | `cargo-ndk` not finding the right NDK linker | Re-export `ANDROID_NDK_HOME`; check that NDK r27 layout matches what `cargo-ndk` expects; try `cargo-ndk` with `-p 34` for explicit API level. |
| `getrandom: backend 'wasm-bindgen' not selected` | Wrong getrandom feature flag | Check `Cargo.toml` chains; explicitly enable `getrandom` `std` feature on Android. |
| `tokio: feature 'rt-multi-thread' is required for…` | feature mismatch | Already have rt-multi-thread on tokio; widen as needed. |
| Anything in `actix-web` linkage path | actix-web should NOT be in the prover-core build (no `proof-server-http` feature). If it is, remove the leak. | Check that `proof-server-http` feature is off (default). |

Document whichever blocker (if any) you hit in `mobile-bench/README.md` under a new "Cross-compile notes" section.

- [ ] **Step 6.4: If unfixable in <1 day, scope the offending circuit out**

If after 1 day of effort one of the proof paths still cannot cross-compile, flip a `cfg(not(target_os = "android"))` gate on the offending fn in `prover-core` and document the fact in the spec's Risks section. Continuing is more valuable than perfect parity for this iteration.

- [ ] **Step 6.5: Commit any toolchain notes**

If you added anything to `mobile-bench/README.md` or had to add Cargo gates:
```bash
git add mobile-bench
git commit -S -s -m "$(cat <<'EOF'
chore(prover-core): document Android cross-compile findings

Captures what we found when running `cargo ndk -t arm64-v8a build`:
which deps needed adapter feature flags, what the working configuration
is, and any compile-time scope reductions for Android.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 7: HTTP path (desktop only)

**Files:**
- Create: `mobile-bench/prover-core/src/http.rs`
- Create: `mobile-bench/prover-core/src/server.rs`
- Create: `mobile-bench/prover-core/tests/http_path.rs`
- Modify: `mobile-bench/prover-core/src/lib.rs`

- [ ] **Step 7.1: Write the failing http_path test**

Create `mobile-bench/prover-core/tests/http_path.rs`:

```rust
#![cfg(feature = "proof-server-http")]
#![cfg(not(target_os = "android"))]

use prover_core::{BenchOpts, ProverCore, spawn_local_server};

#[tokio::test(flavor = "multi_thread")]
async fn prove_zkir_example_via_http_matches_library() {
    let _ = tracing_subscriber::fmt::try_init();
    let cache = std::env::temp_dir().join(format!("prover-core-http-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&cache);
    let pc = ProverCore::new(cache).await.expect("init");

    // Library reference run.
    let ref_run = pc.prove_zkir_example(BenchOpts::default()).await.expect("lib");
    assert_eq!(ref_run.verified, Some(true));

    // Spawn the in-process proof-server.
    let server = spawn_local_server().await.expect("server");
    let base_url = server.base_url();

    // HTTP run — must verify too. Latency may differ; equivalence here
    // means "produces a verifying proof for the same circuit", not "byte
    // identical proof".
    let http_run = pc
        .prove_via_http("zkir-fallible-count", &base_url)
        .await
        .expect("http");
    assert_eq!(http_run.verified, Some(true));
    eprintln!(
        "lib={:?}  http={:?}  ratio={:.2}x",
        ref_run.elapsed,
        http_run.elapsed,
        http_run.elapsed.as_secs_f64() / ref_run.elapsed.as_secs_f64()
    );
}
```

- [ ] **Step 7.2: Implement the in-process server spawn**

Create `mobile-bench/prover-core/src/server.rs`:

```rust
#![cfg(feature = "proof-server-http")]

use midnight_proof_server::server;
use midnight_proof_server::worker_pool::WorkerPool;

pub struct LocalServer {
    handle: actix_web::dev::ServerHandle,
    port: u16,
}

impl LocalServer {
    pub fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }
}

pub async fn spawn_local_server() -> std::io::Result<LocalServer> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        actix_web::rt::System::new().block_on(async move {
            // 2 workers, 2 jobs queue, 600s job timeout — same defaults the
            // existing integration tests use.
            let pool = WorkerPool::new(2, 2, 600.0);
            let (srv, port) = server(0, false, pool).expect("server");
            tx.send((srv.handle(), port)).ok();
            srv.await.ok();
        });
    });
    let (handle, port) = rx.recv().map_err(|_| {
        std::io::Error::new(std::io::ErrorKind::Other, "server failed to start")
    })?;
    Ok(LocalServer { handle, port })
}
```

- [ ] **Step 7.3: Implement `prove_via_http`**

Create `mobile-bench/prover-core/src/http.rs`:

```rust
#![cfg(feature = "proof-server-http")]

use std::time::{Duration, Instant};

use crate::{Error, ProofRun, ProverCore, Result};

impl ProverCore {
    pub async fn prove_via_http(&self, label: &'static str, base_url: &str) -> Result<ProofRun> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(600))
            .build()
            .map_err(|e| Error::Anyhow(anyhow::anyhow!("client: {e}")))?;

        // Build the request payload:
        //   - For the zkir example, POST to /prove with the ProofPreimage +
        //     embedded WrappedIr (mirroring proof-server/endpoints.rs:check
        //     payload shape).
        //   - For Dust, POST to /prove-tx.
        // For now, only the zkir-example label is supported.
        let body: Vec<u8> = match label {
            "zkir-fallible-count" => build_zkir_example_payload()?,
            other => return Err(Error::Anyhow(anyhow::anyhow!("unknown label: {other}"))),
        };

        let started = Instant::now();
        let resp = client
            .post(format!("{base_url}/prove"))
            .body(body)
            .send()
            .await
            .map_err(|e| Error::Anyhow(anyhow::anyhow!("post: {e}")))?
            .error_for_status()
            .map_err(|e| Error::Anyhow(anyhow::anyhow!("status: {e}")))?;
        let proof_bytes = resp.bytes().await
            .map_err(|e| Error::Anyhow(anyhow::anyhow!("body: {e}")))?
            .to_vec();
        let elapsed = started.elapsed();

        Ok(ProofRun {
            label: Box::leak(label.to_string().into_boxed_str()),
            k: 13,
            elapsed,
            verify_elapsed: None,
            verified: None, // verification happens server-side already
            proof_bytes,
        })
    }
}

fn build_zkir_example_payload() -> Result<Vec<u8>> {
    // The /prove endpoint expects this tuple, tagged-serialized
    // (verified against proof-server/src/endpoints.rs lines 246-252):
    //
    //   (ProofPreimageVersioned, Option<ProvingKeyMaterial>, Option<Fr>)
    //
    // We pass the preimage + the embedded proving-key material so the
    // server doesn't need to look the key up itself, and None for the
    // override-binding-input.
    use std::sync::Arc;
    use ledger::structure::ProofPreimageVersioned;
    use transient_crypto::proofs::{ProofPreimage, ProvingKeyMaterial};

    let preimage = ProofPreimage {
        inputs: vec![],
        private_transcript: vec![],
        public_transcript_inputs: vec![],
        public_transcript_outputs: vec![],
        binding_input: Default::default(),
        communications_commitment: None,
        key_location: transient_crypto::proofs::KeyLocation(
            std::borrow::Cow::Borrowed("count"),
        ),
    };
    let pkm = ProvingKeyMaterial {
        ir_source: super::zkir_example::IR_BYTES.to_vec(),
        prover_key: super::zkir_example::PROVER_BYTES.to_vec(),
        verifier_key: super::zkir_example::VERIFIER_BYTES.to_vec(),
    };
    let triple: (
        ProofPreimageVersioned,
        Option<ProvingKeyMaterial>,
        Option<transient_crypto::curve::Fr>,
    ) = (
        ProofPreimageVersioned::V2(Arc::new(preimage)),
        Some(pkm),
        None,
    );
    let mut buf = Vec::new();
    serialize::tagged_serialize(&triple, &mut buf)
        .map_err(|e| Error::Anyhow(anyhow::anyhow!("serialize: {e}")))?;
    Ok(buf)
}
```

- [ ] **Step 7.4: Wire modules and re-export**

Edit `mobile-bench/prover-core/src/lib.rs`:

```rust
#[cfg(feature = "proof-server-http")]
mod http;
#[cfg(feature = "proof-server-http")]
mod server;

#[cfg(feature = "proof-server-http")]
pub use server::{LocalServer, spawn_local_server};
```

- [ ] **Step 7.5: Run the http_path test**

Run:
```bash
cargo test -p prover-core --features proof-server-http --test http_path
```
Expected: fails initially because `build_zkir_example_payload` is unimplemented. Read `proof-server/src/endpoints.rs` `prove(...)` (lines around 220-280, search for `tagged_deserialize::<(ProofPreimageVersioned, Option<WrappedIr>)>`) and reverse it: tagged-serialize the same tuple from our preimage + None for the IR (since it's looked up server-side via the resolver — but our embedded test resolver only serves "count", and the proof-server's resolver doesn't know about it!). So you must include the `WrappedIr` so the server doesn't try to resolve.

Iterate until the test passes.

- [ ] **Step 7.6: Commit**

```bash
git add mobile-bench/prover-core
git commit -S -s -m "$(cat <<'EOF'
feat(prover-core): add HTTP path behind proof-server-http feature

Spawns midnight-proof-server::server() in-process and exposes
prove_via_http for latency comparison against the library path.
Desktop-only this iteration (gated cfg(not(target_os = \"android\"))).

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 8: `prover-core` benches

**Files:**
- Create: `mobile-bench/prover-core/benches/proofs.rs`

- [ ] **Step 8.1: Write the criterion bench**

Create `mobile-bench/prover-core/benches/proofs.rs`:

```rust
use criterion::{Criterion, criterion_group, criterion_main};
use prover_core::{BenchOpts, ProverCore};

fn zkir_example(c: &mut Criterion) {
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
    let cache = std::env::temp_dir().join("prover-core-bench");
    let pc = rt.block_on(async { ProverCore::new(cache).await.expect("init") });
    c.bench_function("prove_zkir_example", |b| {
        b.iter(|| {
            rt.block_on(async {
                pc.prove_zkir_example(BenchOpts { verify_after: false, seed: Some(0) })
                    .await
                    .expect("prove")
            })
        })
    });
}

criterion_group!(benches, zkir_example);
criterion_main!(benches);
```

- [ ] **Step 8.2: Run it once**

Run:
```bash
cargo bench -p prover-core --features bench --bench proofs -- --warm-up-time 1 --measurement-time 5
```
Expected: completes; prints a `prove_zkir_example` row with mean prove time.

- [ ] **Step 8.3: Commit**

```bash
git add mobile-bench/prover-core/benches mobile-bench/prover-core/Cargo.toml
git commit -S -s -m "$(cat <<'EOF'
test(prover-core): add criterion bench for prove_zkir_example

Library-path-only benchmark, gated on the `bench` feature so it doesn't
slow down a default `cargo bench` invocation.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 9: `dioxus-bench` desktop scaffold

**Files:**
- Create: `mobile-bench/dioxus-bench/Cargo.toml`
- Create: `mobile-bench/dioxus-bench/Dioxus.toml`
- Create: `mobile-bench/dioxus-bench/src/main.rs`
- Create: `mobile-bench/dioxus-bench/src/app.rs`
- Create: `mobile-bench/dioxus-bench/src/runner.rs`
- Create: `mobile-bench/dioxus-bench/src/platform/mod.rs`
- Create: `mobile-bench/dioxus-bench/src/platform/desktop.rs`
- Create: `mobile-bench/dioxus-bench/assets/styles.css`
- Modify: `Cargo.toml` (root) — add `mobile-bench/dioxus-bench` to `members`.

- [ ] **Step 9.1: Add member**

Edit root `Cargo.toml`. Add `"mobile-bench/dioxus-bench",` to `members`.

- [ ] **Step 9.2: Create the dioxus-bench manifest**

Create `mobile-bench/dioxus-bench/Cargo.toml`:

```toml
[package]
name = "dioxus-bench"
version = "0.1.0"
edition = "2024"
license.workspace = true
publish = false

[features]
default = []
proof-server-http = ["prover-core/proof-server-http"]

[dependencies]
prover-core = { path = "../prover-core" }
dioxus = { version = "0.6", features = ["desktop"] }
tokio = { version = "1.46.1", features = ["rt-multi-thread", "macros", "sync"] }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
tracing = "0.1"
tracing-subscriber = "0.3"

[target.'cfg(target_os = "android")'.dependencies]
dioxus = { version = "0.6", features = ["mobile"] }
ndk-context = "0.1"
jni = "0.21"

[target.'cfg(not(target_os = "android"))'.dependencies]
dirs = "5"
```

- [ ] **Step 9.3: Create Dioxus.toml**

Create `mobile-bench/dioxus-bench/Dioxus.toml`:

```toml
[application]
name = "dioxus-bench"
default_platform = "desktop"
asset_dir = "assets"

[application.android]
package = "io.iohk.midnight.bench"
label = "Midnight Proof Bench"
```

- [ ] **Step 9.4: Skeleton main + app**

Create `mobile-bench/dioxus-bench/src/main.rs`:

```rust
#![deny(warnings)]

mod app;
mod platform;
mod runner;

use dioxus::prelude::*;

fn main() {
    let _ = tracing_subscriber::fmt::try_init();
    dioxus::launch(app::App);
}
```

Create `mobile-bench/dioxus-bench/src/platform/mod.rs`:

```rust
#[cfg(target_os = "android")]
mod android;
#[cfg(not(target_os = "android"))]
mod desktop;

#[cfg(target_os = "android")]
pub use android::cache_dir;
#[cfg(not(target_os = "android"))]
pub use desktop::cache_dir;
```

Create `mobile-bench/dioxus-bench/src/platform/desktop.rs`:

```rust
use std::path::PathBuf;

pub fn cache_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| std::env::temp_dir())
        .join("midnight-bench")
        .join("params")
}
```

Create `mobile-bench/dioxus-bench/src/runner.rs`:

```rust
use std::sync::Arc;
use std::time::Duration;

use prover_core::{BenchOpts, ProofRun, ProverCore};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecPath { Library, Http }

#[derive(Debug, Clone)]
pub enum RunStatus {
    Idle,
    Initializing,
    Proving(&'static str),
    Done,
    Error(String),
}

#[derive(Clone)]
pub struct Runner {
    inner: Arc<ProverCore>,
}

impl Runner {
    pub async fn new() -> Result<Self, String> {
        let dir = crate::platform::cache_dir();
        ProverCore::new(dir).await
            .map(|pc| Self { inner: Arc::new(pc) })
            .map_err(|e| e.to_string())
    }

    pub async fn run_zkir(&self) -> Result<ProofRun, String> {
        self.inner.prove_zkir_example(BenchOpts::default()).await
            .map_err(|e| e.to_string())
    }

    pub async fn run_dust(&self) -> Result<ProofRun, String> {
        self.inner.prove_dust_spend(BenchOpts::default()).await
            .map_err(|e| e.to_string())
    }
}

pub fn fmt_duration(d: Duration) -> String {
    if d.as_secs() >= 1 { format!("{:.2} s", d.as_secs_f64()) }
    else { format!("{} ms", d.as_millis()) }
}
```

Create `mobile-bench/dioxus-bench/src/app.rs`:

```rust
use dioxus::prelude::*;
use prover_core::ProofRun;

use crate::runner::{ExecPath, RunStatus, Runner, fmt_duration};

#[component]
pub fn App() -> Element {
    let mut status = use_signal(|| RunStatus::Idle);
    let mut last_run = use_signal::<Option<ProofRun>>(|| None);
    let path = use_signal(|| ExecPath::Library);

    let runner = use_resource(|| async move { Runner::new().await });

    rsx! {
        link { rel: "stylesheet", href: asset!("/assets/styles.css") }
        h1 { "Midnight Proof Bench" }

        div { class: "row",
            label { "Path: " }
            // For iteration 1 the radio is read-only; HTTP path is wired
            // up only when the proof-server-http feature is enabled.
            input { r#type: "radio", checked: true, "Library" }
        }

        div { class: "row",
            button {
                disabled: !matches!(*status.read(), RunStatus::Idle | RunStatus::Done | RunStatus::Error(_)),
                onclick: move |_| {
                    let r = runner.clone();
                    spawn(async move {
                        if let Some(Ok(r)) = r.read_unchecked().as_ref() {
                            status.set(RunStatus::Proving("zkir-fallible-count"));
                            match r.run_zkir().await {
                                Ok(run) => { last_run.set(Some(run)); status.set(RunStatus::Done); }
                                Err(e) => status.set(RunStatus::Error(e)),
                            }
                        }
                    });
                },
                "Run zkir example"
            }
            button {
                disabled: !matches!(*status.read(), RunStatus::Idle | RunStatus::Done | RunStatus::Error(_)),
                onclick: move |_| {
                    let r = runner.clone();
                    spawn(async move {
                        if let Some(Ok(r)) = r.read_unchecked().as_ref() {
                            status.set(RunStatus::Proving("dust-spend"));
                            match r.run_dust().await {
                                Ok(run) => { last_run.set(Some(run)); status.set(RunStatus::Done); }
                                Err(e) => status.set(RunStatus::Error(e)),
                            }
                        }
                    });
                },
                "Run Dust spend"
            }
        }

        div { class: "status", "Status: {format_status(&status.read())}" }

        if let Some(run) = last_run.read().as_ref() {
            div { class: "result",
                h3 { "Last run" }
                div { "Label:        {run.label}" }
                div { "k:            {run.k}" }
                div { "Prove time:   {fmt_duration(run.elapsed)}" }
                if let Some(v) = run.verify_elapsed {
                    div { "Verify time:  {fmt_duration(v)}" }
                }
                div { "Verified:     {run.verified.map(|b| if b {\"yes\"} else {\"no\"}).unwrap_or(\"n/a\")}" }
                div { "Proof size:   {run.proof_bytes.len()} B" }
                button {
                    onclick: move |_| {
                        let json = serde_json::to_string_pretty(&*last_run.read()).unwrap();
                        // Print to stdout for now; the real "copy to clipboard"
                        // can be added later.
                        println!("{json}");
                    },
                    "Print result as JSON"
                }
            }
        }
    }
}

fn format_status(s: &RunStatus) -> String {
    match s {
        RunStatus::Idle => "idle".into(),
        RunStatus::Initializing => "initializing".into(),
        RunStatus::Proving(l) => format!("proving {l}…"),
        RunStatus::Done => "done".into(),
        RunStatus::Error(e) => format!("error: {e}"),
    }
}
```

Create `mobile-bench/dioxus-bench/assets/styles.css`:

```css
body { font-family: -apple-system, sans-serif; padding: 20px; }
h1 { font-size: 1.4rem; }
.row { margin: 12px 0; display: flex; gap: 8px; align-items: center; }
.status { margin: 16px 0; font-family: ui-monospace, monospace; }
.result { border: 1px solid #ddd; border-radius: 6px; padding: 12px; margin-top: 12px; }
.result div { font-family: ui-monospace, monospace; }
button[disabled] { opacity: 0.5; }
```

- [ ] **Step 9.5: Build for desktop**

Run:
```bash
cd mobile-bench/dioxus-bench
dx build --platform desktop
```
Expected: succeeds. If `dx` complains about workspace config, add a top-level `[workspace.metadata.dioxus]` block to root `Cargo.toml` per the Dioxus docs.

- [ ] **Step 9.6: Smoke-test it**

Run:
```bash
cd mobile-bench/dioxus-bench
dx serve --platform desktop
```
Expected: a window appears titled "Midnight Proof Bench". Click "Run zkir example". Watch the status flip to "proving zkir-fallible-count…" then "done". Verify the Last-run panel shows non-zero numbers.

- [ ] **Step 9.7: Commit**

```bash
cd ../..
git add Cargo.toml Cargo.lock mobile-bench/dioxus-bench
git commit -S -s -m "$(cat <<'EOF'
feat(dioxus-bench): scaffold desktop UI calling prover-core

Single-screen Dioxus 0.6 app: two buttons (zkir example, Dust spend),
a status line, and a last-run panel. Wires platform::cache_dir() into
ProverCore::new and the buttons into prover-core's two prove fns.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 10: Android target — emulator first

**Files:**
- Create: `mobile-bench/dioxus-bench/src/platform/android.rs`
- Modify: `mobile-bench/dioxus-bench/Cargo.toml` (already has android-target deps; verify)
- Modify: `mobile-bench/dioxus-bench/Dioxus.toml` (verify android section)

- [ ] **Step 10.1: Implement android cache_dir**

Create `mobile-bench/dioxus-bench/src/platform/android.rs`:

```rust
use std::path::PathBuf;

/// Resolves the Android app-private files dir via JNI. Called once at
/// app startup before constructing ProverCore.
pub fn cache_dir() -> PathBuf {
    let ctx = ndk_context::android_context();
    let vm = unsafe { jni::JavaVM::from_raw(ctx.vm().cast()).unwrap() };
    let env = vm.attach_current_thread().unwrap();
    let context = unsafe { jni::objects::JObject::from_raw(ctx.context().cast()) };
    let files_dir = env
        .call_method(context, "getFilesDir", "()Ljava/io/File;", &[])
        .and_then(|r| r.l())
        .expect("getFilesDir");
    let abs_path: jni::objects::JString = env
        .call_method(files_dir, "getAbsolutePath", "()Ljava/lang/String;", &[])
        .and_then(|r| r.l())
        .map(Into::into)
        .expect("getAbsolutePath");
    let s: String = env.get_string(&abs_path).expect("string").into();
    PathBuf::from(s).join("midnight-bench").join("params")
}
```

- [ ] **Step 10.2: Confirm android manifest pieces**

Run:
```bash
cd mobile-bench/dioxus-bench
dx bundle --platform android --release 2>&1 | tee /tmp/dx-android.log
```

Expected: produces an APK under `target/dx/dioxus-bench/release/android/`. If it fails because of manifest permissions, edit the generated `AndroidManifest.xml` (Dioxus may regenerate this; if so, look for the per-app `Dioxus.toml` directive that lets you add `<uses-permission android:name="android.permission.INTERNET"/>` and `android:largeHeap="true"`).

- [ ] **Step 10.3: Boot the emulator and install**

Run:
```bash
emulator -avd midnight_bench_arm64_api34 &
adb wait-for-device
adb install target/dx/dioxus-bench/release/android/dioxus-bench.apk
adb shell am start -n io.iohk.midnight.bench/.MainActivity
adb logcat -s dioxus-bench midnight-bench prover-core
```

Expected: app launches in emulator. Tap "Run zkir example". Watch logcat for the "prove_zkir_example: …" eprintln, or for any panic.

- [ ] **Step 10.4: Triage common Android failures**

| Symptom | Fix |
|---------|-----|
| App crashes immediately on launch | Most likely `cache_dir()` JNI call panicked. Wrap in `Result` and surface the error to the UI. |
| OOM during params load | Manifest needs `android:largeHeap="true"`. |
| Network unreachable during download | Manifest needs `<uses-permission android:name="android.permission.INTERNET"/>`. |
| Hangs during proving | Long latencies are expected; let it run for several minutes. Confirm via logcat that it's actually running. |

- [ ] **Step 10.5: Commit**

```bash
cd ../..
git add mobile-bench/dioxus-bench
git commit -S -s -m "$(cat <<'EOF'
feat(dioxus-bench): wire up Android target with JNI-resolved cache dir

Android cache_dir() resolves Context.getFilesDir() via ndk-context + jni.
Manifest gets INTERNET permission and largeHeap for KZG params load.
Smoke-tested on arm64 emulator (API 34) with both proof buttons.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 11: S24 Ultra deployment + RESULTS.md

**Files:**
- Create: `mobile-bench/RESULTS.md`
- Create or update: `mobile-bench/README.md`

- [ ] **Step 11.1: Plug in the S24 Ultra and verify ADB sees it**

Run:
```bash
adb devices
```
Expected: lists at least one entry with the phone's serial and `device` status. If it shows `unauthorized`, accept the trust prompt on the phone.

- [ ] **Step 11.2: Sideload the APK**

Run:
```bash
cd mobile-bench/dioxus-bench
adb -s <serial> install -r target/dx/dioxus-bench/release/android/dioxus-bench.apk
adb -s <serial> shell am start -n io.iohk.midnight.bench/.MainActivity
adb -s <serial> logcat -s dioxus-bench midnight-bench prover-core
```

Expected: app launches on the device. The first run will take time to download params (~256 MB on Wi-Fi).

- [ ] **Step 11.3: Capture three latency numbers**

In the app, tap each button, copy the JSON output (or read it from logcat), and paste into `mobile-bench/RESULTS.md`:

```bash
cat > mobile-bench/RESULTS.md <<'MD'
# Mobile Proof Bench — iteration 1 results

Captured 2026-04-28. Each run = 3 sequential proves, median of runs 2 and 3.

## zkir example (fallible/count)

| Surface         | Prove time | Verify time | Proof bytes |
|-----------------|------------|-------------|-------------|
| macOS desktop   | …          | …           | …           |
| Android emulator (arm64-v8a) | … | … | … |
| Samsung S24 Ultra | …        | …           | …           |

## Dust spend

| Surface         | Prove time | Verify time | Proof bytes |
|-----------------|------------|-------------|-------------|
| macOS desktop   | …          | …           | …           |
| Android emulator (arm64-v8a) | … | … | … |
| Samsung S24 Ultra | …        | …           | …           |

## Raw JSON

```jsonl
<paste copy-button output for each run, one per line>
```
MD
```

Fill in actual numbers as you collect them.

- [ ] **Step 11.4: Update README with the run-it instructions**

Create or update `mobile-bench/README.md` with:

```markdown
# mobile-bench

A Dioxus app that proves a Midnight ZK circuit on macOS desktop, Android
emulator, and Android device, recording prove/verify latency.

See `docs/superpowers/specs/2026-04-28-mobile-proof-bench-design.md`.

## One-time setup

```bash
bash mobile-bench/scripts/setup-android-toolchain.sh
# Then add the printed env vars to ~/.zshrc and `source` it.
```

## Run

### Desktop

```bash
cd mobile-bench/dioxus-bench
dx serve --platform desktop
```

### Android emulator

```bash
emulator -avd midnight_bench_arm64_api34 &
cd mobile-bench/dioxus-bench
dx serve --platform android
```

### Real device (Samsung S24 Ultra etc.)

```bash
# On the phone: enable Developer options → USB debugging.
# Plug in via USB, accept the trust prompt.
cd mobile-bench/dioxus-bench
dx bundle --platform android --release
adb install -r target/dx/dioxus-bench/release/android/dioxus-bench.apk
adb shell am start -n io.iohk.midnight.bench/.MainActivity
adb logcat -s dioxus-bench midnight-bench prover-core
```

## Tests

```bash
# Library path:
cargo test -p prover-core --test library_path

# Library + Dust (downloads ~256 MB on first run):
cargo test -p prover-core --test library_path -- --ignored

# HTTP path comparison (desktop only):
cargo test -p prover-core --features proof-server-http --test http_path

# Bench:
cargo bench -p prover-core --features bench
```

## Latency snapshots

See `RESULTS.md`.
```

- [ ] **Step 11.5: Commit**

```bash
git add mobile-bench/README.md mobile-bench/RESULTS.md
git commit -S -s -m "$(cat <<'EOF'
docs(mobile-bench): add README and RESULTS table

Run-it instructions for the three surfaces, plus a populated table of
captured latency numbers from a real S24 Ultra run.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Self-review checklist

- [ ] `cargo test -p prover-core` (default features) passes on macOS arm64.
- [ ] `cargo test -p prover-core --test library_path -- --ignored` passes (Dust).
- [ ] `cargo test -p prover-core --features proof-server-http --test http_path` passes (desktop).
- [ ] `cargo bench -p prover-core --features bench` runs to completion.
- [ ] `dx serve --platform desktop` opens a window, both buttons work.
- [ ] Android emulator runs both proofs to completion (latency irrelevant; it must finish).
- [ ] S24 Ultra runs both proofs to completion; numbers are in `RESULTS.md`.
- [ ] No new warnings under `#![deny(warnings)]` in any new crate.
- [ ] All commits are GPG-signed (`G`) and DCO-signed-off.
- [ ] No file in `mobile-bench/` was added to root `Cargo.toml`'s `default-members`.

## Risks acknowledged in this plan

- **Task 3 step 3.4** assumes `MIDNIGHT_PARAM_CACHE_DIR` is the env var honoured by `base_crypto::data_provider`. If it isn't, the step explicitly tells you to find the real hook before proceeding.
- **Task 5 steps 5.3 / 5.4 / 5.5** contain `todo!()` placeholders that **must** be replaced by real code derived from `ledger::dust` and `proof-server::endpoints`. They are explicitly flagged as research-and-fill.
- **Task 6** is the cross-compile gate. If a transitive dep won't build for `aarch64-linux-android`, this plan offers two escape hatches (feature flag the offender; or scope-out the offending circuit on Android via cfg). Either is acceptable for iteration 1.
- **Task 7 step 7.5** has a `build_zkir_example_payload` placeholder that must be filled in by reverse-engineering `proof-server::endpoints::prove`. The plan tells you exactly where to look.

These four are the only places in this plan where exact code is intentionally left to the implementer because it depends on details that change with each `transient-crypto` / `ledger` revision and is faster to read at execution time than to anticipate here.
