//! VcStore CRUD API.
//!
//! Redb-backed adapter implementing the `VcStorage` port. The
//! inherent methods stay public so existing callers can continue
//! to hit them directly during the transition off the `VcStore`
//! type alias; new callers should prefer `&dyn VcStorage`.

use std::path::Path;
use std::sync::Arc;

use redb::{Database, ReadableTable};

use crate::vc_store::tables::{VCS, VC_OPENINGS, VC_METADATA, opening_key};
use crate::vc_store::types::{StoredVc, VcOpening, VcMetadata};
use crate::vc_store::VcStorage;

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

pub struct RedbVcStore {
    db: Arc<Database>,
}

impl RedbVcStore {
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

    pub fn insert_opening(&self, op: &VcOpening) -> Result<(), VcStoreError> {
        let wtx = self.db.begin_write()?;
        {
            let mut t = wtx.open_table(VC_OPENINGS)?;
            let key = opening_key(&op.vc_uri, &op.claim_path);
            t.insert(key.as_str(), serde_cbor::to_vec(op)?)?;
        }
        wtx.commit()?;
        Ok(())
    }

    pub fn get_opening(&self, vc_uri: &str, claim_path: &str)
        -> Result<Option<VcOpening>, VcStoreError>
    {
        let rtx = self.db.begin_read()?;
        let t = rtx.open_table(VC_OPENINGS)?;
        let key = opening_key(vc_uri, claim_path);
        match t.get(key.as_str())? {
            Some(g) => Ok(Some(serde_cbor::from_slice(&g.value())?)),
            None => Ok(None),
        }
    }

    pub fn update_metadata(&self, vc_uri: &str, f: impl FnOnce(&mut VcMetadata))
        -> Result<(), VcStoreError>
    {
        let wtx = self.db.begin_write()?;
        let mut md = {
            let t = wtx.open_table(VC_METADATA)?;
            match t.get(vc_uri)? {
                Some(g) => serde_cbor::from_slice::<VcMetadata>(&g.value())?,
                None => VcMetadata { vc_uri: vc_uri.into(), ..Default::default() },
            }
        };
        f(&mut md);
        {
            let mut t = wtx.open_table(VC_METADATA)?;
            t.insert(vc_uri, serde_cbor::to_vec(&md)?)?;
        }
        wtx.commit()?;
        Ok(())
    }

    pub fn get_metadata(&self, vc_uri: &str)
        -> Result<Option<VcMetadata>, VcStoreError>
    {
        let rtx = self.db.begin_read()?;
        let t = rtx.open_table(VC_METADATA)?;
        match t.get(vc_uri)? {
            Some(g) => Ok(Some(serde_cbor::from_slice(&g.value())?)),
            None => Ok(None),
        }
    }

    /// Returns all VCs sorted by `VcMetadata.display_order` ascending.
    /// VCs without metadata sort last (display_order = u32::MAX).
    pub fn list_ordered(&self) -> Result<Vec<StoredVc>, VcStoreError> {
        let rtx = self.db.begin_read()?;
        let vcs_t = rtx.open_table(VCS)?;
        let md_t = rtx.open_table(VC_METADATA)?;
        let mut rows: Vec<(u32, StoredVc)> = Vec::new();
        for entry in vcs_t.iter()? {
            let (k, v) = entry?;
            let vc: StoredVc = serde_cbor::from_slice(&v.value())?;
            let order = match md_t.get(k.value())? {
                Some(g) => {
                    let md: VcMetadata = serde_cbor::from_slice(&g.value())?;
                    md.display_order
                }
                None => u32::MAX,
            };
            rows.push((order, vc));
        }
        rows.sort_by_key(|(o, _)| *o);
        Ok(rows.into_iter().map(|(_, vc)| vc).collect())
    }

    pub fn delete_vc(&self, vc_uri: &str) -> Result<(), VcStoreError> {
        let wtx = self.db.begin_write()?;
        {
            let mut vt = wtx.open_table(VCS)?;
            vt.remove(vc_uri)?;
        }
        {
            let mut mt = wtx.open_table(VC_METADATA)?;
            mt.remove(vc_uri)?;
        }
        {
            // Range-scan openings under the vc_uri prefix.
            let mut ot = wtx.open_table(VC_OPENINGS)?;
            let prefix_end = format!("{vc_uri}\x20"); // 0x20 = 0x1f + 1
            let prefix_start = format!("{vc_uri}\x1f");
            let keys: Vec<String> = ot
                .range(prefix_start.as_str()..prefix_end.as_str())?
                .filter_map(Result::ok)
                .map(|(k, _)| k.value().to_string())
                .collect();
            for k in keys {
                ot.remove(k.as_str())?;
            }
        }
        wtx.commit()?;
        Ok(())
    }

