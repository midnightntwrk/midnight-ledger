//! wasm-bindgen wrapper around `contract_benchmark::run_proof_with_params`.
//!
//! Exposes a single `runProof(k, paramsProvider)` async function to
//! JS. The `paramsProvider` is any JS object with a `getParams(k)`
//! method that returns a `Promise<Uint8Array>` of the matching BLS
//! SRS bytes (`bls_midnight_2pN`). The serving HTML in
//! `web/index.html` implements one that fetches from
//! `https://srs.midnight.network/`.
//!
//! Modelled after `zkir-wasm`'s `JsKeyProvider`. We only need
//! `ParamsProverProvider` here — the contract-benchmark constructs
//! its own `Resolver` internally (the synthetic IR is small enough
//! that the prover-key cache is built inline during keygen).

use contract_benchmark::{MAX_K, MIN_K, RunOpts, RunStats, run_proof_with_params};
use js_sys::{Function, JsString, Promise, Uint8Array};
use transient_crypto::proofs::{ParamsProver, ParamsProverProvider};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

// `initThreadPool(numThreads)` — JS-callable. Must be awaited
// once at page boot before any `runProof` call. Without this,
// rayon falls back to its single-threaded "no-pool" mode and we
// don't get any parallelism even when the page is served with
// the COOP/COEP headers `SharedArrayBuffer` needs.
pub use wasm_bindgen_rayon::init_thread_pool;

/// Bridge between JS-side `getParams(k)` callbacks and the
/// `ParamsProverProvider` trait the prover expects. Receives a JS
/// object whose `getParams` method returns
/// `Promise<Uint8Array>` of SRS bytes for the requested `k`.
struct JsParamsProvider(JsValue);

fn err(msg: impl Into<String>) -> std::io::Error {
    std::io::Error::other(msg.into())
}

fn try_to_string(jsv: JsValue) -> String {
    let res = js_sys::Reflect::get(&jsv, &"toString".into())
        .and_then(|f| f.dyn_into::<Function>())
        .and_then(|f| f.call0(&jsv))
        .and_then(|s| s.dyn_into::<JsString>());
    match res {
        Ok(s) => s.into(),
        Err(_) => "<failed to stringify>".into(),
    }
}

impl ParamsProverProvider for JsParamsProvider {
    async fn get_params(&self, k: u8) -> std::io::Result<ParamsProver> {
        let get_params = js_sys::Reflect::get(&self.0, &"getParams".into())
            .map_err(|_| err("could not get property 'getParams' on provider"))?
            .dyn_into::<Function>()
            .map_err(|_| err("property 'getParams' on provider is not a function"))?;
        let promise = get_params
            .call1(&self.0, &JsValue::from(k))
            .map_err(|e| err(format!("error calling getParams: {}", try_to_string(e))))?
            .dyn_into::<Promise>()
            .map_err(|_| err("result of getParams was not a Promise"))?;
        let res = JsFuture::from(promise)
            .await
            .map_err(|e| {
                err(format!(
                    "getParams promise rejected: {}",
                    try_to_string(e)
                ))
            })?
            .dyn_into::<Uint8Array>()
            .map_err(|_| err("result of getParams was not a Uint8Array"))?
            .to_vec();
        ParamsProver::read(&res[..])
    }
}

/// One-time setup — install the panic hook so any prover panic
/// surfaces in the browser devtools console instead of an opaque
/// `unreachable` trap, and forward `tracing` events to the same
/// console. Idempotent (safe to call from every JS entry point).
#[wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook::set_once();
    tracing_wasm::set_as_global_default();
}

/// Run the benchmark for a single `k`. Resolves to a JSON-stringified
/// `RunStats` payload (verify is always skipped on wasm — the
/// embedded verifier-params blob isn't bundled and the timings the
/// user cares about are keygen + prove).
///
/// Throws on:
/// - `k` outside `[MIN_K, MAX_K]`,
/// - the provider's `getParams(k)` promise rejecting,
/// - the SRS bytes failing to deserialize,
/// - the prover itself returning an error.
#[wasm_bindgen(js_name = "runProof")]
pub async fn run_proof(k: u32, provider: JsValue) -> Result<JsValue, JsError> {
    let provider = JsParamsProvider(provider);
    // Verify-after stays off — we'd need to bundle PARAMS_VERIFIER
    // bytes (or a separate verifier hook) to do it on wasm, and the
    // benchmark's primary signal is the prove time anyway.
    let opts = RunOpts {
        verify_after: false,
        ..RunOpts::default()
    };
    let stats: RunStats = run_proof_with_params(k, &opts, &provider)
        .await
        .map_err(|e| JsError::new(&e.to_string()))?;
    // Serialize via serde_json then hand back as a JsValue so the
    // browser-side code gets a plain object with `prove_ms`,
    // `keygen_ms`, etc. fields.
    let json = serde_json::to_string(&stats)
        .map_err(|e| JsError::new(&format!("serialize RunStats: {e}")))?;
    let parsed = js_sys::JSON::parse(&json)
        .map_err(|e| JsError::new(&format!("re-parse RunStats JSON: {}", try_to_string(e))))?;
    Ok(parsed)
}

/// Expose the legal `k` range so the HTML page can render the
/// right number of bench rows without hard-coding.
#[wasm_bindgen(js_name = "minK")]
pub fn min_k() -> u32 {
    MIN_K
}

#[wasm_bindgen(js_name = "maxK")]
pub fn max_k() -> u32 {
    MAX_K
}
