// This file is part of midnight-ledger.
// Copyright (C) Midnight Foundation
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

//! # Proving a *particular* deposit, without revealing which
//!
//! `shielded_airdrop.rs` pays an airdrop against the vault's *balance*: a
//! property of the vault, not of the claimer. Anyone may take that payout once
//! the threshold is met, and take it again. This test is the other design --
//! the airdrop pays the depositor of one particular deposit, once -- and the
//! interesting part is that it does so without publishing which deposit that
//! is.
//!
//! The contracts are `deposit-receipt-vault.compact` and
//! `deposit-receipt-airdrop.compact`. Their mechanism:
//!
//!  - Each `depositShielded` inserts a *receipt* into the vault's
//!    `HistoricMerkleTree`: a commitment to (coin nonce, depositor pk, value),
//!    blinded with an opening derived from the depositor's secret key.
//!  - `proveDeposit` proves in zero knowledge that the caller can open a leaf
//!    under a root the tree really had, and returns the receipt to its caller.
//!    Its only public effect is the root check.
//!  - `claimAirdrop` claims that call (`kernel.claimContractCall`), re-checks
//!    the receipt's pk against its own witnessed key, and spends a nullifier.
//!
//! ## What this test proves, and by what
//!
//! Five of the six parts below are enforced by the ledger, with erased proofs:
//!
//!  - the receipt tree and the nullifier set are public VM state, so every
//!    `checkRoot` and `Set.member` result in a transcript is re-executed
//!    against the real chain state at apply time (`Popeq` -> `ReadMismatch`);
//!  - the claimed contract call must be a real call in the same transaction
//!    with the same `(address, entry_point, communication_commitment)`, and
//!    that commitment covers the whole receipt -- so the value the airdrop
//!    tests its threshold against is the value the vault actually returned.
//!
//! What is *not* ledger-enforced, and is called out where it matters: the
//! in-circuit assertions (`r.pk == publicKey(sk)`, the path/leaf equality).
//! Those are proof obligations, and these tests erase proofs. Part 6 shows the
//! nullifier catching the replay that the pk check is the first line against.
//!
//! ## Unlinkability
//!
//! Part 7 checks the privacy claim as a property of the bytes: with two
//! receipts in the tree, the claim transaction contains the root -- shared by
//! both leaves -- and contains neither leaf, neither depositor's public key,
//! nor any secret. That is the whole difference from a `Map<pk, value>`
//! design, where `Map_lookup` would publish the claimer's key.
//!
//! ## Fidelity of the transcripts
//!
//! Both contracts compile, and every transcript below was checked op-for-op
//! against what the compiler emits:
//!
//!     compact compile --feature-zkir-v3 \
//!         ledger/tests/deposit-receipt-vault.compact .build/deposit-receipt-vault
//!
//! and reading the `queryLedgerState` sequences in
//! `.build/*/contract/index.js` (`compact 0.5.1`, compiler 0.34.0). That is
//! worth redoing after a compiler bump: a hand-written transcript the VM
//! accepts is a well-formed program consistent with state, which is not the
//! same as being the program this circuit would produce.
//!
//! The repository's own build has no compactc (the `compactc` inputs in
//! flake.nix are commented out) and builds its test keys from the checked-in
//! `zkir-precompiles`, so the artifacts hold no keys for these circuits and
//! both contracts are deployed with placeholder verifier keys, as in
//! `shielded_airdrop.rs`: `proveDeposit` borrows `getShieldedBalance`'s and
//! `claimAirdrop` borrows `withdrawShielded`'s. Like the rest of the ledger
//! tests, this one runs without `--features proving`, see
//! `zkir-artifacts.rs`.

#![deny(warnings)]
#![allow(unused_imports)]

mod token_vault_common;

use coin_structure::coin::ShieldedTokenType;
use midnight_ledger_v10::construct::communication_commitment;
use midnight_ledger_v10::error::{SubsetCheckFailure, TransactionInvalid};
use midnight_ledger_v10::semantics::TransactionResult;
use midnight_ledger_v10::structure::Signature;
use onchain_runtime::error::TranscriptRejected;
use onchain_runtime::state::{ChargedState, ContractOperation};
use onchain_vm::error::OnchainProgramError;
use token_vault_common::*;
use transient_crypto::merkle_tree::MerklePath;

/// Shielded funds handed to the test wallet up front.
const REWARDS_AMOUNT: u128 = 10_000_000_000;
/// Shielded tokens the airdrop contract has available to pay out.
const AIRDROP_POT: u128 = 5_000_000;
/// Alice's deposit -- over the threshold.
const ALICE_DEPOSIT: u128 = 1_500_000;
/// Bob's deposit -- under it.
const BOB_DEPOSIT: u128 = 800_000;
/// The deposit an airdrop claim must prove (state index 10).
const MIN_DEPOSIT: u128 = 1_000_000;
/// Shielded tokens paid out per claim (state index 11).
const AIRDROP_AMOUNT: u128 = 250_000;

/// Height of the vault's receipt tree, as declared in the contract. The
/// program-fragment macros need it as a literal, so `10` is written out at
/// every `HistoricMerkleTree_*!` below; this keeps the two in step.
const RECEIPT_TREE_HEIGHT: u8 = 10;
const _: () = assert!(RECEIPT_TREE_HEIGHT == 10);

/// Vault state: `token-vault.compact`'s eight fields, then the receipt tree.
const STATE_IDX_RECEIPTS: u8 = 8;

/// Airdrop state: the same eight fields, then the airdrop's own five.
const STATE_IDX_VAULT: u8 = 8;
const STATE_IDX_PROVE_ENTRY_POINT: u8 = 9;
const STATE_IDX_MIN_DEPOSIT: u8 = 10;
const STATE_IDX_AIRDROP_AMOUNT: u8 = 11;
const STATE_IDX_CLAIMED: u8 = 12;

/// The entry point the airdrop requires the receipt proof to come from.
const PROVE_ENTRY_POINT: &str = "proveDeposit";

/// Domain separator of the airdrop's claim nullifier, as `pad(8, "adrp:cn")`.
const CLAIM_NUL_DOMAIN_SEP: [u8; 8] = *b"adrp:cn\0";

// ═══════════════════════════════════════════════════════════════════════════
//  RECEIPTS
// ═══════════════════════════════════════════════════════════════════════════
//
// The Rust side of `deposit-receipt-vault.compact`'s receipt scheme. The
// ledger never recomputes a leaf -- a leaf reaches the chain as a public value
// in the deposit transcript, and a nullifier as a public value in the claim's
// -- so what matters here is that these agree with each other and with the
// field order the circuit's `persistentCommit<Receipt>` would use.

/// What a deposit receipt commits to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Receipt {
    /// The deposited coin's nonce: the deposit's unique handle.
    nonce: HashOutput,
    /// The depositor, as `publicKey(sk)`.
    pk: HashOutput,
    value: u128,
}

