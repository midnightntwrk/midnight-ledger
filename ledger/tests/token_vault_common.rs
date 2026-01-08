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
// limitations under the License.

//! Token Vault Test Utilities
//!
//! **REFERENCE IMPLEMENTATION ONLY**
//! This code is provided for educational and testing purposes to demonstrate
//! Midnight ledger features. DO NOT use this code as-is in production without
//! proper security review, auditing, and hardening.
//!
//! This module contains shared helper functions and utilities used by both
//! the shielded and unshielded token vault tests. It provides:
//!
//! - Common imports and re-exports
//! - Helper functions for building transcripts
//! - Op sequence generators for unshielded operations
//! - Contract state layout constants
//!
//! ## Architecture Overview
//!
//! The Midnight ledger supports two types of tokens:
//!
//! 1. **Shielded tokens**: Privacy-preserving tokens using ZK proofs. These use
//!    ZSwap for private transfers with committed values and nullifiers.
//!
//! 2. **Unshielded tokens**: Transparent tokens stored as UTXOs (Unspent Transaction
//!    Outputs) similar to Bitcoin. These use the UTXO model for transfers.
//!
//! ## Test Configuration
//!
//! Tests require the `MIDNIGHT_LEDGER_TEST_STATIC_DIR` environment variable
//! to be set to the tests directory containing verifier keys:
//!
//! ```bash
//! MIDNIGHT_LEDGER_TEST_STATIC_DIR=/path/to/ledger/tests cargo test --test token-vault-shielded
//! MIDNIGHT_LEDGER_TEST_STATIC_DIR=/path/to/ledger/tests cargo test --test token-vault-unshielded
//! ```

#![allow(dead_code)]

// ============================================================================
// Common Imports
// ============================================================================

pub use base_crypto::fab::{AlignedValue, Value};
pub use base_crypto::hash::{HashOutput, persistent_commit};
pub use base_crypto::rng::SplittableRng;
pub use base_crypto::signatures::Signature;
pub use base_crypto::time::Timestamp;
pub use coin_structure::coin::{
    Info as CoinInfo, QualifiedInfo as QualifiedCoinInfo, NIGHT, UserAddress,
    TokenType, UnshieldedTokenType,
};
pub use coin_structure::contract::ContractAddress;
pub use coin_structure::transfer::{Recipient, SenderEvidence};
pub use futures::FutureExt;
pub use lazy_static::lazy_static;
pub use midnight_ledger::construct::{ContractCallPrototype, PreTranscript, partition_transcripts};
pub use midnight_ledger::error::{EffectsCheckError, MalformedTransaction};
pub use midnight_ledger::semantics::{ErasedTransactionResult::Success, ZswapLocalStateExt};
pub use midnight_ledger::structure::{
    ContractDeploy, INITIAL_PARAMETERS, Intent, IntentHash, LedgerState, ProofPreimageMarker, Transaction,
    UnshieldedOffer, UtxoOutput, UtxoSpend,
};
pub use midnight_ledger::test_utilities::{Resolver, verifier_key};
pub use midnight_ledger::test_utilities::{TestState, tx_prove_bind};
pub use midnight_ledger::test_utilities::{Tx, TxBound};
pub use midnight_ledger::test_utilities::{test_intents, test_resolver};
pub use midnight_ledger::verify::WellFormedStrictness;
pub use onchain_runtime::context::QueryContext;
pub use onchain_runtime::ops::{Key, Op, key};
pub use onchain_runtime::program_fragments::*;
pub use onchain_runtime::result_mode::{ResultModeGather, ResultModeVerify};
pub use onchain_runtime::state::{ContractOperation, ContractState, StateValue, stval};
pub use rand::rngs::StdRng;
pub use rand::{CryptoRng, Rng, SeedableRng};
pub use serialize::Serializable;
pub use std::borrow::Cow;
pub use storage::arena::Sp;
pub use storage::db::{DB, InMemoryDB};
pub use storage::storage::{Array, HashMap};
pub use transient_crypto::commitment::PedersenRandomness;
pub use transient_crypto::curve::Fr;
pub use transient_crypto::fab::ValueReprAlignedValue;
pub use transient_crypto::merkle_tree::{MerkleTree, leaf_hash};
pub use transient_crypto::proofs::PARAMS_VERIFIER;
pub use transient_crypto::proofs::{KeyLocation, ProofPreimage};
pub use zswap::verify::{OUTPUT_VK, SIGN_VK, SPEND_VK};
pub use zswap::{
    Delta, Input as ZswapInput, Offer as ZswapOffer, Output as ZswapOutput,
    Transient as ZswapTransient,
};

