// This file is part of midnight-ledger.
// Copyright (C) 2025 Midnight Foundation
// SPDX-License-Identifier: Apache-2.0

//! Intent type for iOS bindings.

use crate::error::LedgerError;
use crate::unshielded::UnshieldedOffer;
use crate::util::to_hex_ser;
use base_crypto::signatures::Signature;
use base_crypto::time::Timestamp;
use ledger::structure::{Intent as LedgerIntent, ProofPreimageMarker};
use rand::rngs::OsRng;
use serialize::{tagged_deserialize, tagged_serialize};
use std::ops::Deref;
use std::sync::Arc;
use storage::arena::Sp;
use storage::db::InMemoryDB;
use transient_crypto::commitment::{Pedersen, PedersenRandomness, PureGeneratorPedersen};

// Type aliases for the different binding states
type PreBinding = PedersenRandomness;
type Binding = PureGeneratorPedersen;
type NoBinding = Pedersen;

/// The internal type state for Intent.
/// For iOS, we primarily work with unproven intents.
#[derive(Clone)]
pub(crate) enum IntentTypes {
    /// Unproven intent with signatures, pre-binding (for building)
    UnprovenWithSignaturePreBinding(
        LedgerIntent<Signature, ProofPreimageMarker, PreBinding, InMemoryDB>,
    ),
    /// Unproven intent with signatures, bound
    UnprovenWithSignatureBinding(
        LedgerIntent<Signature, ProofPreimageMarker, Binding, InMemoryDB>,
    ),
    /// Proof-erased intent (for deserialization/inspection)
    ProofErasedNoBinding(LedgerIntent<Signature, (), NoBinding, InMemoryDB>),
}

/// An intent represents a user's intention to perform actions on the ledger.
pub struct Intent {
    pub(crate) inner: IntentTypes,
}

impl Intent {
    /// Creates a new empty intent with the given TTL (time-to-live) in seconds since epoch.
    pub fn new(ttl_seconds: u64) -> Self {
        let ttl = Timestamp::from_secs(ttl_seconds);
        Intent {
            inner: IntentTypes::UnprovenWithSignaturePreBinding(LedgerIntent::new(
                &mut OsRng,
                None,
                None,
                vec![],
                vec![],
                vec![],
                None,
                ttl,
            )),
        }
    }

    /// Returns the TTL in seconds since epoch.
    pub fn ttl_seconds(&self) -> u64 {
        match &self.inner {
            IntentTypes::UnprovenWithSignaturePreBinding(i) => i.ttl.to_secs(),
            IntentTypes::UnprovenWithSignatureBinding(i) => i.ttl.to_secs(),
            IntentTypes::ProofErasedNoBinding(i) => i.ttl.to_secs(),
        }
    }

    /// Sets the TTL in seconds since epoch.
    pub fn set_ttl(&self, ttl_seconds: u64) -> Arc<Intent> {
        let ttl = Timestamp::from_secs(ttl_seconds);
        Arc::new(Intent {
            inner: match &self.inner {
                IntentTypes::UnprovenWithSignaturePreBinding(i) => {
                    let mut new_intent = i.clone();
                    new_intent.ttl = ttl;
                    IntentTypes::UnprovenWithSignaturePreBinding(new_intent)
                }
                IntentTypes::UnprovenWithSignatureBinding(i) => {
                    let mut new_intent = i.clone();
                    new_intent.ttl = ttl;
                    IntentTypes::UnprovenWithSignatureBinding(new_intent)
                }
                IntentTypes::ProofErasedNoBinding(i) => {
                    let mut new_intent = i.clone();
                    new_intent.ttl = ttl;
                    IntentTypes::ProofErasedNoBinding(new_intent)
                }
            },
        })
    }

    /// Returns the signature data for signing.
    /// The segment_id is the identifier for this segment within the transaction.
    pub fn signature_data(&self, segment_id: u16) -> Vec<u8> {
        match &self.inner {
            IntentTypes::UnprovenWithSignaturePreBinding(i) => {
                i.erase_proofs().erase_signatures().data_to_sign(segment_id)
            }
            IntentTypes::UnprovenWithSignatureBinding(i) => {
                i.erase_proofs().erase_signatures().data_to_sign(segment_id)
            }
            IntentTypes::ProofErasedNoBinding(i) => i.erase_signatures().data_to_sign(segment_id),
        }
    }

    /// Returns the intent hash for a given segment.
    pub fn intent_hash(&self, segment_id: u16) -> String {
        let hash = match &self.inner {
            IntentTypes::UnprovenWithSignaturePreBinding(i) => {
                i.erase_proofs().erase_signatures().intent_hash(segment_id)
            }
            IntentTypes::UnprovenWithSignatureBinding(i) => {
                i.erase_proofs().erase_signatures().intent_hash(segment_id)
            }
            IntentTypes::ProofErasedNoBinding(i) => i.erase_signatures().intent_hash(segment_id),
        };
        to_hex_ser(&hash)
    }

    /// Returns the guaranteed unshielded offer, if any.
    pub fn guaranteed_unshielded_offer(&self) -> Option<Arc<UnshieldedOffer>> {
        let offer = match &self.inner {
            IntentTypes::UnprovenWithSignaturePreBinding(i) => i
                .guaranteed_unshielded_offer
                .as_ref()
                .map(|sp| sp.deref().clone()),
            IntentTypes::UnprovenWithSignatureBinding(i) => i
                .guaranteed_unshielded_offer
                .as_ref()
                .map(|sp| sp.deref().clone()),
            IntentTypes::ProofErasedNoBinding(i) => i
                .guaranteed_unshielded_offer
                .as_ref()
                .map(|sp| sp.deref().clone()),
        };
        offer.map(|o| Arc::new(UnshieldedOffer { inner: o }))
    }

