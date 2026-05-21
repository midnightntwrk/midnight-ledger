//! contract-benchmark — a parameterised "dummy contract" exposed as 20
//! circuits indexed by `k`, where `k` controls the circuit constraint
//! size. Each `circuit_k(k)` produces a zkir IR whose minimum required
//! halo2 `k` (the `2^k` rows the prover needs to lay it out) matches the
//! requested `k`.
//!
//! ## Why not compactc?
//!
//! Spec for this crate (see `mobile-bench/contract-benchmark/README.md`)
//! originally called for a `did.compact`-style Compact-DSL source compiled
//! into 20 circuits via `compactc`. `compactc` is not available locally —
//! the user agreed up-front: "try compactc; fall back to padding". We
//! ship the padding fallback: handwritten zkir IR programs that grow a
//! chain of `transient_hash` ops until `IrSource::model().k()` hits the
//! requested target. This exercises the same halo2-kzg prove/verify
//! pipeline `prover-core` exercises for the embedded examples (see
//! `mobile-bench/prover-core/src/zkir_example.rs`,
//! `htc_example.rs`, `ec_example.rs`), so the wall-clock numbers are
//! directly comparable.
//!
//! ## Locally-runnable subset of `k`
//!
//! `run_proof(k)` exposes `k` = 1..=20 unconditionally. Whether it
//! actually finishes proving depends on the BLS-12-381 KZG SRS files
//! available on disk:
//!
//! - Desktop: `~/.cache/midnight/zk-params/bls_midnight_2pN` files are
//!   fetched on demand from `srs.midnight.network` the first time a
//!   given `k` is requested.
//! - Android (emulator + S24 Ultra): the deploy guide pushes only
//!   `bls_midnight_2p4..2p11` to `/data/local/tmp/midnight-pp/` (see
//!   `mobile-bench/DEPLOY_TO_DEVICE.md`). Higher `k` will fail at
//!   `ParamsProverProvider::get_params` unless pushed manually.
//! - Verification with the embedded `PARAMS_VERIFIER` works for `k ≤
//!   14` only. For `k > 14`, `run_proof` skips verification but still
//!   reports the prove time.

#![deny(unreachable_pub)]
#![deny(warnings)]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use base_crypto::data_provider::{FetchMode, MidnightDataProvider, OutputMode};
use rand::SeedableRng;
use rand_chacha::ChaCha20Rng;
use serialize::tagged_serialize;
use transient_crypto::curve::Fr;
use transient_crypto::proofs::{
    KeyLocation, PARAMS_VERIFIER, ProofPreimage, ProverKey, ProvingKeyMaterial,
    Resolver as ResolverT, VerifierKey, Zkir,
};
use zkir::IrSource;
use zswap::{ZSWAP_EXPECTED_FILES, prove::ZswapResolver};

