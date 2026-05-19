//! Schema migrations.
//!
//! On every `WalletStore::open()` we read
//! `meta["schema_version"]` and dispatch into the appropriate
//! `v_to_v+1` closure for each missing step. If the on-disk
//! version is *higher* than the binary's `SCHEMA_VERSION` we
//! refuse to open — a downgrade attempt would silently corrupt
//! newer rows the binary doesn't understand.
//!
//! v0 → v1: empty file → mint the schema_version row. The first
//! mutating write is what materialises the file on disk.

use redb::ReadableTable;

use crate::store::error::StoreError;
use crate::store::schema::{
    CONTROLLER_SECRETS, DID_INVENTORY, DIDS_BY_NETWORK, KEYS, KEYS_BY_WALLET, LOGS, META,
    META_SCHEMA_VERSION_KEY, RESOLVED_CACHE, SCHEMA_VERSION, SESSIONS, WALLETS,
};
use crate::store::WalletStore;

pub(crate) fn run(store: &WalletStore) -> Result<(), StoreError> {
    let current = read_version(store)?;
    if current > SCHEMA_VERSION {
        return Err(StoreError::Migration(format!(
            "on-disk schema version {current} is newer than binary's {SCHEMA_VERSION}; \
             refusing to open (binary downgrade detected)",
        )));
    }
    if current == SCHEMA_VERSION {
        return Ok(());
    }
    // Apply each step. Today the table is `[0 → 1]`; later
    // versions slot in here without touching the loop.
    let mut v = current;
    while v < SCHEMA_VERSION {
        let next = v + 1;
        match (v, next) {
            (0, 1) => migrate_v0_to_v1(store)?,
            (1, 2) => migrate_v1_to_v2(store)?,
            (2, 3) => migrate_v2_to_v3(store)?,
            (3, 4) => migrate_v3_to_v4(store)?,
            (4, 5) => migrate_v4_to_v5(store)?,
            (from, to) => {
                return Err(StoreError::Migration(format!(
                    "no migration registered for {from} → {to}",
                )));
            }
        }
        v = next;
    }
    Ok(())
}

fn read_version(store: &WalletStore) -> Result<u32, StoreError> {
    // Use a write transaction so the table exists even on a
    // freshly-created database. redb refuses to open a read
    // transaction on a table that's never been written to.
    let txn = store
        .db()
        .begin_write()
        .map_err(|e| StoreError::Backend(e.to_string()))?;
    let v = {
        let table = txn
            .open_table(META)
            .map_err(|e| StoreError::Backend(e.to_string()))?;
        table
            .get(META_SCHEMA_VERSION_KEY)
            .map_err(|e| StoreError::Backend(e.to_string()))?
            .map(|g| {
                let bytes = g.value();
                if bytes.len() == 4 {
                    u32::from_le_bytes(bytes.try_into().unwrap())
                } else {
                    0
                }
            })
            .unwrap_or(0)
    };
    txn.commit()
        .map_err(|e| StoreError::Backend(e.to_string()))?;
    Ok(v)
}

fn write_version(store: &WalletStore, v: u32) -> Result<(), StoreError> {
    let bytes = v.to_le_bytes();
    let txn = store
        .db()
        .begin_write()
        .map_err(|e| StoreError::Backend(e.to_string()))?;
    {
        let mut table = txn
            .open_table(META)
            .map_err(|e| StoreError::Backend(e.to_string()))?;
        table
            .insert(META_SCHEMA_VERSION_KEY, bytes.as_slice())
            .map_err(|e| StoreError::Backend(e.to_string()))?;
    }
    txn.commit()
        .map_err(|e| StoreError::Backend(e.to_string()))?;
    Ok(())
}

