//! VcStore CRUD API.

use std::path::Path;
use std::sync::Arc;

use redb::Database;

use crate::vc_store::tables::{VCS, VC_OPENINGS, VC_METADATA};
use crate::vc_store::types::StoredVc;

#[derive(Debug, thiserror::Error)]
pub enum VcStoreError {
    #[error("redb error: {0}")]
    Redb(#[from] redb::Error),
    #[error("redb tx commit error: {0}")]
    Commit(#[from] redb::CommitError),
    #[error("redb tx begin error: {0}")]
    Begin(#[from] redb::TransactionError),
    #[error("redb table error: {0}")]
    Table(#[from] redb::TableError),
    #[error("redb storage error: {0}")]
    Storage(#[from] redb::StorageError),
    #[error("cbor error: {0}")]
    Cbor(#[from] serde_cbor::Error),
}

pub struct VcStore {
    db: Arc<Database>,
}

impl VcStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, VcStoreError> {
        let db = Database::create(path).map_err(redb::Error::from)?;
        // Materialise the three tables so first use doesn't race.
        let wtx = db.begin_write()?;
        let _ = wtx.open_table(VCS)?;
        let _ = wtx.open_table(VC_OPENINGS)?;
        let _ = wtx.open_table(VC_METADATA)?;
        wtx.commit()?;
        Ok(Self { db: Arc::new(db) })
    }

    pub fn insert_vc(&self, vc: &StoredVc) -> Result<(), VcStoreError> {
        let wtx = self.db.begin_write()?;
        {
            let mut t = wtx.open_table(VCS)?;
            t.insert(vc.vc_uri.as_str(), serde_cbor::to_vec(vc)?)?;
        }
        wtx.commit()?;
        Ok(())
    }

    pub fn get_vc(&self, vc_uri: &str) -> Result<Option<StoredVc>, VcStoreError> {
        let rtx = self.db.begin_read()?;
        let t = rtx.open_table(VCS)?;
        let row = t.get(vc_uri)?;
        match row {
            Some(g) => Ok(Some(serde_cbor::from_slice(&g.value())?)),
            None => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn open_store() -> (VcStore, TempDir) {
        let dir = TempDir::new().unwrap();
        let store = VcStore::open(dir.path().join("test.redb")).expect("open");
        (store, dir)
    }

    fn sample_vc() -> StoredVc {
        StoredVc {
            vc_uri: "urn:uuid:abc-123".into(),
            issuer_did: "did:midnight:issuer".into(),
            holder_did: "did:midnight:alice".into(),
            format: "midnight-vc-compact".into(),
            body: vec![1, 2, 3, 4],
            issued_at_ms: 1_000_000,
        }
    }

    #[test]
    fn insert_then_get_round_trips() {
        let (store, _g) = open_store();
        let vc = sample_vc();
        store.insert_vc(&vc).expect("insert");
        let back = store.get_vc(&vc.vc_uri).expect("get").expect("present");
        assert_eq!(back.vc_uri, vc.vc_uri);
        assert_eq!(back.body, vc.body);
    }

    #[test]
    fn get_missing_returns_none() {
        let (store, _g) = open_store();
        assert!(store.get_vc("urn:uuid:nope").expect("get").is_none());
    }
}
