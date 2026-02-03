// This file is part of midnight-ledger.
// Copyright (C) 2025 Midnight Foundation
// SPDX-License-Identifier: Apache-2.0

//! Transaction type for iOS bindings.

use crate::error::LedgerError;
use crate::intent::{Intent, IntentTypes};
use crate::util::to_hex_ser;
use base_crypto::signatures::Signature;
use ledger::structure::{
    ProofMarker, ProofPreimageMarker, Transaction as LedgerTransaction,
};
use rand::rngs::OsRng;
use serialize::{tagged_deserialize, tagged_serialize};
use std::collections::HashMap;
use std::sync::Arc;
use storage::db::InMemoryDB;
use transient_crypto::commitment::{Pedersen, PedersenRandomness, PureGeneratorPedersen};

// Type aliases for the different binding states
type PreBinding = PedersenRandomness;
type Binding = PureGeneratorPedersen;
type NoBinding = Pedersen;

/// The internal type state for Transaction.
#[derive(Clone)]
pub(crate) enum TransactionTypes {
    /// Unproven transaction with signatures, pre-binding (for building)
    UnprovenWithSignaturePreBinding(
        LedgerTransaction<Signature, ProofPreimageMarker, PreBinding, InMemoryDB>,
    ),
    /// Unproven transaction with signatures, bound
    UnprovenWithSignatureBinding(
        LedgerTransaction<Signature, ProofPreimageMarker, Binding, InMemoryDB>,
    ),
    /// Proven transaction with signatures, bound (after mock_prove)
    ProvenWithSignatureBinding(
        LedgerTransaction<Signature, ProofMarker, Binding, InMemoryDB>,
    ),
    /// Proof-erased transaction (for deserialization/inspection)
    ProofErasedNoBinding(LedgerTransaction<Signature, (), NoBinding, InMemoryDB>),
    /// Proof-erased, signature-erased transaction
    ProofErasedSignatureErasedNoBinding(LedgerTransaction<(), (), NoBinding, InMemoryDB>),
}

/// A transaction on the Midnight ledger.
pub struct Transaction {
    pub(crate) inner: TransactionTypes,
}

impl Transaction {
    /// Creates a new transaction from parts.
    ///
    /// - `network_id`: The network identifier string
    /// - `intent`: Optional intent for this transaction
    pub fn from_parts(network_id: String, intent: Option<Arc<Intent>>) -> Result<Self, LedgerError> {
        let intents: HashMap<u16, _> = if let Some(intent) = intent {
            let inner_intent = match &intent.inner {
                IntentTypes::UnprovenWithSignaturePreBinding(i) => i.clone(),
                _ => {
                    return Err(LedgerError::TransactionError(
                        "Intent must be unproven and pre-bound".to_string(),
                    ))
                }
            };
            [(1u16, inner_intent)].into_iter().collect()
        } else {
            HashMap::new()
        };

        Ok(Transaction {
            inner: TransactionTypes::UnprovenWithSignaturePreBinding(LedgerTransaction::new(
                network_id,
                intents.into_iter().collect(),
                None, // guaranteed_coins
                HashMap::new(), // fallible_coins
            )),
        })
    }

    /// Creates a new transaction with a random segment ID.
    pub fn from_parts_randomized(
        network_id: String,
        intent: Option<Arc<Intent>>,
    ) -> Result<Self, LedgerError> {
        use rand::Rng;

        let segment_id = OsRng.gen_range(2..u16::MAX);
        let intents: HashMap<u16, _> = if let Some(intent) = intent {
            let inner_intent = match &intent.inner {
                IntentTypes::UnprovenWithSignaturePreBinding(i) => i.clone(),
                _ => {
                    return Err(LedgerError::TransactionError(
                        "Intent must be unproven and pre-bound".to_string(),
                    ))
                }
            };
            [(segment_id, inner_intent)].into_iter().collect()
        } else {
            HashMap::new()
        };

        Ok(Transaction {
            inner: TransactionTypes::UnprovenWithSignaturePreBinding(LedgerTransaction::new(
                network_id,
                intents.into_iter().collect(),
                None,
                HashMap::new(),
            )),
        })
    }

