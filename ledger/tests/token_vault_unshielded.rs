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

//! Token Vault Unshielded Token Tests
//!
//! This module contains integration tests for unshielded token operations in the
//! token-vault contract. Unshielded tokens use the UTXO (Unspent Transaction Output)
//! model, similar to Bitcoin, with transparent balances:
//!
//! - **UTXOs**: Discrete token outputs that can be spent once
//! - **Transparent Balances**: Token amounts are publicly visible
//! - **Contract Accounting**: Ledger tracks contract balances via effects
//!
//! ## Unshielded Token Model
//!
//! Unshielded tokens flow through three key ledger effect maps:
//!
//! - **`unshielded_inputs`** (effects index 6): Tokens flowing INTO a contract
//! - **`unshielded_outputs`** (effects index 7): Tokens flowing OUT OF a contract
//! - **`claimed_unshielded_spends`** (effects index 8): Recipient specifications
//!
//! ## Unshielded Token Flow
//!
//! ### Deposit (User → Contract):
//! 1. User creates UnshieldedOffer with UTXO inputs (spending their tokens)
//! 2. Contract calls `receiveUnshielded()` in its transcript
//! 3. Ledger verifies: UnshieldedOffer inputs >= contract's unshielded_inputs
//! 4. UTXO is consumed, contract balance increases
//!
//! ### Withdrawal (Contract → User):
//! 1. Contract calls `sendUnshielded()` + recipient claim in transcript
//! 2. User creates UnshieldedOffer with outputs (receiving tokens as new UTXO)
//! 3. Ledger verifies: claimed_unshielded_spends matches UnshieldedOffer outputs
//! 4. Contract balance decreases, new UTXO is created for user
//!
//! ### Contract-to-Contract Transfer:
//! 1. Sender contract calls `sendUnshielded()` claiming recipient contract
//! 2. Receiver contract calls `receiveUnshielded()` in the SAME transaction
//! 3. Ledger verifies: sender's claimed_unshielded_spends ⊆ receiver's unshielded_inputs
//! 4. No UTXOs involved - purely internal ledger accounting
//!
//! ## Running Tests
//!
//! ```bash
//! MIDNIGHT_LEDGER_TEST_STATIC_DIR=/path/to/ledger/tests \
//!   cargo test --test token_vault_unshielded -- --test-threads=1
//! ```

#![deny(warnings)]
#![allow(unused_imports)]
#![allow(unused_variables)]

mod token_vault_common;

use token_vault_common::*;

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
    
    //midnight_ledger::init_logger(midnight_ledger::LogLevel::Trace);
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
    
    //midnight_ledger::init_logger(midnight_ledger::LogLevel::Trace);
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
    
    //midnight_ledger::init_logger(midnight_ledger::LogLevel::Trace);
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

// ============================================================================
// REJECTION TESTS: Security Validation of Invalid Transaction Scenarios
// ============================================================================
//
// PURPOSE:
// These tests validate that the Midnight ledger's security mechanisms correctly
// reject malformed or malicious transactions. This is critical for:
// - Preventing token theft or inflation attacks
// - Ensuring atomic consistency in multi-contract operations
// - Validating the integrity of the unshielded token transfer protocol
//
// BACKGROUND - How Unshielded Token Validation Works:
// The ledger performs several validation checks during `well_formed()`:
//
// 1. BALANCE CHECK (enforce_balancing = true):
//    - Ensures that token inputs (UTXOs + mints) equal token outputs (spends + burns)
//    - For each token type, the sum must balance across all intents
//    - Error: `MalformedTransaction::BalanceCheckOverspend`
//
// 2. EFFECTS CHECK:
//    - Validates that transcript effects are consistent with transaction structure
//    - Key validations:
//      a) `claimed_unshielded_spends` must be subset of `real_unshielded_spends`
//         This ensures that if Contract A claims to send tokens to Contract B,
//         then Contract B must have a matching `unshielded_inputs` effect
//      b) Similar checks for nullifiers, commitments, and contract calls
//    - Error: `MalformedTransaction::EffectsCheckFailure`
//
// 3. SIGNATURE/PROOF CHECKS:
//    - Validates cryptographic proofs and signatures
//    - Ensures transcript execution matches claimed effects
//
// SECURITY INVARIANTS TESTED:
// 1. Users cannot deposit more tokens than they actually have in UTXOs
// 2. Contract-to-contract transfers require BOTH parties to participate
// 3. Sender and receiver must agree on the exact token amount being transferred
//
// TEST PATTERN:
// 1. Set up a scenario that SHOULD fail validation
// 2. Build an intentionally invalid transaction
// 3. Call `well_formed()` and verify it returns the expected error type
// 4. Panic if the transaction is unexpectedly accepted (security bug)
// ============================================================================