// ============================================================================
// Test Configuration
// ============================================================================

lazy_static! {
    pub static ref RESOLVER: Resolver = test_resolver("token-vault");
}

/// Domain separator for public key derivation (matches contract)
pub const PK_DOMAIN_SEP: &[u8] = b"token:vault:pk";

/// Derive public key from secret key (matches compact contract's publicKey circuit)
pub fn derive_public_key(sk: HashOutput) -> HashOutput {
    persistent_commit(PK_DOMAIN_SEP, sk)
}

// ============================================================================
// Contract State Layout (matches token-vault.compact)
// ============================================================================
// Index 0: shieldedVault (QualifiedShieldedCoinInfo)
// Index 1: hasShieldedTokens (Boolean)
// Index 2: owner (Bytes<32>)
// Index 3: authorized (Set<Bytes<32>>)
// Index 4: totalShieldedDeposits (Counter)
// Index 5: totalShieldedWithdrawals (Counter)
// Index 6: totalUnshieldedDeposits (Counter)
// Index 7: totalUnshieldedWithdrawals (Counter)

pub const STATE_IDX_SHIELDED_VAULT: u8 = 0;
pub const STATE_IDX_HAS_SHIELDED_TOKENS: u8 = 1;
pub const STATE_IDX_OWNER: u8 = 2;
pub const STATE_IDX_AUTHORIZED: u8 = 3;
pub const STATE_IDX_TOTAL_SHIELDED_DEPOSITS: u8 = 4;
pub const STATE_IDX_TOTAL_SHIELDED_WITHDRAWALS: u8 = 5;
pub const STATE_IDX_TOTAL_UNSHIELDED_DEPOSITS: u8 = 6;
pub const STATE_IDX_TOTAL_UNSHIELDED_WITHDRAWALS: u8 = 7;

// ============================================================================
// Helper Functions
// ============================================================================

/// Convert program operations with results for verification
pub fn program_with_results<D: DB>(
    prog: &[Op<ResultModeGather, D>],
    results: &[AlignedValue],
) -> Vec<Op<ResultModeVerify, D>> {
    let mut res_iter = results.iter();

    prog.iter()
        .map(|op| op.clone().translate(|()| res_iter.next().unwrap().clone()))
        .filter(|op| match op {
            Op::Idx { path, .. } => !path.is_empty(),
            Op::Ins { n, .. } => *n != 0,
            _ => true,
        })
        .collect::<Vec<_>>()
}

/// Create query context with optional ZSwap offer
pub fn context_with_offer<D: DB>(
    ledger: &LedgerState<D>,
    addr: ContractAddress,
    offer: Option<&ZswapOffer<ProofPreimage, D>>,
) -> QueryContext<D> {
    let mut res = QueryContext::new(ledger.index(addr).unwrap().data, addr);
    if let Some(offer) = offer {
        let (_, indices) = ledger.zswap.try_apply(offer, None).unwrap();
        res.call_context.com_indices = indices;
    }
    res
}

/// Initialize crypto parameters for tests
pub fn init_crypto() {
    lazy_static::initialize(&PARAMS_VERIFIER);
    SPEND_VK.init().ok();
    OUTPUT_VK.init().ok();
    SIGN_VK.init().ok();
}

// ============================================================================
// Helper Functions for Unshielded Operations
// ============================================================================
//
// These helper functions generate the exact Op sequences that the Compact compiler
// produces for unshielded token operations. Understanding these is crucial for
// building valid transcripts.
//
// The Midnight VM uses a stack-based architecture. The "effects" structure is
// accessed via stack operations, and maps are modified using Idx/Ins operations.
//
// Key insight: The transcript you build MUST match exactly what the compiled
// Compact circuit produces. If there's a mismatch, the ledger will reject the
// transaction during verification.
// ============================================================================