impl Receipt {
    /// The receipt the vault records for `coin`, deposited by the holder of
    /// `sk`.
    fn of(coin: &CoinInfo, sk: HashOutput) -> Receipt {
        Receipt {
            nonce: coin.nonce.0,
            pk: derive_public_key(sk),
            value: coin.value,
        }
    }

    /// `receiptOpening(sk, nonce)`: derived, so a wallet stores no blinding
    /// factor of its own and can recompute the leaf from chain data plus `sk`.
    fn opening(&self, sk: HashOutput) -> HashOutput {
        persistent_commit(&self.nonce, sk)
    }

    /// `receiptLeaf(r, opening)`: the value the vault inserts into its tree.
    fn leaf(&self, sk: HashOutput) -> HashOutput {
        persistent_commit(&(self.nonce, self.pk, self.value), self.opening(sk))
    }

    /// The receipt as the circuit's return value, for the communication
    /// commitment.
    fn aligned(&self) -> AlignedValue {
        AlignedValue::concat([
            &AlignedValue::from(self.nonce),
            &AlignedValue::from(self.pk),
            &AlignedValue::from(self.value),
        ])
    }

    /// The commitment the ledger recomputes from `proveDeposit`'s
    /// (input, output) pair: nothing in, this receipt out.
    fn commitment(&self, rand: Fr) -> Fr {
        communication_commitment(().into(), self.aligned(), rand)
    }
}

/// `claimNullifier(nonce, sk)`: one per (deposit, claimer, airdrop).
fn claim_nullifier(nonce: HashOutput, sk: HashOutput) -> HashOutput {
    persistent_commit(&(nonce, CLAIM_NUL_DOMAIN_SEP), sk)
}

fn entry_point_hash(entry_point: &[u8]) -> HashOutput {
    persistent_commit(
        entry_point,
        HashOutput(*b"midnight:entry-point\0\0\0\0\0\0\0\0\0\0\0\0"),
    )
}

// ═══════════════════════════════════════════════════════════════════════════
//  STATE READERS
// ═══════════════════════════════════════════════════════════════════════════

/// The shielded coin a contract currently holds, from its ledger state.
fn shielded_pot(ledger: &LedgerState<InMemoryDB>, addr: ContractAddress) -> QualifiedCoinInfo {
    let cstate = ledger.contract.get(&addr).unwrap();
    let StateValue::Array(arr) = &cstate.data.get_ref() else {
        unreachable!("contract state is an array")
    };
    let Some(StateValue::Cell(pot)) = arr.get(STATE_IDX_SHIELDED_VAULT as usize) else {
        unreachable!("shielded vault is a cell")
    };
    QualifiedCoinInfo::try_from(&*pot.value).unwrap()
}

fn has_shielded_tokens(ledger: &LedgerState<InMemoryDB>, addr: ContractAddress) -> bool {
    let cstate = ledger.contract.get(&addr).unwrap();
    let StateValue::Array(arr) = &cstate.data.get_ref() else {
        unreachable!("contract state is an array")
    };
    let Some(StateValue::Cell(has_tokens)) = arr.get(STATE_IDX_HAS_SHIELDED_TOKENS as usize) else {
        unreachable!("hasShieldedTokens is a cell")
    };
    bool::try_from(&*has_tokens.value).unwrap()
}

fn shielded_balance(ledger: &LedgerState<InMemoryDB>, addr: ContractAddress) -> u128 {
    if has_shielded_tokens(ledger, addr) {
        shielded_pot(ledger, addr).value
    } else {
        0
    }
}

/// The receipt tree of a vault state, real or fabricated.
fn receipt_path(data: &ChargedState<InMemoryDB>, leaf: HashOutput) -> MerklePath<HashOutput> {
    let StateValue::Array(arr) = data.get_ref() else {
        unreachable!("contract state is an array")
    };
    let Some(StateValue::Array(hmt)) = arr.get(STATE_IDX_RECEIPTS as usize) else {
        unreachable!("a HistoricMerkleTree is [tree, next index, roots]")
    };
    let Some(StateValue::BoundedMerkleTree(tree)) = hmt.get(0) else {
        unreachable!("the tree is at index 0")
    };
    tree.find_path_for_leaf(leaf)
        .expect("leaf should be in this tree")
}

/// Whether the nullifier set holds `nul`.
fn is_claimed(ledger: &LedgerState<InMemoryDB>, addr: ContractAddress, nul: HashOutput) -> bool {
    let cstate = ledger.contract.get(&addr).unwrap();
    let StateValue::Array(arr) = &cstate.data.get_ref() else {
        unreachable!("contract state is an array")
    };
    let Some(StateValue::Map(claimed)) = arr.get(STATE_IDX_CLAIMED as usize) else {
        unreachable!("claimed is a set")
    };
    claimed.contains_key(&AlignedValue::from(nul))
}

/// A vault state whose receipt tree has been replaced by one holding just
/// `leaf` -- what an attacker's local simulation would say if they could
/// invent receipts. Everything the deposit half of the state holds is kept, so
/// the only lie is the tree.
fn vault_state_with_forged_tree(
    ledger: &LedgerState<InMemoryDB>,
    addr: ContractAddress,
    leaf: HashOutput,
) -> ChargedState<InMemoryDB> {
    let cstate = ledger.contract.get(&addr).unwrap();
    let StateValue::Array(arr) = cstate.data.get_ref() else {
        unreachable!("contract state is an array")
    };
    let arr = arr.clone();
    let forged_root = MerkleTree::<()>::blank(RECEIPT_TREE_HEIGHT)
        .try_update_hash(0, leaf_hash(&leaf), ())
        .expect("blank tree is not collapsed")
        .rehash()
        .find_path_for_leaf(leaf)
        .expect("just-inserted leaf")
        .root();
    let forged = stval!([
        {MT(RECEIPT_TREE_HEIGHT) { 0u64 => leaf_hash(&leaf) }},
        (1u64),
        { forged_root => null }
    ]);
    ChargedState::new(StateValue::Array(
        arr.insert(STATE_IDX_RECEIPTS as usize, forged)
            .expect("receipt tree index is in range"),
    ))
}

/// An airdrop state whose nullifier set has been emptied -- what a replayed
/// claim's local simulation would say. Everything else is kept, so the only
/// lie is that the receipt has not been claimed yet.
fn airdrop_state_with_cleared_nullifiers(
    ledger: &LedgerState<InMemoryDB>,
    addr: ContractAddress,
) -> ChargedState<InMemoryDB> {
    let cstate = ledger.contract.get(&addr).unwrap();
    let StateValue::Array(arr) = cstate.data.get_ref() else {
        unreachable!("contract state is an array")
    };
    ChargedState::new(StateValue::Array(
        arr.clone()
            .insert(STATE_IDX_CLAIMED as usize, stval!({}))
            .expect("nullifier set index is in range"),
    ))
}