    /// Returns the network ID.
    pub fn network_id(&self) -> String {
        match &self.inner {
            TransactionTypes::UnprovenWithSignaturePreBinding(
                LedgerTransaction::Standard(tx),
            ) => tx.network_id.clone(),
            TransactionTypes::UnprovenWithSignatureBinding(LedgerTransaction::Standard(tx)) => {
                tx.network_id.clone()
            }
            TransactionTypes::ProvenWithSignatureBinding(LedgerTransaction::Standard(tx)) => {
                tx.network_id.clone()
            }
            TransactionTypes::ProofErasedNoBinding(LedgerTransaction::Standard(tx)) => {
                tx.network_id.clone()
            }
            TransactionTypes::ProofErasedSignatureErasedNoBinding(
                LedgerTransaction::Standard(tx),
            ) => tx.network_id.clone(),
            _ => String::new(), // ClaimRewards transactions
        }
    }

    /// Binds the transaction (seals it with random binding values).
    pub fn bind(&self) -> Result<Arc<Transaction>, LedgerError> {
        match &self.inner {
            TransactionTypes::UnprovenWithSignaturePreBinding(tx) => Ok(Arc::new(Transaction {
                inner: TransactionTypes::UnprovenWithSignatureBinding(tx.seal(OsRng)),
            })),
            TransactionTypes::UnprovenWithSignatureBinding(_) => Err(LedgerError::TransactionError(
                "Transaction is already bound".to_string(),
            )),
            TransactionTypes::ProvenWithSignatureBinding(_) => Err(LedgerError::TransactionError(
                "Transaction is already bound".to_string(),
            )),
            TransactionTypes::ProofErasedNoBinding(_)
            | TransactionTypes::ProofErasedSignatureErasedNoBinding(_) => {
                Err(LedgerError::TransactionError(
                    "Cannot bind proof-erased transaction".to_string(),
                ))
            }
        }
    }

    /// Mock proves the transaction (produces a transaction that won't verify but is accurate for fee computation).
    pub fn mock_prove(&self) -> Result<Arc<Transaction>, LedgerError> {
        match &self.inner {
            TransactionTypes::UnprovenWithSignaturePreBinding(tx) => {
                let proven = tx.mock_prove().map_err(|e| {
                    LedgerError::TransactionError(format!("Mock prove failed: {:?}", e))
                })?;
                Ok(Arc::new(Transaction {
                    inner: TransactionTypes::ProvenWithSignatureBinding(proven),
                }))
            }
            TransactionTypes::UnprovenWithSignatureBinding(_) => Err(LedgerError::TransactionError(
                "Cannot prove bound transaction".to_string(),
            )),
            TransactionTypes::ProvenWithSignatureBinding(_) => Err(LedgerError::TransactionError(
                "Transaction is already proven".to_string(),
            )),
            TransactionTypes::ProofErasedNoBinding(_)
            | TransactionTypes::ProofErasedSignatureErasedNoBinding(_) => {
                Err(LedgerError::TransactionError(
                    "Cannot prove proof-erased transaction".to_string(),
                ))
            }
        }
    }

    /// Merges this transaction with another.
    pub fn merge(&self, other: Arc<Transaction>) -> Result<Arc<Transaction>, LedgerError> {
        use TransactionTypes::*;

        let merged = match (&self.inner, &other.inner) {
            (UnprovenWithSignaturePreBinding(tx1), UnprovenWithSignaturePreBinding(tx2)) => {
                UnprovenWithSignaturePreBinding(
                    tx1.merge(tx2)
                        .map_err(|e| LedgerError::TransactionError(format!("{:?}", e)))?,
                )
            }
            (UnprovenWithSignatureBinding(tx1), UnprovenWithSignatureBinding(tx2)) => {
                UnprovenWithSignatureBinding(
                    tx1.merge(tx2)
                        .map_err(|e| LedgerError::TransactionError(format!("{:?}", e)))?,
                )
            }
            (ProvenWithSignatureBinding(tx1), ProvenWithSignatureBinding(tx2)) => {
                ProvenWithSignatureBinding(
                    tx1.merge(tx2)
                        .map_err(|e| LedgerError::TransactionError(format!("{:?}", e)))?,
                )
            }
            (ProofErasedNoBinding(tx1), ProofErasedNoBinding(tx2)) => ProofErasedNoBinding(
                tx1.merge(tx2)
                    .map_err(|e| LedgerError::TransactionError(format!("{:?}", e)))?,
            ),
            (ProofErasedSignatureErasedNoBinding(tx1), ProofErasedSignatureErasedNoBinding(tx2)) => {
                ProofErasedSignatureErasedNoBinding(
                    tx1.merge(tx2)
                        .map_err(|e| LedgerError::TransactionError(format!("{:?}", e)))?,
                )
            }
            _ => {
                return Err(LedgerError::TransactionError(
                    "Cannot merge transactions of different types".to_string(),
                ))
            }
        };

        Ok(Arc::new(Transaction { inner: merged }))
    }

