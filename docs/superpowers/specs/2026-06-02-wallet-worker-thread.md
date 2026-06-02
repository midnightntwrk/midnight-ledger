# Wallet Worker Thread — Structural Fix for Stack-Overflow Class

**Date:** 2026-06-02
**Status:** spec
**Scope:** `mobile-bench/dioxus-wallet` only. Wallet-core stays untouched (the worker is a UI-side concern).

## Problem

Every heavy onclick in `app.rs` and `identity_centre.rs` follows the same
shape:

```rust
onclick: move |_| {
    busy.set(true);
    spawn(async move {
        let wallet = app_wallet_for(net);
        let result = run_some_heavy_op(&wallet, …).await;
        match result { … }   // mutate signals
        busy.set(false);
    });
}
```

On Android the `spawn(...)` call site sits on a **Chromium WebView dispatch
thread with ~256 KiB stack**. Rust materialises the `async move {…}`
state machine on that stack before handing it to the scheduler, and the
state machine carries the full `Wallet` + indexer client + node client
+ prover + the entire awaited future hierarchy (often several KiB).
That overruns the guard page and either crashes
(`Runtime::with_current_scope+32` SIGSEGV — Bootstrap, before
`f1dffe5e`) or silently stalls at the next deep await (OID4VCI, before
`a6e8c361`).

The current defensive sweep — wrapping every heavy `spawn(async move
{…})` in `spawn(Box::pin(async move {…}))` (5 sites; commits
`f1dffe5e`, `a6e8c361`, `86d97649`) — works because rustc + LLVM
optimise `Box::pin(async)` to a direct-heap construction in release
builds. It's **load-bearing on every future heavy onclick**:

- New code paths (VC self-verify, batch wizard, multi-DID refresh, sync triggers) all repeat the pattern.
- Easy to forget in PRs.
- Brittle to LLVM optimisation changes / debug builds / dependency churn.
- Adds verbosity at every call site.

## Goals