/// Create the Op sequence for receiveUnshielded (effects index 6: unshielded_inputs)
///
/// This function generates the exact VM operations that the Compact compiler produces
/// when a contract calls `receiveUnshielded(color, amount)`. The ledger uses these
/// operations to track incoming unshielded tokens.
///
/// ## What this does:
/// 1. Accesses the effects structure at index 6 (unshielded_inputs map)
/// 2. Uses the token type as the map key
/// 3. If the key exists, adds the amount to the existing value
/// 4. If the key doesn't exist, inserts the amount as a new entry
///
/// ## Why we need this:
/// When testing unshielded deposits, we need to construct a transcript that
/// matches what the depositUnshielded circuit would produce. This includes
/// the receiveUnshielded operation which tells the ledger "I'm receiving X tokens".
///
/// ## Token type encoding:
/// `TokenType::Unshielded(NIGHT)` is encoded as:
/// - Byte 0: 1 (tag for unshielded variant)
/// - Bytes 1-32: color (NIGHT = all zeros for native token)
/// - Bytes 33-64: padding (zeros)
pub fn receive_unshielded_ops<D: DB>(
    token_type: TokenType,
    amount: u128,
) -> Vec<Op<ResultModeGather, D>> {
    // Convert token type to AlignedValue for use in VM operations
    // TokenType::Unshielded is encoded as: [1u8 (tag), color (32 bytes), empty (32 bytes)]
    let token_type_av: AlignedValue = token_type.into();
    let amount_av: AlignedValue = amount.into();
    
    vec![
        // Swap to access effects on stack
        Op::Swap { n: 0.try_into().unwrap() },
        // Index into effects at position 6 (unshielded_inputs map), push path for later insert
        Op::Idx {
            cached: true,
            push_path: true,
            path: vec![Key::Value(6u8.into())].try_into().unwrap(),
        },
        // Push the token type as key
        Op::Push {
            storage: false,
            value: StateValue::Cell(Sp::new(token_type_av.clone())),
        },
        // Duplicate for member check
        Op::Dup { n: 1.try_into().unwrap() },
        Op::Dup { n: 1.try_into().unwrap() },
        // Check if key exists in map
        Op::Member,
        // Push the amount
        Op::Push {
            storage: false,
            value: StateValue::Cell(Sp::new(amount_av.clone())),
        },
        // Swap and negate for branching
        Op::Swap { n: 0.try_into().unwrap() },
        Op::Neg,
        // Branch: skip 4.try_into().unwrap() ops if key doesn't exist
        Op::Branch { skip: 4.try_into().unwrap() },
        // If exists: get current value and add amount
        Op::Dup { n: 2.try_into().unwrap() },
        Op::Dup { n: 2.try_into().unwrap() },
        Op::Idx {
            cached: true,
            push_path: false,
            path: vec![Key::Stack].try_into().unwrap(),
        },
        Op::Add,
        // Insert the value
        Op::Ins { cached: true, n: 2.try_into().unwrap() },
        // Swap back
        Op::Swap { n: 0.try_into().unwrap() },
    ]
}

/// Create the Op sequence for sendUnshielded (effects index 7: unshielded_outputs)
///
/// This function generates the VM operations for a contract sending unshielded tokens.
/// It mirrors what the Compact compiler generates for `sendUnshielded(color, amount, recipient)`.
///
/// ## What this does:
/// 1. Accesses the effects structure at index 7 (unshielded_outputs map)
/// 2. Increments the total amount being sent for this token type
///
/// ## Important:
/// This function only handles the OUTPUT side. For a complete withdrawal,
/// you also need `claim_unshielded_spend_ops` to specify WHO receives the tokens.
///
/// ## When to use:
/// - Contract withdrawals (contract → user): paired with claim_unshielded_spend_ops
/// - Contract-to-contract transfers: paired with claim_unshielded_spend_ops
pub fn send_unshielded_ops<D: DB>(
    token_type: TokenType,
    amount: u128,
) -> Vec<Op<ResultModeGather, D>> {
    // Convert to AlignedValue for VM operations
    let token_type_av: AlignedValue = token_type.into();
    let amount_av: AlignedValue = amount.into();
    
    vec![
        Op::Swap { n: 0.try_into().unwrap() },
        Op::Idx {
            cached: true,
            push_path: true,
            path: vec![Key::Value(7u8.into())].try_into().unwrap(),
        },
        Op::Push {
            storage: false,
            value: StateValue::Cell(Sp::new(token_type_av.clone())),
        },
        Op::Dup { n: 1.try_into().unwrap() },
        Op::Dup { n: 1.try_into().unwrap() },
        Op::Member,
        Op::Push {
            storage: false,
            value: StateValue::Cell(Sp::new(amount_av.clone())),
        },
        Op::Swap { n: 0.try_into().unwrap() },
        Op::Neg,
        Op::Branch { skip: 4.try_into().unwrap() },
        Op::Dup { n: 2.try_into().unwrap() },
        Op::Dup { n: 2.try_into().unwrap() },
        Op::Idx {
            cached: true,
            push_path: false,
            path: vec![Key::Stack].try_into().unwrap(),
        },
        Op::Add,
        Op::Ins { cached: true, n: 2.try_into().unwrap() },
        Op::Swap { n: 0.try_into().unwrap() },
    ]
}