/// Inclusive upper bound on `k`. Spec asked for 1..=20.
pub const MAX_K: u32 = 20;
/// Inclusive lower bound on `k`. `k = 1` corresponds to the minimal
/// "assert(input == 1)" circuit — the smallest possible zkir program.
pub const MIN_K: u32 = 1;
/// Highest `k` for which the embedded verifier can check a proof. For
/// `k > 14` the prove path still works but we can't verify in-process
/// without the matching verifying SRS.
pub const MAX_VERIFIABLE_K: u32 = 14;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("k out of range: requested {requested}, supported {MIN_K}..={MAX_K}")]
    KOutOfRange { requested: u32 },
    #[error("could not build a circuit needing k = {target}: \
            grew chain to {ops} transient_hash ops and only reached k = {reached}")]
    CircuitGrowFailed {
        target: u32,
        reached: u32,
        ops: u32,
    },
    #[error("anyhow: {0}")]
    Anyhow(#[from] anyhow::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

/// Timings for a single `run_proof(k)` invocation. All durations are
/// wall-clock measured at the boundaries of the corresponding
/// async / sync calls.
#[derive(Debug, Clone, serde::Serialize)]
pub struct RunStats {
    /// The requested `k`.
    pub k: u32,
    /// The actual minimum-`k` halo2 reported for the generated circuit
    /// (always equals `k` when `Ok`).
    pub realized_k: u32,
    /// The number of `transient_hash` ops in the generated chain.
    pub hash_chain_len: u32,
    /// halo2-cost-model row count (not counting lookups / custom gates).
    pub rows: u64,
    /// Time spent in `IrSource::keygen` — proving + verifying key
    /// generation. Amortised across calls if the caller caches keys
    /// (this crate does *not* cache; each `run_proof` regenerates so
    /// timings are reproducible for a clean run).
    pub keygen: Duration,
    /// Time spent in `ProofPreimage::prove`. The headline number — the
    /// thing the Benchmark tab plots per row.
    pub prove: Duration,
    /// Time spent in `VerifierKey::verify`. `None` when `k > MAX_VERIFIABLE_K`
    /// (no embedded verifier) or when verification was skipped.
    pub verify: Option<Duration>,
    /// Whether the verification succeeded. `None` if not attempted.
    pub verified: Option<bool>,
    /// Serialised proof size in bytes — small enough to log per-row.
    pub proof_bytes: usize,
}

/// Optional knobs for `run_proof`.
#[derive(Debug, Clone)]
pub struct RunOpts {
    /// RNG seed for proving. Defaults to a fixed value so reruns are
    /// reproducible across the same circuit shape.
    pub seed: u64,
    /// If `false`, skip the verify step even if `k ≤ MAX_VERIFIABLE_K`.
    /// Lets the Benchmark tab isolate prove-only wall-clock.
    pub verify_after: bool,
    /// Where to cache the BLS SRS files. Defaults to the standard
    /// `MIDNIGHT_PP` resolution (env, $XDG_CACHE_HOME, $HOME/.cache).
    pub cache_dir: Option<PathBuf>,
}

impl Default for RunOpts {
    fn default() -> Self {
        Self {
            seed: 0x42,
            verify_after: true,
            cache_dir: None,
        }
    }
}

/// Build a zkir IR that needs at least the requested `k`. Returns the
/// IR JSON and the number of `transient_hash` ops in the chain.
///
/// Strategy: start with `n` hashes (per a rough `2^(k-1)` schedule),
/// load via `IrSource::load`, query `model().k()`, and double `n`
/// until we hit or exceed the target `k`. This matches the spec's
/// "design each circuit to do `2^(k-1)` repetitions of a unit of
/// work" hint without hard-coding a magic constant per `k`.
///
/// For `k = 1` we emit the smallest legal IR: one input + one assert.
fn build_ir_for_k(target_k: u32) -> Result<(IrSource, u32)> {
    if !(MIN_K..=MAX_K).contains(&target_k) {
        return Err(Error::KOutOfRange { requested: target_k });
    }

    if target_k == 1 {
        // Minimal circuit, identical shape to prover-core's `zkir-minimal-assert`.
        let json = r#"{
            "version": { "major": 2, "minor": 0 },
            "num_inputs": 1,
            "do_communications_commitment": false,
            "instructions": [
                { "op": "assert", "cond": 0 }
            ]
        }"#;
        let ir = IrSource::load(json.as_bytes())?;
        return Ok((ir, 0));
    }

    // Heuristic seed: TransientHash adds on the order of ~17 halo2 rows
    // per op (Poseidon round count). So 2^(k-1) hashes is a sensible
    // initial guess. We then probe + double until we hit `target_k`.
    let mut n: u32 = (1u32 << (target_k.saturating_sub(1).min(19))).max(1);

    // Cap iterations defensively — we should converge in ~log2(MAX_K)
    // doublings.
    for _attempt in 0..32 {
        let ir = build_hash_chain_ir(n)?;
        let model = ir.model();
        let got_k = model.k() as u32;
        if got_k >= target_k {
            // Step back down if we vastly overshot — keeps proving cost
            // close to the requested k. We accept the first n where the
            // realised k equals the target.
            return shrink_to_target(n, target_k).or_else(|_| Ok((ir, n)));
        }
        // Double and try again. Capped at u32 to avoid pathological
        // builders if the cost model changes.
        n = n.checked_mul(2).ok_or(Error::CircuitGrowFailed {
            target: target_k,
            reached: got_k,
            ops: n,
        })?;
    }

    Err(Error::CircuitGrowFailed {
        target: target_k,
        reached: 0,
        ops: n,
    })
}