    pub fn insert_vc_with_openings(
        &self,
        vc: &StoredVc,
        openings: &[VcOpening],
    ) -> Result<(), VcStoreError> {
        let wtx = self.db.begin_write()?;
        {
            let mut t = wtx.open_table(VCS)?;
            t.insert(vc.vc_uri.as_str(), serde_cbor::to_vec(vc)?)?;
        }
        {
            let mut t = wtx.open_table(VC_OPENINGS)?;
            for op in openings {
                let key = opening_key(&op.vc_uri, &op.claim_path);
                t.insert(key.as_str(), serde_cbor::to_vec(op)?)?;
            }
        }
        wtx.commit()?;
        Ok(())
    }
}

/// `VcStorage` impl delegates to the inherent methods. The
/// `update_metadata` signature differs (closure-by-trait-object vs
/// `impl FnOnce`) so we wrap the trait-object closure into the
/// generic inherent form here.
impl VcStorage for RedbVcStore {
    fn insert_vc(&self, vc: &StoredVc) -> Result<(), VcStoreError> {
        RedbVcStore::insert_vc(self, vc)
    }
    fn get_vc(&self, vc_uri: &str) -> Result<Option<StoredVc>, VcStoreError> {
        RedbVcStore::get_vc(self, vc_uri)
    }
    fn insert_opening(&self, op: &VcOpening) -> Result<(), VcStoreError> {
        RedbVcStore::insert_opening(self, op)
    }
    fn get_opening(
        &self,
        vc_uri: &str,
        claim_path: &str,
    ) -> Result<Option<VcOpening>, VcStoreError> {
        RedbVcStore::get_opening(self, vc_uri, claim_path)
    }
    fn get_metadata(&self, vc_uri: &str) -> Result<Option<VcMetadata>, VcStoreError> {
        RedbVcStore::get_metadata(self, vc_uri)
    }
    fn list_ordered(&self) -> Result<Vec<StoredVc>, VcStoreError> {
        RedbVcStore::list_ordered(self)
    }
    fn delete_vc(&self, vc_uri: &str) -> Result<(), VcStoreError> {
        RedbVcStore::delete_vc(self, vc_uri)
    }
    fn insert_vc_with_openings(
        &self,
        vc: &StoredVc,
        openings: &[VcOpening],
    ) -> Result<(), VcStoreError> {
        RedbVcStore::insert_vc_with_openings(self, vc, openings)
    }
    fn update_metadata(
        &self,
        vc_uri: &str,
        update: &mut dyn FnMut(&mut VcMetadata),
    ) -> Result<(), VcStoreError> {
        RedbVcStore::update_metadata(self, vc_uri, |m| update(m))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn open_store() -> (RedbVcStore, TempDir) {
        let dir = TempDir::new().unwrap();
        let store = RedbVcStore::open(dir.path().join("test.redb")).expect("open");
        (store, dir)
    }

    fn sample_vc() -> StoredVc {
        StoredVc {
            vc_uri: "urn:uuid:abc-123".into(),
            issuer_did: "did:midnight:issuer".into(),
            holder_did: "did:midnight:alice".into(),
            format: "midnight_compact_vc".into(),
            body: vec![1, 2, 3, 4],
            proof: vec![],
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

    #[test]
    fn opening_round_trips() {
        let (store, _g) = open_store();
        let op = VcOpening {
            vc_uri: "urn:uuid:abc".into(),
            claim_path: "/credentialSubject/dateOfBirth".into(),
            plaintext: b"1985-01-01".to_vec(),
            opening: vec![9, 8, 7],
        };
        store.insert_opening(&op).unwrap();
        let back = store.get_opening("urn:uuid:abc", "/credentialSubject/dateOfBirth")
            .unwrap().unwrap();
        assert_eq!(back.plaintext, op.plaintext);
        assert_eq!(back.opening, op.opening);
    }

    #[test]
    fn metadata_update_then_read() {
        let (store, _g) = open_store();
        let vc = sample_vc();
        store.insert_vc(&vc).unwrap();
        store.update_metadata(&vc.vc_uri, |m| {
            m.display_order = 3;
            m.last_verified_ms = Some(42);
            m.last_verify_outcome = Some("Valid".into());
        }).unwrap();
        let md = store.get_metadata(&vc.vc_uri).unwrap().expect("present");
        assert_eq!(md.display_order, 3);
        assert_eq!(md.last_verified_ms, Some(42));
    }

    #[test]
    fn list_ordered_returns_by_display_order() {
        let (store, _g) = open_store();
        for (i, uri) in ["urn:b", "urn:a", "urn:c"].iter().enumerate() {
            store.insert_vc(&StoredVc {
                vc_uri: (*uri).into(),
                issuer_did: "did:midnight:i".into(),
                holder_did: "did:midnight:h".into(),
                format: "f".into(),
                body: vec![i as u8],
                proof: vec![],
                issued_at_ms: i as u64,
            }).unwrap();
            // Note: "urn:a" gets order 2, "urn:b" order 0, "urn:c" order 1 below
            let order = match *uri { "urn:b" => 0u32, "urn:c" => 1, "urn:a" => 2, _ => unreachable!() };
            store.update_metadata(uri, |m| m.display_order = order).unwrap();
        }
        let list = store.list_ordered().unwrap();
        let uris: Vec<&str> = list.iter().map(|v| v.vc_uri.as_str()).collect();
        assert_eq!(uris, vec!["urn:b", "urn:c", "urn:a"]);
    }

    #[test]
    fn delete_removes_vc_openings_and_metadata() {
        let (store, _g) = open_store();
        let vc = sample_vc();
        store.insert_vc(&vc).unwrap();
        store.insert_opening(&VcOpening {
            vc_uri: vc.vc_uri.clone(),
            claim_path: "/x".into(),
            plaintext: vec![1],
            opening: vec![2],
        }).unwrap();
        store.update_metadata(&vc.vc_uri, |m| m.display_order = 1).unwrap();

        store.delete_vc(&vc.vc_uri).unwrap();

        assert!(store.get_vc(&vc.vc_uri).unwrap().is_none());
        assert!(store.get_opening(&vc.vc_uri, "/x").unwrap().is_none());
        assert!(store.get_metadata(&vc.vc_uri).unwrap().is_none());
    }

    #[test]
    fn insert_vc_with_openings_lands_atomically() {
        let (store, _g) = open_store();
        let vc = sample_vc();
        let openings = vec![
            VcOpening { vc_uri: vc.vc_uri.clone(), claim_path: "/a".into(), plaintext: vec![1], opening: vec![2] },
            VcOpening { vc_uri: vc.vc_uri.clone(), claim_path: "/b".into(), plaintext: vec![3], opening: vec![4] },
        ];
        store.insert_vc_with_openings(&vc, &openings).unwrap();
        assert!(store.get_vc(&vc.vc_uri).unwrap().is_some());
        assert!(store.get_opening(&vc.vc_uri, "/a").unwrap().is_some());
        assert!(store.get_opening(&vc.vc_uri, "/b").unwrap().is_some());
    }

    // ── Trait-surface parity tests ────────────────────────────────────
    //
    // The same scenarios exercised against `&dyn VcStorage` so the
    // port contract is exercised end-to-end. The `InMemoryVcStore`
    // adapter has its own block in `in_memory.rs`; this one pins
    // `RedbVcStore`'s trait impl.

    fn open_store_trait() -> (Box<dyn VcStorage>, TempDir) {
        let dir = TempDir::new().unwrap();
        let store = RedbVcStore::open(dir.path().join("trait.redb")).expect("open");
        (Box::new(store), dir)
    }

    #[test]
    fn trait_insert_then_get_round_trips() {
        let (store, _g) = open_store_trait();
        let vc = sample_vc();
        store.insert_vc(&vc).unwrap();
        let back = store.get_vc(&vc.vc_uri).unwrap().unwrap();
        assert_eq!(back.body, vc.body);
    }

    #[test]
    fn trait_update_metadata_via_closure_object() {
        let (store, _g) = open_store_trait();
        let vc = sample_vc();
        store.insert_vc(&vc).unwrap();
        store
            .update_metadata(&vc.vc_uri, &mut |m| {
                m.display_order = 7;
                m.last_verify_outcome = Some("Valid".into());
            })
            .unwrap();
        let md = store.get_metadata(&vc.vc_uri).unwrap().unwrap();
        assert_eq!(md.display_order, 7);
        assert_eq!(md.last_verify_outcome.as_deref(), Some("Valid"));
    }

    // ── Backward compatibility: pre-proof-field CBOR rows ───────────
    //
    // VCs written before the `proof` field was added have no
    // `proof` key in their CBOR representation. `#[serde(default)]`
    // ensures they deserialize with `proof = vec![]`.

    /// Minimal struct matching the *old* StoredVc shape (no proof field)
    /// so we can serialize CBOR that lacks the `proof` key.
    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
    struct OldStoredVc {
        vc_uri: String,
        issuer_did: String,
        holder_did: String,
        format: String,
        body: Vec<u8>,
        issued_at_ms: u64,
    }

    #[test]
    fn proof_field_defaults_when_reading_old_cbor_rows() {
        // Simulate a pre-proof-field VC written by an older wallet build.
        let old = OldStoredVc {
            vc_uri: "urn:uuid:legacy-1".into(),
            issuer_did: "did:midnight:issuer".into(),
            holder_did: "did:midnight:alice".into(),
            format: "midnight_compact_vc".into(),
            body: vec![0xDE, 0xAD, 0xBE, 0xEF],
            issued_at_ms: 999_999,
        };
        let old_cbor = serde_cbor::to_vec(&old).expect("serialize old VC");
        // Deserialize the legacy CBOR into the new StoredVc (which has `proof`).
        let back: StoredVc = serde_cbor::from_slice(&old_cbor).expect("deserialize old CBOR into new StoredVc");
        assert_eq!(back.vc_uri, "urn:uuid:legacy-1");
        assert_eq!(back.body, vec![0xDE, 0xAD, 0xBE, 0xEF]);
        assert!(back.proof.is_empty(), "proof should default to empty vec for legacy rows");
    }
}