/// Query context over a contract's state -- or over a state it does not have.
fn context_for(
    ledger: &LedgerState<InMemoryDB>,
    addr: ContractAddress,
    data: Option<ChargedState<InMemoryDB>>,
    offer: Option<&ZswapOffer<ProofPreimage, InMemoryDB>>,
) -> QueryContext<InMemoryDB> {
    let data = data.unwrap_or_else(|| ledger.index(addr).unwrap().data);
    let mut res = QueryContext::new(data, addr);
    if let Some(offer) = offer {
        let (_, indices) = ledger.zswap.try_apply(offer, None).unwrap();
        res.call_context.com_indices = indices;
    }
    res
}

// ═══════════════════════════════════════════════════════════════════════════
//  DEPOSITS
// ═══════════════════════════════════════════════════════════════════════════

/// Deposit a fresh shielded coin, and -- when `receipt_sk` is given -- record
/// the depositor's receipt in the receipt tree at index 8.
///
/// Handles both the empty-pot and the merge path of `depositShielded`, so the
/// second depositor's transcript is the real one. Without `receipt_sk` this is
/// `token-vault.compact`'s plain deposit, which is how the airdrop's own pot
/// is funded (the pot needs no receipts).
async fn deposit_shielded(
    rng: &mut StdRng,
    state: &mut TestState<InMemoryDB>,
    addr: ContractAddress,
    op: &ContractOperation,
    amount: u128,
    token: ShieldedTokenType,
    receipt_sk: Option<HashOutput>,
) -> Option<Receipt> {
    let coin = CoinInfo::new(rng, amount, token);
    let out = ZswapOutput::new_contract_owned(rng, &coin, None, addr).unwrap();
    let coin_com = coin.commitment(&Recipient::Contract(addr));
    let merging = has_shielded_tokens(&state.ledger, addr);

    let mut public_transcript: Vec<Op<ResultModeGather, InMemoryDB>> = Vec::new();
    let mut public_transcript_results: Vec<AlignedValue> = Vec::new();
    let offer;

    if !merging {
        // The pot is empty: the deposited coin becomes the pot as it stands.
        public_transcript.extend(
            [
                &kernel_self!((), ())[..],
                &kernel_claim_zswap_coin_receive!((), (), coin_com)[..],
                &Cell_read!([key!(STATE_IDX_HAS_SHIELDED_TOKENS)], false, bool)[..],
                &kernel_self!((), ())[..],
                &Cell_write_coin!(
                    [key!(STATE_IDX_SHIELDED_VAULT)],
                    true,
                    QualifiedCoinInfo,
                    coin.clone(),
                    Recipient::Contract(addr)
                )[..],
                &Cell_write!([key!(STATE_IDX_HAS_SHIELDED_TOKENS)], true, bool, true)[..],
                &Counter_increment!([key!(STATE_IDX_TOTAL_SHIELDED_DEPOSITS)], false, 1u64)[..],
            ]
            .into_iter()
            .flatten()
            .cloned(),
        );
        public_transcript_results.extend([
            AlignedValue::from(addr),  // kernel.self()
            AlignedValue::from(false), // hasShieldedTokens
            AlignedValue::from(addr),  // kernel.self()
        ]);
        offer = ZswapOffer {
            inputs: vec![].into(),
            outputs: vec![out].into(),
            transient: vec![].into(),
            deltas: vec![Delta {
                token_type: token,
                value: -(amount as i128),
            }]
            .into(),
        };
    } else {
        // The pot holds a coin: mergeCoinImmediate spends both and writes one.
        let pot = shielded_pot(&state.ledger, addr);
        let merged_coin = CoinInfo::from(&pot).evolve_from(
            b"midnight:kernel:nonce_evolve",
            pot.value + coin.value,
            pot.type_,
        );
        let merged_coin_com = merged_coin.commitment(&Recipient::Contract(addr));
        let pot_nul = CoinInfo::from(&pot).nullifier(&SenderEvidence::Contract(addr));
        let coin_nul = coin.nullifier(&SenderEvidence::Contract(addr));

        public_transcript.extend(
            [
                &kernel_self!((), ())[..],
                &kernel_claim_zswap_coin_receive!((), (), coin_com)[..],
                &Cell_read!([key!(STATE_IDX_HAS_SHIELDED_TOKENS)], false, bool)[..],
                &Cell_read!([key!(STATE_IDX_SHIELDED_VAULT)], false, QualifiedCoinInfo)[..],
                &kernel_self!((), ())[..],
                &kernel_claim_zswap_nullifier!((), (), pot_nul)[..],
                &kernel_claim_zswap_nullifier!((), (), coin_nul)[..],
                &kernel_claim_zswap_coin_spend!((), (), merged_coin_com)[..],
                &kernel_claim_zswap_coin_receive!((), (), merged_coin_com)[..],
                &kernel_self!((), ())[..],
                &Cell_write_coin!(
                    [key!(STATE_IDX_SHIELDED_VAULT)],
                    true,
                    QualifiedCoinInfo,
                    merged_coin.clone(),
                    Recipient::Contract(addr)
                )[..],
                &Counter_increment!([key!(STATE_IDX_TOTAL_SHIELDED_DEPOSITS)], false, 1u64)[..],
            ]
            .into_iter()
            .flatten()
            .cloned(),
        );
        public_transcript_results.extend([
            AlignedValue::from(addr), // kernel.self()
            AlignedValue::from(true), // hasShieldedTokens
            AlignedValue::from(pot),  // shieldedVault, for the merge
            AlignedValue::from(addr), // kernel.self()
            AlignedValue::from(addr), // kernel.self()
        ]);

        let pot_in =
            ZswapInput::new_contract_owned(rng, &pot, None, addr, &state.ledger.zswap.coin_coms)
                .unwrap();
        let transient =
            ZswapTransient::new_from_contract_owned_output(rng, &coin.qualify(0), None, out)
                .unwrap();
        let merged_out = ZswapOutput::new_contract_owned(rng, &merged_coin, None, addr).unwrap();
        offer = ZswapOffer {
            inputs: vec![pot_in].into(),
            outputs: vec![merged_out].into(),
            transient: vec![transient].into(),
            deltas: vec![Delta {
                token_type: token,
                value: -(amount as i128),
            }]
            .into(),
        };
    }

    // The receipt: inserted by the same circuit that received the coin, so a
    // leaf can only exist for a deposit the vault was really paid.
    let receipt = receipt_sk.map(|sk| Receipt::of(&coin, sk));
    if let (Some(sk), Some(receipt)) = (receipt_sk, receipt) {
        public_transcript.extend(
            HistoricMerkleTree_insert!(
                [key!(STATE_IDX_RECEIPTS)],
                false,
                10,
                [u8; 32],
                receipt.leaf(sk)
            )
            .iter()
            .cloned(),
        );
    }

    let transcripts = partition_transcripts(
        &[PreTranscript {
            context: context_for(&state.ledger, addr, None, Some(&offer)),
            program: program_with_results(&public_transcript, &public_transcript_results),
            comm_comm: None,
        }],
        &INITIAL_PARAMETERS,
    )
    .unwrap();

    let call = ContractCallPrototype {
        address: addr,
        entry_point: b"depositShielded"[..].into(),
        op: op.clone(),
        input: coin.into(),
        output: ().into(),
        guaranteed_public_transcript: transcripts[0].0.clone(),
        fallible_public_transcript: transcripts[0].1.clone(),
        // localSecretKey()
        private_transcript_outputs: receipt_sk
            .map(|sk| vec![AlignedValue::from(sk)])
            .unwrap_or_default(),
        communication_commitment_rand: rng.r#gen(),
        key_location: KeyLocation(Cow::Borrowed("depositShielded")),
    };

    // With no `kernel.checkpoint()` in the circuit there is one section, which
    // goes wholly into the guaranteed transcript or wholly into the fallible
    // one, depending on whether it fits the guaranteed budget. The coins have
    // to be offered in the same segment as the transcript claiming them.
    let (guaranteed_coins, fallible_coins) = if transcripts[0].0.is_some() {
        (Some(offer), HashMap::new())
    } else {
        (None, [(1u16, offer)].into_iter().collect())
    };
    let tx = Transaction::new(
        "local-test",
        test_intents(rng, vec![call], Vec::new(), Vec::new(), state.time),
        guaranteed_coins,
        fallible_coins,
    );

    let mut unbalanced_strictness = WellFormedStrictness::default();
    unbalanced_strictness.enforce_balancing = false;

    let tx = tx_prove_bind(rng.split(), &tx, &RESOLVER).await.unwrap();
    tx.well_formed(&state.ledger, unbalanced_strictness, state.time)
        .unwrap();
    let balanced = state.balance_tx(rng.split(), tx, &RESOLVER).await.unwrap();
    state.assert_apply(&balanced, WellFormedStrictness::default());

    receipt
}