1. **One dedicated worker thread** owned by the wallet UI, with a generous (≥8 MiB) stack, runs *all* heavy chain ops.
2. **Click handlers become tiny** — they only build a `WorkMsg`, send it on a channel, and return. ≤10 lines.
3. **Results flow back through a Dioxus `use_coroutine`** so signal updates happen inside the Dioxus runtime (correct scope context).
4. **Structurally immune** to stack-overflow on any thread the click is delivered on — the worker thread's stack size is independent of WebView Chromium internals.
5. **Backpressure** — single worker means ops serialise; the wallet UI never has 5 in-flight chain ops fighting for sockets / signals.
6. **Cancellation token plumbing** — each `WorkMsg` carries an `action_id`; future "cancel" support can target a specific in-flight op.
7. **No regression** for the desktop build (where the small-stack issue doesn't exist) — same Rust code, just an extra hop.

## Non-Goals

- Removing the existing `Box::pin` sites in this commit. Migrate one at a time; the sweep stays as a safety net during the move.
- Cancellation UI (Cancel buttons during long ops). Plumb the token; UI uses it later.
- Sharing the worker with other features (background sync, push notifications). Future scope.
- Multi-tenant / parallel-op execution. Single-threaded by design — backpressure is a feature.

## Architecture

```
┌──────────────────────────────────────────────────────────────────┐
│  WebView dispatch thread (Chromium pool, ~256 KiB stack)         │
│                                                                  │
│  onclick=move |_| {                                              │
│      busy.set(true);                                             │
│      let id = NEXT_ACTION.fetch_add(1);                          │
│      outcomes.register(id, Box::new(move |out: WorkOutcome| {   │
│          match out {                                             │
│              WorkOutcome::BootstrapOk { did, .. } => {           │
│                  ic_did.set(Some(did));                          │
│                  busy.set(false);                                │
│              }                                                   │
│              WorkOutcome::Err { msg, .. } => {                   │
│                  err_msg.set(Some(msg));                         │
│                  busy.set(false);                                │
│              }                                                   │
│              _ => {}                                             │
│          }                                                       │
│      }));                                                        │
│      worker.tx.send(WorkMsg::Bootstrap { action_id: id, .. });  │
│  }                                                               │
└────────────────────┬─────────────────────────────────────────────┘
                     │ mpsc::UnboundedSender<WorkMsg>
                     ▼
┌──────────────────────────────────────────────────────────────────┐
│  wallet-worker thread                                            │
│  (std::thread::Builder::new().stack_size(8 << 20))               │
│  owns a current-thread tokio::runtime::Runtime                   │
│                                                                  │
│  worker_loop:                                                    │
│    while let Some(msg) = rx.recv().await {                       │
│        match msg {                                               │
│            WorkMsg::Bootstrap { action_id, seed } => {           │
│                let result = bootstrap_did_with_keys(…).await;    │
│                tx_back.send(WorkOutcome::from_bootstrap(         │
│                    action_id, result,                            │
│                ));                                               │
│            }                                                     │
│            WorkMsg::OID4VPAuthenticate {..} => { … }            │
│            WorkMsg::OID4VCIIssuance {..} => { … }               │
│            WorkMsg::UnlockWallet {..} => { … }                  │
│            // …                                                  │
│        }                                                         │
│    }                                                             │
└────────────────────┬─────────────────────────────────────────────┘
                     │ mpsc::UnboundedSender<WorkOutcome>
                     ▼
┌──────────────────────────────────────────────────────────────────┐
│  Dioxus runtime — use_coroutine in App root                      │
│                                                                  │
│  loop {                                                          │
│      let outcome = rx_back.recv().await.unwrap();                │
│      let action_id = outcome.action_id();                        │
│      if let Some(handler) = outcomes.take(action_id) {           │
│          handler(outcome);   // runs inside Dioxus scope         │
│      }                                                           │
│  }                                                               │
└──────────────────────────────────────────────────────────────────┘
```

Three primitives, no magic.

## Files

### Created

- `mobile-bench/dioxus-wallet/src/worker/mod.rs` — `AppWorker` struct (channels, registry), `spawn()` constructor, `WorkMsg` + `WorkOutcome` enums, `OutcomeRouter` helper.
- `mobile-bench/dioxus-wallet/src/worker/handlers.rs` — `handle_bootstrap`, `handle_oid4vp_authenticate`, `handle_oid4vci_issuance`, `handle_unlock`, `handle_refresh_did`, `handle_create_did`, `handle_deactivate_did`, `handle_vc_self_verify`. Each is an `async fn` taking `&BridgeState` + the msg's payload, returning `WorkOutcome`.
- `mobile-bench/dioxus-wallet/src/worker/router.rs` — process-global `OUTCOME_ROUTER` keyed by `action_id` → `Box<dyn FnOnce(WorkOutcome) + Send>`. Inserts on `register`, removes on `take`.

### Modified

- `mobile-bench/dioxus-wallet/src/lib.rs` — `mod worker;` plus a single `use_context_provider` at the top of `run()` so every component can fetch the worker handle from context.
- `mobile-bench/dioxus-wallet/src/app.rs` — replace each `spawn(Box::pin(async move {…}))` with `worker.send(...)` + `outcomes.register(...)`. One spawn site per migration commit.
- `mobile-bench/dioxus-wallet/src/identity_centre.rs` — same: replace `run_oid4vp_authenticate` + `run_oid4vci_request` + `BootstrapSection::bootstrap`.

## Key types (sketch)

```rust
// worker/mod.rs

/// Process-local monotonic action token. u64 → no realistic
/// wrap-around in a session.
pub fn next_action_id() -> u64 {
    static N: AtomicU64 = AtomicU64::new(1);
    N.fetch_add(1, Ordering::Relaxed)
}

/// One variant per heavy op. `action_id` is on every variant so
/// the outcome router can route back without inspecting the
/// payload.
pub enum WorkMsg {
    Bootstrap   { action_id: u64, seed: [u8; 32], network: Network },
    Oid4vp      { action_id: u64, did: DidId, url: String, network: Network },
    Oid4vci     { action_id: u64, did: DidId, url: String, network: Network },
    Unlock      { action_id: u64, passphrase: String, network: Network, seed_hex: Option<String> },
    RefreshDid  { action_id: u64, did: String, network: Network },
    CreateDid   { action_id: u64, network: Network, counter_cursor: u32, /* … */ },
    DeactivateDid { action_id: u64, did: String, network: Network },
    VcVerify    { action_id: u64, vc_uri: String, network: Network },
}

pub enum WorkOutcome {
    BootstrapOk { action_id: u64, did: String, controller_sk: Vec<u8> },
    Oid4vpOk    { action_id: u64, session_id: String, status: String },
    Oid4vciOk   { action_id: u64, vc_uri: String },
    UnlockOk    { action_id: u64, wallet_id: WalletId },
    RefreshDidOk { action_id: u64, entry: DidInventoryEntry },
    CreateDidOk { action_id: u64, did_id: String },
    DeactivateDidOk { action_id: u64, did_id: String },
    VcVerifyOk  { action_id: u64, valid: bool },
    Err         { action_id: u64, msg: String },
}

impl WorkOutcome {
    pub fn action_id(&self) -> u64 { /* match all arms */ }
}

pub struct AppWorker {
    tx: mpsc::UnboundedSender<WorkMsg>,
    rx_back: Arc<Mutex<mpsc::UnboundedReceiver<WorkOutcome>>>,
}

impl AppWorker {
    pub fn spawn(bridge_state: BridgeState) -> Self {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let (tx_back, rx_back) = mpsc::unbounded_channel();
        std::thread::Builder::new()
            .name("wallet-worker".into())
            .stack_size(8 << 20)
            .spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("worker tokio rt");
                rt.block_on(async move {
                    while let Some(msg) = rx.recv().await {
                        let outcome = handlers::dispatch(&bridge_state, msg).await;
                        let _ = tx_back.send(outcome);
                    }
                });
            })
            .expect("spawn wallet-worker thread");
        Self { tx, rx_back: Arc::new(Mutex::new(rx_back)) }
    }

    pub fn send(&self, msg: WorkMsg) {
        // Unbounded channel never errors except after worker
        // panic; we log and continue so the wallet UI stays
        // responsive even in that degraded state.
        if let Err(e) = self.tx.send(msg) {
            tracing::error!(target: "wallet_worker", error = %e,
                "WorkMsg dropped — worker thread is gone");
        }
    }
}

// router.rs

pub type OutcomeHandler = Box<dyn FnOnce(WorkOutcome) + Send>;

pub struct OutcomeRouter {
    pending: Mutex<HashMap<u64, OutcomeHandler>>,
}

impl OutcomeRouter {
    pub fn register(&self, action_id: u64, handler: OutcomeHandler) { /* … */ }
    pub fn take(&self, action_id: u64) -> Option<OutcomeHandler> { /* … */ }
}

// in lib.rs::run(), after BridgeState construction:
let worker = AppWorker::spawn(bridge_state.clone());
let outcomes = Arc::new(OutcomeRouter::default());
// Pass both into the root component as use_context_provider.
```

### Dioxus side coroutine

```rust
// In App or a hook:
let worker = use_context::<AppWorker>();
let outcomes = use_context::<Arc<OutcomeRouter>>();

use_future(move || {
    let worker = worker.clone();
    let outcomes = outcomes.clone();
    async move {
        loop {
            let outcome = {
                let mut rx = worker.rx_back.lock().await;
                match rx.recv().await {
                    Some(o) => o,
                    None => return,
                }
            };
            let action_id = outcome.action_id();
            if let Some(handler) = outcomes.take(action_id) {
                handler(outcome);   // runs in Dioxus scope
            }
        }
    }
});
```

## Migration order (within Phase 3)

Each task migrates one onclick + its outcome handling. Each ends in a
signed commit. After each migration, on-device smoke-test the affected
flow.

1. **`worker` module skeleton + `AppWorker::spawn` + root `use_future` outcome-pump.** No migrations yet; just plumbing + an empty `WorkMsg::Noop`/`WorkOutcome::NoopAck` round-trip to prove the channels work.
2. **Migrate Bootstrap** (`identity_centre.rs::BootstrapSection`). First real client. After this, the pattern is proven; subsequent migrations are mechanical.
3. **Migrate OID4VP authenticate** (`identity_centre.rs::run_oid4vp_authenticate`).
4. **Migrate OID4VCI issuance** (`identity_centre.rs::run_oid4vci_request`).
5. **Migrate Unlock** (`app.rs:1278`).
6. **Migrate DID refresh** (`app.rs:1428` — both unlock auto-refresh and per-row refresh).
7. **Migrate Create DID** (`app.rs:3924` — wizard stream).
8. **Migrate Deactivate DID** (`app.rs:6907`).
9. **Migrate medium-risk sites** (VC self-verify, batch wizard, sync triggers) — same pattern, in one commit if they're small enough.
10. **Cleanup pass** — remove the now-unused `spawn(Box::pin(...))` defensive pattern; drop any signals that no longer need to cross threads; tighten comments referencing the old design.

Tasks 1-2 establish the architecture; tasks 3-9 are mechanical
replications. Task 10 retires the per-site Box::pin pattern.

## Key risks & mitigations

| Risk | Mitigation |
|---|---|
| Worker thread panics mid-op → all subsequent messages silently dropped | `std::panic::catch_unwind` inside the loop body; on panic, log + send `WorkOutcome::Err { action_id, msg: "worker panic, retry" }` so the UI surfaces it. |
| Bridge state references inside handlers must be `Send + Sync` | `BridgeState` is `Send + Sync` via its current shape (Arc-wrapped internals). Verify during Phase 3 Task 1. |
| `WorkMsg::CreateDid` carries a `WizardStage` stream — channels don't transport streams | Send only the *start* command; the worker emits multiple `WorkOutcome::CreateDidProgress { ... }` events for each stage. The router holds the handler across multiple deliveries until the final `CreateDidOk`/`Err`. |
| Wizard streams + multi-event delivery break the "FnOnce handler" model | Switch the router to `Box<dyn Fn(WorkOutcome) + Send + Sync>` (multi-shot) for wizard-style ops; terminal outcomes still call `.take()` to drop the handler. |
| Stale signal references if the handler outlives the component scope | Dioxus signals are scope-bound; writing to a dropped signal is a no-op. No crash, just lost UI update. Acceptable. |
| The desktop build now has an extra thread for no benefit | Negligible — std::thread is cheap, 8 MiB stack is mmap'd lazily, the worker is idle 99.99% of the time. |
| Order of execution surprises the user (batch ops queue rather than parallel) | Document; this is intended behaviour. If parallel ops become a real need later, switch the worker to multi-thread tokio (still avoids the WebView stack). |

## Performance

- Channel latency: ~1µs per WorkMsg on x86_64; bounded by mutex contention in tokio mpsc. Indistinguishable from the inline path.
- Stack-size cost: 8 MiB mmap reservation, lazily backed. Real RSS impact ≈ 64 KiB (one OS page touched during boot).
- Thread overhead: one extra OS thread per app. Negligible.

## Test plan

Per migration task, on the physical phone (Galaxy S24 Ultra, R5CX82NAS0P):

- [ ] The migrated flow still works end-to-end with no regressions.
- [ ] An immediate second tap is queued by the worker (busy state visible, no double-submit).
- [ ] Switching networks mid-op doesn't crash (worker keeps running, may produce stale-network outcome which the handler ignores).
- [ ] Logcat shows `wallet-worker` thread is alive (no panic, no early exit) after a full demo run.
- [ ] No `Cause: stack pointer is not in a rw map` SIGSEGV from any onclick.

After cleanup pass (Task 10):

- [ ] No `Box::pin(async move` lines remain in `app.rs` or `identity_centre.rs`.
- [ ] Full demo (Bootstrap → OID4VP → OID4VCI → Deactivate) still passes.
- [ ] Desktop build unchanged in behaviour.

## Rollback

Every migration task is independently revertable — the old
`spawn(Box::pin(...))` site stays alongside the new `worker.send(...)`
site temporarily during refactor. If something breaks, revert that
task's commit and the per-site Box::pin path is back.

The full worker module can be removed by reverting the foundational
commits (Task 1) without touching any wallet logic — wallet-core has
zero awareness of the worker.

## Follow-ups (not in this spec)

- Cancel button UX (cancellation token threaded through `WorkMsg`).
- Multi-shot wizard streams: refactor the router to support progress events without unwiring on every event.
- Worker-thread health metrics surfaced under Diagnostics → Logs.
- Apply the same pattern on iOS (the dispatch thread there is WKWebView, also small).
