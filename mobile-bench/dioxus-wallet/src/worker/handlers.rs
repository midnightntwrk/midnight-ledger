//! Per-message dispatch. Phase 2 / Task 2 handles
//! [`super::WorkMsg::Noop`] and [`super::WorkMsg::Bootstrap`];
//! later tasks add OID4VP, OID4VCI, Unlock, RefreshDid, CreateDid,
//! DeactivateDid, VcSelfVerify.
//!
//! Each handler should:
//! - Take whatever it needs from the `WorkMsg` arm + the shared
//!   `BridgeState` clone the worker holds.
//! - Run the actual `wallet-core` async op on the worker's
//!   tokio runtime (no special precautions; we have 8 MiB of
//!   stack and a proper Tokio runtime here).
//! - Return a [`super::WorkOutcome`] that mirrors the request's
//!   arm, with the matching `action_id` echoed back so the
//!   router can find the right handler.

use std::sync::Arc;

use wallet_core::clock::SystemClock;
use wallet_core::oid4vp_client::{
    IdTokenBuilder, LoginCoordinator, run_authentication,
};
use wallet_core::secret_storage::redb_secret_store::RedbSecretStore;
use wallet_core::store::WalletStore;
use wallet_core::{
    DidId, HttpClient, MeteredHttpClient, RedbVcStore, ReqwestHttpClient,
    Wallet, bootstrap_did_with_keys, oid4vci_run_issuance, time_op,
};

use crate::app::wallet_store_path;

use super::{BridgeState, Network, WorkMsg, WorkOutcome};
use crate::app::metered_app_wallet_for;
use crate::did_ports::{CachedWalletAuthnDiscovery, RedbDidSigner};
use crate::identity_centre::vc_store_path;

/// Dispatch a [`WorkMsg`] to its handler and return the
/// [`WorkOutcome`]. Pure routing — no shared mutable state.
pub(super) async fn dispatch(state: &BridgeState, msg: WorkMsg) -> WorkOutcome {
    match msg {
        WorkMsg::Noop { action_id } => {
            tracing::info!(
                target: "wallet_worker",
                action_id,
                "WorkMsg::Noop received → NoopAck",
            );
            WorkOutcome::NoopAck { action_id }
        }
        WorkMsg::Bootstrap {
            action_id,
            network,
            seed,
        } => handle_bootstrap(state, action_id, network, &seed).await,
        WorkMsg::Oid4vpAuthenticate {
            action_id,
            network,
            did,
            qr_url,
        } => handle_oid4vp_authenticate(state, action_id, network, did, qr_url).await,
        WorkMsg::Oid4vciIssuance {
            action_id,
            network,
            did,
            qr_url,
        } => handle_oid4vci_issuance(state, action_id, network, did, qr_url).await,
        WorkMsg::OpenStore {
            action_id,
            passphrase,
        } => handle_open_store(action_id, &passphrase).await,
    }
}

/// Run `bootstrap_did_with_keys` and persist the controller secret
/// into the shared [`BridgeState::controller_secrets`] map so the
/// Sign / Update / Deactivate buttons can derive keys without the
/// caller having to re-thread the secret. Mirrors the logic that
/// used to live inline in
/// `identity_centre::BootstrapSection::bootstrap` (commit
/// `f1dffe5e`); the UI side now only registers an outcome handler
/// that updates `ic_did` + the metrics counter + the
/// `on_did_minted` event handler.
async fn handle_bootstrap(
    state: &BridgeState,
    action_id: u64,
    network: Network,
    seed: &[u8; 32],
) -> WorkOutcome {
    let Some(store) = state.store().cloned() else {
        return WorkOutcome::Err {
            action_id,
            msg: "wallet store not opened yet".into(),
        };
    };
    let Some(wallet_id) = state.active_wallet_id() else {
        return WorkOutcome::Err {
            action_id,
            msg: "no active wallet".into(),
        };
    };
    let metrics = state.metrics_dyn();
    let probe = state.resource_probe();

    let wallet = metered_app_wallet_for(network, metrics.clone(), probe.clone());
    let mut secret_store = RedbSecretStore::new(store, wallet_id);

    let result = time_op(
        &*metrics,
        &*probe,
        "bootstrap_did",
        bootstrap_did_with_keys(&wallet, &mut secret_store, seed),
    )
    .await;

    match result {
        Ok(b) => {
            let did_str = b.did.to_did_string();
            // Persist the per-DID controller secret here on the
            // worker so the UI side doesn't need to thread it
            // through OutcomeHandler manually. `BridgeState`'s
            // `controller_secrets` map is Arc-wrapped so this
            // write is visible to the UI immediately.
            state.remember_controller_secret(
                network,
                did_str.clone(),
                b.controller_sk,
            );
            // Persist the inventory row here too — same robustness
            // argument as the controller secret. If the WebView
            // gets killed between worker emit + UI re-render
            // (which DOES happen post-bootstrap on Android due to
            // the prover's RSS spike: ~95 MiB delta is enough to
            // trigger an OS-side restart), an
            // `on_did_minted`-only persist gets lost on the
            // floor — the controller secret survives (worker
            // wrote it synchronously above) but the Dids tab
            // comes back empty even though the chain has the
            // contract and the secret store has the
            // `#key-auth` key. Writing both rows here makes the
            // post-bootstrap state durable against arbitrary UI
            // teardown.
            //
            // `vm_count = 2` mirrors the value the UI's
            // `on_did_minted` handler also seeds with — the
            // `bootstrap_did_with_keys` flow always attaches
            // exactly the Ed25519 (auth) + Jubjub (assertion)
            // pair before settling. The UI's
            // `on_did_minted` handler still runs and re-inserts
            // into `did_inventory` (the signal) so the new row
            // appears immediately without waiting for a network
            // switch / re-hydrate; the worker's write here is
            // the durable backstop.
            let inventory_row = wallet_core::store::DidInventoryEntry {
                did: did_str.clone(),
                network,
                status: wallet_core::store::InventoryStatus::Active,
                counter: None,
                vm_count: Some(2),
                service_count: Some(0),
                last_block_height: None,
                created_at: 0,
                updated_at: 0,
            };
            // `store` was moved into `RedbSecretStore::new` above;
            // pull a fresh handle off the bridge state for this
            // write. `state.store()` returns `Option<&WalletStore>`
            // and the inner store wraps an `Arc<Database>`, so
            // re-acquiring it here is a cheap pointer copy.
            if let Some(store_for_inv) = state.store() {
                if let Err(e) = store_for_inv.put_did_inventory(inventory_row) {
                    tracing::warn!(
                        target: "wallet_worker",
                        action_id,
                        did = %did_str,
                        error = %e,
                        "persist DID inventory row failed on worker",
                    );
                }
            }
            WorkOutcome::BootstrapOk {
                action_id,
                did_str,
                controller_sk: b.controller_sk,
            }
        }
        Err(e) => WorkOutcome::Err {
            action_id,
            msg: format!("bootstrap failed: {e}"),
        },
    }
}

