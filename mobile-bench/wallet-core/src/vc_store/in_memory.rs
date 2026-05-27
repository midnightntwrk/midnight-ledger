//! In-memory `VcStorage` adapter. Used by unit tests + the
//! `test_support` story; production callers use `RedbVcStore`.

#![cfg(any(test, feature = "test-support"))]

use std::collections::{BTreeMap, HashMap};
use std::sync::Mutex;

use crate::vc_store::{StoredVc, VcMetadata, VcOpening, VcStorage, VcStoreError};

#[derive(Default, Debug)]
pub struct InMemoryVcStore {
    inner: Mutex<Inner>,
}

#[derive(Default, Debug)]
struct Inner {
    vcs: HashMap<String, StoredVc>,
    /// keyed by (vc_uri, claim_path) — same composite shape as
    /// the redb adapter's `opening_key`. Using BTreeMap so range
    /// scans by prefix work for `delete_vc`.
    openings: BTreeMap<(String, String), VcOpening>,
    metadata: HashMap<String, VcMetadata>,
}

impl InMemoryVcStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl VcStorage for InMemoryVcStore {
    fn insert_vc(&self, vc: &StoredVc) -> Result<(), VcStoreError> {
        self.inner
            .lock()
            .unwrap()
            .vcs
            .insert(vc.vc_uri.clone(), vc.clone());
        Ok(())
    }
    fn get_vc(&self, vc_uri: &str) -> Result<Option<StoredVc>, VcStoreError> {
        Ok(self.inner.lock().unwrap().vcs.get(vc_uri).cloned())
    }
    fn insert_opening(&self, op: &VcOpening) -> Result<(), VcStoreError> {
        self.inner
            .lock()
            .unwrap()
            .openings
            .insert((op.vc_uri.clone(), op.claim_path.clone()), op.clone());
        Ok(())
    }
    fn get_opening(
        &self,
        vc_uri: &str,
        claim_path: &str,
    ) -> Result<Option<VcOpening>, VcStoreError> {
        Ok(self
            .inner
            .lock()
            .unwrap()
            .openings
            .get(&(vc_uri.to_string(), claim_path.to_string()))
            .cloned())
    }
    fn get_metadata(&self, vc_uri: &str) -> Result<Option<VcMetadata>, VcStoreError> {
        Ok(self.inner.lock().unwrap().metadata.get(vc_uri).cloned())
    }
    fn update_metadata(
        &self,
        vc_uri: &str,
        update: &mut dyn FnMut(&mut VcMetadata),
    ) -> Result<(), VcStoreError> {
        let mut inner = self.inner.lock().unwrap();
        let entry = inner
            .metadata
            .entry(vc_uri.to_string())
            .or_insert_with(|| VcMetadata {
                vc_uri: vc_uri.to_string(),
                ..Default::default()
            });
        update(entry);
        Ok(())
    }
    fn list_ordered(&self) -> Result<Vec<StoredVc>, VcStoreError> {
        let inner = self.inner.lock().unwrap();
        let mut rows: Vec<(u32, StoredVc)> = inner
            .vcs
            .values()
            .cloned()
            .map(|vc| {
                let order = inner
                    .metadata
                    .get(&vc.vc_uri)
                    .map(|m| m.display_order)
                    .unwrap_or(u32::MAX);
                (order, vc)
            })
            .collect();
        rows.sort_by_key(|(o, _)| *o);
        Ok(rows.into_iter().map(|(_, vc)| vc).collect())
    }
    fn delete_vc(&self, vc_uri: &str) -> Result<(), VcStoreError> {
        let mut inner = self.inner.lock().unwrap();
        inner.vcs.remove(vc_uri);
        inner.metadata.remove(vc_uri);
        // Range delete over the BTreeMap. The composite key is
        // (vc_uri, claim_path); collect keys to remove to avoid
        // mutating while iterating.
        let keys: Vec<_> = inner
            .openings
            .range((vc_uri.to_string(), String::new())..)
            .take_while(|((u, _), _)| u == vc_uri)
            .map(|(k, _)| k.clone())
            .collect();
        for k in keys {
            inner.openings.remove(&k);
        }
        Ok(())
    }
    fn insert_vc_with_openings(
        &self,
        vc: &StoredVc,
        openings: &[VcOpening],
    ) -> Result<(), VcStoreError> {
        // Atomic vs the public API contract: take the lock once,
        // do both writes, drop. Mirrors `RedbVcStore`'s single-tx
        // semantics for crash-safety on disk (here just locks
        // out concurrent readers for the duration).
        let mut inner = self.inner.lock().unwrap();
        inner.vcs.insert(vc.vc_uri.clone(), vc.clone());
        for op in openings {
            inner
                .openings
                .insert((op.vc_uri.clone(), op.claim_path.clone()), op.clone());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    fn open_store() -> Box<dyn VcStorage> {
        Box::new(InMemoryVcStore::default())
    }

    #[test]
    fn insert_then_get_round_trips() {
        let store = open_store();
        let vc = sample_vc();
        store.insert_vc(&vc).unwrap();
        let back = store.get_vc(&vc.vc_uri).unwrap().unwrap();
        assert_eq!(back.vc_uri, vc.vc_uri);
        assert_eq!(back.body, vc.body);
    }

    #[test]
    fn get_missing_returns_none() {
        let store = open_store();
        assert!(store.get_vc("urn:uuid:nope").unwrap().is_none());
    }

    #[test]
    fn opening_round_trips() {
        let store = open_store();
        let op = VcOpening {
            vc_uri: "urn:uuid:abc".into(),
            claim_path: "/credentialSubject/dateOfBirth".into(),
            plaintext: b"1985-01-01".to_vec(),
            opening: vec![9, 8, 7],
        };
        store.insert_opening(&op).unwrap();
        let back = store
            .get_opening("urn:uuid:abc", "/credentialSubject/dateOfBirth")
            .unwrap()
            .unwrap();
        assert_eq!(back.plaintext, op.plaintext);
        assert_eq!(back.opening, op.opening);
    }

    #[test]
    fn metadata_update_then_read() {
        let store = open_store();
        let vc = sample_vc();
        store.insert_vc(&vc).unwrap();
        store
            .update_metadata(&vc.vc_uri, &mut |m| {
                m.display_order = 3;
                m.last_verified_ms = Some(42);
                m.last_verify_outcome = Some("Valid".into());
            })
            .unwrap();
        let md = store.get_metadata(&vc.vc_uri).unwrap().unwrap();
        assert_eq!(md.display_order, 3);
        assert_eq!(md.last_verified_ms, Some(42));
    }

    #[test]
    fn list_ordered_returns_by_display_order() {
        let store = open_store();
        for (i, uri) in ["urn:b", "urn:a", "urn:c"].iter().enumerate() {
            store
                .insert_vc(&StoredVc {
                    vc_uri: (*uri).into(),
                    issuer_did: "did:midnight:i".into(),
                    holder_did: "did:midnight:h".into(),
                    format: "f".into(),
                    body: vec![i as u8],
                    issued_at_ms: i as u64,
                })
                .unwrap();
            let order = match *uri {
                "urn:b" => 0u32,
                "urn:c" => 1,
                "urn:a" => 2,
                _ => unreachable!(),
            };
            store
                .update_metadata(uri, &mut |m| m.display_order = order)
                .unwrap();
        }
        let list = store.list_ordered().unwrap();
        let uris: Vec<&str> = list.iter().map(|v| v.vc_uri.as_str()).collect();
        assert_eq!(uris, vec!["urn:b", "urn:c", "urn:a"]);
    }

    #[test]
    fn delete_removes_vc_openings_and_metadata() {
        let store = open_store();
        let vc = sample_vc();
        store.insert_vc(&vc).unwrap();
        store
            .insert_opening(&VcOpening {
                vc_uri: vc.vc_uri.clone(),
                claim_path: "/x".into(),
                plaintext: vec![1],
                opening: vec![2],
            })
            .unwrap();
        store
            .update_metadata(&vc.vc_uri, &mut |m| m.display_order = 1)
            .unwrap();

        store.delete_vc(&vc.vc_uri).unwrap();

        assert!(store.get_vc(&vc.vc_uri).unwrap().is_none());
        assert!(store.get_opening(&vc.vc_uri, "/x").unwrap().is_none());
        assert!(store.get_metadata(&vc.vc_uri).unwrap().is_none());
    }

    #[test]
    fn insert_vc_with_openings_lands_atomically() {
        let store = open_store();
        let vc = sample_vc();
        let openings = vec![
            VcOpening {
                vc_uri: vc.vc_uri.clone(),
                claim_path: "/a".into(),
                plaintext: vec![1],
                opening: vec![2],
            },
            VcOpening {
                vc_uri: vc.vc_uri.clone(),
                claim_path: "/b".into(),
                plaintext: vec![3],
                opening: vec![4],
            },
        ];
        store.insert_vc_with_openings(&vc, &openings).unwrap();
        assert!(store.get_vc(&vc.vc_uri).unwrap().is_some());
        assert!(store.get_opening(&vc.vc_uri, "/a").unwrap().is_some());
        assert!(store.get_opening(&vc.vc_uri, "/b").unwrap().is_some());
    }

    #[test]
    fn delete_scoped_to_vc_uri_prefix() {
        // Two VCs, both with openings — delete one and the other's
        // openings remain.
        let store = open_store();
        let vc_a = StoredVc {
            vc_uri: "urn:a".into(),
            ..sample_vc()
        };
        let vc_b = StoredVc {
            vc_uri: "urn:b".into(),
            ..sample_vc()
        };
        store.insert_vc(&vc_a).unwrap();
        store.insert_vc(&vc_b).unwrap();
        store
            .insert_opening(&VcOpening {
                vc_uri: "urn:a".into(),
                claim_path: "/x".into(),
                plaintext: vec![],
                opening: vec![],
            })
            .unwrap();
        store
            .insert_opening(&VcOpening {
                vc_uri: "urn:b".into(),
                claim_path: "/x".into(),
                plaintext: vec![],
                opening: vec![],
            })
            .unwrap();
        store.delete_vc("urn:a").unwrap();
        assert!(store.get_vc("urn:a").unwrap().is_none());
        assert!(store.get_vc("urn:b").unwrap().is_some());
        assert!(store.get_opening("urn:a", "/x").unwrap().is_none());
        assert!(store.get_opening("urn:b", "/x").unwrap().is_some());
    }
}
