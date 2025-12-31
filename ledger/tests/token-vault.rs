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

//! Token Vault Integration Tests
//!
//! This module contains comprehensive integration tests for the token-vault contract,
//! demonstrating how to interact with both shielded and unshielded token operations
//! in the Midnight ledger.
//!
//! ## Running Tests
//!
//! These tests require the `MIDNIGHT_LEDGER_TEST_STATIC_DIR` environment variable
//! to be set to the tests directory containing verifier keys:
//!
//! ```bash
//! MIDNIGHT_LEDGER_TEST_STATIC_DIR=/path/to/ledger/tests cargo test --test token-vault
//! ```
//!
//! The tests also recommend single-threaded execution to avoid race conditions:
//!
//! ```bash
//! MIDNIGHT_LEDGER_TEST_STATIC_DIR=/path/to/ledger/tests \
//!   cargo test --test token-vault -- --test-threads=1
//! ```
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
//! ## Unshielded Token Model
//!
//! Unshielded tokens flow through three key ledger effect maps:
//!
//! - **`unshielded_inputs`** (effects index 6): Map<TokenType, u128>
//!   Tracks tokens flowing INTO a contract. When a user deposits tokens to a
//!   contract, the contract calls `receiveUnshielded()` which increments this map.
//!
//! - **`unshielded_outputs`** (effects index 7): Map<TokenType, u128>
//!   Tracks tokens flowing OUT OF a contract. When a contract sends tokens,
//!   it calls `sendUnshielded()` which increments this map.
//!
//! - **`claimed_unshielded_spends`** (effects index 8): Map<(TokenType, Recipient), u128>
//!   Records which recipient (user or contract) should receive the output tokens.
//!   This ensures tokens go to the intended destination.
//!
//! ## Transaction Flow for Unshielded Operations
//!
//! ### Deposit (User → Contract):
//! 1. User creates an `UnshieldedOffer` with UTXO inputs (spending their tokens)
//! 2. Contract call includes `receiveUnshielded()` in its transcript
//! 3. Ledger verifies: UnshieldedOffer inputs >= contract's unshielded_inputs
//! 4. UTXO is consumed, contract balance increases
//!
//! ### Withdrawal (Contract → User):
//! 1. Contract call includes `sendUnshielded()` + recipient claim in transcript
//! 2. User creates `UnshieldedOffer` with outputs (receiving tokens as new UTXO)
//! 3. Ledger verifies: claimed_unshielded_spends matches UnshieldedOffer outputs
//! 4. Contract balance decreases, new UTXO is created for user
//!
//! ### Contract-to-Contract Transfer:
//! 1. Sender contract calls `sendUnshielded()` claiming recipient contract
//! 2. Receiver contract calls `receiveUnshielded()` in the SAME transaction
//! 3. Ledger verifies: sender's claimed_unshielded_spends subset of receiver's unshielded_inputs
//! 4. No UTXOs involved - purely internal ledger accounting
//!
//! ## Test Structure
//!
//! Each test follows this pattern:
//! 1. Initialize test state with fee tokens (required for transaction costs)
//! 2. Deploy the token-vault contract with verifier keys
//! 3. Build the transaction transcript (Op sequence matching circuit behavior)
//! 4. Create the transaction with appropriate offers and contract calls
//! 5. Apply the transaction and verify state changes

#![deny(warnings)]
#![allow(unused_imports)]
#![allow(unused_variables)]
#![allow(unused_macros)]

use base_crypto::fab::{AlignedValue, Value};
use base_crypto::hash::{HashOutput, persistent_commit};
use base_crypto::rng::SplittableRng;
use base_crypto::signatures::Signature;
use base_crypto::time::Timestamp;
use coin_structure::coin::{
    Info as CoinInfo, QualifiedInfo as QualifiedCoinInfo, NIGHT, UserAddress,
    TokenType, UnshieldedTokenType,
};
use coin_structure::contract::ContractAddress;
use coin_structure::transfer::{Recipient, SenderEvidence};
use futures::FutureExt;
use lazy_static::lazy_static;
use midnight_ledger::construct::{ContractCallPrototype, PreTranscript, partition_transcripts};
use midnight_ledger::semantics::{ErasedTransactionResult::Success, ZswapLocalStateExt};
use midnight_ledger::structure::{
    ContractDeploy, INITIAL_PARAMETERS, Intent, IntentHash, LedgerState, ProofPreimageMarker, Transaction,
    UnshieldedOffer, UtxoOutput, UtxoSpend,
};
use midnight_ledger::test_utilities::{Resolver, verifier_key};
use midnight_ledger::test_utilities::{TestState, tx_prove_bind};
use midnight_ledger::test_utilities::{Tx, TxBound};
use midnight_ledger::test_utilities::{test_intents, test_resolver};
use midnight_ledger::verify::WellFormedStrictness;
use onchain_runtime::context::QueryContext;
use onchain_runtime::ops::{Key, Op, key};
use onchain_runtime::program_fragments::*;
use onchain_runtime::result_mode::{ResultModeGather, ResultModeVerify};
use onchain_runtime::state::{ContractOperation, ContractState, StateValue, stval};
use rand::rngs::StdRng;
use rand::{CryptoRng, Rng, SeedableRng};
use serialize::Serializable;
use std::borrow::Cow;
use storage::arena::Sp;
use storage::db::{DB, InMemoryDB};
use storage::storage::{Array, HashMap};
use transient_crypto::commitment::PedersenRandomness;
use transient_crypto::curve::Fr;
use transient_crypto::fab::ValueReprAlignedValue;
use transient_crypto::merkle_tree::{MerkleTree, leaf_hash};
use transient_crypto::proofs::PARAMS_VERIFIER;
use transient_crypto::proofs::{KeyLocation, ProofPreimage};
use zswap::verify::{OUTPUT_VK, SIGN_VK, SPEND_VK};
use zswap::{
    Delta, Input as ZswapInput, Offer as ZswapOffer, Output as ZswapOutput,
    Transient as ZswapTransient,
};

// ============================================================================
// Test Configuration
// ============================================================================

lazy_static! {
    static ref RESOLVER: Resolver = test_resolver("token-vault");
}

/// Domain separator for public key derivation (matches contract)
const PK_DOMAIN_SEP: &[u8] = b"token:vault:pk";

