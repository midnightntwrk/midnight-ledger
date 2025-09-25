// This file is part of midnight-ledger.
// Copyright (C) 2025 Midnight Foundation
// SPDX-License-Identifier: Apache-2.0
// Licensed under the Apache License, Version 2.0 (the "License");
// You may not use this file except in compliance with the License.
// You may obtain a copy of the License at
// http://www.apache.org/licenses/LICENSE-2.0
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License

use base_crypto::hash::HashOutput;
use base_crypto::repr::BinaryHashRepr;

use crate::FfiError;

// TransactionHash
#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct TransactionHash {
    pub hash: Vec<u8>,
}

impl From<ledger::structure::TransactionHash> for TransactionHash {
    fn from(th: ledger::structure::TransactionHash) -> Self {
        Self {
            hash: th.0.0.to_vec(),
        }
    }
}

impl From<TransactionHash> for ledger::structure::TransactionHash {
    fn from(th: TransactionHash) -> Self {
        let mut hash_bytes = [0u8; 32];
        hash_bytes.copy_from_slice(&th.hash[..32]);
        Self(HashOutput(hash_bytes))
    }
}

// IntentHash
#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct IntentHash {
    pub hash: Vec<u8>,
}

impl From<ledger::structure::IntentHash> for IntentHash {
    fn from(ih: ledger::structure::IntentHash) -> Self {
        Self {
            hash: ih.0.0.to_vec(),
        }
    }
}

impl From<IntentHash> for ledger::structure::IntentHash {
    fn from(ih: IntentHash) -> Self {
        let mut hash_bytes = [0u8; 32];
        hash_bytes.copy_from_slice(&ih.hash[..32]);
        Self(HashOutput(hash_bytes))
    }
}

// UtxoOutput
#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct UtxoOutput {
    pub value: i64, // Using i64 instead of u128 for UniFFI compatibility
    pub owner: crate::objects::token_types::UserAddress,
    pub token_type: crate::objects::token_types::UnshieldedTokenType,
}

impl From<ledger::structure::UtxoOutput> for UtxoOutput {
    fn from(utxo: ledger::structure::UtxoOutput) -> Self {
        Self {
            value: utxo.value as i64, // Convert u128 to i64
            owner: utxo.owner.into(),
            token_type: utxo.type_.into(),
        }
    }
}

impl From<UtxoOutput> for ledger::structure::UtxoOutput {
    fn from(utxo: UtxoOutput) -> Self {
        Self {
            value: utxo.value as u128, // Convert i64 to u128
            owner: utxo.owner.into(),
            type_: utxo.token_type.into(),
        }
    }
}

// UtxoSpend
#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct UtxoSpend {
    pub value: i64,     // Using i64 instead of u128 for UniFFI compatibility
    pub owner: Vec<u8>, // VerifyingKey as bytes
    pub token_type: crate::objects::token_types::UnshieldedTokenType,
    pub intent_hash: IntentHash,
    pub output_no: u32,
}

impl From<ledger::structure::UtxoSpend> for UtxoSpend {
    fn from(utxo: ledger::structure::UtxoSpend) -> Self {
        Self {
            value: utxo.value as i64, // Convert u128 to i64
            owner: utxo.owner.binary_vec(),
            token_type: utxo.type_.into(),
            intent_hash: utxo.intent_hash.into(),
            output_no: utxo.output_no,
        }
    }
}

impl From<UtxoSpend> for ledger::structure::UtxoSpend {
    fn from(utxo: UtxoSpend) -> Self {
        use base_crypto::signatures::VerifyingKey;
        use rand::Rng;

        // For now, we'll create a random VerifyingKey since we can't easily deserialize from bytes
        // In a real implementation, you'd need proper deserialization
        let verifying_key: VerifyingKey = rand::thread_rng().r#gen();

        Self {
            value: utxo.value as u128, // Convert i64 to u128
            owner: verifying_key,
            type_: utxo.token_type.into(),
            intent_hash: utxo.intent_hash.into(),
            output_no: utxo.output_no,
        }
    }
}

// OutputInstructionShielded
#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct OutputInstructionShielded {
    pub amount: i64, // Using i64 instead of u128 for UniFFI compatibility
    pub target_key: crate::objects::token_types::PublicKey,
}

impl From<ledger::structure::OutputInstructionShielded> for OutputInstructionShielded {
    fn from(ois: ledger::structure::OutputInstructionShielded) -> Self {
        Self {
            amount: ois.amount as i64, // Convert u128 to i64
            target_key: ois.target_key.into(),
        }
    }
}

impl From<OutputInstructionShielded> for ledger::structure::OutputInstructionShielded {
    fn from(ois: OutputInstructionShielded) -> Self {
        Self {
            amount: ois.amount as u128, // Convert i64 to u128
            target_key: ois.target_key.into(),
        }
    }
}

// OutputInstructionUnshielded
#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct OutputInstructionUnshielded {
    pub amount: i64, // Using i64 instead of u128 for UniFFI compatibility
    pub target_address: crate::objects::token_types::UserAddress,
    pub nonce: crate::objects::token_types::Nonce,
}

impl From<ledger::structure::OutputInstructionUnshielded> for OutputInstructionUnshielded {
    fn from(oiu: ledger::structure::OutputInstructionUnshielded) -> Self {
        Self {
            amount: oiu.amount as i64, // Convert u128 to i64
            target_address: oiu.target_address.into(),
            nonce: oiu.nonce.into(),
        }
    }
}

impl From<OutputInstructionUnshielded> for ledger::structure::OutputInstructionUnshielded {
    fn from(oiu: OutputInstructionUnshielded) -> Self {
        Self {
            amount: oiu.amount as u128, // Convert i64 to u128
            target_address: oiu.target_address.into(),
            nonce: oiu.nonce.into(),
        }
    }
}

// ClaimKind
#[derive(uniffi::Enum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaimKind {
    Reward,
    CardanoBridge,
}

impl From<ledger::structure::ClaimKind> for ClaimKind {
    fn from(ck: ledger::structure::ClaimKind) -> Self {
        match ck {
            ledger::structure::ClaimKind::Reward => ClaimKind::Reward,
            ledger::structure::ClaimKind::CardanoBridge => ClaimKind::CardanoBridge,
        }
    }
}

impl From<ClaimKind> for ledger::structure::ClaimKind {
    fn from(ck: ClaimKind) -> Self {
        match ck {
            ClaimKind::Reward => ledger::structure::ClaimKind::Reward,
            ClaimKind::CardanoBridge => ledger::structure::ClaimKind::CardanoBridge,
        }
    }
}

// Helper functions
#[uniffi::export]
pub fn transaction_hash_from_bytes(bytes: Vec<u8>) -> Result<TransactionHash, FfiError> {
    if bytes.len() != 32 {
        return Err(FfiError::InvalidInput {
            details: format!("Expected 32 bytes, got {}", bytes.len()),
        });
    }
    Ok(TransactionHash { hash: bytes })
}

#[uniffi::export]
pub fn intent_hash_from_bytes(bytes: Vec<u8>) -> Result<IntentHash, FfiError> {
    if bytes.len() != 32 {
        return Err(FfiError::InvalidInput {
            details: format!("Expected 32 bytes, got {}", bytes.len()),
        });
    }
    Ok(IntentHash { hash: bytes })
}

impl TransactionHash {
    pub fn to_bytes(&self) -> Vec<u8> {
        self.hash.clone()
    }
}

impl IntentHash {
    pub fn to_bytes(&self) -> Vec<u8> {
        self.hash.clone()
    }
}
