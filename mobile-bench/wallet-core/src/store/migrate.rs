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
    CONTROLLER_SECRETS, META, META_SCHEMA_VERSION_KEY, SCHEMA_VERSION, WALLETS,
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