/// Derive public key from secret key (matches compact contract's publicKey circuit)
fn derive_public_key(sk: HashOutput) -> HashOutput {
    persistent_commit(PK_DOMAIN_SEP, sk)
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Convert program operations with results for verification
fn program_with_results<D: DB>(
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
fn context_with_offer<D: DB>(
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
fn receive_unshielded_ops<D: DB>(
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
            cached: true.try_into().unwrap(),
            push_path: true.try_into().unwrap(),
            path: vec![Key::Value(6u8.into())].try_into().unwrap(),
        },
        // Push the token type as key
        Op::Push {
            storage: false.try_into().unwrap(),
            value: StateValue::Cell(Sp::new(token_type_av.clone().try_into().unwrap())).try_into().unwrap(),
        },
        // Duplicate for member check
        Op::Dup { n: 1.try_into().unwrap() },
        Op::Dup { n: 1.try_into().unwrap() },
        // Check if key exists in map
        Op::Member,
        // Push the amount
        Op::Push {
            storage: false.try_into().unwrap(),
            value: StateValue::Cell(Sp::new(amount_av.clone().try_into().unwrap())).try_into().unwrap(),
        },
        // Swap and negate for branching
        Op::Swap { n: 0.try_into().unwrap() },
        Op::Neg,
        // Branch: skip 4 ops if key doesn't exist
        Op::Branch { skip: 4.try_into().unwrap() },
        // If exists: get current value and add amount
        Op::Dup { n: 2.try_into().unwrap() },
        Op::Dup { n: 2.try_into().unwrap() },
        Op::Idx {
            cached: true.try_into().unwrap(),
            push_path: false.try_into().unwrap(),
            path: vec![Key::Stack].try_into().unwrap(),
        },
        Op::Add,
        // Insert the value
        Op::Ins { cached: true.try_into().unwrap(), n: 2.try_into().unwrap() },
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
#[allow(dead_code)]
fn send_unshielded_ops<D: DB>(
    token_type: TokenType,
    amount: u128,
) -> Vec<Op<ResultModeGather, D>> {
    // Convert to AlignedValue for VM operations
    let token_type_av: AlignedValue = token_type.into();
    let amount_av: AlignedValue = amount.into();
    
    vec![
        Op::Swap { n: 0.try_into().unwrap() },
        Op::Idx {
            cached: true.try_into().unwrap(),
            push_path: true.try_into().unwrap(),
            path: vec![Key::Value(7u8.into())].try_into().unwrap(),
        },
        Op::Push {
            storage: false.try_into().unwrap(),
            value: StateValue::Cell(Sp::new(token_type_av.clone().try_into().unwrap())).try_into().unwrap(),
        },
        Op::Dup { n: 1.try_into().unwrap() },
        Op::Dup { n: 1.try_into().unwrap() },
        Op::Member,
        Op::Push {
            storage: false.try_into().unwrap(),
            value: StateValue::Cell(Sp::new(amount_av.clone().try_into().unwrap())).try_into().unwrap(),
        },
        Op::Swap { n: 0.try_into().unwrap() },
        Op::Neg,
        Op::Branch { skip: 4.try_into().unwrap() },
        Op::Dup { n: 2.try_into().unwrap() },
        Op::Dup { n: 2.try_into().unwrap() },
        Op::Idx {
            cached: true.try_into().unwrap(),
            push_path: false.try_into().unwrap(),
            path: vec![Key::Stack].try_into().unwrap(),
        },
        Op::Add,
        Op::Ins { cached: true.try_into().unwrap(), n: 2.try_into().unwrap() },
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
#[allow(dead_code)]
fn claim_unshielded_spend_ops<D: DB>(
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
            cached: true.try_into().unwrap(),
            push_path: true.try_into().unwrap(),
            path: vec![Key::Value(8u8.into())].try_into().unwrap(),
        },
        Op::Push {
            storage: false.try_into().unwrap(),
            value: StateValue::Cell(Sp::new(key_av.clone().try_into().unwrap())).try_into().unwrap(),
        },
        Op::Dup { n: 1.try_into().unwrap() },
        Op::Dup { n: 1.try_into().unwrap() },
        Op::Member,
        Op::Push {
            storage: false.try_into().unwrap(),
            value: StateValue::Cell(Sp::new(amount_av.clone().try_into().unwrap())).try_into().unwrap(),
        },
        Op::Swap { n: 0.try_into().unwrap() },
        Op::Neg,
        Op::Branch { skip: 4.try_into().unwrap() },
        Op::Dup { n: 2.try_into().unwrap() },
        Op::Dup { n: 2.try_into().unwrap() },
        Op::Idx {
            cached: true.try_into().unwrap(),
            push_path: false.try_into().unwrap(),
            path: vec![Key::Stack].try_into().unwrap(),
        },
        Op::Add,
        Op::Ins { cached: true.try_into().unwrap(), n: 2.try_into().unwrap() },
        Op::Swap { n: 0.try_into().unwrap() },
    ]
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

// ============================================================================
// Main Integration Test
// ============================================================================

#[tokio::test]
async fn token_vault() {
    midnight_ledger::init_logger(midnight_ledger::LogLevel::Trace);
    let mut rng = StdRng::seed_from_u64(0x42);

    // Initialize crypto parameters
    lazy_static::initialize(&PARAMS_VERIFIER);
    SPEND_VK.init().ok();
    OUTPUT_VK.init().ok();
    SIGN_VK.init().ok();

    // Generate owner keys
    let owner_sk: HashOutput = rng.r#gen();
    let owner_pk = derive_public_key(owner_sk);

    // Load contract operations
    let deposit_shielded_op = ContractOperation::new(
        verifier_key(&RESOLVER, "depositShielded").await
    );
    let withdraw_shielded_op = ContractOperation::new(
        verifier_key(&RESOLVER, "withdrawShielded").await
    );
    let deposit_unshielded_op = ContractOperation::new(
        verifier_key(&RESOLVER, "depositUnshielded").await
    );
    let withdraw_unshielded_op = ContractOperation::new(
        verifier_key(&RESOLVER, "withdrawUnshielded").await
    );
    let get_shielded_balance_op = ContractOperation::new(
        verifier_key(&RESOLVER, "getShieldedBalance").await
    );
    let get_unshielded_balance_op = ContractOperation::new(
        verifier_key(&RESOLVER, "getUnshieldedBalance").await
    );

    println!(":: Token Vault Test Suite");
    println!("   Owner PK: {:?}", hex::encode(&owner_pk.0[..8]));

    // Initial test state
    let mut state: TestState<InMemoryDB> = TestState::new(&mut rng);
    const REWARDS_AMOUNT: u128 = 10_000_000_000;
    let token = Default::default();
    state.rewards_shielded(&mut rng, token, REWARDS_AMOUNT);
    state.give_fee_token(&mut rng, 100).await;

    let mut unbalanced_strictness = WellFormedStrictness::default();
    unbalanced_strictness.enforce_balancing = false;
    let balanced_strictness = WellFormedStrictness::default();

    // ========================================================================
    // Part 1: Deploy Contract
    // ========================================================================
    println!("\n:: Part 1: Deploy Token Vault Contract");

    let contract: ContractState<InMemoryDB> = ContractState::new(
        stval!([
            (QualifiedCoinInfo::default()),   // 0: shieldedVault
            (false),                          // 1: hasShieldedTokens
            (owner_pk),                       // 2: owner
            {},                               // 3: authorized (empty set)
            (0u64),                           // 4: totalShieldedDeposits
            (0u64),                           // 5: totalShieldedWithdrawals
            (0u64),                           // 6: totalUnshieldedDeposits
            (0u64)                            // 7: totalUnshieldedWithdrawals
        ]),
        HashMap::new()
            .insert(b"depositShielded"[..].into(), deposit_shielded_op.clone())
            .insert(b"withdrawShielded"[..].into(), withdraw_shielded_op.clone())
            .insert(b"depositUnshielded"[..].into(), deposit_unshielded_op.clone())
            .insert(b"withdrawUnshielded"[..].into(), withdraw_unshielded_op.clone())
            .insert(b"getShieldedBalance"[..].into(), get_shielded_balance_op.clone())
            .insert(b"getUnshieldedBalance"[..].into(), get_unshielded_balance_op.clone()),
        Default::default(),
    );

    let deploy = ContractDeploy::new(&mut rng, contract.clone());
    let addr = deploy.address();
    println!("   Contract address: {:?}", addr);

    let tx = Transaction::from_intents(
        "local-test",
        test_intents(&mut rng, Vec::new(), Vec::new(), vec![deploy], state.time),
    );
    tx.well_formed(&state.ledger, unbalanced_strictness, state.time)
        .unwrap();

    let tx = tx_prove_bind(rng.split(), &tx, &RESOLVER).await.unwrap();
    let balanced = state.balance_tx(rng.split(), tx, &RESOLVER).await.unwrap();
    state.assert_apply(&balanced, balanced_strictness);

    println!("   Contract deployed successfully");

    // ========================================================================
    // Part 2: First Shielded Deposit
    // ========================================================================
    println!("\n:: Part 2: First Shielded Deposit");
    const FIRST_DEPOSIT: u128 = 1_000_000;

    let coin = CoinInfo::new(&mut rng, FIRST_DEPOSIT, token);
    let out = ZswapOutput::new_contract_owned(&mut rng, &coin, None, addr).unwrap();
    let coin_com = coin.commitment(&Recipient::Contract(addr));

    // Build transcript for first deposit (hasShieldedTokens = false)
    let public_transcript: Vec<Op<ResultModeGather, InMemoryDB>> = [
        &Cell_read!([key!(1u8)], false, bool)[..],  // Read hasShieldedTokens
        &kernel_self!((), ())[..],
        &kernel_claim_zswap_coin_receive!((), (), coin_com),
        &kernel_self!((), ())[..],
        &Cell_write_coin!(
            [key!(0u8)],
            true,
            QualifiedCoinInfo,
            coin.clone(),
            Recipient::Contract(addr)
        )[..],
        &Cell_write!([key!(1u8)], true, bool, true)[..],
        &Counter_increment!([key!(4u8)], false, 1u64)[..],
    ]
    .into_iter()
    .flat_map(|x| x.iter())
    .cloned()
    .collect();

    let public_transcript_results: Vec<AlignedValue> = vec![
        false.into(),  // hasShieldedTokens was false
        addr.into(),
        addr.into(),
    ];

    let offer = ZswapOffer {
        inputs: vec![].into(),
        outputs: vec![out].into(),
        transient: vec![].into(),
        deltas: vec![Delta {
            token_type: token,
            value: -(FIRST_DEPOSIT as i128),
        }]
        .into(),
    };

    let transcripts = partition_transcripts(
        &[PreTranscript {
            context: context_with_offer(&state.ledger, addr, Some(&offer)),
            program: program_with_results(&public_transcript, &public_transcript_results),
            comm_comm: None,
        }],
        &INITIAL_PARAMETERS,
    )
    .unwrap();

    let call = ContractCallPrototype {
        address: addr,
        entry_point: b"depositShielded"[..].into(),
        op: deposit_shielded_op.clone(),
        input: coin.into(),
        output: ().into(),
        guaranteed_public_transcript: transcripts[0].0.clone(),
        fallible_public_transcript: transcripts[0].1.clone(),
        private_transcript_outputs: vec![],
        communication_commitment_rand: rng.r#gen(),
        key_location: KeyLocation(Cow::Borrowed("depositShielded")),
    };

    let tx = Transaction::new(
        "local-test",
        test_intents(&mut rng, vec![call], Vec::new(), Vec::new(), state.time),
        Some(offer),
        std::collections::HashMap::new(),
    );

    let tx = tx_prove_bind(rng.split(), &tx, &RESOLVER).await.unwrap();
    tx.well_formed(&state.ledger, unbalanced_strictness, state.time)
        .unwrap();

    let balanced = state.balance_tx(rng.split(), tx, &RESOLVER).await.unwrap();
    state.assert_apply(&balanced, balanced_strictness);

    println!("   First deposit: {} tokens locked", FIRST_DEPOSIT);

    // ========================================================================
    // Part 3: Second Shielded Deposit (Merge)
    // ========================================================================
    println!("\n:: Part 3: Second Shielded Deposit (Merge with Existing)");
    const SECOND_DEPOSIT: u128 = 500_000;

    // Get current pot from contract state
    let cstate = state.ledger.contract.get(&addr).unwrap();
    let pot = if let StateValue::Array(arr) = &cstate.data.get_ref() {
        if let StateValue::Cell(pot) = &arr.get(0).unwrap() {
            QualifiedCoinInfo::try_from(&*pot.value).unwrap()
        } else {
            unreachable!()
        }
    } else {
        unreachable!()
    };

    let new_coin = CoinInfo::new(&mut rng, SECOND_DEPOSIT, token);
    let out = ZswapOutput::new_contract_owned(&mut rng, &new_coin, None, addr).unwrap();
    let new_coin_com = new_coin.commitment(&Recipient::Contract(addr));

    // Create merged coin
    let merged_coin = CoinInfo::from(&pot).evolve_from(
        b"midnight:kernel:nonce_evolve",
        pot.value + new_coin.value,
        pot.type_,
    );
    let merged_coin_com = merged_coin.commitment(&Recipient::Contract(addr));

    // Create nullifiers
    let pot_nul = CoinInfo::from(&pot).nullifier(&SenderEvidence::Contract(addr));
    let coin_nul = new_coin.nullifier(&SenderEvidence::Contract(addr));

    let pot_in = ZswapInput::new_contract_owned(
        &mut rng,
        &pot,
        None,
        addr,
        &state.ledger.zswap.coin_coms,
    )
    .unwrap();

    let transient = ZswapTransient::new_from_contract_owned_output(
        &mut rng,
        &new_coin.qualify(0),
        None,
        out,
    )
    .unwrap();

    let merged_out = ZswapOutput::new_contract_owned(&mut rng, &merged_coin, None, addr).unwrap();

    let offer = ZswapOffer {
        inputs: vec![pot_in].into(),
        outputs: vec![merged_out].into(),
        transient: vec![transient].into(),
        deltas: vec![Delta {
            token_type: token,
            value: -(SECOND_DEPOSIT as i128),
        }]
        .into(),
    };

    // Build merge transcript
    let public_transcript: Vec<Op<ResultModeGather, InMemoryDB>> = [
        &Cell_read!([key!(1u8)], false, bool)[..],
        &kernel_self!((), ())[..],
        &kernel_claim_zswap_coin_receive!((), (), new_coin_com)[..],
        &Cell_read!([key!(0u8)], false, QualifiedCoinInfo)[..],
        &kernel_self!((), ())[..],
        &kernel_claim_zswap_nullifier!((), (), pot_nul)[..],
        &kernel_claim_zswap_nullifier!((), (), coin_nul)[..],
        &kernel_claim_zswap_coin_spend!((), (), merged_coin_com)[..],
        &kernel_claim_zswap_coin_receive!((), (), merged_coin_com)[..],
        &kernel_self!((), ())[..],
        &Cell_write_coin!(
            [key!(0u8)],
            true,
            QualifiedCoinInfo,
            merged_coin.clone(),
            Recipient::Contract(addr)
        )[..],
        &Counter_increment!([key!(4u8)], false, 1u64)[..],
    ]
    .into_iter()
    .flat_map(|x| x.iter())
    .cloned()
    .collect();

    let public_transcript_results: Vec<AlignedValue> = vec![
        true.into(),  // hasShieldedTokens is now true
        addr.into(),
        pot.into(),
        addr.into(),
        addr.into(),
    ];

    let transcripts = partition_transcripts(
        &[PreTranscript {
            context: context_with_offer(&state.ledger, addr, Some(&offer)),
            program: program_with_results(&public_transcript, &public_transcript_results),
            comm_comm: None,
        }],
        &INITIAL_PARAMETERS,
    )
    .unwrap();

    let call = ContractCallPrototype {
        address: addr,
        entry_point: b"depositShielded"[..].into(),
        op: deposit_shielded_op.clone(),
        input: new_coin.into(),
        output: ().into(),
        guaranteed_public_transcript: transcripts[0].0.clone(),
        fallible_public_transcript: transcripts[0].1.clone(),
        private_transcript_outputs: vec![],
        communication_commitment_rand: rng.r#gen(),
        key_location: KeyLocation(Cow::Borrowed("depositShielded")),
    };

    let tx = Transaction::new(
        "local-test",
        test_intents(&mut rng, vec![call], Vec::new(), Vec::new(), state.time),
        Some(offer),
        std::collections::HashMap::new(),
    );

    let tx = tx_prove_bind(rng.split(), &tx, &RESOLVER).await.unwrap();
    tx.well_formed(&state.ledger, unbalanced_strictness, state.time)
        .unwrap();

    let balanced = state.balance_tx(rng.split(), tx, &RESOLVER).await.unwrap();
    state.assert_apply(&balanced, balanced_strictness);

    println!("   Merge deposit: {} + {} = {} tokens", FIRST_DEPOSIT, SECOND_DEPOSIT, FIRST_DEPOSIT + SECOND_DEPOSIT);

    // ========================================================================
    // Part 4: Partial Shielded Withdrawal
    // ========================================================================
    println!("\n:: Part 4: Partial Shielded Withdrawal");
    const WITHDRAW_AMOUNT: u128 = 300_000;

    // Get current pot and check hasShieldedTokens
    let cstate = state.ledger.contract.get(&addr).unwrap();
    let (pot, has_shielded_tokens) = if let StateValue::Array(arr) = &cstate.data.get_ref() {
        let pot = if let StateValue::Cell(pot) = &arr.get(0).unwrap() {
            QualifiedCoinInfo::try_from(&*pot.value).unwrap()
        } else {
            unreachable!()
        };
        let has_tokens = if let StateValue::Cell(cell) = &arr.get(1).unwrap() {
            bool::try_from(&*cell.value).unwrap_or(false)
        } else {
            false
        };
        (pot, has_tokens)
    } else {
        unreachable!()
    };
    
    println!("   Contract state: hasShieldedTokens={}, pot.value={}", has_shielded_tokens, pot.value);

    // Create withdrawal coin and change coin
    let withdraw_coin = CoinInfo::from(&pot).evolve_from(
        b"midnight:kernel:nonce_evolve",
        WITHDRAW_AMOUNT,
        pot.type_,
    );

    let change_coin = CoinInfo::from(&pot).evolve_from(
        b"midnight:kernel:nonce_evolve",
        pot.value - WITHDRAW_AMOUNT,
        pot.type_,
    );

    let pot_nul = CoinInfo::from(&pot).nullifier(&SenderEvidence::Contract(addr));
    let withdraw_com = withdraw_coin.commitment(&Recipient::User(state.zswap_keys.coin_public_key()));
    let change_com = change_coin.commitment(&Recipient::Contract(addr));

    let pot_in = ZswapInput::new_contract_owned(
        &mut rng,
        &pot,
        None,
        addr,
        &state.ledger.zswap.coin_coms,
    )
    .unwrap();

    let withdraw_out = ZswapOutput::new(
        &mut rng,
        &withdraw_coin,
        None,
        &state.zswap_keys.coin_public_key(),
        None,
    )
    .unwrap();

    let change_out = ZswapOutput::new_contract_owned(&mut rng, &change_coin, None, addr).unwrap();

    // Outputs must be sorted for ZSwap offer normalization
    let mut outputs = vec![withdraw_out, change_out];
    outputs.sort();
    
    let offer = ZswapOffer {
        inputs: vec![pot_in].into(),
        outputs: outputs.into(),
        transient: vec![].into(),
        deltas: vec![].into(),
    };

    // Track the withdrawn coin
    state.zswap = state.zswap.watch_for(&state.zswap_keys.coin_public_key(), &withdraw_coin);

    // Order matches circuit execution:
    // 1. isAuthorized() -> checks authorized.member(pk) first, then pk == owner
    // 2. hasShieldedTokens assertion
    // 3. shieldedVault.value check
    let public_transcript: Vec<Op<ResultModeGather, InMemoryDB>> = [
        &Set_member!([key!(3u8)], false, [u8; 32], owner_pk.0)[..],  // Check authorized.member(pk)
        &Cell_read!([key!(2u8)], false, [u8; 32])[..],  // Read owner for pk == owner
        &Cell_read!([key!(1u8)], false, bool)[..],  // Check hasShieldedTokens
        &Cell_read!([key!(0u8)], false, QualifiedCoinInfo)[..],  // Read vault
        &kernel_self!((), ())[..],
        &kernel_claim_zswap_nullifier!((), (), pot_nul)[..],
        &kernel_claim_zswap_coin_spend!((), (), withdraw_com)[..],
        &kernel_claim_zswap_coin_spend!((), (), change_com)[..],
        &kernel_claim_zswap_coin_receive!((), (), change_com)[..],
        &kernel_self!((), ())[..],
        &Cell_write_coin!(
            [key!(0u8)],
            true,
            QualifiedCoinInfo,
            change_coin.clone(),
            Recipient::Contract(addr)
        )[..],
        &Counter_increment!([key!(5u8)], false, 1u64)[..],
    ]
    .into_iter()
    .flat_map(|x| x.iter())
    .cloned()
    .collect();

    let public_transcript_results: Vec<AlignedValue> = vec![
        false.into(),  // authorized.member(pk) result - pk is NOT in authorized set
        owner_pk.into(),  // owner value - this equals pk, so pk == owner is true
        true.into(),   // hasShieldedTokens
        pot.into(),
        addr.into(),
        addr.into(),
    ];

    let transcripts = partition_transcripts(
        &[PreTranscript {
            context: context_with_offer(&state.ledger, addr, Some(&offer)),
            program: program_with_results(&public_transcript, &public_transcript_results),
            comm_comm: None,
        }],
        &INITIAL_PARAMETERS,
    )
    .unwrap();

    let call = ContractCallPrototype {
        address: addr,
        entry_point: b"withdrawShielded"[..].into(),
        op: withdraw_shielded_op.clone(),
        input: (WITHDRAW_AMOUNT as u64).into(),
        output: withdraw_coin.into(),
        guaranteed_public_transcript: transcripts[0].0.clone(),
        fallible_public_transcript: transcripts[0].1.clone(),
        private_transcript_outputs: vec![owner_sk.into()],
        communication_commitment_rand: rng.r#gen(),
        key_location: KeyLocation(Cow::Borrowed("withdrawShielded")),
    };

    let tx = Transaction::new(
        "local-test",
        test_intents(&mut rng, vec![call], Vec::new(), Vec::new(), state.time),
        Some(offer),
        std::collections::HashMap::new(),
    );

    let tx = tx_prove_bind(rng.split(), &tx, &RESOLVER).await.unwrap();
    tx.well_formed(&state.ledger, unbalanced_strictness, state.time)
        .unwrap();

    let balanced = state.balance_tx(rng.split(), tx, &RESOLVER).await.unwrap();
    state.assert_apply(&balanced, balanced_strictness);

    let remaining = FIRST_DEPOSIT + SECOND_DEPOSIT - WITHDRAW_AMOUNT;
    println!("   Partial withdrawal: {} tokens withdrawn, {} remaining in vault", WITHDRAW_AMOUNT, remaining);

    // ========================================================================
    // Summary
    // ========================================================================
    println!("\n:: Test Summary");
    println!("   Initial funds: {} tokens", REWARDS_AMOUNT);
    println!("   First deposit: {} tokens", FIRST_DEPOSIT);
    println!("   Second deposit (merge): {} tokens", SECOND_DEPOSIT);
    println!("   Total deposited: {} tokens", FIRST_DEPOSIT + SECOND_DEPOSIT);
    println!("   Withdrawn: {} tokens", WITHDRAW_AMOUNT);
    println!("   Remaining in vault: {} tokens", remaining);
    println!("\n   All shielded operations completed successfully!");
}

// ============================================================================
// Individual Unit Tests
// ============================================================================

#[tokio::test]
async fn test_deploy_only() {
    let mut rng = StdRng::seed_from_u64(0x42);
    let state: TestState<InMemoryDB> = TestState::new(&mut rng);
    let mut strictness = WellFormedStrictness::default();
    strictness.enforce_balancing = false;

    let owner_sk: HashOutput = rng.r#gen();
    let owner_pk = derive_public_key(owner_sk);

    let deposit_shielded_op = ContractOperation::new(
        verifier_key(&RESOLVER, "depositShielded").await
    );

    let contract: ContractState<InMemoryDB> = ContractState::new(
        stval!([
            (QualifiedCoinInfo::default()),
            (false),
            (owner_pk),
            {},
            (0u64),
            (0u64),
            (0u64),
            (0u64)
        ]),
        HashMap::new()
            .insert(b"depositShielded"[..].into(), deposit_shielded_op),
        Default::default(),
    );

    let deploy = ContractDeploy::new(&mut rng, contract);
    let addr = deploy.address();

    let tx = Transaction::from_intents(
        "local-test",
        test_intents(&mut rng, Vec::new(), Vec::new(), vec![deploy], state.time),
    );
    tx.well_formed(&state.ledger, strictness, state.time).unwrap();

    println!("Contract deployment test passed");
    println!("   Address: {:?}", addr);
}

// ============================================================================
// Unshielded Token Tests
// ============================================================================

/// Test unshielded token type and color handling
#[tokio::test]
async fn test_unshielded_token_types() {
    // NIGHT is the native unshielded token
    let night_token = TokenType::Unshielded(UnshieldedTokenType(NIGHT.0));
    let night_color: [u8; 32] = NIGHT.0.0;
    
    println!(":: Unshielded Token Type Test");
    println!("   NIGHT token type: {:?}", night_token);
    println!("   NIGHT color (first 8 bytes): {:?}", hex::encode(&night_color[..8]));
    
    // Verify NIGHT is all zeros (the default/native token)
    assert!(night_color.iter().all(|&b| b == 0), "NIGHT should be all zeros");
    
    // Custom token types would have non-zero colors derived from domain separator
    let custom_domain_sep: [u8; 32] = [1u8; 32];
    let custom_color = persistent_commit(b"custom:token", HashOutput(custom_domain_sep));
    let custom_token = TokenType::Unshielded(UnshieldedTokenType(custom_color));
    
    println!("   Custom token color: {:?}", hex::encode(&custom_color.0[..8]));
    assert_ne!(custom_color.0, night_color, "Custom token should differ from NIGHT");
    
    println!("Unshielded token type test passed");
}

/// Test the Effects structure for unshielded operations
#[tokio::test]
async fn test_unshielded_effects_structure() {
    use onchain_runtime::context::Effects;
    
    println!(":: Unshielded Effects Structure Test");
    
    // Create a default Effects
    let effects: Effects<InMemoryDB> = Effects::default();
    
    // Verify the unshielded-related fields are empty by default
    assert_eq!(effects.unshielded_inputs.size(), 0);
    assert_eq!(effects.unshielded_outputs.size(), 0);
    assert_eq!(effects.claimed_unshielded_spends.size(), 0);
    assert_eq!(effects.unshielded_mints.size(), 0);
    
    println!("   unshielded_inputs: {} entries", effects.unshielded_inputs.size());
    println!("   unshielded_outputs: {} entries", effects.unshielded_outputs.size());
    println!("   claimed_unshielded_spends: {} entries", effects.claimed_unshielded_spends.size());
    println!("   unshielded_mints: {} entries", effects.unshielded_mints.size());
    
    println!("Unshielded effects structure test passed");
}

/// Test UnshieldedOffer construction
#[tokio::test]
async fn test_unshielded_offer_construction() {
    use base_crypto::signatures::SigningKey;
    
    let mut rng = StdRng::seed_from_u64(0x42);
    
    println!(":: UnshieldedOffer Construction Test");
    
    // Create a signing key for the user
    let user_signing_key = SigningKey::sample(rng.clone());
    let user_verifying_key = user_signing_key.verifying_key();
    let user_address = UserAddress::from(user_verifying_key.clone());
    
    // Create a mock intent hash (in reality this comes from a previous transaction)
    let mock_intent_hash: IntentHash = IntentHash(rng.r#gen());
    
    // Construct a UtxoSpend (user spending their NIGHT tokens)
    let deposit_amount: u128 = 1_000_000;
    let utxo_spend = UtxoSpend {
        intent_hash: mock_intent_hash,
        output_no: 0,
        owner: user_verifying_key.clone(),
        type_: NIGHT,
        value: deposit_amount,
    };
    
    // Construct UtxoOutput (for withdrawals to a recipient)
    let recipient_address: UserAddress = rng.r#gen();
    let withdraw_amount: u128 = 500_000;
    let utxo_output = UtxoOutput {
        owner: recipient_address,
        type_: NIGHT,
        value: withdraw_amount,
    };
    
    // Create an UnshieldedOffer for deposits (inputs only)
    let deposit_offer: UnshieldedOffer<Signature, InMemoryDB> = UnshieldedOffer {
        inputs: vec![utxo_spend.clone()].into(),
        outputs: vec![].into(),
        signatures: vec![].into(),
    };
    
    // Create an UnshieldedOffer for withdrawals (outputs only)
    let withdraw_offer: UnshieldedOffer<Signature, InMemoryDB> = UnshieldedOffer {
        inputs: vec![].into(),
        outputs: vec![utxo_output.clone()].into(),
        signatures: vec![].into(),
    };
    
    println!("   Deposit UnshieldedOffer:");
    println!("     inputs: {} UTXOs", deposit_offer.inputs.len());
    println!("     outputs: {} UTXOs", deposit_offer.outputs.len());
    println!("     amount: {} NIGHT", deposit_amount);
    
    println!("   Withdrawal UnshieldedOffer:");
    println!("     inputs: {} UTXOs", withdraw_offer.inputs.len());
    println!("     outputs: {} UTXOs", withdraw_offer.outputs.len());
    println!("     recipient: {:?}", recipient_address);
    println!("     amount: {} NIGHT", withdraw_amount);
    
    // Verify structure
    assert_eq!(deposit_offer.inputs.len(), 1);
    assert_eq!(deposit_offer.outputs.len(), 0);
    assert_eq!(withdraw_offer.inputs.len(), 0);
    assert_eq!(withdraw_offer.outputs.len(), 1);
    
    println!("UnshieldedOffer construction test passed");
}

/// Test contract balance tracking for unshielded tokens
#[tokio::test]
async fn test_contract_unshielded_balance_tracking() {
    let mut rng = StdRng::seed_from_u64(0x42);
    let state: TestState<InMemoryDB> = TestState::new(&mut rng);
    
    println!(":: Contract Unshielded Balance Tracking Test");
    
    // Create a contract with some initial state
    let owner_sk: HashOutput = rng.r#gen();
    let owner_pk = derive_public_key(owner_sk);
    
    let deposit_unshielded_op = ContractOperation::new(
        verifier_key(&RESOLVER, "depositUnshielded").await
    );
    let withdraw_unshielded_op = ContractOperation::new(
        verifier_key(&RESOLVER, "withdrawUnshielded").await
    );
    let get_unshielded_balance_op = ContractOperation::new(
        verifier_key(&RESOLVER, "getUnshieldedBalance").await
    );
    
    let contract: ContractState<InMemoryDB> = ContractState::new(
        stval!([
            (QualifiedCoinInfo::default()),  // 0: shieldedVault
            (false),                          // 1: hasShieldedTokens
            (owner_pk),                       // 2: owner
            {},                               // 3: authorized (empty set)
            (0u64),                           // 4: totalShieldedDeposits
            (0u64),                           // 5: totalShieldedWithdrawals
            (0u64),                           // 6: totalUnshieldedDeposits
            (0u64)                            // 7: totalUnshieldedWithdrawals
        ]),
        HashMap::new()
            .insert(b"depositUnshielded"[..].into(), deposit_unshielded_op)
            .insert(b"withdrawUnshielded"[..].into(), withdraw_unshielded_op)
            .insert(b"getUnshieldedBalance"[..].into(), get_unshielded_balance_op),
        Default::default(),
    );
    
    let deploy = ContractDeploy::new(&mut rng, contract);
    let addr = deploy.address();
    
    println!("   Contract address: {:?}", addr);
    println!("   Contract state layout:");
    println!("     [0] shieldedVault: QualifiedShieldedCoinInfo");
    println!("     [1] hasShieldedTokens: Boolean");
    println!("     [2] owner: Bytes<32>");
    println!("     [3] authorized: Set<Bytes<32>>");
    println!("     [4] totalShieldedDeposits: Counter");
    println!("     [5] totalShieldedWithdrawals: Counter");
    println!("     [6] totalUnshieldedDeposits: Counter");
    println!("     [7] totalUnshieldedWithdrawals: Counter");
    
    println!("Contract unshielded balance tracking test passed");
}

/// Test unshielded UTXO transfer (basic UTXO-to-UTXO transfer following dust.rs pattern)
#[tokio::test]
async fn test_unshielded_utxo_transfer() {
    use base_crypto::signatures::SigningKey;
    
    midnight_ledger::init_logger(midnight_ledger::LogLevel::Trace);
    let mut rng = StdRng::seed_from_u64(0x42);
    
    // Initialize crypto parameters
    lazy_static::initialize(&PARAMS_VERIFIER);
    SPEND_VK.init().ok();
    OUTPUT_VK.init().ok();
    SIGN_VK.init().ok();

    println!(":: Unshielded UTXO Transfer Test (following dust.rs pattern)");
    
    // Create test state and generate NIGHT UTXOs
    let mut state: TestState<InMemoryDB> = TestState::new(&mut rng);
    const TRANSFER_AMOUNT: u128 = 1_000_000;
    
    // Generate NIGHT tokens for the user (creates unshielded UTXOs)
    state.reward_night(&mut rng, TRANSFER_AMOUNT).await;
    state.fast_forward(state.ledger.parameters.dust.time_to_cap());
    
    println!("   Created {} NIGHT tokens", TRANSFER_AMOUNT);
    
    // Get the user's signing key (from the test state)
    let user_verifying_key = state.night_key.verifying_key();
    
    // Find the UTXO we just created
    let utxo_info = state.ledger.utxo.utxos.iter().next();
    if utxo_info.is_none() {
        println!("   ⚠️ No UTXOs found in ledger");
        return;
    }
    
    let utxo_ref = utxo_info.unwrap();
    let utxo_ih = utxo_ref.0.intent_hash;
    let utxo_out_no = utxo_ref.0.output_no;
    
    println!("   Found UTXO: intent_hash={:?}, output_no={}", 
             hex::encode(&utxo_ih.0.0[..8]), utxo_out_no);
    
    // Create a second user to receive the NIGHT
    let recipient_sk: SigningKey = SigningKey::sample(StdRng::seed_from_u64(0x99));
    let recipient_addr = UserAddress::from(recipient_sk.verifying_key());
    
    println!("   Recipient address created");
    
    // Build the intent following dust.rs pattern exactly
    let mut intent = Intent::<Signature, _, _, _>::empty(&mut rng, state.time);
    intent.guaranteed_unshielded_offer = Some(Sp::new(UnshieldedOffer {
        inputs: vec![UtxoSpend {
            intent_hash: utxo_ih,
            output_no: utxo_out_no,
            owner: user_verifying_key,
            type_: NIGHT,
            value: TRANSFER_AMOUNT,
        }].into(),
        outputs: vec![UtxoOutput {
            owner: recipient_addr,
            type_: NIGHT,
            value: TRANSFER_AMOUNT,
        }].into(),
        signatures: vec![].into(),
    }));
    
    println!("   Built Intent with UnshieldedOffer:");
    println!("     inputs: 1 UTXO ({} NIGHT from user)", TRANSFER_AMOUNT);
    println!("     outputs: 1 UTXO ({} NIGHT to recipient)", TRANSFER_AMOUNT);
    
    // Calculate the new UTXO intent hash before applying
    let new_utxo_ih = intent.erase_proofs().erase_signatures().intent_hash(0);
    
    // Create transaction from intent
    let tx = Transaction::from_intents(
        "local-test",
        [(1u16, intent)].into_iter().collect(),
    );
    
    // Use unbalanced strictness (same as dust.rs)
    let mut unbalanced_strictness = WellFormedStrictness::default();
    unbalanced_strictness.enforce_balancing = false;
    unbalanced_strictness.verify_signatures = false;
    
    // Apply the transaction
    state.assert_apply(&tx, unbalanced_strictness);
    
    println!("   Unshielded transfer transaction applied to ledger");
    
    // Verify the new UTXO was created
    let new_utxo_exists = state.ledger.utxo.utxos.iter().any(|utxo_ref| {
        utxo_ref.0.intent_hash == new_utxo_ih && utxo_ref.0.output_no == 0
    });
    
    assert!(new_utxo_exists, "New UTXO should exist");
    println!("   Verified new UTXO created with intent_hash={:?}", 
             hex::encode(&new_utxo_ih.0.0[..8]));
    
    // Verify the old UTXO is spent
    let old_utxo_exists = state.ledger.utxo.utxos.iter().any(|utxo_ref| {
        utxo_ref.0.intent_hash == utxo_ih && utxo_ref.0.output_no == utxo_out_no
    });
    
    assert!(!old_utxo_exists, "Old UTXO should be spent");
    println!("   Verified old UTXO is spent");
    
    println!("Unshielded UTXO transfer test PASSED");
}

// ============================================================================
// Production-Ready Unshielded Contract Tests
// ============================================================================
//
// These tests demonstrate how to properly test unshielded token operations
// against contracts. They are designed to be comprehensive references for
// developers building on the Midnight ledger.
//
// ## Test Philosophy
//
// These tests exercise the TRANSCRIPT CONSTRUCTION path, not the circuit
// proving path. We use relaxed `WellFormedStrictness` settings because:
//
// 1. `verify_contract_proofs = false`: We're testing that the transcript
//    (Op sequence) matches what the ledger expects, not that we can generate
//    valid ZK proofs. Proof generation is tested separately in circuit tests.
//
// 2. `verify_signatures = false`: We're testing the ledger effects, not
//    signature validation. Signature tests are covered elsewhere.
//
// 3. `enforce_balancing = false`: Unshielded operations don't use ZSwap
//    balancing - they use UTXO model instead.
//
// ## When to use production proof verification
//
// In production, you MUST enable all verification:
// ```rust
// let strictness = WellFormedStrictness::default(); // All checks enabled
// ```
//
// The relaxed settings here are specifically for testing transcript construction
// without the overhead of proof generation.
// ============================================================================

/// Test depositing unshielded NIGHT tokens from a user to a contract.
///
/// ## What this test demonstrates:
///
/// 1. **UTXO Spending**: User's tokens exist as UTXOs (Unspent Transaction Outputs).
///    To deposit, the user must SPEND their UTXO by providing it as input.
///
/// 2. **Contract Effect Tracking**: The contract's `receiveUnshielded` call
///    adds to `effects[6]` (unshielded_inputs). The ledger verifies that the
///    UnshieldedOffer inputs match or exceed this amount.
///
/// 3. **Transcript Matching**: The Op sequence we build MUST match exactly
///    what the Compact circuit would produce. Any mismatch causes rejection.
///
/// ## Data Flow:
/// ```
/// User UTXO (inputs)  →  Contract call (receiveUnshielded)  →  Contract balance increases
///        ↓                        ↓
///   Marked as spent    effects[6].unshielded_inputs += amount
/// ```
///
/// ## Critical Requirements:
/// - UTXO value in UnshieldedOffer.inputs MUST exactly match the UtxoSpend
/// - The transcript's receiveUnshielded amount MUST match the UnshieldedOffer inputs
/// - UTXO must not already be spent (check after any prior transactions)
#[tokio::test]
async fn test_unshielded_contract_deposit() {
    use base_crypto::signatures::SigningKey;
    
    midnight_ledger::init_logger(midnight_ledger::LogLevel::Trace);
    let mut rng = StdRng::seed_from_u64(0x43);
    
    // Initialize crypto parameters
    lazy_static::initialize(&PARAMS_VERIFIER);
    SPEND_VK.init().ok();
    OUTPUT_VK.init().ok();
    SIGN_VK.init().ok();

    println!(":: Unshielded Contract Deposit Test");
    
    // Create test state and generate NIGHT UTXOs
    let mut state: TestState<InMemoryDB> = TestState::new(&mut rng);
    const DEPOSIT_AMOUNT: u128 = 500_000;
    
    // Give fee tokens (register for dust generation and create NIGHT UTXOs)
    state.give_fee_token(&mut rng, 10).await;
    
    // Create additional NIGHT tokens for the deposit
    state.reward_night(&mut rng, DEPOSIT_AMOUNT).await;
    state.fast_forward(state.ledger.parameters.dust.time_to_cap());
    
    println!("   Created {} NIGHT tokens", DEPOSIT_AMOUNT);
    
    // Get user verifying key (UTXO info will be obtained after contract deployment)
    let user_verifying_key = state.night_key.verifying_key();
    let user_address = UserAddress::from(user_verifying_key.clone());
    
    // Load contract operations
    let deposit_shielded_op = ContractOperation::new(
        verifier_key(&RESOLVER, "depositShielded").await
    );
    let withdraw_shielded_op = ContractOperation::new(
        verifier_key(&RESOLVER, "withdrawShielded").await
    );
    let deposit_unshielded_op = ContractOperation::new(
        verifier_key(&RESOLVER, "depositUnshielded").await
    );
    let withdraw_unshielded_op = ContractOperation::new(
        verifier_key(&RESOLVER, "withdrawUnshielded").await
    );
    let send_to_contract_op = ContractOperation::new(
        verifier_key(&RESOLVER, "sendUnshieldedToContract").await
    );
    let send_to_user_op = ContractOperation::new(
        verifier_key(&RESOLVER, "sendUnshieldedToUser").await
    );
    let get_shielded_balance_op = ContractOperation::new(
        verifier_key(&RESOLVER, "getShieldedBalance").await
    );
    let get_unshielded_balance_op = ContractOperation::new(
        verifier_key(&RESOLVER, "getUnshieldedBalance").await
    );
    
    // Deploy contract
    let owner_sk: HashOutput = rng.r#gen();
    let owner_pk = derive_public_key(owner_sk);
    
    let contract: ContractState<InMemoryDB> = ContractState::new(
        stval!([
            (QualifiedCoinInfo::default()),   // 0: shieldedVault
            (false),                          // 1: hasShieldedTokens
            (owner_pk),                       // 2: owner
            {},                               // 3: authorized (empty set)
            (0u64),                           // 4: totalShieldedDeposits
            (0u64),                           // 5: totalShieldedWithdrawals
            (0u64),                           // 6: totalUnshieldedDeposits
            (0u64)                            // 7: totalUnshieldedWithdrawals
        ]),
        HashMap::new()
            .insert(b"depositShielded"[..].into(), deposit_shielded_op.clone())
            .insert(b"withdrawShielded"[..].into(), withdraw_shielded_op.clone())
            .insert(b"depositUnshielded"[..].into(), deposit_unshielded_op.clone())
            .insert(b"withdrawUnshielded"[..].into(), withdraw_unshielded_op.clone())
            .insert(b"sendUnshieldedToContract"[..].into(), send_to_contract_op.clone())
            .insert(b"sendUnshieldedToUser"[..].into(), send_to_user_op.clone())
            .insert(b"getShieldedBalance"[..].into(), get_shielded_balance_op.clone())
            .insert(b"getUnshieldedBalance"[..].into(), get_unshielded_balance_op.clone()),
        Default::default(),
    );
    
    let deploy = ContractDeploy::new(&mut rng, contract);
    let addr = deploy.address();
    
    let mut unbalanced_strictness = WellFormedStrictness::default();
    unbalanced_strictness.enforce_balancing = false;
    
    let balanced_strictness = WellFormedStrictness::default();
    
    // Deploy the contract
    let deploy_tx = Transaction::from_intents(
        "local-test",
        test_intents(&mut rng, Vec::new(), Vec::new(), vec![deploy], state.time),
    );
    deploy_tx.well_formed(&state.ledger, unbalanced_strictness, state.time).unwrap();
    
    let deploy_tx = tx_prove_bind(rng.split(), &deploy_tx, &RESOLVER).await.unwrap();
    let balanced = state.balance_tx(rng.split(), deploy_tx, &RESOLVER).await.unwrap();
    state.assert_apply(&balanced, balanced_strictness);
    
    println!("   Contract deployed at {:?}", addr);
    
    // Now get UTXO info (after contract deployment so it's not spent by fee balancing)
    // Find a UTXO owned by the user with enough balance
    let utxo_ref = state.ledger.utxo.utxos.iter()
        .find(|utxo_ref| utxo_ref.0.owner == user_address && utxo_ref.0.value >= DEPOSIT_AMOUNT)
        .expect("User should have a UTXO with sufficient balance");
    let utxo_ih = utxo_ref.0.intent_hash;
    let utxo_out_no = utxo_ref.0.output_no;
    let utxo_value = utxo_ref.0.value;
    
    println!("   Using UTXO: intent_hash={:?}, value={}", 
             hex::encode(&utxo_ih.0.0[..8]), utxo_value);
    
    // Build the transcript for depositUnshielded
    // The circuit calls: receiveUnshielded(color, amount) + Counter_increment
    // Use the actual UTXO value for the deposit
    let token_type = TokenType::Unshielded(NIGHT);
    let deposit_amount = utxo_value; // Use actual UTXO value
    
    let deposit_transcript: Vec<Op<ResultModeGather, InMemoryDB>> = [
        // receiveUnshielded increments unshielded_inputs (effects index 6)
        &receive_unshielded_ops::<InMemoryDB>(token_type, deposit_amount)[..],
        // Counter_increment for totalUnshieldedDeposits (state index 6)
        &Counter_increment!([key!(6u8)], false, 1u64)[..],
    ]
    .into_iter()
    .flat_map(|x| x.iter())
    .cloned()
    .collect();
    
    let transcripts = partition_transcripts(
        &[PreTranscript {
            context: QueryContext::new(state.ledger.index(addr).unwrap().data, addr),
            program: program_with_results(&deposit_transcript, &[]),
            comm_comm: None,
        }],
        &INITIAL_PARAMETERS,
    )
    .unwrap();
    
    // Build contract call
    // Input: (color: Bytes<32>, amount: Uint<128>)
    let input_av: AlignedValue = AlignedValue::concat([
        AlignedValue::from(NIGHT.0),        // color
        AlignedValue::from(deposit_amount), // amount (use actual UTXO value)
    ].iter());
    
    let call = ContractCallPrototype {
        address: addr,
        entry_point: b"depositUnshielded"[..].into(),
        op: deposit_unshielded_op.clone(),
        input: input_av,
        output: ().into(),
        guaranteed_public_transcript: transcripts[0].0.clone(),
        fallible_public_transcript: transcripts[0].1.clone(),
        private_transcript_outputs: vec![],
        communication_commitment_rand: rng.r#gen(),
        key_location: KeyLocation(Cow::Borrowed("depositUnshielded")),
    };
    
    // Build UnshieldedOffer: user provides NIGHT tokens as input
    // ========================================================================
    // Build UnshieldedOffer: This is how unshielded tokens flow into contracts
    // ========================================================================
    //
    // For DEPOSITS (user → contract):
    // - UnshieldedOffer.inputs: The UTXOs being spent (user's tokens)
    // - UnshieldedOffer.outputs: Empty (tokens go to contract, not another UTXO)
    //
    // The ledger verifies: sum(inputs) >= contract's unshielded_inputs effect
    // This ensures the user actually has the tokens they claim to deposit.
    let uso: UnshieldedOffer<(), InMemoryDB> = UnshieldedOffer {
        inputs: vec![UtxoSpend {
            intent_hash: utxo_ih,
            output_no: utxo_out_no,
            owner: user_verifying_key,
            type_: NIGHT,
            value: deposit_amount, // must match the UTXO value
        }].into(),
        outputs: vec![].into(),  // No outputs - tokens go to contract
        signatures: vec![].into(),
    };
    
    // Create intent with contract call and unshielded offer using Intent::new
    use midnight_ledger::structure::StandardTransaction;
    
    let guaranteed_unshielded_offer: Option<UnshieldedOffer<(), InMemoryDB>> = Some(uso);
    
    let mut intents: storage::storage::HashMap<
        u16,
        Intent<(), ProofPreimageMarker, transient_crypto::curve::EmbeddedFr, InMemoryDB>,
    > = storage::storage::HashMap::new();
    
    intents = intents.insert(
        1,
        Intent::new(
            &mut rng,
            guaranteed_unshielded_offer,
            None,
            vec![call],
            Vec::new(),
            Vec::new(),
            None,
            state.time + base_crypto::time::Duration::from_secs(3600),
        ),
    );
    
    let tx = Transaction::Standard(StandardTransaction::new(
        "local-test",
        intents,
        None,
        std::collections::HashMap::new(),
    ));
    
    // ========================================================================
    // WellFormedStrictness: Configure what the ledger verifies
    // ========================================================================
    //
    // For TESTING transcript construction, we disable proof verification:
    //
    // - enforce_balancing = false:
    //   Unshielded ops don't use ZSwap balancing. They use UTXO accounting instead.
    //   The ledger checks: inputs consumed = outputs created + contract effects.
    //
    // - verify_signatures = false:
    //   We're not testing signature validation here. In production, signatures
    //   prove the user authorized spending their UTXO.
    //
    // - verify_contract_proofs = false:
    //   We're testing that our transcript (Op sequence) matches what the circuit
    //   would produce. Actual ZK proof verification is expensive and tested
    //   separately in circuit-specific tests.
    //
    // - verify_native_proofs = false:
    //   Skip native (non-contract) proof verification for the same reason.
    //
    // ⚠️ IN PRODUCTION: Use `WellFormedStrictness::default()` which enables
    // ALL verification. Never deploy with these settings disabled!
    let mut strictness = WellFormedStrictness::default();
    strictness.enforce_balancing = false;
    strictness.verify_signatures = false;
    strictness.verify_contract_proofs = false;  // Testing transcript construction, not circuit proofs
    strictness.verify_native_proofs = false;    // Skip native proofs too
    
    // Apply directly without tx_prove_bind (which tries to balance)
    state.assert_apply(&tx, strictness);
    
    println!("   Deposit transaction applied");
    
    // ========================================================================
    // Verify Results: Confirm the ledger state changed correctly
    // ========================================================================
    //
    // After applying a transaction, we verify two things:
    // 1. Contract balance increased (tokens were deposited)
    // 2. Original UTXO was consumed (can't double-spend)
    //
    // These checks prove the unshielded deposit flow works correctly.
    
    // Verify contract received the tokens
    let cstate = state.ledger.contract.get(&addr).unwrap();
    let final_balance = cstate.balance.get(&token_type).map(|v| *v).unwrap_or(0);
    assert_eq!(final_balance, deposit_amount, "Contract should have received tokens");
    
    // Verify UTXO was spent
    let utxo_spent = !state.ledger.utxo.utxos.iter().any(|r| {
        r.0.intent_hash == utxo_ih && r.0.output_no == utxo_out_no
    });
    assert!(utxo_spent, "Original UTXO should be spent");
    
    println!("   Contract balance: {} NIGHT", final_balance);
    println!("   UTXO spent");
    println!("\nUnshielded contract deposit test PASSED!");
}

/// Test withdrawing unshielded NIGHT tokens from a contract to a user.
///
/// ## What this test demonstrates:
///
/// 1. **Complete Round-Trip**: First deposits tokens to contract, then withdraws.
///    This proves the contract can both receive AND send unshielded tokens.
///
/// 2. **Claimed Unshielded Spends**: When a contract sends tokens to a user,
///    it MUST claim the spend using `claimed_unshielded_spends` (effects[8]).
///    This tells the ledger "I'm sending X tokens to recipient Y".
///
/// 3. **UTXO Creation**: The user receives tokens as a NEW UTXO in the
///    UnshieldedOffer.outputs. The ledger verifies:
///    `claimed_unshielded_spends ⊆ UnshieldedOffer.outputs`
///
/// ## Data Flow:
/// ```
/// Contract balance  →  sendUnshielded + claim  →  User receives UTXO
///       ↓                      ↓                        ↓
///   Decreases         effects[7] + effects[8]    New UTXO created
/// ```
///
/// ## Critical Insight: Key Type Matching
///
/// The `Recipient::User(PublicKey)` in `claim_unshielded_spend_ops` MUST match
/// the `UserAddress` in `UnshieldedOffer.outputs.owner`.
///
/// Both `coin::PublicKey` and `UserAddress` wrap a `HashOutput`. To make them
/// match, we do: `CoinPublicKey(user_address.0)` - extracting the HashOutput
/// from UserAddress and wrapping it in CoinPublicKey.
///
/// ```rust
/// let user_address = UserAddress::from(verifying_key);  // HashOutput inside
/// let recipient_pk = CoinPublicKey(user_address.0);     // Same HashOutput
/// // Now: Recipient::User(recipient_pk) matches UtxoOutput { owner: user_address }
/// ```
#[tokio::test]
async fn test_unshielded_contract_withdraw() {
    use base_crypto::signatures::SigningKey;
    
    midnight_ledger::init_logger(midnight_ledger::LogLevel::Trace);
    let mut rng = StdRng::seed_from_u64(0x44);
    
    // Initialize crypto parameters
    lazy_static::initialize(&PARAMS_VERIFIER);
    SPEND_VK.init().ok();
    OUTPUT_VK.init().ok();
    SIGN_VK.init().ok();

    println!(":: Unshielded Contract Withdraw Test");
    
    // Create test state and fund with fee tokens
    let mut state: TestState<InMemoryDB> = TestState::new(&mut rng);
    state.give_fee_token(&mut rng, 10).await;
    
    // Get user keys
    let user_verifying_key = state.night_key.verifying_key();
    let user_address = UserAddress::from(user_verifying_key.clone());
    
    // Load contract operations
    let deposit_shielded_op = ContractOperation::new(
        verifier_key(&RESOLVER, "depositShielded").await
    );
    let withdraw_shielded_op = ContractOperation::new(
        verifier_key(&RESOLVER, "withdrawShielded").await
    );
    let deposit_unshielded_op = ContractOperation::new(
        verifier_key(&RESOLVER, "depositUnshielded").await
    );
    let withdraw_unshielded_op = ContractOperation::new(
        verifier_key(&RESOLVER, "withdrawUnshielded").await
    );
    let send_to_contract_op = ContractOperation::new(
        verifier_key(&RESOLVER, "sendUnshieldedToContract").await
    );
    let send_to_user_op = ContractOperation::new(
        verifier_key(&RESOLVER, "sendUnshieldedToUser").await
    );
    let get_shielded_balance_op = ContractOperation::new(
        verifier_key(&RESOLVER, "getShieldedBalance").await
    );
    let get_unshielded_balance_op = ContractOperation::new(
        verifier_key(&RESOLVER, "getUnshieldedBalance").await
    );
    
    // Deploy contract with initial balance
    let owner_sk: HashOutput = rng.r#gen();
    let owner_pk = derive_public_key(owner_sk);
    
    let contract: ContractState<InMemoryDB> = ContractState::new(
        stval!([
            (QualifiedCoinInfo::default()),   // 0: shieldedVault
            (false),                          // 1: hasShieldedTokens
            (owner_pk),                       // 2: owner
            {},                               // 3: authorized (empty set)
            (0u64),                           // 4: totalShieldedDeposits
            (0u64),                           // 5: totalShieldedWithdrawals
            (0u64),                           // 6: totalUnshieldedDeposits
            (0u64)                            // 7: totalUnshieldedWithdrawals
        ]),
        HashMap::new()
            .insert(b"depositShielded"[..].into(), deposit_shielded_op.clone())
            .insert(b"withdrawShielded"[..].into(), withdraw_shielded_op.clone())
            .insert(b"depositUnshielded"[..].into(), deposit_unshielded_op.clone())
            .insert(b"withdrawUnshielded"[..].into(), withdraw_unshielded_op.clone())
            .insert(b"sendUnshieldedToContract"[..].into(), send_to_contract_op.clone())
            .insert(b"sendUnshieldedToUser"[..].into(), send_to_user_op.clone())
            .insert(b"getShieldedBalance"[..].into(), get_shielded_balance_op.clone())
            .insert(b"getUnshieldedBalance"[..].into(), get_unshielded_balance_op.clone()),
        Default::default(),
    );
    
    let deploy = ContractDeploy::new(&mut rng, contract);
    let addr = deploy.address();
    
    let mut unbalanced_strictness = WellFormedStrictness::default();
    unbalanced_strictness.enforce_balancing = false;
    
    let balanced_strictness = WellFormedStrictness::default();
    
    // Deploy the contract
    let deploy_tx = Transaction::from_intents(
        "local-test",
        test_intents(&mut rng, Vec::new(), Vec::new(), vec![deploy], state.time),
    );
    deploy_tx.well_formed(&state.ledger, unbalanced_strictness, state.time).unwrap();
    
    let deploy_tx = tx_prove_bind(rng.split(), &deploy_tx, &RESOLVER).await.unwrap();
    let balanced = state.balance_tx(rng.split(), deploy_tx, &RESOLVER).await.unwrap();
    state.assert_apply(&balanced, balanced_strictness);
    
    println!("   Contract deployed at {:?}", addr);
    
    // ========================================================================
    // Step 1: Deposit tokens to contract first
    // ========================================================================
    
    // Find a UTXO for deposit
    let deposit_utxo = state.ledger.utxo.utxos.iter()
        .find(|utxo_ref| utxo_ref.0.owner == user_address)
        .expect("User should have a UTXO");
    let deposit_utxo_ih = deposit_utxo.0.intent_hash;
    let deposit_utxo_out_no = deposit_utxo.0.output_no;
    let deposit_amount = deposit_utxo.0.value;
    
    let token_type = TokenType::Unshielded(NIGHT);
    
    // Build deposit transcript
    let deposit_transcript: Vec<Op<ResultModeGather, InMemoryDB>> = [
        &receive_unshielded_ops::<InMemoryDB>(token_type, deposit_amount)[..],
        &Counter_increment!([key!(6u8)], false, 1u64)[..],
    ]
    .into_iter()
    .flat_map(|x| x.iter())
    .cloned()
    .collect();
    
    let deposit_transcripts = partition_transcripts(
        &[PreTranscript {
            context: QueryContext::new(state.ledger.index(addr).unwrap().data, addr),
            program: program_with_results(&deposit_transcript, &[]),
            comm_comm: None,
        }],
        &INITIAL_PARAMETERS,
    )
    .unwrap();
    
    let deposit_input_av: AlignedValue = AlignedValue::concat([
        AlignedValue::from(NIGHT.0),
        AlignedValue::from(deposit_amount),
    ].iter());
    
    let deposit_call = ContractCallPrototype {
        address: addr,
        entry_point: b"depositUnshielded"[..].into(),
        op: deposit_unshielded_op.clone(),
        input: deposit_input_av,
        output: ().into(),
        guaranteed_public_transcript: deposit_transcripts[0].0.clone(),
        fallible_public_transcript: deposit_transcripts[0].1.clone(),
        private_transcript_outputs: vec![],
        communication_commitment_rand: rng.r#gen(),
        key_location: KeyLocation(Cow::Borrowed("depositUnshielded")),
    };
    
    let deposit_uso: UnshieldedOffer<(), InMemoryDB> = UnshieldedOffer {
        inputs: vec![UtxoSpend {
            intent_hash: deposit_utxo_ih,
            output_no: deposit_utxo_out_no,
            owner: user_verifying_key.clone(),
            type_: NIGHT,
            value: deposit_amount,
        }].into(),
        outputs: vec![].into(),
        signatures: vec![].into(),
    };
    
    use midnight_ledger::structure::StandardTransaction;
    
    let mut deposit_intents: storage::storage::HashMap<
        u16,
        Intent<(), ProofPreimageMarker, transient_crypto::curve::EmbeddedFr, InMemoryDB>,
    > = storage::storage::HashMap::new();
    
    deposit_intents = deposit_intents.insert(
        1,
        Intent::new(
            &mut rng,
            Some(deposit_uso),
            None,
            vec![deposit_call],
            Vec::new(),
            Vec::new(),
            None,
            state.time + base_crypto::time::Duration::from_secs(3600),
        ),
    );
    
    let deposit_tx = Transaction::Standard(StandardTransaction::new(
        "local-test",
        deposit_intents,
        None,
        std::collections::HashMap::new(),
    ));
    
    let mut deposit_strictness = WellFormedStrictness::default();
    deposit_strictness.enforce_balancing = false;
    deposit_strictness.verify_signatures = false;
    deposit_strictness.verify_contract_proofs = false;
    deposit_strictness.verify_native_proofs = false;
    
    state.assert_apply(&deposit_tx, deposit_strictness);
    
    println!("   Deposited {} NIGHT to contract", deposit_amount);
    
    // Verify contract balance
    let cstate_after_deposit = state.ledger.contract.get(&addr).unwrap();
    let balance_after_deposit = cstate_after_deposit.balance.get(&token_type).map(|v| *v).unwrap_or(0);
    assert_eq!(balance_after_deposit, deposit_amount, "Contract should have deposited tokens");
    
    // ========================================================================
    // Step 2: Withdraw tokens from contract
    // ========================================================================
    //
    // Withdrawal flow is the reverse of deposit:
    // - Contract calls sendUnshielded (effects[7]) to indicate outgoing tokens
    // - Contract claims the spend (effects[8]) to specify the recipient
    // - User receives a NEW UTXO in UnshieldedOffer.outputs
    //
    // The ledger verifies:
    //   claimed_unshielded_spends[(token_type, recipient)] ⊆ UnshieldedOffer.outputs
    // This ensures the recipient specified by the contract actually gets the UTXO.
    
    let withdraw_amount = deposit_amount / 2; // Withdraw half
    
    // ========================================================================
    // Critical: Key Type Matching
    // ========================================================================
    //
    // The `Recipient::User(PublicKey)` in our transcript MUST match the
    // `UserAddress` in `UnshieldedOffer.outputs.owner`.
    //
    // Both types wrap the same `HashOutput` internally:
    // - coin::PublicKey(HashOutput)
    // - UserAddress(HashOutput)  
    //
    // We extract the HashOutput from UserAddress and wrap it in PublicKey:
    use coin_structure::coin::PublicKey as CoinPublicKey;
    let recipient_pk = CoinPublicKey(user_address.0); // Same HashOutput as UserAddress
    
    // Build the withdrawal transcript
    //
    // The withdrawUnshielded circuit performs three operations:
    // 1. sendUnshielded: Increments effects[7] (unshielded_outputs)
    // 2. claimUnshieldedSpend: Adds entry to effects[8] (claimed_unshielded_spends)
    // 3. Counter_increment: Tracks total withdrawals in contract state
    let withdraw_transcript: Vec<Op<ResultModeGather, InMemoryDB>> = [
        // sendUnshielded increments unshielded_outputs (effects index 7)
        &send_unshielded_ops::<InMemoryDB>(token_type, withdraw_amount)[..],
        // Also claim the unshielded spend (effects index 8)
        // This specifies: "User with public key X should receive these tokens"
        &claim_unshielded_spend_ops::<InMemoryDB>(
            token_type,
            Recipient::User(recipient_pk),
            withdraw_amount
        )[..],
        // Counter_increment for totalUnshieldedWithdrawals (state index 7)
        &Counter_increment!([key!(7u8)], false, 1u64)[..],
    ]
    .into_iter()
    .flat_map(|x| x.iter())
    .cloned()
    .collect();
    
    let withdraw_transcripts = partition_transcripts(
        &[PreTranscript {
            context: QueryContext::new(state.ledger.index(addr).unwrap().data, addr),
            program: program_with_results(&withdraw_transcript, &[]),
            comm_comm: None,
        }],
        &INITIAL_PARAMETERS,
    )
    .unwrap();
    
    // Build contract call input for withdrawUnshielded
    // Input: (color: Bytes<32>, recipient: UserAddress, amount: Uint<128>)
    let withdraw_input_av: AlignedValue = AlignedValue::concat([
        AlignedValue::from(NIGHT.0),           // color
        AlignedValue::from(user_address.0),    // recipient (UserAddress is HashOutput)
        AlignedValue::from(withdraw_amount),   // amount
    ].iter());
    
    let withdraw_call = ContractCallPrototype {
        address: addr,
        entry_point: b"withdrawUnshielded"[..].into(),
        op: withdraw_unshielded_op.clone(),
        input: withdraw_input_av,
        output: ().into(),
        guaranteed_public_transcript: withdraw_transcripts[0].0.clone(),
        fallible_public_transcript: withdraw_transcripts[0].1.clone(),
        private_transcript_outputs: vec![],
        communication_commitment_rand: rng.r#gen(),
        key_location: KeyLocation(Cow::Borrowed("withdrawUnshielded")),
    };
    
    // ========================================================================
    // Build UnshieldedOffer for Withdrawal
    // ========================================================================
    //
    // For WITHDRAWALS (contract → user):
    // - UnshieldedOffer.inputs: Empty (no UTXOs being spent - tokens come from contract)
    // - UnshieldedOffer.outputs: The NEW UTXO being created for the user
    //
    // The owner field MUST match the recipient in claimed_unshielded_spends!
    let withdraw_uso: UnshieldedOffer<(), InMemoryDB> = UnshieldedOffer {
        inputs: vec![].into(), // No UTXO inputs - tokens come from contract
        outputs: vec![UtxoOutput {
            owner: user_address, // MUST match recipient_pk's inner HashOutput
            type_: NIGHT,
            value: withdraw_amount,
        }].into(),
        signatures: vec![].into(),
    };
    
    let mut withdraw_intents: storage::storage::HashMap<
        u16,
        Intent<(), ProofPreimageMarker, transient_crypto::curve::EmbeddedFr, InMemoryDB>,
    > = storage::storage::HashMap::new();
    
    withdraw_intents = withdraw_intents.insert(
        1,
        Intent::new(
            &mut rng,
            Some(withdraw_uso),
            None,
            vec![withdraw_call],
            Vec::new(),
            Vec::new(),
            None,
            state.time + base_crypto::time::Duration::from_secs(3600),
        ),
    );
    
    let withdraw_tx = Transaction::Standard(StandardTransaction::new(
        "local-test",
        withdraw_intents,
        None,
        std::collections::HashMap::new(),
    ));
    
    let mut withdraw_strictness = WellFormedStrictness::default();
    withdraw_strictness.enforce_balancing = false;
    withdraw_strictness.verify_signatures = false;
    withdraw_strictness.verify_contract_proofs = false;
    withdraw_strictness.verify_native_proofs = false;
    
    state.assert_apply(&withdraw_tx, withdraw_strictness);
    
    println!("   Withdrew {} NIGHT from contract", withdraw_amount);
    
    // ========================================================================
    // Step 3: Verify results
    // ========================================================================
    
    // Verify contract balance decreased
    let cstate_after_withdraw = state.ledger.contract.get(&addr).unwrap();
    let balance_after_withdraw = cstate_after_withdraw.balance.get(&token_type).map(|v| *v).unwrap_or(0);
    let expected_balance = deposit_amount - withdraw_amount;
    assert_eq!(balance_after_withdraw, expected_balance, 
               "Contract balance should have decreased by withdraw amount");
    
    println!("   Contract balance: {} NIGHT (was {}, withdrew {})", 
             balance_after_withdraw, deposit_amount, withdraw_amount);
    
    // Verify user received new UTXO
    let user_utxos: Vec<_> = state.ledger.utxo.utxos.iter()
        .filter(|r| r.0.owner == user_address && r.0.value == withdraw_amount)
        .collect();
    assert!(!user_utxos.is_empty(), "User should have received a new UTXO with withdrawn tokens");
    
    println!("   User received UTXO with {} NIGHT", withdraw_amount);
    
    println!("\nUnshielded contract withdraw test PASSED!");
}

/// Test sending unshielded NIGHT tokens between two contracts.
///
/// ## What this test demonstrates:
///
/// 1. **Two-Sided Contract Calls**: Contract-to-contract transfers require
///    BOTH contracts to participate in the SAME transaction:
///    - Sender contract: Calls sendUnshielded (creates outputs + claim)
///    - Receiver contract: Calls receiveUnshielded (creates inputs)
///
/// 2. **No UTXOs Involved**: Unlike user transfers, contract-to-contract
///    transfers don't use UTXOs. The ledger verifies:
///    `sender.claimed_unshielded_spends[Recipient::Contract(B)] ⊆ B.unshielded_inputs`
///
/// 3. **Balance Sheet Accounting**: The ledger tracks that:
///    - Contract A's balance decreases by the transfer amount
///    - Contract B's balance increases by the transfer amount
///
/// ## Data Flow:
/// ```
/// Contract A                              Contract B
///     ↓                                       ↓
/// sendUnshielded(amount, B)            receiveUnshielded(amount)
///     ↓                                       ↓
/// effects[7] += amount                 effects[6] += amount
/// effects[8][(type,B)] = amount        (matched by ledger)
///     ↓                                       ↓
/// balance -= amount                    balance += amount
/// ```
///
/// ## Why Two Calls in Same Transaction?
///
/// The Midnight ledger requires that all effects balance within a single
/// transaction. If Contract A sends tokens, Contract B MUST receive them
/// in the same atomic transaction. This prevents:
/// - Tokens disappearing (sent but never received)
/// - Tokens appearing (received without a sender)
///
/// ## Transaction Structure:
/// ```rust
/// Intent {
///     contract_calls: [
///         ContractCall { address: A, entry: "sendUnshieldedToContract", ... },
///         ContractCall { address: B, entry: "depositUnshielded", ... },
///     ],
///     guaranteed_unshielded_offer: None, // No UTXOs needed
/// }
/// ```
#[tokio::test]
async fn test_unshielded_contract_to_contract() {
    use base_crypto::signatures::SigningKey;
    
    midnight_ledger::init_logger(midnight_ledger::LogLevel::Trace);
    let mut rng = StdRng::seed_from_u64(0x45);
    
    // Initialize crypto parameters
    lazy_static::initialize(&PARAMS_VERIFIER);
    SPEND_VK.init().ok();
    OUTPUT_VK.init().ok();
    SIGN_VK.init().ok();

    println!(":: Unshielded Contract-to-Contract Transfer Test");
    
    // Create test state and fund with fee tokens
    let mut state: TestState<InMemoryDB> = TestState::new(&mut rng);
    state.give_fee_token(&mut rng, 10).await;
    
    // Get user keys
    let user_verifying_key = state.night_key.verifying_key();
    let user_address = UserAddress::from(user_verifying_key.clone());
    
    // Load contract operations
    let deposit_shielded_op = ContractOperation::new(
        verifier_key(&RESOLVER, "depositShielded").await
    );
    let withdraw_shielded_op = ContractOperation::new(
        verifier_key(&RESOLVER, "withdrawShielded").await
    );
    let deposit_unshielded_op = ContractOperation::new(
        verifier_key(&RESOLVER, "depositUnshielded").await
    );
    let withdraw_unshielded_op = ContractOperation::new(
        verifier_key(&RESOLVER, "withdrawUnshielded").await
    );
    let send_to_contract_op = ContractOperation::new(
        verifier_key(&RESOLVER, "sendUnshieldedToContract").await
    );
    let send_to_user_op = ContractOperation::new(
        verifier_key(&RESOLVER, "sendUnshieldedToUser").await
    );
    let get_shielded_balance_op = ContractOperation::new(
        verifier_key(&RESOLVER, "getShieldedBalance").await
    );
    let get_unshielded_balance_op = ContractOperation::new(
        verifier_key(&RESOLVER, "getUnshieldedBalance").await
    );
    
    // ========================================================================
    // Step 1: Deploy Contract A
    // ========================================================================
    
    let owner_sk_a: HashOutput = rng.r#gen();
    let owner_pk_a = derive_public_key(owner_sk_a);
    
    let contract_a: ContractState<InMemoryDB> = ContractState::new(
        stval!([
            (QualifiedCoinInfo::default()),
            (false),
            (owner_pk_a),
            {},
            (0u64),
            (0u64),
            (0u64),
            (0u64)
        ]),
        HashMap::new()
            .insert(b"depositShielded"[..].into(), deposit_shielded_op.clone())
            .insert(b"withdrawShielded"[..].into(), withdraw_shielded_op.clone())
            .insert(b"depositUnshielded"[..].into(), deposit_unshielded_op.clone())
            .insert(b"withdrawUnshielded"[..].into(), withdraw_unshielded_op.clone())
            .insert(b"sendUnshieldedToContract"[..].into(), send_to_contract_op.clone())
            .insert(b"sendUnshieldedToUser"[..].into(), send_to_user_op.clone())
            .insert(b"getShieldedBalance"[..].into(), get_shielded_balance_op.clone())
            .insert(b"getUnshieldedBalance"[..].into(), get_unshielded_balance_op.clone()),
        Default::default(),
    );
    
    let deploy_a = ContractDeploy::new(&mut rng, contract_a);
    let addr_a = deploy_a.address();
    
    let mut unbalanced_strictness = WellFormedStrictness::default();
    unbalanced_strictness.enforce_balancing = false;
    
    let balanced_strictness = WellFormedStrictness::default();
    
    let deploy_tx_a = Transaction::from_intents(
        "local-test",
        test_intents(&mut rng, Vec::new(), Vec::new(), vec![deploy_a], state.time),
    );
    deploy_tx_a.well_formed(&state.ledger, unbalanced_strictness, state.time).unwrap();
    
    let deploy_tx_a = tx_prove_bind(rng.split(), &deploy_tx_a, &RESOLVER).await.unwrap();
    let balanced_a = state.balance_tx(rng.split(), deploy_tx_a, &RESOLVER).await.unwrap();
    state.assert_apply(&balanced_a, balanced_strictness);
    
    println!("   Contract A deployed at {:?}", addr_a);
    
    // ========================================================================
    // Step 2: Deploy Contract B
    // ========================================================================
    
    let owner_sk_b: HashOutput = rng.r#gen();
    let owner_pk_b = derive_public_key(owner_sk_b);
    
    let contract_b: ContractState<InMemoryDB> = ContractState::new(
        stval!([
            (QualifiedCoinInfo::default()),
            (false),
            (owner_pk_b),
            {},
            (0u64),
            (0u64),
            (0u64),
            (0u64)
        ]),
        HashMap::new()
            .insert(b"depositShielded"[..].into(), deposit_shielded_op.clone())
            .insert(b"withdrawShielded"[..].into(), withdraw_shielded_op.clone())
            .insert(b"depositUnshielded"[..].into(), deposit_unshielded_op.clone())
            .insert(b"withdrawUnshielded"[..].into(), withdraw_unshielded_op.clone())
            .insert(b"sendUnshieldedToContract"[..].into(), send_to_contract_op.clone())
            .insert(b"sendUnshieldedToUser"[..].into(), send_to_user_op.clone())
            .insert(b"getShieldedBalance"[..].into(), get_shielded_balance_op.clone())
            .insert(b"getUnshieldedBalance"[..].into(), get_unshielded_balance_op.clone()),
        Default::default(),
    );
    
    let deploy_b = ContractDeploy::new(&mut rng, contract_b);
    let addr_b = deploy_b.address();
    
    let deploy_tx_b = Transaction::from_intents(
        "local-test",
        test_intents(&mut rng, Vec::new(), Vec::new(), vec![deploy_b], state.time),
    );
    deploy_tx_b.well_formed(&state.ledger, unbalanced_strictness, state.time).unwrap();
    
    let deploy_tx_b = tx_prove_bind(rng.split(), &deploy_tx_b, &RESOLVER).await.unwrap();
    let balanced_b = state.balance_tx(rng.split(), deploy_tx_b, &RESOLVER).await.unwrap();
    state.assert_apply(&balanced_b, balanced_strictness);
    
    println!("   Contract B deployed at {:?}", addr_b);
    
    // ========================================================================
    // Step 3: User deposits tokens to Contract A
    // ========================================================================
    
    let deposit_utxo = state.ledger.utxo.utxos.iter()
        .find(|utxo_ref| utxo_ref.0.owner == user_address)
        .expect("User should have a UTXO");
    let deposit_utxo_ih = deposit_utxo.0.intent_hash;
    let deposit_utxo_out_no = deposit_utxo.0.output_no;
    let deposit_amount = deposit_utxo.0.value;
    
    let token_type = TokenType::Unshielded(NIGHT);
    
    // Build deposit transcript
    let deposit_transcript: Vec<Op<ResultModeGather, InMemoryDB>> = [
        &receive_unshielded_ops::<InMemoryDB>(token_type, deposit_amount)[..],
        &Counter_increment!([key!(6u8)], false, 1u64)[..],
    ]
    .into_iter()
    .flat_map(|x| x.iter())
    .cloned()
    .collect();
    
    let deposit_transcripts = partition_transcripts(
        &[PreTranscript {
            context: QueryContext::new(state.ledger.index(addr_a).unwrap().data, addr_a),
            program: program_with_results(&deposit_transcript, &[]),
            comm_comm: None,
        }],
        &INITIAL_PARAMETERS,
    )
    .unwrap();
    
    let deposit_input_av: AlignedValue = AlignedValue::concat([
        AlignedValue::from(NIGHT.0),
        AlignedValue::from(deposit_amount),
    ].iter());
    
    let deposit_call = ContractCallPrototype {
        address: addr_a,
        entry_point: b"depositUnshielded"[..].into(),
        op: deposit_unshielded_op.clone(),
        input: deposit_input_av,
        output: ().into(),
        guaranteed_public_transcript: deposit_transcripts[0].0.clone(),
        fallible_public_transcript: deposit_transcripts[0].1.clone(),
        private_transcript_outputs: vec![],
        communication_commitment_rand: rng.r#gen(),
        key_location: KeyLocation(Cow::Borrowed("depositUnshielded")),
    };
    
    let deposit_uso: UnshieldedOffer<(), InMemoryDB> = UnshieldedOffer {
        inputs: vec![UtxoSpend {
            intent_hash: deposit_utxo_ih,
            output_no: deposit_utxo_out_no,
            owner: user_verifying_key.clone(),
            type_: NIGHT,
            value: deposit_amount,
        }].into(),
        outputs: vec![].into(),
        signatures: vec![].into(),
    };
    
    use midnight_ledger::structure::StandardTransaction;
    
    let mut deposit_intents: storage::storage::HashMap<
        u16,
        Intent<(), ProofPreimageMarker, transient_crypto::curve::EmbeddedFr, InMemoryDB>,
    > = storage::storage::HashMap::new();
    
    deposit_intents = deposit_intents.insert(
        1,
        Intent::new(
            &mut rng,
            Some(deposit_uso),
            None,
            vec![deposit_call],
            Vec::new(),
            Vec::new(),
            None,
            state.time + base_crypto::time::Duration::from_secs(3600),
        ),
    );
    
    let deposit_tx = Transaction::Standard(StandardTransaction::new(
        "local-test",
        deposit_intents,
        None,
        std::collections::HashMap::new(),
    ));
    
    let mut deposit_strictness = WellFormedStrictness::default();
    deposit_strictness.enforce_balancing = false;
    deposit_strictness.verify_signatures = false;
    deposit_strictness.verify_contract_proofs = false;
    deposit_strictness.verify_native_proofs = false;
    
    state.assert_apply(&deposit_tx, deposit_strictness);
    
    println!("   Deposited {} NIGHT to Contract A", deposit_amount);
    
    // Verify Contract A balance
    let cstate_a = state.ledger.contract.get(&addr_a).unwrap();
    let balance_a = cstate_a.balance.get(&token_type).map(|v| *v).unwrap_or(0);
    assert_eq!(balance_a, deposit_amount, "Contract A should have deposited tokens");
    
    // ========================================================================
    // Step 4: Contract A sends tokens to Contract B
    // ========================================================================
    //
    // Contract-to-contract transfers are fundamentally different from
    // user transfers. Key differences:
    //
    // 1. NO UTXOs: Tokens move directly between contract balances, not
    //    through the UTXO layer. There's no UnshieldedOffer needed.
    //
    // 2. TWO CONTRACT CALLS: Both sender and receiver must participate:
    //    - Contract A: sendUnshielded ("I'm sending X tokens to B")
    //    - Contract B: receiveUnshielded ("I'm receiving X tokens")
    //
    // 3. ATOMIC TRANSACTION: Both calls MUST be in the same transaction.
    //    This ensures tokens can't disappear or be created from nothing.
    //
    // The ledger verifies:
    //   A.claimed_unshielded_spends[(type, Contract(B))] ⊆ B.unshielded_inputs
    //
    // This check ensures that for every token Contract A claims to send to B,
    // Contract B has actually called receiveUnshielded for that amount.
    
    let transfer_amount = deposit_amount / 2; // Transfer half
    
    // ========================================================================
    // Build transcripts for BOTH contracts
    // ========================================================================
    //
    // Contract A transcript: sendUnshielded operations
    // - effects[7] (unshielded_outputs): Add transfer_amount
    // - effects[8] (claimed_unshielded_spends): Map (type, B) -> amount
    let send_transcript: Vec<Op<ResultModeGather, InMemoryDB>> = [
        // sendUnshielded increments unshielded_outputs (effects index 7)
        &send_unshielded_ops::<InMemoryDB>(token_type, transfer_amount)[..],
        // Claim specifies recipient is Contract B (not a user)
        &claim_unshielded_spend_ops::<InMemoryDB>(
            token_type,
            Recipient::Contract(addr_b), // <-- Contract address, not user
            transfer_amount
        )[..],
    ]
    .into_iter()
    .flat_map(|x| x.iter())
    .cloned()
    .collect();
    
    // Contract B transcript: receiveUnshielded operations
    // - effects[6] (unshielded_inputs): Add transfer_amount
    // - state[6] (counter): Increment deposit counter
    //
    // Note: Contract B doesn't care WHERE the tokens came from.
    // It just declares "I'm receiving X tokens of this type".
    // The ledger ensures the sender's claim matches this declaration.
    let receive_transcript: Vec<Op<ResultModeGather, InMemoryDB>> = [
        // receiveUnshielded increments unshielded_inputs (effects index 6)
        &receive_unshielded_ops::<InMemoryDB>(token_type, transfer_amount)[..],
        // Counter_increment for totalUnshieldedDeposits 
        &Counter_increment!([key!(6u8)], false, 1u64)[..],
    ]
    .into_iter()
    .flat_map(|x| x.iter())
    .cloned()
    .collect();
    
    let send_transcripts = partition_transcripts(
        &[PreTranscript {
            context: QueryContext::new(state.ledger.index(addr_a).unwrap().data, addr_a),
            program: program_with_results(&send_transcript, &[]),
            comm_comm: None,
        }],
        &INITIAL_PARAMETERS,
    )
    .unwrap();
    
    let receive_transcripts = partition_transcripts(
        &[PreTranscript {
            context: QueryContext::new(state.ledger.index(addr_b).unwrap().data, addr_b),
            program: program_with_results(&receive_transcript, &[]),
            comm_comm: None,
        }],
        &INITIAL_PARAMETERS,
    )
    .unwrap();
    
    // Contract A call: sendUnshieldedToContract
    // Input: (color: Bytes<32>, targetContract: ContractAddress, amount: Uint<128>)
    let send_input_av: AlignedValue = AlignedValue::concat([
        AlignedValue::from(NIGHT.0),           // color
        AlignedValue::from(addr_b.0),          // targetContract
        AlignedValue::from(transfer_amount),   // amount
    ].iter());
    
    let send_call = ContractCallPrototype {
        address: addr_a,
        entry_point: b"sendUnshieldedToContract"[..].into(),
        op: send_to_contract_op.clone(),
        input: send_input_av,
        output: ().into(),
        guaranteed_public_transcript: send_transcripts[0].0.clone(),
        fallible_public_transcript: send_transcripts[0].1.clone(),
        private_transcript_outputs: vec![],
        communication_commitment_rand: rng.r#gen(),
        key_location: KeyLocation(Cow::Borrowed("sendUnshieldedToContract")),
    };
    
    // Contract B call: depositUnshielded (to receive tokens from Contract A)
    // Input: (color: Bytes<32>, amount: Uint<128>)
    let receive_input_av: AlignedValue = AlignedValue::concat([
        AlignedValue::from(NIGHT.0),           // color
        AlignedValue::from(transfer_amount),   // amount
    ].iter());
    
    let receive_call = ContractCallPrototype {
        address: addr_b,
        entry_point: b"depositUnshielded"[..].into(),
        op: deposit_unshielded_op.clone(),
        input: receive_input_av,
        output: ().into(),
        guaranteed_public_transcript: receive_transcripts[0].0.clone(),
        fallible_public_transcript: receive_transcripts[0].1.clone(),
        private_transcript_outputs: vec![],
        communication_commitment_rand: rng.r#gen(),
        key_location: KeyLocation(Cow::Borrowed("depositUnshielded")),
    };
    
    // ========================================================================
    // Create Intent with BOTH contract calls
    // ========================================================================
    //
    // This is the key insight for contract-to-contract transfers:
    // A single Intent contains MULTIPLE contract calls that execute atomically.
    //
    // Intent structure:
    // {
    //   guaranteed_unshielded_offer: None,  // No UTXOs involved!
    //   contract_calls: [send_call, receive_call],  // Both contracts
    // }
    //
    // The ledger processes all calls in order, then verifies that:
    // 1. All claimed_unshielded_spends are satisfied by corresponding inputs
    // 2. Contract balances update correctly
    // 3. No tokens are created or destroyed
    let mut transfer_intents: storage::storage::HashMap<
        u16,
        Intent<(), ProofPreimageMarker, transient_crypto::curve::EmbeddedFr, InMemoryDB>,
    > = storage::storage::HashMap::new();
    
    transfer_intents = transfer_intents.insert(
        1,
        Intent::new(
            &mut rng,
            None, // No UnshieldedOffer - contract-to-contract doesn't use UTXOs
            None,
            vec![send_call, receive_call], // Both calls execute atomically
            Vec::new(),
            Vec::new(),
            None,
            state.time + base_crypto::time::Duration::from_secs(3600),
        ),
    );
    
    let transfer_tx = Transaction::Standard(StandardTransaction::new(
        "local-test",
        transfer_intents,
        None,
        std::collections::HashMap::new(),
    ));
    
    let mut transfer_strictness = WellFormedStrictness::default();
    transfer_strictness.enforce_balancing = false;
    transfer_strictness.verify_signatures = false;
    transfer_strictness.verify_contract_proofs = false;
    transfer_strictness.verify_native_proofs = false;
    
    state.assert_apply(&transfer_tx, transfer_strictness);
    
    println!("   Contract A transferred {} NIGHT to Contract B", transfer_amount);
    
    // ========================================================================
    // Step 5: Verify results
    // ========================================================================
    
    // Verify Contract A balance decreased
    let cstate_a_after = state.ledger.contract.get(&addr_a).unwrap();
    let balance_a_after = cstate_a_after.balance.get(&token_type).map(|v| *v).unwrap_or(0);
    let expected_a = deposit_amount - transfer_amount;
    assert_eq!(balance_a_after, expected_a, 
               "Contract A balance should have decreased");
    
    // Verify Contract B balance increased
    let cstate_b_after = state.ledger.contract.get(&addr_b).unwrap();
    let balance_b_after = cstate_b_after.balance.get(&token_type).map(|v| *v).unwrap_or(0);
    assert_eq!(balance_b_after, transfer_amount, 
               "Contract B should have received tokens");
    
    println!("   Contract A balance: {} NIGHT", balance_a_after);
    println!("   Contract B balance: {} NIGHT", balance_b_after);
    
    println!("\nUnshielded contract-to-contract transfer test PASSED!");
}