    /// Returns the transaction identifiers.
    pub fn identifiers(&self) -> Result<Vec<String>, LedgerError> {
        let ids: Vec<String> = match &self.inner {
            TransactionTypes::UnprovenWithSignaturePreBinding(tx) => {
                tx.identifiers().map(|id| to_hex_ser(&id)).collect()
            }
            TransactionTypes::UnprovenWithSignatureBinding(tx) => {
                tx.identifiers().map(|id| to_hex_ser(&id)).collect()
            }
            TransactionTypes::ProvenWithSignatureBinding(tx) => {
                tx.identifiers().map(|id| to_hex_ser(&id)).collect()
            }
            TransactionTypes::ProofErasedNoBinding(tx) => {
                tx.identifiers().map(|id| to_hex_ser(&id)).collect()
            }
            TransactionTypes::ProofErasedSignatureErasedNoBinding(tx) => {
                tx.identifiers().map(|id| to_hex_ser(&id)).collect()
            }
        };
        Ok(ids)
    }

    /// Erases the proofs from the transaction.
    pub fn erase_proofs(&self) -> Arc<Transaction> {
        Arc::new(Transaction {
            inner: match &self.inner {
                TransactionTypes::UnprovenWithSignaturePreBinding(tx) => {
                    TransactionTypes::ProofErasedNoBinding(tx.erase_proofs())
                }
                TransactionTypes::UnprovenWithSignatureBinding(tx) => {
                    TransactionTypes::ProofErasedNoBinding(tx.erase_proofs())
                }
                TransactionTypes::ProvenWithSignatureBinding(tx) => {
                    TransactionTypes::ProofErasedNoBinding(tx.erase_proofs())
                }
                TransactionTypes::ProofErasedNoBinding(tx) => {
                    TransactionTypes::ProofErasedNoBinding(tx.erase_proofs())
                }
                TransactionTypes::ProofErasedSignatureErasedNoBinding(tx) => {
                    TransactionTypes::ProofErasedSignatureErasedNoBinding(tx.erase_proofs())
                }
            },
        })
    }

    /// Erases the signatures from the transaction.
    pub fn erase_signatures(&self) -> Arc<Transaction> {
        Arc::new(Transaction {
            inner: match &self.inner {
                TransactionTypes::UnprovenWithSignaturePreBinding(tx) => {
                    TransactionTypes::ProofErasedSignatureErasedNoBinding(
                        tx.erase_proofs().erase_signatures(),
                    )
                }
                TransactionTypes::UnprovenWithSignatureBinding(tx) => {
                    TransactionTypes::ProofErasedSignatureErasedNoBinding(
                        tx.erase_proofs().erase_signatures(),
                    )
                }
                TransactionTypes::ProvenWithSignatureBinding(tx) => {
                    TransactionTypes::ProofErasedSignatureErasedNoBinding(
                        tx.erase_proofs().erase_signatures(),
                    )
                }
                TransactionTypes::ProofErasedNoBinding(tx) => {
                    TransactionTypes::ProofErasedSignatureErasedNoBinding(tx.erase_signatures())
                }
                TransactionTypes::ProofErasedSignatureErasedNoBinding(tx) => {
                    TransactionTypes::ProofErasedSignatureErasedNoBinding(tx.erase_signatures())
                }
            },
        })
    }