fn migrate_v0_to_v1(store: &WalletStore) -> Result<(), StoreError> {
    // v0 == "no file ever opened". The wallet, controller-
    // secret, and meta tables don't exist on disk yet. redb's
    // read transactions refuse to open a table that's never
    // been created, so opening + dropping each table inside a
    // write txn materialises them.
    let txn = store
        .db()
        .begin_write()
        .map_err(|e| StoreError::Backend(e.to_string()))?;
    {
        let _ = txn
            .open_table(WALLETS)
            .map_err(|e| StoreError::Backend(e.to_string()))?;
        let _ = txn
            .open_table(CONTROLLER_SECRETS)
            .map_err(|e| StoreError::Backend(e.to_string()))?;
        // META is already opened by `read_version` above, but
        // touch it again to keep the "every table mentioned in
        // schema.rs is created here" invariant obvious.
        let _ = txn
            .open_table(META)
            .map_err(|e| StoreError::Backend(e.to_string()))?;
    }
    txn.commit()
        .map_err(|e| StoreError::Backend(e.to_string()))?;
    write_version(store, 1)
}

fn migrate_v4_to_v5(store: &WalletStore) -> Result<(), StoreError> {
    // v4 → v5 adds the `logs` table — chronological archive
    // of the dioxus-wallet's `tracing` events. Empty on
    // creation; the UI's `WalletLogLayer` populates it in
    // batches.
    let txn = store
        .db()
        .begin_write()
        .map_err(|e| StoreError::Backend(e.to_string()))?;
    {
        let _ = txn
            .open_table(LOGS)
            .map_err(|e| StoreError::Backend(e.to_string()))?;
    }
    txn.commit()
        .map_err(|e| StoreError::Backend(e.to_string()))?;
    write_version(store, 5)
}

fn migrate_v3_to_v4(store: &WalletStore) -> Result<(), StoreError> {
    // v3 → v4 adds the `sessions` single-row table.
    let txn = store
        .db()
        .begin_write()
        .map_err(|e| StoreError::Backend(e.to_string()))?;
    {
        let _ = txn
            .open_table(SESSIONS)
            .map_err(|e| StoreError::Backend(e.to_string()))?;
    }
    txn.commit()
        .map_err(|e| StoreError::Backend(e.to_string()))?;
    write_version(store, 4)
}

fn migrate_v2_to_v3(store: &WalletStore) -> Result<(), StoreError> {
    // v2 → v3 adds three tables: `did_inventory`, the
    // `dids_by_network` index, and `resolved_cache`. No
    // existing rows to walk — pre-v3 wallets just have no
    // inventory or cache yet.
    let txn = store
        .db()
        .begin_write()
        .map_err(|e| StoreError::Backend(e.to_string()))?;
    {
        let _ = txn
            .open_table(DID_INVENTORY)
            .map_err(|e| StoreError::Backend(e.to_string()))?;
        let _ = txn
            .open_multimap_table(DIDS_BY_NETWORK)
            .map_err(|e| StoreError::Backend(e.to_string()))?;
        let _ = txn
            .open_table(RESOLVED_CACHE)
            .map_err(|e| StoreError::Backend(e.to_string()))?;
    }
    txn.commit()
        .map_err(|e| StoreError::Backend(e.to_string()))?;
    write_version(store, 3)
}

fn migrate_v1_to_v2(store: &WalletStore) -> Result<(), StoreError> {
    // v1 → v2 adds the keys table + the wallet-id index. No
    // existing rows to walk — v1 didn't have a keys table at
    // all. Materialise the new tables so subsequent read txns
    // don't fault.
    let txn = store
        .db()
        .begin_write()
        .map_err(|e| StoreError::Backend(e.to_string()))?;
    {
        let _ = txn
            .open_table(KEYS)
            .map_err(|e| StoreError::Backend(e.to_string()))?;
        let _ = txn
            .open_multimap_table(KEYS_BY_WALLET)
            .map_err(|e| StoreError::Backend(e.to_string()))?;
    }
    txn.commit()
        .map_err(|e| StoreError::Backend(e.to_string()))?;
    write_version(store, 2)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::WalletStore;

    #[test]
    fn fresh_in_memory_store_lands_at_v1() {
        let store = WalletStore::open_in_memory("pw").unwrap();
        assert_eq!(store.schema_version().unwrap(), SCHEMA_VERSION);
    }
}