// ═══════════════════════════════════════════════════════════════════════════
//  CLAIMS
// ═══════════════════════════════════════════════════════════════════════════

/// How a claim transaction is put together, honestly or otherwise.
struct ClaimSpec<'a> {
    vault: ContractAddress,
    airdrop: ContractAddress,
    prove_op: &'a ContractOperation,
    claim_op: &'a ContractOperation,
    /// The receipt `vault.proveDeposit` really returns.
    returned: Receipt,
    /// The receipt `claimAirdrop`'s claimed call commits to. The same, except
    /// where a part below sets them apart.
    committed: Receipt,
    /// The key the claimer proves the receipt and derives the nullifier with.
    claimer_sk: HashOutput,
    /// With this unset the `proveDeposit` call is left out of the transaction
    /// altogether, so the airdrop claims a call that isn't there.
    include_prove_call: bool,
    /// A vault state to build the `proveDeposit` transcript against, when it
    /// should not be the real one.
    forged_vault_state: Option<ChargedState<InMemoryDB>>,
    /// Likewise for the airdrop's own state, and its nullifier set.
    forged_airdrop_state: Option<ChargedState<InMemoryDB>>,
}

struct ClaimTx {
    tx: TxBound<Signature, InMemoryDB>,
    /// The coin the airdrop pays to the claimer.
    airdrop_coin: CoinInfo,
    /// The coin the airdrop keeps.
    change_coin: CoinInfo,
    /// The nullifier the claim spends.
    nullifier: HashOutput,
    /// The root `proveDeposit` proves the receipt under.
    root: AlignedValue,
    /// The commitment `claimAirdrop` claims for the vault's call.
    cc_claimed: Fr,
    /// The commitment the vault's call really has.
    cc_returned: Fr,
}