    /// Sets the guaranteed unshielded offer.
    pub fn set_guaranteed_unshielded_offer(
        &self,
        offer: Option<Arc<UnshieldedOffer>>,
    ) -> Result<Arc<Intent>, LedgerError> {
        let new_intent = match &self.inner {
            IntentTypes::UnprovenWithSignaturePreBinding(i) => {
                let mut new_i = i.clone();
                new_i.guaranteed_unshielded_offer = offer.map(|o| Sp::new(o.inner.clone()));
                IntentTypes::UnprovenWithSignaturePreBinding(new_i)
            }
            IntentTypes::UnprovenWithSignatureBinding(_) => {
                return Err(LedgerError::TransactionError(
                    "Cannot modify bound intent".to_string(),
                ))
            }
            IntentTypes::ProofErasedNoBinding(i) => {
                let mut new_i = i.clone();
                new_i.guaranteed_unshielded_offer = offer.map(|o| Sp::new(o.inner.clone()));
                IntentTypes::ProofErasedNoBinding(new_i)
            }
        };
        Ok(Arc::new(Intent { inner: new_intent }))
    }

    /// Returns the fallible unshielded offer, if any.
    pub fn fallible_unshielded_offer(&self) -> Option<Arc<UnshieldedOffer>> {
        let offer = match &self.inner {
            IntentTypes::UnprovenWithSignaturePreBinding(i) => i
                .fallible_unshielded_offer
                .as_ref()
                .map(|sp| sp.deref().clone()),
            IntentTypes::UnprovenWithSignatureBinding(i) => i
                .fallible_unshielded_offer
                .as_ref()
                .map(|sp| sp.deref().clone()),
            IntentTypes::ProofErasedNoBinding(i) => i
                .fallible_unshielded_offer
                .as_ref()
                .map(|sp| sp.deref().clone()),
        };
        offer.map(|o| Arc::new(UnshieldedOffer { inner: o }))
    }

    /// Sets the fallible unshielded offer.
    pub fn set_fallible_unshielded_offer(
        &self,
        offer: Option<Arc<UnshieldedOffer>>,
    ) -> Result<Arc<Intent>, LedgerError> {
        let new_intent = match &self.inner {
            IntentTypes::UnprovenWithSignaturePreBinding(i) => {
                let mut new_i = i.clone();
                new_i.fallible_unshielded_offer = offer.map(|o| Sp::new(o.inner.clone()));
                IntentTypes::UnprovenWithSignaturePreBinding(new_i)
            }
            IntentTypes::UnprovenWithSignatureBinding(_) => {
                return Err(LedgerError::TransactionError(
                    "Cannot modify bound intent".to_string(),
                ))
            }
            IntentTypes::ProofErasedNoBinding(i) => {
                let mut new_i = i.clone();
                new_i.fallible_unshielded_offer = offer.map(|o| Sp::new(o.inner.clone()));
                IntentTypes::ProofErasedNoBinding(new_i)
            }
        };
        Ok(Arc::new(Intent { inner: new_intent }))
    }

    /// Binds the intent with the given segment ID.
    /// Returns a bound intent that cannot be modified further (except for signatures).
    pub fn bind(&self, segment_id: u16) -> Result<Arc<Intent>, LedgerError> {
        if segment_id == 0 {
            return Err(LedgerError::TransactionError(
                "Segment ID cannot be 0".to_string(),
            ));
        }

        match &self.inner {
            IntentTypes::UnprovenWithSignaturePreBinding(i) => Ok(Arc::new(Intent {
                inner: IntentTypes::UnprovenWithSignatureBinding(i.seal(OsRng, segment_id)),
            })),
            IntentTypes::UnprovenWithSignatureBinding(_) => Err(LedgerError::TransactionError(
                "Intent is already bound".to_string(),
            )),
            IntentTypes::ProofErasedNoBinding(_) => Err(LedgerError::TransactionError(
                "Cannot bind proof-erased intent".to_string(),
            )),
        }
    }

    /// Serializes the intent to bytes.
    pub fn serialize(&self) -> Result<Vec<u8>, LedgerError> {
        let mut buf = Vec::new();
        match &self.inner {
            IntentTypes::UnprovenWithSignaturePreBinding(i) => tagged_serialize(i, &mut buf)?,
            IntentTypes::UnprovenWithSignatureBinding(i) => tagged_serialize(i, &mut buf)?,
            IntentTypes::ProofErasedNoBinding(i) => tagged_serialize(i, &mut buf)?,
        }
        Ok(buf)
    }

    /// Deserializes an intent from bytes (as proof-erased form).
    pub fn deserialize(raw: Vec<u8>) -> Result<Self, LedgerError> {
        // Try to deserialize as proof-erased first (most common for received intents)
        let inner: LedgerIntent<Signature, (), NoBinding, InMemoryDB> =
            tagged_deserialize(&mut &raw[..])?;
        Ok(Intent {
            inner: IntentTypes::ProofErasedNoBinding(inner),
        })
    }

    /// Returns a debug string representation.
    pub fn to_debug_string(&self) -> String {
        match &self.inner {
            IntentTypes::UnprovenWithSignaturePreBinding(i) => format!("{:#?}", i),
            IntentTypes::UnprovenWithSignatureBinding(i) => format!("{:#?}", i),
            IntentTypes::ProofErasedNoBinding(i) => format!("{:#?}", i),
        }
    }
}
