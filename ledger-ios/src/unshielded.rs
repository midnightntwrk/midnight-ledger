// This file is part of midnight-ledger.
// Copyright (C) 2025 Midnight Foundation
// SPDX-License-Identifier: Apache-2.0

//! Unshielded offer types for iOS bindings.

use crate::error::LedgerError;
use crate::util::{from_hex_ser, to_hex_ser};
use base_crypto::signatures::Signature;
use coin_structure::coin::UnshieldedTokenType;
use ledger::structure::{
    IntentHash, UnshieldedOffer as LedgerUnshieldedOffer, UtxoOutput as LedgerUtxoOutput,
    UtxoSpend as LedgerUtxoSpend,
};
use serialize::{tagged_deserialize, tagged_serialize};
use std::sync::Arc;
use storage::db::InMemoryDB;

/// A UTXO spend (input to an unshielded offer).
pub struct UtxoSpend {
    pub(crate) inner: LedgerUtxoSpend,
}

impl UtxoSpend {
    /// Creates a new UTXO spend.
    pub fn new(
        value: String,
        owner: String,
        token_type: String,
        intent_hash: String,
        output_no: u32,
    ) -> Result<Self, LedgerError> {
        let value: u128 = value.parse().map_err(|_| LedgerError::InvalidData)?;
        let owner = from_hex_ser(&owner)?;
        let token_type: UnshieldedTokenType = from_hex_ser(&token_type)?;
        let intent_hash: IntentHash = from_hex_ser(&intent_hash)?;

        Ok(UtxoSpend {
            inner: LedgerUtxoSpend {
                value,
                owner,
                type_: token_type,
                intent_hash,
                output_no,
            },
        })
    }

    /// Returns the value as a string (u128 serialized).
    pub fn value(&self) -> String {
        self.inner.value.to_string()
    }

    /// Returns the owner as a hex-encoded string.
    pub fn owner(&self) -> String {
        to_hex_ser(&self.inner.owner)
    }

    /// Returns the token type as a hex-encoded string.
    pub fn token_type(&self) -> String {
        to_hex_ser(&self.inner.type_)
    }

    /// Returns the intent hash as a hex-encoded string.
    pub fn intent_hash(&self) -> String {
        to_hex_ser(&self.inner.intent_hash)
    }

    /// Returns the output number.
    pub fn output_no(&self) -> u32 {
        self.inner.output_no
    }

    /// Serializes the UTXO spend to bytes.
    pub fn serialize(&self) -> Result<Vec<u8>, LedgerError> {
        let mut buf = Vec::new();
        tagged_serialize(&self.inner, &mut buf)?;
        Ok(buf)
    }

    /// Deserializes a UTXO spend from bytes.
    pub fn deserialize(raw: Vec<u8>) -> Result<Self, LedgerError> {
        let inner: LedgerUtxoSpend = tagged_deserialize(&mut &raw[..])?;
        Ok(UtxoSpend { inner })
    }
}

/// A UTXO output (output of an unshielded offer).
pub struct UtxoOutput {
    pub(crate) inner: LedgerUtxoOutput,
}

impl UtxoOutput {
    /// Creates a new UTXO output.
    pub fn new(value: String, owner: String, token_type: String) -> Result<Self, LedgerError> {
        let value: u128 = value.parse().map_err(|_| LedgerError::InvalidData)?;
        let owner = from_hex_ser(&owner)?;
        let token_type: UnshieldedTokenType = from_hex_ser(&token_type)?;

        Ok(UtxoOutput {
            inner: LedgerUtxoOutput {
                value,
                owner,
                type_: token_type,
            },
        })
    }

    /// Returns the value as a string (u128 serialized).
    pub fn value(&self) -> String {
        self.inner.value.to_string()
    }

    /// Returns the owner as a hex-encoded string.
    pub fn owner(&self) -> String {
        to_hex_ser(&self.inner.owner)
    }

    /// Returns the token type as a hex-encoded string.
    pub fn token_type(&self) -> String {
        to_hex_ser(&self.inner.type_)
    }

    /// Serializes the UTXO output to bytes.
    pub fn serialize(&self) -> Result<Vec<u8>, LedgerError> {
        let mut buf = Vec::new();
        tagged_serialize(&self.inner, &mut buf)?;
        Ok(buf)
    }