/// Build `[vault.proveDeposit, airdrop.claimAirdrop]`.
async fn claim_tx(rng: &mut StdRng, state: &TestState<InMemoryDB>, spec: ClaimSpec<'_>) -> ClaimTx {
    let pot = shielded_pot(&state.ledger, spec.airdrop);
    let ep_hash = entry_point_hash(PROVE_ENTRY_POINT.as_bytes());

    // The commitment the airdrop claims: the (input, output) of the vault's
    // call, blinded with `cc_rand`. `claimAirdrop` gets both from its
    // witnesses; the vault's call is proven against the same randomness.
    let cc_rand = rng.r#gen();
    let cc_claimed = spec.committed.commitment(cc_rand);
    let cc_returned = spec.returned.commitment(cc_rand);

    // -- the vault's side: one root check, and nothing else.
    let vault_state = spec
        .forged_vault_state
        .clone()
        .unwrap_or_else(|| state.ledger.index(spec.vault).unwrap().data);
    let leaf = spec.returned.leaf(spec.claimer_sk);
    let path = receipt_path(&vault_state, leaf);
    let root = path.root();
    let prove_transcript: Vec<Op<ResultModeGather, InMemoryDB>> =
        HistoricMerkleTree_check_root!([key!(STATE_IDX_RECEIPTS)], false, 10, [u8; 32], root)
            .to_vec();
    let prove_transcript_results: Vec<AlignedValue> = vec![true.into()];

    // -- the airdrop's side: the claimed call, the nullifier, the payout.
    let nul = claim_nullifier(spec.committed.nonce, spec.claimer_sk);
    let airdrop_coin = CoinInfo::from(&pot).evolve_from(
        b"midnight:kernel:nonce_evolve",
        AIRDROP_AMOUNT,
        pot.type_,
    );
    let change_coin = CoinInfo::from(&pot).evolve_from(
        b"midnight:kernel:nonce_evolve/2",
        pot.value - AIRDROP_AMOUNT,
        pot.type_,
    );
    let pot_nul = CoinInfo::from(&pot).nullifier(&SenderEvidence::Contract(spec.airdrop));
    let airdrop_com = airdrop_coin.commitment(&Recipient::User(state.zswap_keys.coin_public_key()));
    let change_com = change_coin.commitment(&Recipient::Contract(spec.airdrop));

    let claim_transcript: Vec<Op<ResultModeGather, InMemoryDB>> = [
        // assert(r.value >= minDeposit)
        &Cell_read!([key!(STATE_IDX_MIN_DEPOSIT)], false, u128)[..],
        // kernel.claimContractCall(vault, proveEntryPoint, cc)
        &Cell_read!([key!(STATE_IDX_VAULT)], false, ContractAddress)[..],
        &Cell_read!([key!(STATE_IDX_PROVE_ENTRY_POINT)], false, HashOutput)[..],
        &kernel_claim_contract_call!(
            (),
            (),
            AlignedValue::from(spec.vault),
            AlignedValue::from(ep_hash),
            AlignedValue::from(cc_claimed)
        )[..],
        // assert(!claimed.member(nul)); claimed.insert(nul)
        &Set_member!([key!(STATE_IDX_CLAIMED)], false, [u8; 32], nul.0)[..],
        &Set_insert!([key!(STATE_IDX_CLAIMED)], false, [u8; 32], nul.0)[..],
        // assert(hasShieldedTokens); assert(shieldedVault.value >= airdropAmount)
        &Cell_read!([key!(STATE_IDX_HAS_SHIELDED_TOKENS)], false, bool)[..],
        &Cell_read!([key!(STATE_IDX_SHIELDED_VAULT)], false, QualifiedCoinInfo)[..],
        &Cell_read!([key!(STATE_IDX_AIRDROP_AMOUNT)], false, u128)[..],
        // sendShielded(shieldedVault, ownPublicKey(), airdropAmount) -- each
        // argument is read again where it is passed.
        &Cell_read!([key!(STATE_IDX_SHIELDED_VAULT)], false, QualifiedCoinInfo)[..],
        &Cell_read!([key!(STATE_IDX_AIRDROP_AMOUNT)], false, u128)[..],
        &kernel_self!((), ())[..],
        &kernel_claim_zswap_nullifier!((), (), pot_nul)[..],
        &kernel_claim_zswap_coin_spend!((), (), airdrop_com)[..],
        &kernel_claim_zswap_coin_spend!((), (), change_com)[..],
        &kernel_claim_zswap_coin_receive!((), (), change_com)[..],
        // shieldedVault.writeCoin(result.change.value, kernel.self())
        &kernel_self!((), ())[..],
        &Cell_write_coin!(
            [key!(STATE_IDX_SHIELDED_VAULT)],
            true,
            QualifiedCoinInfo,
            change_coin.clone(),
            Recipient::Contract(spec.airdrop)
        )[..],
        &Counter_increment!([key!(STATE_IDX_TOTAL_SHIELDED_WITHDRAWALS)], false, 1u64)[..],
    ]
    .into_iter()
    .flatten()
    .cloned()
    .collect();

    let claim_transcript_results: Vec<AlignedValue> = vec![
        MIN_DEPOSIT.into(),    // minDeposit
        spec.vault.into(),     // vault
        ep_hash.into(),        // proveEntryPoint
        false.into(),          // claimed.member(nul)
        true.into(),           // hasShieldedTokens
        pot.into(),            // shieldedVault, for the value check
        AIRDROP_AMOUNT.into(), // airdropAmount, for the value check
        pot.into(),            // shieldedVault, for sendShielded
        AIRDROP_AMOUNT.into(), // airdropAmount, for sendShielded
        spec.airdrop.into(),   // kernel.self()
        spec.airdrop.into(),   // kernel.self()
    ];

    // The payout: the pot goes in, the claimer's coin and the new pot come out.
    let pot_in = ZswapInput::new_contract_owned(
        rng,
        &pot,
        None,
        spec.airdrop,
        &state.ledger.zswap.coin_coms,
    )
    .unwrap();
    let airdrop_out = ZswapOutput::new(
        rng,
        &airdrop_coin,
        None,
        &state.zswap_keys.coin_public_key(),
        Some(state.zswap_keys.enc_public_key()),
    )
    .unwrap();
    let change_out =
        ZswapOutput::new_contract_owned(rng, &change_coin, None, spec.airdrop).unwrap();
    let mut outputs = vec![airdrop_out, change_out];
    outputs.sort();
    let offer = ZswapOffer {
        inputs: vec![pot_in].into(),
        outputs: outputs.into(),
        transient: vec![].into(),
        deltas: vec![].into(),
    };

    let mut pre_transcripts = Vec::new();
    if spec.include_prove_call {
        pre_transcripts.push(PreTranscript {
            context: context_for(&state.ledger, spec.vault, Some(vault_state.clone()), None),
            program: program_with_results(&prove_transcript, &prove_transcript_results),
            comm_comm: Some(cc_returned),
        });
    }
    pre_transcripts.push(PreTranscript {
        context: context_for(
            &state.ledger,
            spec.airdrop,
            spec.forged_airdrop_state.clone(),
            Some(&offer),
        ),
        program: program_with_results(&claim_transcript, &claim_transcript_results),
        comm_comm: None,
    });
    let mut transcripts = partition_transcripts(&pre_transcripts, &INITIAL_PARAMETERS).unwrap();

    // A root check's cost depends on the tree and root history it runs
    // against, and the gas a transcript declares is measured against the state
    // it was built on. Leave a margin, so that a transcript built against a
    // fabricated state is rejected on the check it got wrong rather than on
    // gas -- as `simple-merkle-tree.rs` does for the same reason.
    if spec.include_prove_call {
        if let Some(ref mut transcript) = transcripts[0].0 {
            transcript.gas = transcript.gas * 1.2;
        }
        if let Some(ref mut transcript) = transcripts[0].1 {
            transcript.gas = transcript.gas * 1.2;
        }
    }

    let mut calls = Vec::new();
    if spec.include_prove_call {
        calls.push(ContractCallPrototype {
            address: spec.vault,
            entry_point: PROVE_ENTRY_POINT.as_bytes().into(),
            op: spec.prove_op.clone(),
            input: ().into(),
            output: spec.returned.aligned(),
            guaranteed_public_transcript: transcripts[0].0.clone(),
            fallible_public_transcript: transcripts[0].1.clone(),
            // localSecretKey(), localReceipt(), localReceiptPath()
            private_transcript_outputs: vec![
                AlignedValue::from(spec.claimer_sk),
                spec.returned.aligned(),
                AlignedValue::from(path.clone()),
            ],
            communication_commitment_rand: cc_rand,
            key_location: KeyLocation(Cow::Borrowed(PROVE_ENTRY_POINT)),
        });
    }
    let claim_idx = if spec.include_prove_call { 1 } else { 0 };
    calls.push(ContractCallPrototype {
        address: spec.airdrop,
        entry_point: b"claimAirdrop"[..].into(),
        op: spec.claim_op.clone(),
        input: ().into(),
        output: airdrop_coin.into(),
        guaranteed_public_transcript: transcripts[claim_idx].0.clone(),
        fallible_public_transcript: transcripts[claim_idx].1.clone(),
        // tmpDoCall(), localSecretKey(), tmpCallRand(), ownPublicKey()
        private_transcript_outputs: vec![
            spec.committed.aligned(),
            AlignedValue::from(spec.claimer_sk),
            AlignedValue::from(cc_rand),
            state.zswap_keys.coin_public_key().into(),
        ],
        communication_commitment_rand: rng.r#gen(),
        key_location: KeyLocation(Cow::Borrowed("claimAirdrop")),
    });

    let (guaranteed_coins, fallible_coins) = if transcripts[claim_idx].0.is_some() {
        (Some(offer), HashMap::new())
    } else {
        (None, [(1u16, offer)].into_iter().collect())
    };
    let tx = Transaction::new(
        "local-test",
        test_intents(rng, calls, Vec::new(), Vec::new(), state.time),
        guaranteed_coins,
        fallible_coins,
    );

    ClaimTx {
        tx: tx_prove_bind(rng.split(), &tx, &RESOLVER).await.unwrap(),
        airdrop_coin,
        change_coin,
        nullifier: nul,
        root: root.into(),
        cc_claimed,
        cc_returned,
    }
}