/// Drive the OID4VP / SIOPv2 authentication flow end-to-end:
/// build the chain-op-metered Wallet + persistent secret store
/// + DID-port adapters, hand them to a Mode-A
/// [`LoginCoordinator`], call
/// [`wallet_core::oid4vp_client::run_authentication`].
///
/// The single-resolve / single-sign architectural payoff (see
/// Login-with-DID spec) carries through — `CachedWalletAuthnDiscovery`
/// has a 30s TTL cache so back-to-back logins for the same DID
/// don't re-hit the indexer.
///
/// Mirrors the pre-Worker `identity_centre::run_oid4vp_authenticate`
/// (commit 758a5fa3) body; the click site now only registers an
/// outcome handler + dispatches the [`WorkMsg::Oid4vpAuthenticate`].
async fn handle_oid4vp_authenticate(
    state: &BridgeState,
    action_id: u64,
    network: Network,
    did: DidId,
    qr_url: String,
) -> WorkOutcome {
    let Some(store) = state.store().cloned() else {
        return WorkOutcome::Err {
            action_id,
            msg: "wallet store not opened yet".into(),
        };
    };
    let Some(wallet_id) = state.active_wallet_id() else {
        return WorkOutcome::Err {
            action_id,
            msg: "no active wallet".into(),
        };
    };
    let metrics = state.metrics_dyn();
    let probe = state.resource_probe();

    let wallet = metered_app_wallet_for(network, metrics.clone(), probe.clone());
    let secret_store = RedbSecretStore::new(store, wallet_id);
    let raw_http: Arc<dyn HttpClient> = Arc::new(ReqwestHttpClient::default());
    let http: Arc<dyn HttpClient> =
        Arc::new(MeteredHttpClient::new(raw_http, metrics.clone()));

    let discovery =
        Arc::new(CachedWalletAuthnDiscovery::new(Arc::new(wallet)))
            as Arc<dyn wallet_core::oid4vp_client::DidAuthnDiscovery>;
    let signer = Arc::new(RedbDidSigner::new(secret_store))
        as Arc<dyn wallet_core::oid4vp_client::DidSigner>;
    let clock: Arc<dyn wallet_core::clock::Clock> = Arc::new(SystemClock);
    let coordinator = LoginCoordinator::mode_a(IdTokenBuilder::new(
        discovery, signer, clock, did,
    ));

    let result = time_op(
        &*metrics,
        &*probe,
        "oid4vp_authenticate",
        run_authentication(&*http, &coordinator, &qr_url),
    )
    .await;

    match result {
        Ok(r) => WorkOutcome::Oid4vpOk {
            action_id,
            session_id: r.session_id,
            status: r.status,
        },
        Err(e) => WorkOutcome::Err {
            action_id,
            msg: format!("authenticate failed: {e}"),
        },
    }
}