    /// Deserializes a UTXO output from bytes.
    pub fn deserialize(raw: Vec<u8>) -> Result<Self, LedgerError> {
        let inner: LedgerUtxoOutput = tagged_deserialize(&mut &raw[..])?;
        Ok(UtxoOutput { inner })
    }
}

/// An unshielded offer containing UTXO spends and outputs.
pub struct UnshieldedOffer {
    pub(crate) inner: LedgerUnshieldedOffer<Signature, InMemoryDB>,
}

impl UnshieldedOffer {
    /// Creates a new unshielded offer with the given inputs, outputs, and signatures.
    pub fn new(
        inputs: Vec<Arc<UtxoSpend>>,
        outputs: Vec<Arc<UtxoOutput>>,
        signatures: Vec<String>,
    ) -> Result<Self, LedgerError> {
        let mut inputs_vec: Vec<LedgerUtxoSpend> =
            inputs.iter().map(|i| i.inner.clone()).collect();
        let mut outputs_vec: Vec<LedgerUtxoOutput> =
            outputs.iter().map(|o| o.inner.clone()).collect();
        let mut sigs: Vec<Signature> = signatures
            .into_iter()
            .map(|s| from_hex_ser(&s))
            .collect::<Result<Vec<_>, _>>()?;

        // Sort inputs and outputs to ensure deterministic ordering
        if sigs.len() == inputs_vec.len() {
            // Sort signatures along with inputs
            let mut input_sigs: Vec<_> = inputs_vec.iter().cloned().zip(sigs).collect();
            input_sigs.sort_by(|a, b| a.0.cmp(&b.0));
            sigs = input_sigs.into_iter().map(|(_, s)| s).collect();
        }
        inputs_vec.sort();
        outputs_vec.sort();

        Ok(UnshieldedOffer {
            inner: LedgerUnshieldedOffer {
                inputs: inputs_vec.into_iter().collect(),
                outputs: outputs_vec.into_iter().collect(),
                signatures: sigs.into_iter().collect(),
            },
        })
    }

    /// Creates an unshielded offer without signatures (signature-erased form).
    pub fn new_unsigned(
        inputs: Vec<Arc<UtxoSpend>>,
        outputs: Vec<Arc<UtxoOutput>>,
    ) -> Result<Self, LedgerError> {
        Self::new(inputs, outputs, vec![])
    }

    /// Returns the inputs.
    pub fn inputs(&self) -> Vec<Arc<UtxoSpend>> {
        self.inner
            .inputs
            .iter_deref()
            .map(|i| Arc::new(UtxoSpend { inner: i.clone() }))
            .collect()
    }

    /// Returns the outputs.
    pub fn outputs(&self) -> Vec<Arc<UtxoOutput>> {
        self.inner
            .outputs
            .iter_deref()
            .map(|o| Arc::new(UtxoOutput { inner: o.clone() }))
            .collect()
    }

    /// Returns the signatures as hex-encoded strings.
    pub fn signatures(&self) -> Vec<String> {
        self.inner
            .signatures
            .iter_deref()
            .map(|s| to_hex_ser(&s))
            .collect()
    }

    /// Adds signatures to the offer.
    pub fn add_signatures(&self, signatures: Vec<String>) -> Result<Arc<UnshieldedOffer>, LedgerError> {
        let sigs: Vec<Signature> = signatures
            .into_iter()
            .map(|s| from_hex_ser(&s))
            .collect::<Result<Vec<_>, _>>()?;

        let mut new_inner = self.inner.clone();
        new_inner.add_signatures(sigs);

        Ok(Arc::new(UnshieldedOffer { inner: new_inner }))
    }

    /// Serializes the offer to bytes.
    pub fn serialize(&self) -> Result<Vec<u8>, LedgerError> {
        let mut buf = Vec::new();
        tagged_serialize(&self.inner, &mut buf)?;
        Ok(buf)
    }

    /// Deserializes an offer from bytes.
    pub fn deserialize(raw: Vec<u8>) -> Result<Self, LedgerError> {
        let inner: LedgerUnshieldedOffer<Signature, InMemoryDB> =
            tagged_deserialize(&mut &raw[..])?;
        Ok(UnshieldedOffer { inner })
    }

    /// Returns a debug string representation.
    pub fn to_debug_string(&self) -> String {
        format!("{:#?}", &self.inner)
    }
}