/// The claimed calls and the transaction's real calls, from a rejection.
type CallSubsetCheck = SubsetCheckFailure<(u16, (ContractAddress, HashOutput, Fr))>;

fn call_subset_check(err: MalformedTransaction<InMemoryDB>) -> CallSubsetCheck {
    match err {
        MalformedTransaction::EffectsCheckFailure(
            EffectsCheckError::RealCallsSubsetCheckFailure(failure),
        ) => failure,
        other => panic!("expected a claimed-call subset failure, got {other:?}"),
    }
}

/// Whether `calls` contains a call to `addr`'s receipt-proof entry point
/// committing to `cc`.
fn contains_prove_call(
    calls: &[(u16, (ContractAddress, HashOutput, Fr))],
    addr: ContractAddress,
    cc: Fr,
) -> bool {
    let ep_hash = entry_point_hash(PROVE_ENTRY_POINT.as_bytes());
    calls
        .iter()
        .any(|(_, (a, ep, c))| *a == addr && *ep == ep_hash && *c == cc)
}

/// Assert that a transaction was rejected because the VM, replaying its
/// transcript against the real chain state, read something other than what the
/// transcript claimed.
fn assert_read_mismatch(
    result: Result<TransactionResult<InMemoryDB>, MalformedTransaction<InMemoryDB>>,
    expected: bool,
) {
    match result {
        Ok(TransactionResult::Failure(TransactionInvalid::Transcript(
            TranscriptRejected::Execution(OnchainProgramError::ReadMismatch {
                expected: claimed,
                actual,
            }),
        ))) => {
            assert_eq!(
                claimed,
                AlignedValue::from(expected),
                "the transcript claimed to read {expected}"
            );
            assert_eq!(
                actual,
                AlignedValue::from(!expected),
                "the real state reads {}",
                !expected
            );
            println!(
                "   rejected at apply: read {} where the transcript claimed {expected}",
                !expected
            );
        }
        other => panic!("expected a transcript read mismatch, got {other:?}"),
    }
}

/// Every byte of a transaction that goes on chain.
fn tx_bytes(tx: &TxBound<Signature, InMemoryDB>) -> Vec<u8> {
    let mut bytes = Vec::new();
    Serializable::serialize(tx, &mut bytes).expect("serializing a transaction");
    bytes
}

fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