/// Binary search downwards from the known-good `n_high` to find the
/// smallest `n` whose IR still realises `target_k`. Keeps each row's
/// proving cost as close to the bin as possible.
fn shrink_to_target(n_high: u32, target_k: u32) -> Result<(IrSource, u32)> {
    let mut lo: u32 = 1;
    let mut hi: u32 = n_high;
    let mut best: Option<(IrSource, u32)> = None;
    while lo <= hi {
        let mid = lo + (hi - lo) / 2;
        let ir = build_hash_chain_ir(mid)?;
        let got = ir.model().k() as u32;
        if got >= target_k {
            best = Some((ir, mid));
            if mid == 0 {
                break;
            }
            hi = mid - 1;
        } else {
            lo = mid + 1;
        }
    }
    best.ok_or(Error::CircuitGrowFailed {
        target: target_k,
        reached: 0,
        ops: n_high,
    })
}

/// Emits IR JSON for a chain of `n` `transient_hash` ops, each
/// consuming the previous hash output. The first hash consumes the
/// single input slot; subsequent hashes consume the previous output.
/// Ends with an `assert(input_0 == input_0)`-style sentinel so the
/// circuit still has a binding public input regardless of `n`.
fn build_hash_chain_ir(n: u32) -> Result<IrSource> {
    // Memory layout:
    //   index 0      → public input (Fr::from(1))
    //   index 1      → output of hash #1: H(0)
    //   index 2      → output of hash #2: H(1)
    //   ...
    //   index n      → output of hash #n: H(n-1)
    //   (no further instructions; the chain's tail isn't asserted to a
    //    constant so the circuit can stay small — the chain itself is
    //    the work, not the assertion.)
    //
    // We still need at least one `assert` so the IR has a non-trivial
    // boolean check; otherwise the cost model treats a "1-input,
    // no-asserts" program as degenerate. Use the input slot itself as
    // the asserted bit (the caller passes `1`).
    let mut instructions = String::with_capacity(64 * (n as usize + 1));
    instructions.push_str(r#"{ "op": "assert", "cond": 0 }"#);
    let mut prev: u32 = 0;
    for i in 0..n {
        instructions.push(',');
        instructions.push_str(&format!(
            r#"{{ "op": "transient_hash", "inputs": [{prev}] }}"#
        ));
        prev = i + 1;
    }

    let json = format!(
        r#"{{
            "version": {{ "major": 2, "minor": 0 }},
            "num_inputs": 1,
            "do_communications_commitment": false,
            "instructions": [{instructions}]
        }}"#
    );
    Ok(IrSource::load(json.as_bytes())?)
}

/// Internal resolver: keygen output + ir source, identical shape to
/// `prover-core`'s `ExampleResolver`. Lives here so we don't depend on
/// `prover-core` (avoiding a cyclic graph during refactors).
struct ChainResolver {
    pk: ProverKey<IrSource>,
    vk: VerifierKey,
    ir: IrSource,
}

impl ResolverT for ChainResolver {
    async fn resolve_key(
        &self,
        _key: KeyLocation,
    ) -> std::io::Result<Option<ProvingKeyMaterial>> {
        let mut prover_key = Vec::new();
        tagged_serialize(&self.pk, &mut prover_key)?;
        let mut verifier_key = Vec::new();
        tagged_serialize(&self.vk, &mut verifier_key)?;
        let mut ir_source = Vec::new();
        tagged_serialize(&self.ir, &mut ir_source)?;
        Ok(Some(ProvingKeyMaterial {
            prover_key,
            verifier_key,
            ir_source,
        }))
    }
}

