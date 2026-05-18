//! Bincode wrapper for redb row values. Lets us store any
//! `Serialize + Deserialize` Rust struct under a single
//! `&'static [u8]` value column without writing a hand-rolled
//! `redb::Value` impl per row type.
//!
//! Versioning policy: a struct change creates a new
//! `WhateverRowVN` and a migration in `migrate.rs` walks the
//! table. Decoding mismatches surface as `StoreError::Codec`
//! at row-read time — the migration code catches these and
//! routes the row through the corresponding upgrade closure.

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::store::error::StoreError;

/// Helper for the bincode `encode → Vec<u8> → as_slice()` dance
/// callers go through. Owning `Vec` so the caller can pass a
/// borrowed slice into `redb::Table::insert` without lifetime
/// gymnastics.
pub(crate) struct Bincoded(Vec<u8>);

impl Bincoded {
    /// Encode a value to a fresh `Bincoded` blob. Bincode's
    /// default config (fixed-int, little-endian, no varint) is
    /// the most compact form — fine for typed rows where we
    /// control both sides.
    pub(crate) fn encode<T: Serialize>(value: &T) -> Result<Self, StoreError> {
        let bytes = bincode::serialize(value)
            .map_err(|e| StoreError::Codec(format!("bincode encode: {e}")))?;
        Ok(Bincoded(bytes))
    }

    /// Decode a redb value-bytes slice back to its struct.
    /// Fails with `StoreError::Codec` on schema mismatch — the
    /// migration code uses that as the signal to walk the old
    /// row through an upgrade closure.
    pub(crate) fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, StoreError> {
        bincode::deserialize(bytes)
            .map_err(|e| StoreError::Codec(format!("bincode decode: {e}")))
    }

    pub(crate) fn as_slice(&self) -> &[u8] {
        &self.0
    }
}