/// Drive the OID4VCI Pre-Authorized Code Flow on the worker
/// thread: token exchange → DID-bound proof-of-possession JWS
/// → credential POST → persist the issued VC to the wallet's
/// redb vc_store. Returns the freshly-issued `vc_uri`.
///
/// Mirrors `handle_oid4vp_authenticate`: builds the same
/// `CachedWalletAuthnDiscovery` + `RedbDidSigner` port pair and
/// hands them to `oid4vci_run_issuance`. The 30 s discovery
/// cache means back-to-back OID4VP login + OID4VCI issuance
/// against the same DID re-uses one indexer roundtrip total
/// (not two), and the JWS proof-of-possession now costs
/// 1 × resolve + 1 × sign (down from the legacy 2 × resolve +
/// 2 × sign probe pattern).
async fn handle_oid4vci_issuance(
    state: &BridgeState,
    action_id: u64,
    network: Network,
    did: DidId,
    qr_url: String,
) -> WorkOutcome {
    let Some(store) = state.store().cloned() else {
        return WorkOutcome::Err {
            action_id,
            msg: "wallet store not opened yet".into(),
        };
    };
    let Some(wallet_id) = state.active_wallet_id() else {
        return WorkOutcome::Err {
            action_id,
            msg: "no active wallet".into(),
        };
    };
    let metrics = state.metrics_dyn();
    let probe = state.resource_probe();

    let wallet = metered_app_wallet_for(network, metrics.clone(), probe.clone());
    let secret_store = RedbSecretStore::new(store, wallet_id);
    let vc_store = match RedbVcStore::open(vc_store_path()) {
        Ok(v) => v,
        Err(e) => {
            return WorkOutcome::Err {
                action_id,
                msg: format!("open vc store: {e}"),
            };
        }
    };
    let raw_http: Arc<dyn HttpClient> = Arc::new(ReqwestHttpClient::default());
    let http: Arc<dyn HttpClient> =
        Arc::new(MeteredHttpClient::new(raw_http, metrics.clone()));

    // Arc-share Wallet so both discovery and run_issuance can
    // hold a reference without fighting over ownership.
    let wallet_arc: Arc<Wallet> = Arc::new(wallet);
    let discovery =
        Arc::new(CachedWalletAuthnDiscovery::new(wallet_arc.clone()))
            as Arc<dyn wallet_core::oid4vp_client::DidAuthnDiscovery>;
    let signer = Arc::new(RedbDidSigner::new(secret_store.clone()))
        as Arc<dyn wallet_core::oid4vp_client::DidSigner>;
    let clock: Arc<dyn wallet_core::clock::Clock> = Arc::new(SystemClock);

    // Build the coordinator with the canonical Phase-1 JWT
    // proof. Other proof types (`ldp_vp`, `mso_mdoc`, EBSI)
    // would substitute a different `ProofBuilder` here without
    // touching `run_issuance` or `request_credential`.
    let did_for_coordinator = did.clone();
    let coordinator = wallet_core::oid4vci_client::CredentialCoordinator::jwt(
        wallet_core::oid4vci_client::IdTokenProofBuilder::new(
            discovery,
            signer,
            clock.clone(),
            did_for_coordinator,
        ),
    );

    let result = time_op(
        &*metrics,
        &*probe,
        "issuance",
        oid4vci_run_issuance(
            &*http,
            &*clock,
            wallet_arc.js_bridge(),
            &qr_url,
            &coordinator,
            &wallet_arc,
            &secret_store,
            &did,
            &vc_store,
        ),
    )
    .await;

    match result {
        Ok(vc_uri) => WorkOutcome::Oid4vciOk { action_id, vc_uri },
        Err(e) => WorkOutcome::Err {
            action_id,
            msg: format!("issue failed: {e}"),
        },
    }
}

/// Open the wallet store on the worker thread.
///
/// `WalletStore::open` runs all pending migrations and decodes the
/// per-network ledger snapshot — on the preprod-live demo store
/// (~534k DUST events cached) this is the deepest async state
/// machine in the app. The Bootstrap fix in commit `f1dffe5e`
/// originally box-pinned this on the WebView dispatch thread to
/// avoid SIGSEGV; the worker thread (8 MiB stack, current-thread
/// Tokio runtime) is the right home for it.
///
/// The handler returns the `WalletStore` itself rather than
/// piggybacking the full unlock pipeline. Hydration of inventory
/// signals + `did_inventory` / `resolved_cache` / `unlock_state`
/// mutations stay on the UI thread (those Dioxus `Signal`s are
/// `!Send`). The UI's outcome handler runs the rest of the
/// pipeline once the store handle arrives — see
/// `app::on_unlock`.
async fn handle_open_store(action_id: u64, passphrase: &str) -> WorkOutcome {
    let path = wallet_store_path();
    match WalletStore::open(&path, passphrase) {
        Ok(store) => {
            tracing::info!(
                target: "wallet_worker",
                action_id,
                path = %path.display(),
                "WalletStore opened on worker thread",
            );
            WorkOutcome::OpenStoreOk { action_id, store }
        }
        Err(e) => WorkOutcome::Err {
            action_id,
            msg: format!("open store: {e}"),
        },
    }
}