/// Create the Op sequence for claiming unshielded spend (effects index 8: claimed_unshielded_spends)
///
/// This function specifies WHO should receive the tokens being sent via sendUnshielded.
/// The recipient can be either a user (identified by their public key) or another contract.
///
/// ## Critical for verification:
/// The ledger performs a SUBSET CHECK during transaction verification:
/// - For user recipients: The `claimed_unshielded_spends` must be a subset of
///   the `UnshieldedOffer.outputs`. This ensures the user actually receives a UTXO.
/// - For contract recipients: The `claimed_unshielded_spends` must be a subset of
///   the recipient contract's `unshielded_inputs`. This ensures the receiving
///   contract acknowledges the incoming tokens.
///
/// ## Key types:
/// - `Recipient::User(PublicKey)`: Sending to a user address (creates UTXO)
/// - `Recipient::Contract(ContractAddress)`: Sending to another contract
///
/// ## Important note on user addresses:
/// The `PublicKey` in `Recipient::User` wraps a `HashOutput`. For UTXO recipients,
/// this must match the `UserAddress` in the `UnshieldedOffer.outputs`.
/// Both `PublicKey` and `UserAddress` wrap the same `HashOutput` type.
pub fn claim_unshielded_spend_ops<D: DB>(
    token_type: TokenType,
    recipient: Recipient,
    amount: u128,
) -> Vec<Op<ResultModeGather, D>> {
    use coin_structure::coin::PublicAddress;
    use onchain_runtime::context::ClaimedUnshieldedSpendsKey;
    
    // Convert Recipient to PublicAddress for the effects key
    // This determines where the tokens will be delivered
    let public_addr: PublicAddress = match recipient {
        Recipient::Contract(addr) => PublicAddress::Contract(addr),
        // PublicKey(HashOutput) -> UserAddress(HashOutput)
        // The inner HashOutput must match the UTXO output owner
        Recipient::User(pk) => PublicAddress::User(UserAddress(pk.0)),
    };
    
    let key = ClaimedUnshieldedSpendsKey(token_type, public_addr);
    let key_av: AlignedValue = key.into();
    let amount_av: AlignedValue = amount.into();
    
    vec![
        Op::Swap { n: 0.try_into().unwrap() },
        Op::Idx {
            cached: true,
            push_path: true,
            path: vec![Key::Value(8u8.into())].try_into().unwrap(),
        },
        Op::Push {
            storage: false,
            value: StateValue::Cell(Sp::new(key_av.clone())),
        },
        Op::Dup { n: 1.try_into().unwrap() },
        Op::Dup { n: 1.try_into().unwrap() },
        Op::Member,
        Op::Push {
            storage: false,
            value: StateValue::Cell(Sp::new(amount_av.clone())),
        },
        Op::Swap { n: 0.try_into().unwrap() },
        Op::Neg,
        Op::Branch { skip: 4.try_into().unwrap() },
        Op::Dup { n: 2.try_into().unwrap() },
        Op::Dup { n: 2.try_into().unwrap() },
        Op::Idx {
            cached: true,
            push_path: false,
            path: vec![Key::Stack].try_into().unwrap(),
        },
        Op::Add,
        Op::Ins { cached: true, n: 2.try_into().unwrap() },
        Op::Swap { n: 0.try_into().unwrap() },
    ]
}
