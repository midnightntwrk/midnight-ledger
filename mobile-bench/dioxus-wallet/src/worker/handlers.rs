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

use wallet_core::secret_storage::redb_secret_store::RedbSecretStore;
use wallet_core::{bootstrap_did_with_keys, time_op};

use super::{BridgeState, Network, WorkMsg, WorkOutcome};
use crate::app::metered_app_wallet_for;

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
