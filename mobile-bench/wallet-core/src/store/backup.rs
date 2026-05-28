//! Export / Import of the wallet's irrecoverable secrets — the
//! durable backup story that decouples the per-DID controller
//! secrets from the per-app sandbox lifetime.
//!
//! ## What's in the backup
//!
//! Two tables, both carrying scrypt(passphrase)+AES-GCM-encrypted
//! envelopes:
//!
//! - `WALLETS` — wallet HD seeds. Each row is a `WalletRowV1`
//!   bincode-blob (the seed lives inside one of its fields,
//!   wrapped in an envelope). Recoverable from a remembered
//!   seed phrase, but bundled here for one-shot restore
//!   convenience.
//! - `CONTROLLER_SECRETS` — per-DID random 32-byte controller
//!   secrets minted at DID-deploy time. NOT seed-derived (R10
//!   schema). Without these the operator can't sign any update,
//!   relation, or deactivate transaction against their own DIDs —
//!   the DID exists on chain forever as a read-only artifact.
//!
//! Skipped (intentionally): `KEYS`, `DID_INVENTORY`,
//! `RESOLVED_CACHE`, `SESSIONS`, `LOGS`. All of these are
//! re-derivable: KEYS from the wallet seed via HD; the others
//! from `resolve_did_full` calls against the chain.
//!
//! ## Encryption story
//!
//! The export file carries the rows' **existing** encrypted
//! envelopes verbatim. No re-encryption, no extra passphrase
//! prompt. The same scrypt(unlock-passphrase) that decrypts
//! the live store decrypts the restored rows after import.
//!
//! Practical consequence: a backup file is only usable by an
//! operator who remembers the wallet's unlock passphrase. The
//! file itself adds zero attack surface beyond what
//! `wallet.redb` already exposes — leak either, lose the same
//! amount.
//!
//! ## File format
//!
//! JSON for portability (operator can eyeball it). Top-level
//! shape:
//!
//! ```json
//! {
//!   "format": "midnight-wallet-backup",
//!   "version": 1,
//!   "exported_at_ms": 1748541234567,
//!   "wallets": [
//!     { "wallet_id_hex": "<32-char hex>", "row_b64": "<base64>" }
//!   ],
//!   "controller_secrets": [
//!     { "network": "preprod", "did": "did:midnight:preprod:abc…",
//!       "envelope_b64": "<base64>" }
//!   ]
//! }
//! ```
//!
//! `wallet_id_hex` is the 16-byte WalletId as lowercase hex.
//! `network` is the lowercase `Network::label()` (preprod /
//! testnet / mainnet / undeployed / devnet).
//!
//! ## Import semantics
//!
//! Overwrite on conflict. Two reasonable behaviours exist —
//! merge-on-conflict (keep DB value) or replace-on-conflict
//! (use backup value). The backup represents the operator's
//! intended state at export time; restoring it should yield
//! exactly that state, so we replace. Rows present in the DB
//! but absent from the backup are NOT removed — restoring
//! never deletes data, only adds or overwrites.

use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine;
use redb::ReadableTable;
use serde::{Deserialize, Serialize};

use super::schema::{CONTROLLER_SECRETS, NetworkTag, WALLETS};
use super::{StoreError, WalletStore};
use crate::network::Network;

/// Top-level export envelope. Versioned so future schema
/// changes can detect + reject incompatible files at import.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalletBackup {
    /// Magic string. Importers should reject anything that
    /// isn't this exact value — guards against accidentally
    /// pointing the importer at an unrelated JSON file.
    pub format: String,
    pub version: u32,
    pub exported_at_ms: i64,
    #[serde(default)]
    pub wallets: Vec<BackupWalletRow>,
    #[serde(default)]
    pub controller_secrets: Vec<BackupControllerSecretRow>,
}

/// One wallet's HD-seed row, ciphertext + metadata. The
/// `row_b64` is the same bincoded `WalletRowV1` blob the redb
/// `WALLETS` table stores — the seed envelope lives inside that
/// blob, encrypted under the wallet passphrase.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupWalletRow {
    pub wallet_id_hex: String,
    pub row_b64: String,
}