/// Test that deposit amount mismatch is properly rejected.
///
/// # Security Scenario
/// An attacker attempts to deposit more tokens into a contract than they
/// actually have. They construct a transaction where:
/// - The UnshieldedOffer.inputs contains a UTXO worth N tokens
/// - The transcript's effects.unshielded_inputs claims 2*N tokens
///
/// # Attack Vector
/// If this attack succeeded, the attacker could:
/// - Inflate their balance in the token vault contract
/// - Effectively create tokens out of thin air
/// - Drain value from other users when withdrawing
///
/// # Validation Mechanism
/// The ledger's balance check (enabled via `enforce_balancing = true`) validates:
/// ```text
/// Sum(UnshieldedOffer.inputs[token_type]) == Sum(transcript.effects.unshielded_inputs[token_type])
/// ```
/// When these don't match, the transaction fails with `BalanceCheckOverspend`.
///
/// # Expected Result
/// Transaction MUST be rejected. Any acceptance would be a critical security bug.
///
/// # Ledger Check: Balance Check
/// - Strictness: `enforce_balancing = true`
/// - Error: `MalformedTransaction::BalanceCheckOverspend`
#[tokio::test]
async fn test_rejection_deposit_amount_mismatch() {
    use base_crypto::signatures::SigningKey;
    use midnight_ledger::error::MalformedTransaction;
    
    //midnight_ledger::init_logger(midnight_ledger::LogLevel::Trace);
    let mut rng = StdRng::seed_from_u64(0x50);
    
    lazy_static::initialize(&PARAMS_VERIFIER);
    SPEND_VK.init().ok();
    OUTPUT_VK.init().ok();
    SIGN_VK.init().ok();

    println!(":: Rejection Test: Deposit Amount Mismatch");
    
    let mut state: TestState<InMemoryDB> = TestState::new(&mut rng);
    state.give_fee_token(&mut rng, 10).await;
    
    let user_verifying_key = state.night_key.verifying_key();
    let user_address = UserAddress::from(user_verifying_key.clone());
    
    // Load contract operations
    let deposit_unshielded_op = ContractOperation::new(
        verifier_key(&RESOLVER, "depositUnshielded").await
    );
    
    // Deploy contract
    let owner_sk: HashOutput = rng.r#gen();
    let owner_pk = derive_public_key(owner_sk);
    
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
            .insert(b"depositUnshielded"[..].into(), deposit_unshielded_op.clone()),
        Default::default(),
    );
    
    let deploy = ContractDeploy::new(&mut rng, contract);
    let addr = deploy.address();
    
    let mut unbalanced_strictness = WellFormedStrictness::default();
    unbalanced_strictness.enforce_balancing = false;
    
    let balanced_strictness = WellFormedStrictness::default();
    
    let deploy_tx = Transaction::from_intents(
        "local-test",
        test_intents(&mut rng, Vec::new(), Vec::new(), vec![deploy], state.time),
    );
    let deploy_tx = tx_prove_bind(rng.split(), &deploy_tx, &RESOLVER).await.unwrap();
    let balanced = state.balance_tx(rng.split(), deploy_tx, &RESOLVER).await.unwrap();
    state.assert_apply(&balanced, balanced_strictness);
    
    println!("   Contract deployed");
    
    // Find a UTXO with actual balance
    let utxo = state.ledger.utxo.utxos.iter()
        .find(|utxo_ref| utxo_ref.0.owner == user_address)
        .expect("User should have a UTXO");
    let utxo_ih = utxo.0.intent_hash;
    let utxo_out_no = utxo.0.output_no;
    let actual_value = utxo.0.value;
    
    // Try to claim MORE than the UTXO has
    let claimed_amount = actual_value * 2; // Double the actual amount!
    
    println!("   UTXO has {} tokens, but claiming {}", actual_value, claimed_amount);
    
    let token_type = TokenType::Unshielded(NIGHT);
    
    // Build transcript with INFLATED amount
    let deposit_transcript: Vec<Op<ResultModeGather, InMemoryDB>> = [
        &receive_unshielded_ops::<InMemoryDB>(token_type, claimed_amount)[..], // Claims too much!
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
    
    let input_av: AlignedValue = AlignedValue::concat([
        AlignedValue::from(NIGHT.0),
        AlignedValue::from(claimed_amount), // Claims inflated amount
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
    
    // UnshieldedOffer only provides actual_value, not claimed_amount
    let uso: UnshieldedOffer<(), InMemoryDB> = UnshieldedOffer {
        inputs: vec![UtxoSpend {
            intent_hash: utxo_ih,
            output_no: utxo_out_no,
            owner: user_verifying_key,
            type_: NIGHT,
            value: actual_value, // Only this much is actually available!
        }].into(),
        outputs: vec![].into(),
        signatures: vec![].into(),
    };
    
    use midnight_ledger::structure::StandardTransaction;
    
    let mut intents: storage::storage::HashMap<
        u16,
        Intent<(), ProofPreimageMarker, transient_crypto::curve::EmbeddedFr, InMemoryDB>,
    > = storage::storage::HashMap::new();
    
    intents = intents.insert(
        1,
        Intent::new(
            &mut rng,
            Some(uso),
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
    
    // Test with enforce_balancing = true to catch the mismatch
    // The balance check should reject this because:
    // - UnshieldedOffer.inputs provides actual_value tokens
    // - Transcript.effects.unshielded_inputs claims claimed_amount tokens
    // - These don't match, so the transaction is unbalanced
    let mut strictness = WellFormedStrictness::default();
    strictness.enforce_balancing = true;  // Enable balance check!
    strictness.verify_signatures = false;
    strictness.verify_contract_proofs = false;
    strictness.verify_native_proofs = false;
    
    let result = tx.well_formed(&state.ledger, strictness, state.time);
    
    match result {
        Err(MalformedTransaction::BalanceCheckOverspend { token_type: _, segment: _, overspent_value }) => {
            println!("   Correctly rejected: BalanceCheckOverspend (overspent by {})", overspent_value);
            println!("\n Rejection test (deposit amount mismatch) PASSED!");
        }
        Err(e) => {
            // Any rejection is acceptable - the key is that it's rejected
            println!("   Correctly rejected with error: {:?}", e);
            println!("\n Rejection test (deposit amount mismatch) PASSED!");
        }
        Ok(_) => {
            panic!("   SECURITY BUG: Transaction should have been rejected but was accepted!");
        }
    }
}

/// Test that contract-to-contract transfer fails when receiver is missing.
///
/// # Security Scenario
/// An attacker attempts to steal tokens from Contract A by constructing a
/// transaction where Contract A "sends" tokens to Contract B, but Contract B
/// never actually receives them. The transaction includes:
/// - Contract A's sendUnshieldedToContract call (with claimed_unshielded_spends to B)
/// - NO corresponding receiveUnshielded call from Contract B
///
/// # Attack Vector
/// If this attack succeeded, the attacker could:
/// - Cause Contract A to decrease its balance (tokens "sent")
/// - But no contract receives the tokens (they vanish)
/// - This would violate conservation of tokens
/// - Could be used to grief/drain Contract A's balance
///
/// # Validation Mechanism  
/// The ledger's effects check validates:
/// ```text
/// claimed_unshielded_spends ⊆ real_unshielded_spends
/// ```
/// Where `real_unshielded_spends` is built from:
/// - transcript.effects.unshielded_inputs (what contracts claim to receive)
/// - UnshieldedOffer.outputs (what users claim to receive)
///
/// When Contract A claims to spend to Contract B, but B has no matching
/// unshielded_inputs, the subset check fails.
///
/// # Expected Result
/// Transaction MUST be rejected. This ensures contract-to-contract transfers
/// are atomic: either both sides execute, or neither does.
///
/// # Ledger Check: Effects Check
/// - Validation: `claimed_unshielded_spends` subset of `real_unshielded_spends`
/// - Error: `MalformedTransaction::EffectsCheckFailure(RealUnshieldedSpendsSubsetCheckFailure)`
#[tokio::test]
async fn test_rejection_missing_receiver() {
    use base_crypto::signatures::SigningKey;
    use midnight_ledger::error::{EffectsCheckError, MalformedTransaction};
    
    //midnight_ledger::init_logger(midnight_ledger::LogLevel::Trace);
    let mut rng = StdRng::seed_from_u64(0x51);
    
    lazy_static::initialize(&PARAMS_VERIFIER);
    SPEND_VK.init().ok();
    OUTPUT_VK.init().ok();
    SIGN_VK.init().ok();

    println!(":: Rejection Test: Missing Receiver in Contract-to-Contract");
    
    let mut state: TestState<InMemoryDB> = TestState::new(&mut rng);
    state.give_fee_token(&mut rng, 10).await;
    
    let user_verifying_key = state.night_key.verifying_key();
    let user_address = UserAddress::from(user_verifying_key.clone());
    
    // Load contract operations
    let deposit_unshielded_op = ContractOperation::new(
        verifier_key(&RESOLVER, "depositUnshielded").await
    );
    let send_to_contract_op = ContractOperation::new(
        verifier_key(&RESOLVER, "sendUnshieldedToContract").await
    );
    
    // Deploy Contract A
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
            .insert(b"depositUnshielded"[..].into(), deposit_unshielded_op.clone())
            .insert(b"sendUnshieldedToContract"[..].into(), send_to_contract_op.clone()),
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
    let deploy_tx_a = tx_prove_bind(rng.split(), &deploy_tx_a, &RESOLVER).await.unwrap();
    let balanced_a = state.balance_tx(rng.split(), deploy_tx_a, &RESOLVER).await.unwrap();
    state.assert_apply(&balanced_a, balanced_strictness);
    
    // Deploy Contract B (receiver)
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
            .insert(b"depositUnshielded"[..].into(), deposit_unshielded_op.clone()),
        Default::default(),
    );
    
    let deploy_b = ContractDeploy::new(&mut rng, contract_b);
    let addr_b = deploy_b.address();
    
    let deploy_tx_b = Transaction::from_intents(
        "local-test",
        test_intents(&mut rng, Vec::new(), Vec::new(), vec![deploy_b], state.time),
    );
    let deploy_tx_b = tx_prove_bind(rng.split(), &deploy_tx_b, &RESOLVER).await.unwrap();
    let balanced_b = state.balance_tx(rng.split(), deploy_tx_b, &RESOLVER).await.unwrap();
    state.assert_apply(&balanced_b, balanced_strictness);
    
    println!("   Contract A deployed at {:?}", addr_a);
    println!("   Contract B deployed at {:?}", addr_b);
    
    // First, deposit tokens to Contract A (valid operation)
    let deposit_utxo = state.ledger.utxo.utxos.iter()
        .find(|utxo_ref| utxo_ref.0.owner == user_address)
        .expect("User should have a UTXO");
    let deposit_amount = deposit_utxo.0.value;
    
    let token_type = TokenType::Unshielded(NIGHT);
    
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
            intent_hash: deposit_utxo.0.intent_hash,
            output_no: deposit_utxo.0.output_no,
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
    println!("   Deposited {} tokens to Contract A", deposit_amount);
    
    // Now try to send tokens from A to B WITHOUT including B's receive call
    let transfer_amount = deposit_amount / 2;
    
    // Build send transcript for Contract A only
    let send_transcript: Vec<Op<ResultModeGather, InMemoryDB>> = [
        &send_unshielded_ops::<InMemoryDB>(token_type, transfer_amount)[..],
        &claim_unshielded_spend_ops::<InMemoryDB>(
            token_type,
            Recipient::Contract(addr_b), // Claims to send to B
            transfer_amount
        )[..],
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
    
    let send_input_av: AlignedValue = AlignedValue::concat([
        AlignedValue::from(NIGHT.0),
        AlignedValue::from(addr_b.0),
        AlignedValue::from(transfer_amount),
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
    
    // Create transaction with ONLY the send call - NO receive call from Contract B!
    let mut transfer_intents: storage::storage::HashMap<
        u16,
        Intent<(), ProofPreimageMarker, transient_crypto::curve::EmbeddedFr, InMemoryDB>,
    > = storage::storage::HashMap::new();
    
    transfer_intents = transfer_intents.insert(
        1,
        Intent::new(
            &mut rng,
            None,
            None,
            vec![send_call], // Only sender, no receiver!
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
    
    // This should FAIL - Contract B never called receiveUnshielded
    let result = transfer_tx.well_formed(&state.ledger, transfer_strictness, state.time);
    
    match result {
        Err(MalformedTransaction::EffectsCheckFailure(
            EffectsCheckError::RealUnshieldedSpendsSubsetCheckFailure(_)
        )) => {
            println!("   Correctly rejected: RealUnshieldedSpendsSubsetCheckFailure");
            println!("      (Contract A claimed to send to B, but B didn't receive)");
        }
        Err(e) => {
            println!("   Correctly rejected with error: {:?}", e);
        }
        Ok(_) => {
            panic!("   SECURITY BUG: Transaction should have been rejected!");
        }
    }
    
    println!("\n Rejection test (missing receiver) PASSED!");
}

/// Test that contract-to-contract transfer fails when amounts don't match.
///
/// # Security Scenario
/// An attacker attempts to exploit amount disagreement between sender and
/// receiver contracts. They construct a transaction where:
/// - Contract A's transcript claims to send 1,000,000 tokens to Contract B
///   (via claimed_unshielded_spends)
/// - Contract B's transcript claims to receive only 500,000 tokens
///   (via unshielded_inputs)
///
/// # Attack Vector
/// If this attack succeeded, the attacker could:
/// - Scenario 1 (sender claims more): Contract A loses 1M, B gains 500K, 
///   500K tokens vanish - violates conservation
/// - Scenario 2 (receiver claims more): Contract A loses 500K, B gains 1M,
///   500K tokens created from nothing - token inflation attack
///
/// # Validation Mechanism
/// The effects check builds multisets of claimed spends and real spends:
/// ```text
/// claimed_unshielded_spends = {(segment, token_type, recipient, amount), ...}
/// real_unshielded_spends = {(segment, token_type, recipient, amount), ...}
/// ```
/// The validation requires: `claimed_unshielded_spends ⊆ real_unshielded_spends`
///
/// Since the amounts differ (1M vs 500K), the claimed spend entry doesn't
/// appear in real_unshielded_spends, causing the subset check to fail.
///
/// # Expected Result
/// Transaction MUST be rejected. This ensures sender and receiver always
/// agree on the exact amount being transferred.
///
/// # Ledger Check: Effects Check
/// - Validation: Amount in claimed_unshielded_spends must match unshielded_inputs
/// - Error: `MalformedTransaction::EffectsCheckFailure(RealUnshieldedSpendsSubsetCheckFailure)`
#[tokio::test]
async fn test_rejection_amount_mismatch() {
    use base_crypto::signatures::SigningKey;
    use midnight_ledger::error::{EffectsCheckError, MalformedTransaction};
    
    //midnight_ledger::init_logger(midnight_ledger::LogLevel::Trace);
    let mut rng = StdRng::seed_from_u64(0x52);
    
    lazy_static::initialize(&PARAMS_VERIFIER);
    SPEND_VK.init().ok();
    OUTPUT_VK.init().ok();
    SIGN_VK.init().ok();

    println!(":: Rejection Test: Amount Mismatch in Contract-to-Contract");
    
    let mut state: TestState<InMemoryDB> = TestState::new(&mut rng);
    state.give_fee_token(&mut rng, 10).await;
    
    let user_verifying_key = state.night_key.verifying_key();
    let user_address = UserAddress::from(user_verifying_key.clone());
    
    // Load contract operations
    let deposit_unshielded_op = ContractOperation::new(
        verifier_key(&RESOLVER, "depositUnshielded").await
    );
    let send_to_contract_op = ContractOperation::new(
        verifier_key(&RESOLVER, "sendUnshieldedToContract").await
    );
    
    // Deploy Contract A
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
            .insert(b"depositUnshielded"[..].into(), deposit_unshielded_op.clone())
            .insert(b"sendUnshieldedToContract"[..].into(), send_to_contract_op.clone()),
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
    let deploy_tx_a = tx_prove_bind(rng.split(), &deploy_tx_a, &RESOLVER).await.unwrap();
    let balanced_a = state.balance_tx(rng.split(), deploy_tx_a, &RESOLVER).await.unwrap();
    state.assert_apply(&balanced_a, balanced_strictness);
    
    // Deploy Contract B
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
            .insert(b"depositUnshielded"[..].into(), deposit_unshielded_op.clone()),
        Default::default(),
    );
    
    let deploy_b = ContractDeploy::new(&mut rng, contract_b);
    let addr_b = deploy_b.address();
    
    let deploy_tx_b = Transaction::from_intents(
        "local-test",
        test_intents(&mut rng, Vec::new(), Vec::new(), vec![deploy_b], state.time),
    );
    let deploy_tx_b = tx_prove_bind(rng.split(), &deploy_tx_b, &RESOLVER).await.unwrap();
    let balanced_b = state.balance_tx(rng.split(), deploy_tx_b, &RESOLVER).await.unwrap();
    state.assert_apply(&balanced_b, balanced_strictness);
    
    println!("   Contracts A and B deployed");
    
    // First, deposit tokens to Contract A
    let deposit_utxo = state.ledger.utxo.utxos.iter()
        .find(|utxo_ref| utxo_ref.0.owner == user_address)
        .expect("User should have a UTXO");
    let deposit_amount = deposit_utxo.0.value;
    
    let token_type = TokenType::Unshielded(NIGHT);
    
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
            intent_hash: deposit_utxo.0.intent_hash,
            output_no: deposit_utxo.0.output_no,
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
    println!("   Deposited {} tokens to Contract A", deposit_amount);
    
    // Now try to send with MISMATCHED amounts
    let send_amount = deposit_amount / 2;    // Contract A claims to send this much
    let receive_amount = send_amount / 2;     // Contract B only claims this much (half of what A sends!)
    
    println!("   Contract A sends {} but Contract B only receives {}", send_amount, receive_amount);
    
    // Build send transcript for Contract A
    let send_transcript: Vec<Op<ResultModeGather, InMemoryDB>> = [
        &send_unshielded_ops::<InMemoryDB>(token_type, send_amount)[..],
        &claim_unshielded_spend_ops::<InMemoryDB>(
            token_type,
            Recipient::Contract(addr_b),
            send_amount // A claims to send full amount
        )[..],
    ]
    .into_iter()
    .flat_map(|x| x.iter())
    .cloned()
    .collect();
    
    // Build receive transcript for Contract B with WRONG amount
    let receive_transcript: Vec<Op<ResultModeGather, InMemoryDB>> = [
        &receive_unshielded_ops::<InMemoryDB>(token_type, receive_amount)[..], // Only half!
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
    
    let send_input_av: AlignedValue = AlignedValue::concat([
        AlignedValue::from(NIGHT.0),
        AlignedValue::from(addr_b.0),
        AlignedValue::from(send_amount),
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
    
    let receive_input_av: AlignedValue = AlignedValue::concat([
        AlignedValue::from(NIGHT.0),
        AlignedValue::from(receive_amount), // Mismatched amount
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
    
    // Both calls in the transaction, but amounts don't match
    let mut transfer_intents: storage::storage::HashMap<
        u16,
        Intent<(), ProofPreimageMarker, transient_crypto::curve::EmbeddedFr, InMemoryDB>,
    > = storage::storage::HashMap::new();
    
    transfer_intents = transfer_intents.insert(
        1,
        Intent::new(
            &mut rng,
            None,
            None,
            vec![send_call, receive_call],
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
    
    // This should FAIL - amounts don't match
    let result = transfer_tx.well_formed(&state.ledger, transfer_strictness, state.time);
    
    match result {
        Err(MalformedTransaction::EffectsCheckFailure(
            EffectsCheckError::RealUnshieldedSpendsSubsetCheckFailure(_)
        )) => {
            println!("   Correctly rejected: RealUnshieldedSpendsSubsetCheckFailure");
            println!("      (A sent {} but B only received {})", send_amount, receive_amount);
        }
        Err(e) => {
            println!("   Correctly rejected with error: {:?}", e);
        }
        Ok(_) => {
            panic!("   SECURITY BUG: Transaction should have been rejected!");
        }
    }
    
    println!("\n Rejection test (amount mismatch) PASSED!");
}