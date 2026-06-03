//! Session-persistence write-throughs + path resolvers.
//!
//! Extracted from `app.rs` (which is still 9k lines and slowly
//! shrinking) to give the wallet's "store boundary" a focused
//! home — every function in this module either reads from or
//! writes to the on-disk redb store, and most just thunk the
//! UI's in-memory shape across a single `WalletStore::put_*`
//! / `WalletStore::list_*` call.
//!
//! All write paths are best-effort: a store error is logged
//! (`tracing::warn!`) but does NOT bubble up — the persist
//! layer is a UX convenience, never a hard dependency for a
//! correct in-memory operation. An unhealthy disk degrades the
//! session into "works for this run, won't survive a reload"
//! rather than crashing the click.
//!
//! See `docs/superpowers/specs/2026-06-03-hex-architecture-audit.md`
//! §5.F — extracting cohesive non-component chunks from `app.rs`
//! is the lower-risk half of the file-split work.

use crate::app::{DidInventoryEntry, DidInventoryStatus, Tab};
use crate::bridge::BridgeState;
use wallet_core::Network;

// ─── Inventory rows ──────────────────────────────────────────────

/// Translate the persisted inventory status enum into the
/// UI-side variant.
pub(crate) fn status_from_store(s: wallet_core::store::InventoryStatus) -> DidInventoryStatus {
    match s {
        wallet_core::store::InventoryStatus::Pending => DidInventoryStatus::Pending,
        wallet_core::store::InventoryStatus::Active => DidInventoryStatus::Active,
        wallet_core::store::InventoryStatus::Deactivated => DidInventoryStatus::Deactivated,
    }
}

/// Inverse of [`status_from_store`].
pub(crate) fn status_to_store(s: DidInventoryStatus) -> wallet_core::store::InventoryStatus {
    match s {
        DidInventoryStatus::Pending => wallet_core::store::InventoryStatus::Pending,
        DidInventoryStatus::Active => wallet_core::store::InventoryStatus::Active,
        DidInventoryStatus::Deactivated => wallet_core::store::InventoryStatus::Deactivated,
    }
}

/// Write-through helper — pushes the latest UI-side inventory
/// state into the persistent store. Best-effort; a store error
/// is logged but doesn't fail the in-memory update, so an
/// unhealthy disk doesn't break the current session.
pub(crate) fn persist_inventory_entry(
    bridge_state: &BridgeState,
    network: Network,
    entry: &DidInventoryEntry,
) {
    let Some(store) = bridge_state.store() else {
        return;
    };
    let row = wallet_core::store::DidInventoryEntry {
        did: entry.did.clone(),
        network,
        status: status_to_store(entry.status),
        counter: entry.counter,
        vm_count: entry.vm_count.map(|v| v as u32),
        service_count: entry.service_count.map(|v| v as u32),
        last_block_height: entry.last_block_height,
        created_at: 0,
        updated_at: 0,
    };
    if let Err(e) = store.put_did_inventory(row) {
        tracing::warn!(error=%e, did=%entry.did, "persist did inventory failed");
    }
}

/// Bulk-load DID inventory rows for `net` into the UI's
/// in-memory map shape. Empty map on store error or empty
/// table; the caller can `.is_empty()` to decide whether to
/// even touch the signal.
pub(crate) fn load_inventory_for_network(
    store: &wallet_core::store::WalletStore,
    net: Network,
) -> std::collections::BTreeMap<String, DidInventoryEntry> {
    let mut map = std::collections::BTreeMap::new();
    let Ok(rows) = store.list_did_inventory(net) else {
        return map;
    };
    for row in rows {
        map.insert(
            row.did.clone(),
            DidInventoryEntry {
                did: row.did,
                network_label: net.label().to_string(),
                status: status_from_store(row.status),
                counter: row.counter,
                vm_count: row.vm_count.map(|v| v as usize),
                service_count: row.service_count.map(|v| v as usize),
                last_block_height: row.last_block_height,
            },
        );
    }
    map
}