/// One per-DID controller-secret row. `network` is the
/// `Network::label()` string so the file stays human-readable;
/// the importer converts back to the `NetworkTag` byte that
/// keys the table.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupControllerSecretRow {
    pub network: String,
    pub did: String,
    pub envelope_b64: String,
}

/// Per-table counts returned by `import_backup`. Lets the
/// caller surface "restored N seeds + M controller secrets"
/// in the UI.
#[derive(Debug, Clone, Default, Copy, PartialEq, Eq)]
pub struct ImportSummary {
    pub wallets_inserted: usize,
    pub wallets_overwritten: usize,
    pub controller_secrets_inserted: usize,
    pub controller_secrets_overwritten: usize,
    pub controller_secrets_skipped_bad_network: usize,
}

pub const BACKUP_FORMAT: &str = "midnight-wallet-backup";
pub const BACKUP_VERSION: u32 = 1;

impl WalletStore {
    /// Snapshot the irrecoverable secrets into a portable
    /// `WalletBackup`. Both tables are read in a single read
    /// transaction so the snapshot is internally consistent;
    /// no holdover state between the two tables.
    ///
    /// Cheap on storage — for a typical operator with one
    /// wallet + a dozen DIDs the resulting JSON is well under
    /// 10 KB.
    pub fn export_backup(&self) -> Result<WalletBackup, StoreError> {
        let txn = self
            .db
            .begin_read()
            .map_err(|e| StoreError::Backend(e.to_string()))?;

        let mut wallets: Vec<BackupWalletRow> = Vec::new();
        {
            let table = txn
                .open_table(WALLETS)
                .map_err(|e| StoreError::Backend(e.to_string()))?;
            let iter = table
                .iter()
                .map_err(|e| StoreError::Backend(e.to_string()))?;
            for entry in iter {
                let (k, v) = entry.map_err(|e| StoreError::Backend(e.to_string()))?;
                wallets.push(BackupWalletRow {
                    wallet_id_hex: hex::encode(k.value()),
                    row_b64: base64::engine::general_purpose::STANDARD.encode(v.value()),
                });
            }
        }

        let mut controller_secrets: Vec<BackupControllerSecretRow> = Vec::new();
        {
            let table = txn
                .open_table(CONTROLLER_SECRETS)
                .map_err(|e| StoreError::Backend(e.to_string()))?;
            let iter = table
                .iter()
                .map_err(|e| StoreError::Backend(e.to_string()))?;
            for entry in iter {
                let (k, v) = entry.map_err(|e| StoreError::Backend(e.to_string()))?;
                let (tag, did) = k.value();
                let Some(net) = NetworkTag(tag).to_network() else {
                    // Unknown tag in the table — skip rather
                    // than corrupt the export. Shouldn't happen
                    // unless a future-network row predates an
                    // older binary doing the export.
                    continue;
                };
                controller_secrets.push(BackupControllerSecretRow {
                    // Lowercase `network_id` ("preprod") not
                    // `label()` ("PreProd") for the JSON wire
                    // form — matches the rest of the codebase's
                    // network strings + `Network::from_label`
                    // accepts both forms case-insensitively on
                    // import.
                    network: net.config().network_id.to_string(),
                    did: did.to_string(),
                    envelope_b64: base64::engine::general_purpose::STANDARD
                        .encode(v.value()),
                });
            }
        }

        Ok(WalletBackup {
            format: BACKUP_FORMAT.to_string(),
            version: BACKUP_VERSION,
            exported_at_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0),
            wallets,
            controller_secrets,
        })
    }

    /// Merge a `WalletBackup` into the live store. Existing rows
    /// with the same key are overwritten (the backup is the
    /// source of truth at restore time); rows present in the
    /// DB but absent from the backup are left untouched.
    ///
    /// Rejects backups whose `format` field isn't the magic
    /// string `BACKUP_FORMAT` or whose `version` is from the
    /// future. Same-version older / newer minor revisions could
    /// be tolerated later by bumping `version` and keeping a
    /// read-compat matrix here; today there's only one version.
    pub fn import_backup(
        &self,
        backup: &WalletBackup,
    ) -> Result<ImportSummary, StoreError> {
        if backup.format != BACKUP_FORMAT {
            return Err(StoreError::Backend(format!(
                "not a midnight-wallet-backup file (format={:?})",
                backup.format
            )));
        }
        if backup.version > BACKUP_VERSION {
            return Err(StoreError::Backend(format!(
                "backup version {} is newer than this build supports ({})",
                backup.version, BACKUP_VERSION
            )));
        }

        let txn = self
            .db
            .begin_write()
            .map_err(|e| StoreError::Backend(e.to_string()))?;

        let mut summary = ImportSummary::default();

        {
            let mut table = txn
                .open_table(WALLETS)
                .map_err(|e| StoreError::Backend(e.to_string()))?;
            for row in &backup.wallets {
                let wid_bytes = hex::decode(&row.wallet_id_hex).map_err(|e| {
                    StoreError::Backend(format!(
                        "wallets: bad wallet_id_hex ({}): {e}",
                        row.wallet_id_hex
                    ))
                })?;
                if wid_bytes.len() != 16 {
                    return Err(StoreError::Backend(format!(
                        "wallets: wallet_id must be 16 bytes, got {}",
                        wid_bytes.len()
                    )));
                }
                let mut wid = [0u8; 16];
                wid.copy_from_slice(&wid_bytes);
                let value_bytes = base64::engine::general_purpose::STANDARD
                    .decode(&row.row_b64)
                    .map_err(|e| {
                        StoreError::Backend(format!("wallets: bad row_b64: {e}"))
                    })?;
                let existed = table
                    .get(wid)
                    .map_err(|e| StoreError::Backend(e.to_string()))?
                    .is_some();
                table
                    .insert(wid, value_bytes.as_slice())
                    .map_err(|e| StoreError::Backend(e.to_string()))?;
                if existed {
                    summary.wallets_overwritten += 1;
                } else {
                    summary.wallets_inserted += 1;
                }
            }
        }

        {
            let mut table = txn
                .open_table(CONTROLLER_SECRETS)
                .map_err(|e| StoreError::Backend(e.to_string()))?;
            for row in &backup.controller_secrets {
                let Some(net) = Network::from_label(&row.network) else {
                    summary.controller_secrets_skipped_bad_network += 1;
                    continue;
                };
                let tag = NetworkTag::from(net).0;
                let value_bytes = base64::engine::general_purpose::STANDARD
                    .decode(&row.envelope_b64)
                    .map_err(|e| {
                        StoreError::Backend(format!(
                            "controller_secrets: bad envelope_b64: {e}"
                        ))
                    })?;
                let key = (tag, row.did.as_str());
                let existed = table
                    .get(key)
                    .map_err(|e| StoreError::Backend(e.to_string()))?
                    .is_some();
                table
                    .insert(key, value_bytes.as_slice())
                    .map_err(|e| StoreError::Backend(e.to_string()))?;
                if existed {
                    summary.controller_secrets_overwritten += 1;
                } else {
                    summary.controller_secrets_inserted += 1;
                }
            }
        }

        txn.commit()
            .map_err(|e| StoreError::Backend(e.to_string()))?;

        Ok(summary)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Round-trip: open an in-memory store, write a seed +
    /// controller secret, export, wipe, import, confirm the
    /// values come back identically.
    #[test]
    fn export_import_round_trip_preserves_rows() {
        let pw = "test-pw";
        let store_a = WalletStore::open_in_memory(pw).expect("open A");
        // Plant a seed and a controller secret in store_a.
        let wallet_id = store_a
            .create_wallet("backup-test", Network::PreProd, &[7u8; 32])
            .expect("put seed");
        store_a
            .put_controller_secret(Network::PreProd, "did:midnight:preprod:abc", &[42u8; 32])
            .expect("put cs");

        let backup = store_a.export_backup().expect("export");
        assert_eq!(backup.format, BACKUP_FORMAT);
        assert_eq!(backup.version, BACKUP_VERSION);
        assert_eq!(backup.wallets.len(), 1);
        assert_eq!(
            backup.wallets[0].wallet_id_hex,
            hex::encode(wallet_id.0)
        );
        assert_eq!(backup.controller_secrets.len(), 1);
        assert_eq!(backup.controller_secrets[0].network, "preprod");
        assert_eq!(
            backup.controller_secrets[0].did,
            "did:midnight:preprod:abc"
        );

        // Fresh empty store_b — same passphrase so the
        // re-imported envelopes decrypt cleanly.
        let store_b = WalletStore::open_in_memory(pw).expect("open B");
        let summary = store_b.import_backup(&backup).expect("import");
        assert_eq!(summary.wallets_inserted, 1);
        assert_eq!(summary.controller_secrets_inserted, 1);
        assert_eq!(summary.wallets_overwritten, 0);
        assert_eq!(summary.controller_secrets_overwritten, 0);

        // Roundtripped controller secret decrypts to the same
        // bytes (proves the envelope_b64 transport preserved the
        // ciphertext intact).
        let cs = store_b
            .get_controller_secret(Network::PreProd, "did:midnight:preprod:abc")
            .expect("get cs")
            .expect("present");
        assert_eq!(cs.as_slice(), &[42u8; 32]);
    }

    #[test]
    fn import_rejects_wrong_format_magic() {
        let store = WalletStore::open_in_memory("pw").expect("open");
        let bad = WalletBackup {
            format: "some-other-format".into(),
            version: 1,
            exported_at_ms: 0,
            wallets: vec![],
            controller_secrets: vec![],
        };
        let err = store.import_backup(&bad).unwrap_err();
        assert!(format!("{err}").contains("not a midnight-wallet-backup"));
    }

    #[test]
    fn import_rejects_future_version() {
        let store = WalletStore::open_in_memory("pw").expect("open");
        let future = WalletBackup {
            format: BACKUP_FORMAT.into(),
            version: BACKUP_VERSION + 99,
            exported_at_ms: 0,
            wallets: vec![],
            controller_secrets: vec![],
        };
        let err = store.import_backup(&future).unwrap_err();
        assert!(format!("{err}").contains("newer than this build"));
    }

    #[test]
    fn import_skips_unknown_network_in_controller_secret() {
        let store = WalletStore::open_in_memory("pw").expect("open");
        let backup = WalletBackup {
            format: BACKUP_FORMAT.into(),
            version: BACKUP_VERSION,
            exported_at_ms: 0,
            wallets: vec![],
            controller_secrets: vec![BackupControllerSecretRow {
                network: "moon-net-3000".into(),
                did: "did:midnight:moon:xxx".into(),
                envelope_b64: base64::engine::general_purpose::STANDARD.encode([0u8; 8]),
            }],
        };
        let summary = store.import_backup(&backup).expect("import");
        assert_eq!(summary.controller_secrets_inserted, 0);
        assert_eq!(summary.controller_secrets_skipped_bad_network, 1);
    }

    #[test]
    fn import_overwrites_existing_rows() {
        let pw = "test-pw";
        let store = WalletStore::open_in_memory(pw).expect("open");
        store
            .put_controller_secret(Network::PreProd, "did:midnight:preprod:abc", &[1u8; 32])
            .expect("put initial");
        // Snapshot a backup that carries a *different* secret
        // for the same key.
        let store_b = WalletStore::open_in_memory(pw).expect("open b");
        store_b
            .put_controller_secret(Network::PreProd, "did:midnight:preprod:abc", &[9u8; 32])
            .expect("put b");
        let backup = store_b.export_backup().expect("export");

        let summary = store.import_backup(&backup).expect("import");
        assert_eq!(summary.controller_secrets_overwritten, 1);
        assert_eq!(summary.controller_secrets_inserted, 0);

        let cs = store
            .get_controller_secret(Network::PreProd, "did:midnight:preprod:abc")
            .expect("get")
            .expect("present");
        assert_eq!(cs.as_slice(), &[9u8; 32]);
    }
}