    /// Serializes the transaction to bytes.
    pub fn serialize(&self) -> Result<Vec<u8>, LedgerError> {
        let mut buf = Vec::new();
        match &self.inner {
            TransactionTypes::UnprovenWithSignaturePreBinding(tx) => {
                tagged_serialize(tx, &mut buf)?
            }
            TransactionTypes::UnprovenWithSignatureBinding(tx) => tagged_serialize(tx, &mut buf)?,
            TransactionTypes::ProvenWithSignatureBinding(tx) => tagged_serialize(tx, &mut buf)?,
            TransactionTypes::ProofErasedNoBinding(tx) => tagged_serialize(tx, &mut buf)?,
            TransactionTypes::ProofErasedSignatureErasedNoBinding(tx) => {
                tagged_serialize(tx, &mut buf)?
            }
        }
        Ok(buf)
    }

    /// Deserializes a transaction from bytes (as proof-erased, signature-erased form).
    pub fn deserialize(raw: Vec<u8>) -> Result<Self, LedgerError> {
        // Try to deserialize as proof-erased, signature-erased first (most common for received transactions)
        let inner: LedgerTransaction<(), (), NoBinding, InMemoryDB> =
            tagged_deserialize(&mut &raw[..])?;
        Ok(Transaction {
            inner: TransactionTypes::ProofErasedSignatureErasedNoBinding(inner),
        })
    }

    /// Deserializes a transaction with specific type markers.
    /// - signature_marker: "signature" or "erased"
    /// - proof_marker: "unproven", "proven", or "erased"
    /// - binding_marker: "pre", "bound", or "none"
    pub fn deserialize_typed(
        signature_marker: String,
        proof_marker: String,
        binding_marker: String,
        raw: Vec<u8>,
    ) -> Result<Self, LedgerError> {
        match (
            signature_marker.as_str(),
            proof_marker.as_str(),
            binding_marker.as_str(),
        ) {
            ("signature", "unproven", "pre") => {
                let inner: LedgerTransaction<Signature, ProofPreimageMarker, PreBinding, InMemoryDB> =
                    tagged_deserialize(&mut &raw[..])?;
                Ok(Transaction {
                    inner: TransactionTypes::UnprovenWithSignaturePreBinding(inner),
                })
            }
            ("signature", "unproven", "bound") => {
                let inner: LedgerTransaction<Signature, ProofPreimageMarker, Binding, InMemoryDB> =
                    tagged_deserialize(&mut &raw[..])?;
                Ok(Transaction {
                    inner: TransactionTypes::UnprovenWithSignatureBinding(inner),
                })
            }
            ("signature", "proven", "bound") => {
                let inner: LedgerTransaction<Signature, ProofMarker, Binding, InMemoryDB> =
                    tagged_deserialize(&mut &raw[..])?;
                Ok(Transaction {
                    inner: TransactionTypes::ProvenWithSignatureBinding(inner),
                })
            }
            ("signature", "erased", "none") => {
                let inner: LedgerTransaction<Signature, (), NoBinding, InMemoryDB> =
                    tagged_deserialize(&mut &raw[..])?;
                Ok(Transaction {
                    inner: TransactionTypes::ProofErasedNoBinding(inner),
                })
            }
            ("erased", "erased", "none") => {
                let inner: LedgerTransaction<(), (), NoBinding, InMemoryDB> =
                    tagged_deserialize(&mut &raw[..])?;
                Ok(Transaction {
                    inner: TransactionTypes::ProofErasedSignatureErasedNoBinding(inner),
                })
            }
            _ => Err(LedgerError::TransactionError(format!(
                "Unsupported transaction type: sig={}, proof={}, bind={}",
                signature_marker, proof_marker, binding_marker
            ))),
        }
    }

    /// Returns a debug string representation.
    pub fn to_debug_string(&self) -> String {
        match &self.inner {
            TransactionTypes::UnprovenWithSignaturePreBinding(tx) => format!("{:#?}", tx),
            TransactionTypes::UnprovenWithSignatureBinding(tx) => format!("{:#?}", tx),
            TransactionTypes::ProvenWithSignatureBinding(tx) => format!("{:#?}", tx),
            TransactionTypes::ProofErasedNoBinding(tx) => format!("{:#?}", tx),
            TransactionTypes::ProofErasedSignatureErasedNoBinding(tx) => format!("{:#?}", tx),
        }
    }
}