// ─── Wallet rows ────────────────────────────────────────────────

/// Find the wallet row matching `network` in the store, or
/// auto-create one. Returns `Some(WalletId)` on success or
/// `None` if no row exists and we couldn't mint one (e.g.
/// `seed_hex_opt` is `None` or doesn't decode).
pub(crate) fn find_or_create_wallet_for_network(
    store: &wallet_core::store::WalletStore,
    net: Network,
    seed_hex_opt: Option<&str>,
) -> Option<wallet_core::store::WalletId> {
    use wallet_core::store::NetworkTag;
    let target = NetworkTag::from(net);

    // Look first — `wallet_meta` is a cheap read txn.
    if let Ok(ids) = store.list_wallet_ids() {
        for id in &ids {
            if let Ok(Some(meta)) = store.wallet_meta(*id) {
                if meta.network == target {
                    return Some(*id);
                }
            }
        }
    }

    // Nothing yet — mint a row tagged with this network's demo
    // seed. The label encodes the network so the wallet picker
    // can render rows like "Demo · PreProd".
    let hex = seed_hex_opt?;
    let bytes = hex::decode(hex).ok()?;
    let seed: [u8; 32] = bytes.as_slice().try_into().ok()?;
    let label = format!("Demo · {}", net.label());
    match store.create_wallet(&label, net, &seed) {
        Ok(id) => {
            tracing::info!(wallet_id=%id, network=?net, "auto-created wallet row");
            Some(id)
        }
        Err(e) => {
            tracing::warn!(error=%e, network=?net, "auto-create wallet row failed");
            None
        }
    }
}

// ─── Resolved-cache snapshots ───────────────────────────────────

/// Bulk-load resolved-cache snapshots for `net`, decoded from
/// the on-disk JSON. Entries that fail to decode are dropped
/// silently — the next manual / auto resolve will refresh them.
pub(crate) fn load_resolved_cache_for_network(
    store: &wallet_core::store::WalletStore,
    net: Network,
) -> std::collections::HashMap<String, wallet_core::ResolvedDid> {
    let mut map = std::collections::HashMap::new();
    let Ok(rows) = store.list_resolved_cache(net) else {
        return map;
    };
    for (did, json, _at) in rows {
        if let Ok(r) = serde_json::from_str::<wallet_core::ResolvedDid>(&json) {
            map.insert(did, r);
        }
    }
    map
}

/// Write-through helper — caches the resolved JSON snapshot
/// under `(network, did)` so the detail tabs survive a reload.
pub(crate) fn persist_resolved_cache(
    bridge_state: &BridgeState,
    network: Network,
    did: &str,
    resolved: &wallet_core::ResolvedDid,
) {
    let Some(store) = bridge_state.store() else {
        return;
    };
    let Ok(json) = serde_json::to_string(resolved) else {
        return;
    };
    if let Err(e) = store.put_resolved_cache(network, did, json) {
        tracing::warn!(error=%e, did=%did, "persist resolved cache failed");
    }
}

// ─── Session snapshot ───────────────────────────────────────────

/// Write-through helper for the single-row session table.
/// Pushes the active tab, current network, open DID, and the
/// last-resolved tuple. Silent on store-write errors — the
/// session row is purely a UX convenience, never a hard
/// dependency.
pub(crate) fn persist_session(
    bridge_state: &BridgeState,
    network: Network,
    active_tab: Tab,
    open_did: Option<String>,
    last_did_id: Option<String>,
    last_resolved: Option<(String, u32)>,
) {
    let Some(store) = bridge_state.store() else {
        return;
    };
    let snap = wallet_core::store::SessionSnapshot {
        network,
        active_tab: active_tab.to_persist(),
        open_did,
        last_did_id,
        last_resolved,
        updated_at: 0,
    };
    if let Err(e) = store.put_session(snap) {
        tracing::warn!(error=%e, "persist session failed");
    }
}