// ═══════════════════════════════════════════════════════════════════════════
//  THE TEST
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
#[allow(clippy::field_reassign_with_default)]
async fn test_receipt_proves_a_particular_deposit() {
    let mut rng = StdRng::seed_from_u64(0x42);
    init_crypto();

    let mut unbalanced_strictness = WellFormedStrictness::default();
    unbalanced_strictness.enforce_balancing = false;
    let balanced_strictness = WellFormedStrictness::default();

    let owner_sk: HashOutput = rng.r#gen();
    let owner_pk = derive_public_key(owner_sk);
    // The two depositors. Only their receipt keys differ: `TestState` has one
    // wallet, so the same wallet funds both deposits -- what a receipt binds
    // is the key below, not the coin's source.
    let alice_sk: HashOutput = rng.r#gen();
    let bob_sk: HashOutput = rng.r#gen();

    let deposit_shielded_op = contract_operation(&RESOLVER, "depositShielded").await;
    // Neither `proveDeposit` nor `claimAirdrop` has a compiled circuit here;
    // the ledger requires *some* verifier key per entry point, and these tests
    // erase proofs, so the closest real keys stand in.
    let prove_deposit_op = contract_operation(&RESOLVER, "getShieldedBalance").await;
    let claim_airdrop_op = contract_operation(&RESOLVER, "withdrawShielded").await;

    let mut state: TestState<InMemoryDB> = TestState::new(&mut rng);
    let token: ShieldedTokenType = Default::default();
    state.rewards_shielded(&mut rng, token, REWARDS_AMOUNT);
    state.give_fee_token(&mut rng, 100).await;

    // ========================================================================
    // Part 1: Deploy the vault and the airdrop contract
    // ========================================================================
    println!(":: Part 1: Deploy vault (with receipt tree) and airdrop");

    let vault_contract: ContractState<InMemoryDB> = ContractState::new(
        stval!([
            (QualifiedCoinInfo::default()), // 0: shieldedVault
            (false),                        // 1: hasShieldedTokens
            (owner_pk),                     // 2: owner
            {},                             // 3: authorized
            (0u64),                         // 4: totalShieldedDeposits
            (0u64),                         // 5: totalShieldedWithdrawals
            (0u64),                         // 6: totalUnshieldedDeposits
            (0u64),                         // 7: totalUnshieldedWithdrawals
            // 8: receipts
            [
                {MT(RECEIPT_TREE_HEIGHT) {}},
                (0u64),
                { MerkleTree::<()>::blank(RECEIPT_TREE_HEIGHT).root() => null }
            ]
        ]),
        HashMap::new()
            .insert(b"depositShielded"[..].into(), deposit_shielded_op.clone())
            .insert(
                PROVE_ENTRY_POINT.as_bytes().into(),
                prove_deposit_op.clone(),
            ),
        Default::default(),
    );
    let deploy_vault = ContractDeploy::new(&mut rng, vault_contract);
    let addr_vault = deploy_vault.address();

    let airdrop_contract: ContractState<InMemoryDB> = ContractState::new(
        stval!([
            (QualifiedCoinInfo::default()), // 0: shieldedVault (the pot)
            (false),                        // 1: hasShieldedTokens
            (owner_pk),                     // 2: owner
            {},                             // 3: authorized
            (0u64),                         // 4: totalShieldedDeposits
            (0u64),                         // 5: totalShieldedWithdrawals
            (0u64),                         // 6: totalUnshieldedDeposits
            (0u64),                         // 7: totalUnshieldedWithdrawals
            (addr_vault),                   // 8: vault
            (entry_point_hash(PROVE_ENTRY_POINT.as_bytes())), // 9: proveEntryPoint
            (MIN_DEPOSIT),                  // 10: minDeposit
            (AIRDROP_AMOUNT),               // 11: airdropAmount
            {}                              // 12: claimed
        ]),
        HashMap::new()
            .insert(b"depositShielded"[..].into(), deposit_shielded_op.clone())
            .insert(b"claimAirdrop"[..].into(), claim_airdrop_op.clone()),
        Default::default(),
    );
    let deploy_airdrop = ContractDeploy::new(&mut rng, airdrop_contract);
    let addr_airdrop = deploy_airdrop.address();

    let tx = Transaction::from_intents(
        "local-test",
        test_intents(
            &mut rng,
            Vec::new(),
            Vec::new(),
            vec![deploy_vault, deploy_airdrop],
            state.time,
        ),
    );
    let tx = tx_prove_bind(rng.split(), &tx, &RESOLVER).await.unwrap();
    tx.well_formed(&state.ledger, unbalanced_strictness, state.time)
        .unwrap();
    let balanced = state.balance_tx(rng.split(), tx, &RESOLVER).await.unwrap();
    state.assert_apply(&balanced, balanced_strictness);

    println!("   Vault:   {addr_vault:?}");
    println!("   Airdrop: {addr_airdrop:?}");

    // ========================================================================
    // Part 2: Fund the airdrop pot
    // ========================================================================
    println!("\n:: Part 2: Fund the airdrop pot with {AIRDROP_POT} tokens");

    deposit_shielded(
        &mut rng,
        &mut state,
        addr_airdrop,
        &deposit_shielded_op,
        AIRDROP_POT,
        token,
        None,
    )
    .await;
    assert_eq!(shielded_balance(&state.ledger, addr_airdrop), AIRDROP_POT);

    // ========================================================================
    // Part 3: Two deposits into the vault, each leaving a receipt
    // ========================================================================
    println!("\n:: Part 3: Alice deposits {ALICE_DEPOSIT}, Bob {BOB_DEPOSIT}");

    let alice = deposit_shielded(
        &mut rng,
        &mut state,
        addr_vault,
        &deposit_shielded_op,
        ALICE_DEPOSIT,
        token,
        Some(alice_sk),
    )
    .await
    .expect("a receipt was recorded");
    let bob = deposit_shielded(
        &mut rng,
        &mut state,
        addr_vault,
        &deposit_shielded_op,
        BOB_DEPOSIT,
        token,
        Some(bob_sk),
    )
    .await
    .expect("a receipt was recorded");

    assert_eq!(
        shielded_balance(&state.ledger, addr_vault),
        ALICE_DEPOSIT + BOB_DEPOSIT,
        "both deposits merged into one pot"
    );
    assert_eq!(alice.pk, derive_public_key(alice_sk));
    assert_ne!(alice.pk, bob.pk, "the receipts name different depositors");
    // Both leaves are under the current root: the anonymity set of a claim
    // proved against it.
    let vault_state = state.ledger.index(addr_vault).unwrap().data;
    let alice_leaf = alice.leaf(alice_sk);
    let bob_leaf = bob.leaf(bob_sk);
    let current_root = receipt_path(&vault_state, alice_leaf).root();
    assert_eq!(
        receipt_path(&vault_state, bob_leaf).root(),
        current_root,
        "both receipts are under the same root"
    );
    println!("   receipt tree root covers both deposits");

    // ========================================================================
    // Part 4: Rejected -- claiming without proving a receipt
    // ========================================================================
    println!("\n:: Part 4: Rejected (receipt proof missing)");

    let claim = claim_tx(
        &mut rng,
        &state,
        ClaimSpec {
            vault: addr_vault,
            airdrop: addr_airdrop,
            prove_op: &prove_deposit_op,
            claim_op: &claim_airdrop_op,
            returned: alice,
            committed: alice,
            claimer_sk: alice_sk,
            include_prove_call: false,
            forged_vault_state: None,
            forged_airdrop_state: None,
        },
    )
    .await;
    let failure = call_subset_check(
        claim
            .tx
            .well_formed(&state.ledger, unbalanced_strictness, state.time)
            .expect_err("claim without a receipt proof succeeded unexpectedly"),
    );
    assert!(
        contains_prove_call(&failure.subset, addr_vault, claim.cc_claimed),
        "the airdrop claimed the vault's receipt proof"
    );
    assert!(
        !failure
            .superset
            .iter()
            .any(|(_, (addr, _, _))| *addr == addr_vault),
        "but the transaction contains no call to the vault at all"
    );
    println!("   rejected, as expected");

    // ========================================================================
    // Part 5: Rejected -- claiming a receipt the vault did not return
    // ========================================================================
    println!("\n:: Part 5: Rejected (Bob inflates his deposit past the threshold)");

    assert!(
        bob.value < MIN_DEPOSIT,
        "Bob's real deposit is below the threshold"
    );
    let inflated = Receipt {
        value: MIN_DEPOSIT,
        ..bob
    };
    let claim = claim_tx(
        &mut rng,
        &state,
        ClaimSpec {
            vault: addr_vault,
            airdrop: addr_airdrop,
            prove_op: &prove_deposit_op,
            claim_op: &claim_airdrop_op,
            // The vault really returns Bob's receipt, for what he deposited...
            returned: bob,
            // ...but the airdrop is claimed against an inflated value.
            committed: inflated,
            claimer_sk: bob_sk,
            include_prove_call: true,
            forged_vault_state: None,
            forged_airdrop_state: None,
        },
    )
    .await;
    let failure = call_subset_check(
        claim
            .tx
            .well_formed(&state.ledger, unbalanced_strictness, state.time)
            .expect_err("claim against an inflated receipt succeeded unexpectedly"),
    );
    assert_ne!(
        claim.cc_claimed, claim.cc_returned,
        "the inflated value commits differently"
    );
    assert!(
        contains_prove_call(&failure.superset, addr_vault, claim.cc_returned),
        "the vault's proof is in the transaction, for the real receipt"
    );
    assert!(
        contains_prove_call(&failure.subset, addr_vault, claim.cc_claimed),
        "but the airdrop claimed it for the inflated one"
    );
    println!("   rejected, as expected");

    // ========================================================================
    // Part 6: Rejected -- a receipt the vault never issued
    // ========================================================================
    println!("\n:: Part 6: Rejected (forged receipt, root never in the tree)");

    // Carol never deposited. She builds her claim against a local state in
    // which her invented receipt *is* in the tree -- the transcript is
    // internally consistent, and its root check passes against her own
    // simulation.
    let carol_sk: HashOutput = rng.r#gen();
    let carol = Receipt {
        nonce: rng.r#gen(),
        pk: derive_public_key(carol_sk),
        value: ALICE_DEPOSIT,
    };
    let forged_state =
        vault_state_with_forged_tree(&state.ledger, addr_vault, carol.leaf(carol_sk));
    let claim = claim_tx(
        &mut rng,
        &state,
        ClaimSpec {
            vault: addr_vault,
            airdrop: addr_airdrop,
            prove_op: &prove_deposit_op,
            claim_op: &claim_airdrop_op,
            returned: carol,
            committed: carol,
            claimer_sk: carol_sk,
            include_prove_call: true,
            forged_vault_state: Some(forged_state),
            forged_airdrop_state: None,
        },
    )
    .await;
    assert_ne!(
        claim.root,
        AlignedValue::from(current_root),
        "Carol's root is not the vault's"
    );
    // The effects check is satisfied -- the call is there, for the receipt it
    // claims. What rejects this is the ledger re-running the root check
    // against the real tree.
    let mut forged_state = state.clone();
    // `receipts.checkRoot` claimed to find Carol's root; the real tree has
    // never held it.
    assert_read_mismatch(forged_state.apply(&claim.tx, unbalanced_strictness), true);

    // ========================================================================
    // Part 7: Alice claims, against her real receipt
    // ========================================================================
    println!("\n:: Part 7: Alice claims the airdrop against her receipt");

    let claim = claim_tx(
        &mut rng,
        &state,
        ClaimSpec {
            vault: addr_vault,
            airdrop: addr_airdrop,
            prove_op: &prove_deposit_op,
            claim_op: &claim_airdrop_op,
            returned: alice,
            committed: alice,
            claimer_sk: alice_sk,
            include_prove_call: true,
            forged_vault_state: None,
            forged_airdrop_state: None,
        },
    )
    .await;
    claim
        .tx
        .well_formed(&state.ledger, unbalanced_strictness, state.time)
        .unwrap();
    assert_eq!(
        claim.cc_claimed, claim.cc_returned,
        "an honest claim commits to the receipt the vault returned"
    );
    assert_eq!(
        claim.root,
        AlignedValue::from(current_root),
        "Alice proves against the latest root, which covers both receipts"
    );

    // -- what the claim publishes, and what it does not.
    // The root it proves against is checked above; what matters here is
    // everything the claim does *not* carry. The nullifier is the control: it
    // is a 32-byte commitment reached only through the claim's transcript, so
    // finding it proves this search would find the others if they were there.
    let bytes = tx_bytes(&claim.tx);
    assert!(
        contains_bytes(&bytes, &claim.nullifier.0),
        "the claim publishes the nullifier it spends"
    );
    for (what, secret) in [
        ("Alice's receipt leaf", alice_leaf.0),
        ("Bob's receipt leaf", bob_leaf.0),
        ("Alice's public key", alice.pk.0),
        ("Alice's secret key", alice_sk.0),
        ("the receipt's opening", alice.opening(alice_sk).0),
    ] {
        assert!(
            !contains_bytes(&bytes, &secret),
            "the claim must not publish {what}"
        );
    }
    println!("   claim publishes the root and a nullifier, and nothing else");

    // Watch for the airdropped coin, so the wallet can spend it afterwards.
    state.zswap = state
        .zswap
        .watch_for(&state.zswap_keys.coin_public_key(), &claim.airdrop_coin);

    let balanced = state
        .balance_tx(rng.split(), claim.tx.clone(), &RESOLVER)
        .await
        .unwrap();
    state.assert_apply(&balanced, balanced_strictness);

    // The airdrop paid out of its pot, and kept the change.
    assert_eq!(
        shielded_balance(&state.ledger, addr_airdrop),
        AIRDROP_POT - AIRDROP_AMOUNT,
        "airdrop pot paid out"
    );
    assert_eq!(
        shielded_pot(&state.ledger, addr_airdrop).nonce,
        claim.change_coin.nonce,
        "airdrop holds the change coin"
    );
    // The vault is untouched: the receipt proof only read its tree.
    assert_eq!(
        shielded_balance(&state.ledger, addr_vault),
        ALICE_DEPOSIT + BOB_DEPOSIT,
        "vault balance unchanged by the claim"
    );
    assert_eq!(
        receipt_path(&state.ledger.index(addr_vault).unwrap().data, alice_leaf).root(),
        current_root,
        "receipt tree unchanged by the claim"
    );
    // And the claim is spent.
    let first_claim_nullifier = claim.nullifier;
    assert!(
        is_claimed(&state.ledger, addr_airdrop, claim.nullifier),
        "the claim's nullifier is now spent"
    );
    assert!(
        state
            .zswap
            .coins
            .iter()
            .any(|(_, qci)| qci.nonce == claim.airdrop_coin.nonce && qci.value == AIRDROP_AMOUNT),
        "Alice received the airdropped coin"
    );

    // ========================================================================
    // Part 8: Rejected -- the same receipt, claimed twice
    // ========================================================================
    println!("\n:: Part 8: Rejected (replay of a spent receipt)");

    // Alice's receipt is still perfectly provable -- the tree is append-only
    // and her leaf is still under the root. What stops the second payout is
    // the nullifier, so that is the one thing this claim has to lie about: it
    // is built against a state in which the set is empty, and the ledger
    // replays the `member` check against the real one.
    let claim = claim_tx(
        &mut rng,
        &state,
        ClaimSpec {
            vault: addr_vault,
            airdrop: addr_airdrop,
            prove_op: &prove_deposit_op,
            claim_op: &claim_airdrop_op,
            returned: alice,
            committed: alice,
            claimer_sk: alice_sk,
            include_prove_call: true,
            forged_vault_state: None,
            forged_airdrop_state: Some(airdrop_state_with_cleared_nullifiers(
                &state.ledger,
                addr_airdrop,
            )),
        },
    )
    .await;
    assert_eq!(
        claim.nullifier, first_claim_nullifier,
        "the replay spends the same nullifier"
    );
    assert!(
        is_claimed(&state.ledger, addr_airdrop, claim.nullifier),
        "which the ledger already holds"
    );
    // The receipt proof is real and the claimed call is real: the effects
    // check has nothing to object to.
    claim
        .tx
        .well_formed(&state.ledger, unbalanced_strictness, state.time)
        .expect("the replay is well-formed -- only its state read is a lie");
    let mut replay_state = state.clone();
    // `claimed.member(nul)` claimed to read false; the real set holds it.
    assert_read_mismatch(replay_state.apply(&claim.tx, unbalanced_strictness), false);
    assert_eq!(
        shielded_balance(&state.ledger, addr_airdrop),
        AIRDROP_POT - AIRDROP_AMOUNT,
        "the pot paid out exactly once"
    );

    println!("\n:: Test Summary");
    println!("   Airdrop pot funded:   {AIRDROP_POT}");
    println!("   Alice's deposit:      {ALICE_DEPOSIT} (receipt proved)");
    println!("   Bob's deposit:        {BOB_DEPOSIT} (below threshold)");
    println!("   Minimum for a claim:  {MIN_DEPOSIT}");
    println!("   Airdrop paid out:     {AIRDROP_AMOUNT}");
    println!("   Airdrop pot left:     {}", AIRDROP_POT - AIRDROP_AMOUNT);
}