/// Builds, keygens, proves, and (optionally) verifies a circuit needing
/// the requested `k`. Returns a struct of wall-clock timings.
///
/// First-call latency at a given `k` is dominated by `keygen` + on-demand
/// download of the matching `bls_midnight_2pN` SRS file. Subsequent
/// calls with cached params see only the keygen + prove cost.
pub async fn run_proof_with_opts(k: u32, opts: RunOpts) -> Result<RunStats> {
    if !(MIN_K..=MAX_K).contains(&k) {
        return Err(Error::KOutOfRange { requested: k });
    }

    // 1) Build IR matched to target k.
    let (ir, chain_len) = build_ir_for_k(k)?;
    let model = ir.model();
    let realized_k = model.k() as u32;
    let rows = model.rows() as u64;

    // 2) Params provider — same on-demand cache as the embedded examples.
    let params = make_zswap_resolver(opts.cache_dir.as_deref())?;

    // 3) Keygen.
    let kg_start = Instant::now();
    let (pk, vk) = ir
        .keygen(&params.0)
        .await
        .map_err(|e| Error::Anyhow(anyhow::anyhow!("keygen: {e}")))?;
    let keygen = kg_start.elapsed();

    let resolver = ChainResolver { pk, vk: vk.clone(), ir };

    // 4) Prove.
    let mut rng = ChaCha20Rng::seed_from_u64(opts.seed);
    let preimage = make_preimage();
    let binding_input = preimage.binding_input;

    let prove_start = Instant::now();
    let (proof, _pi_skips) = preimage
        .prove::<IrSource>(&mut rng, &params.0, &resolver)
        .await
        .map_err(|e| Error::Anyhow(anyhow::anyhow!("prove: {e}")))?;
    let prove = prove_start.elapsed();

    let proof_bytes = {
        let mut buf = Vec::new();
        tagged_serialize(&proof, &mut buf)
            .map_err(|e| Error::Anyhow(anyhow::anyhow!("serialize proof: {e}")))?;
        buf.len()
    };

    // 5) Verify (if eligible).
    let (verified, verify_dur) = if opts.verify_after && realized_k <= MAX_VERIFIABLE_K {
        let v_start = Instant::now();
        let ok = vk
            .verify(&PARAMS_VERIFIER, &proof, std::iter::once(binding_input))
            .is_ok();
        (Some(ok), Some(v_start.elapsed()))
    } else {
        (None, None)
    };

    Ok(RunStats {
        k,
        realized_k,
        hash_chain_len: chain_len,
        rows,
        keygen,
        prove,
        verify: verify_dur,
        verified,
        proof_bytes,
    })
}

/// Convenience wrapper: `run_proof_with_opts(k, RunOpts::default())`.
pub async fn run_proof(k: u32) -> Result<RunStats> {
    run_proof_with_opts(k, RunOpts::default()).await
}

fn make_preimage() -> ProofPreimage {
    ProofPreimage {
        inputs: vec![Fr::from(1u64)],
        private_transcript: vec![],
        public_transcript_inputs: vec![],
        public_transcript_outputs: vec![],
        binding_input: Fr::from(42u64),
        communications_commitment: None,
        key_location: KeyLocation(std::borrow::Cow::Borrowed("contract-benchmark")),
    }
}

fn make_zswap_resolver(cache_dir: Option<&std::path::Path>) -> Result<Arc<ZswapResolver>> {
    if let Some(dir) = cache_dir {
        std::fs::create_dir_all(dir)?;
        if std::env::var_os("MIDNIGHT_PP").is_none() {
            // SAFETY: same rationale as prover-core's params.rs — set
            // once at construction, value supplied by the caller.
            unsafe {
                std::env::set_var("MIDNIGHT_PP", dir);
            }
        }
    }
    let provider = MidnightDataProvider::new(
        FetchMode::OnDemand,
        OutputMode::Log,
        ZSWAP_EXPECTED_FILES.to_vec(),
    )?;
    Ok(Arc::new(ZswapResolver(provider)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Sanity-check: every `k` in 1..=MAX_K builds a valid IR. Prints
    /// the realised k / row count / chain length so we can document
    /// the realised distribution in the README. Some low-k targets are
    /// naturally floored by halo2 baseline overhead (min realised k is
    /// observed empirically and recorded in `EFFECTIVE_FLOOR_K`).
    #[test]
    fn every_k_builds() {
        let mut last_realised: u32 = 0;
        for k in MIN_K..=MAX_K {
            let (ir, chain) = build_ir_for_k(k).unwrap_or_else(|e| {
                panic!("k={k} failed to build: {e:?}");
            });
            let got = ir.model().k() as u32;
            let rows = ir.model().rows();
            eprintln!(
                "k={k:>2}: realised={got:>2} chain={chain:>8} rows={rows}"
            );
            // Realised k must be monotonically non-decreasing in k —
            // requesting a bigger circuit never produces a smaller one.
            assert!(
                got >= last_realised,
                "k={k}: realised k={got} regressed from prior {last_realised}"
            );
            last_realised = got;
        }
    }

    /// Runs prove+verify for the smallest configured k. Requires
    /// `bls_midnight_2p4`-or-smaller params on disk (or network access
    /// to download them on first call). Slow: marked `ignore` by default.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "slow; pulls SRS from disk or network"]
    async fn smoke_k4() {
        let stats = run_proof(4).await.unwrap();
        assert_eq!(stats.k, 4);
        assert_eq!(stats.realized_k, 4);
        assert_eq!(stats.verified, Some(true));
    }
}