// ─── Filesystem paths ───────────────────────────────────────────

/// Path the persistent wallet store lives at — pinned to
/// `~/.midnight/wallet-prototype/wallet.redb` for the
/// prototype. Stays out of platform-specific data dirs
/// (`~/Library/Application Support/...` etc.) so all three
/// host OSes share the same conventional location for the
/// duration of the prototype work; a production build would
/// likely move back to the OS-idiomatic dir.
///
/// Falls back to a `./wallet.redb` next to the binary if the
/// home dir can't be resolved (unlikely on macOS / Linux /
/// Windows but defensive).
pub(crate) fn wallet_store_path() -> std::path::PathBuf {
    #[cfg(target_os = "android")]
    {
        // Android: the app sandbox can write to its own private
        // `files/` dir but not to `/data/local/tmp` (owned by the
        // shell user). Resolve the running app's package name from
        // `/proc/self/cmdline` (Android writes the package id there
        // for every app process) and write under
        // `/data/data/<package>/files/midnight-dx-wallet/`. Falls
        // back to a relative path if we can't read the package id —
        // the binary still launches even if persistence is broken.
        if let Some(pkg) = read_android_package_name() {
            let dir = std::path::PathBuf::from("/data/data")
                .join(&pkg)
                .join("files")
                .join("midnight-dx-wallet");
            let _ = std::fs::create_dir_all(&dir);
            return dir.join("wallet.redb");
        }
        return std::path::PathBuf::from("wallet.redb");
    }
    #[cfg(target_os = "ios")]
    {
        // iOS: the per-app sandbox is reachable via `$HOME` (which
        // the system points at the app's container root). Put
        // persistent user-visible data under `Documents/` so iCloud
        // backup picks it up if the app opts in. `dirs` isn't on
        // the iOS dep set — we resolve `$HOME` directly via
        // `std::env`.
        if let Some(home) = std::env::var_os("HOME") {
            let dir = std::path::PathBuf::from(home)
                .join("Documents")
                .join("midnight-dx-wallet");
            let _ = std::fs::create_dir_all(&dir);
            return dir.join("wallet.redb");
        }
        return std::path::PathBuf::from("wallet.redb");
    }
    #[cfg(all(not(target_os = "android"), not(target_os = "ios")))]
    {
        if let Some(home) = dirs::home_dir() {
            let dir = home.join(".midnight").join("wallet-prototype");
            let _ = std::fs::create_dir_all(&dir);
            return dir.join("wallet.redb");
        }
        std::path::PathBuf::from("wallet.redb")
    }
}

/// Directory where wallet backup files (`*.mwallet.json`) live.
/// Co-located with `wallet_store_path()`'s parent so the operator
/// finds them next to the live database, and on iOS the backups
/// land under the per-app `Documents/` tree — reachable via
/// `xcrun simctl pull` for the sim, and via Files.app for the
/// device (provided `UIFileSharingEnabled=YES` ever gets flipped
/// in Info.plist).
pub(crate) fn wallet_backup_dir() -> std::path::PathBuf {
    let dir = wallet_store_path()
        .parent()
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("backups");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// Read the running app's package name from `/proc/self/cmdline`.
/// Android writes the package id there for every app process. This
/// avoids pulling JNI just to ask Android for `getPackageName()`.
#[cfg(target_os = "android")]
fn read_android_package_name() -> Option<String> {
    let raw = std::fs::read("/proc/self/cmdline").ok()?;
    // cmdline is NUL-separated; the first slot is the executable
    // (the package id, e.g. "io.iohk.midnight.wallet").
    let first = raw.split(|b| *b == 0).next()?;
    let s = std::str::from_utf8(first).ok()?.trim();
    if s.is_empty() {
        None
    } else {
        Some(s.to_string())
    }
}
