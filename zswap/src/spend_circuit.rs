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

//! The zswap spend circuit, written directly against zk-stdlib (v1).
//!
//! This is a hand-written replacement for interpreting
//! `zswap/static/spend.zkir` through the generic
//! [`IrSource`](zkir_v2::IrSource) relation. It is *verifier-key compatible*
//! with the shipped `zswap/static/spend.verifier`: synthesis performs exactly
//! the same standard library calls, in exactly the same order, as the IR
//! interpreter does when executing the spend IR, so key generation produces a
//! byte-for-byte identical verifier key. This equality is asserted in
//! `zswap/tests/spend_circuit.rs`; any change to this module that alters the
//! constraint system will trip that test.
//!
//! # Statement
//!
//! A spend proves, for a coin `(nonce, type, value)`:
//!
//! 1. *Ownership*: the spender knows the coin secret key `sk` (user-owned
//!    coins), or the coin is owned by a contract address (contract-owned
//!    coins, where authorization is enforced by the ledger, not the proof).
//! 2. *Commitment opening*: the coin commitment
//!    `cc = SHA256("midnight:zswap-cc[v1]" || nonce || type || value || is_user || target)`
//!    (where `target` is the public key `SHA256("midnight:zswap-pk[v1]" || sk)`
//!    for users, or the contract address) opens to the Merkle tree leaf.
//! 3. *Membership*: the leaf is in a Merkle tree (height 32, Poseidon) whose
//!    root is declared in the public transcript, where the ledger checks it
//!    against the historic first-free commitment tree roots.
//! 4. *Nullifier*: the declared nullifier is
//!    `SHA256("midnight:zswap-cn[v1]" || nonce || type || value || is_user || sender)`
//!    with `sender = sk` for users or the contract address otherwise.
//! 5. *Value commitment*: the declared Pedersen commitment is
//!    `value * hash_to_curve(type, segment) + rc * G` over the embedded curve.
//!
//! The public input vector additionally carries the field representation of
//! the Impact VM operations that the ledger replays against the zswap
//! contract state (root check, nullifier set insert, optional contract
//! address write, segment read, value commitment write).
//!
//! # 32-byte values in the witness
//!
//! 32-byte quantities (hashes, keys, addresses) do not fit in a BLS12-381
//! scalar; following `FieldRepr` for `[u8; 32]` they are split into two field
//! elements: the final byte (`hi`), then the first 31 bytes interpreted
//! little-endian (`lo`). [`split_bytes32`] implements this split.

use base_crypto::fab::{Alignment, AlignmentAtom, AlignmentSegment};
use group::Group;
use midnight_circuits_v1::instructions::{
    AssignmentInstructions, ControlFlowInstructions, ConversionInstructions,
    DecompositionInstructions, EccInstructions, EqualityInstructions, PublicInputInstructions,
    ZeroInstructions,
};
use midnight_circuits_v1::types::{AssignedBit, AssignedNative};
use midnight_proofs_v1::{
    circuit::{Layouter, Value},
    plonk::Error,
};
use midnight_zk_stdlib_v1::{Relation, ZkStdLib, ZkStdLibArch};
use transient_crypto::curve::{EmbeddedGroupAffine, Fr};
use transient_crypto::hash::{hash_to_curve, transient_hash};

use zkir_v2::ir_v1::{
    OldEmbeddedAffine, OldEmbeddedAffineExt, OldScalar, cvt, v1_assemble_bytes, v1_ecc_from_parts,
    v1_fab_decode_to_bytes,
};

/// The height of the zswap coin commitment Merkle tree.
pub const ZSWAP_TREE_HEIGHT: usize = 32;

/// The number of primary witness inputs (matches `num_inputs` in the IR).
const NUM_INPUTS: usize = 77;

/// The number of public inputs of the spend circuit, including the binding
/// input.
pub const NUM_PUBLIC_INPUTS: usize = 68;

/// Domain separator for deriving a coin public key from a coin secret key.
pub const PK_DOMAIN_SEP: &[u8; 21] = b"midnight:zswap-pk[v1]";
/// Domain separator for coin commitments.
pub const CC_DOMAIN_SEP: &[u8; 21] = b"midnight:zswap-cc[v1]";
/// Domain separator for coin nullifiers.
pub const CN_DOMAIN_SEP: &[u8; 21] = b"midnight:zswap-cn[v1]";

// Impact VM opcodes appearing (as field elements) in the spend transcript.
// See `Op::field_repr` in `onchain-vm`.
const OP_DUP_0: u64 = 0x30; // Dup { n: 0 }
const OP_IDX_1: u64 = 0x50; // Idx { cached: false, push_path: false, 1-entry path }
const OP_IDX_PP_1: u64 = 0x70; // Idx { cached: false, push_path: true, 1-entry path }
const OP_PUSH: u64 = 0x10; // Push { storage: false }
const OP_PUSH_S: u64 = 0x11; // Push { storage: true }
const OP_MEMBER: u64 = 0x18; // Member
const OP_POPEQ: u64 = 0x0C; // Popeq { cached: false }
const OP_POPEQ_C: u64 = 0x0D; // Popeq { cached: true }
const OP_INS_1: u64 = 0x91; // Ins { cached: false, n: 1 }
const OP_INS_C_1: u64 = 0xA1; // Ins { cached: true, n: 1 }

/// Splits a 32-byte value into its circuit field representation:
/// `[bytes[31], LE(bytes[0..31])]`, mirroring `FieldRepr` for `[u8; 32]`.
pub fn split_bytes32(bytes: &[u8; 32]) -> [Fr; 2] {
    [
        Fr::from_le_bytes(&bytes[31..]).expect("single byte fits in Fr"),
        Fr::from_le_bytes(&bytes[..31]).expect("31 bytes fit in Fr"),
    ]
}

/// Inverse of [`split_bytes32`].
pub fn join_bytes32(halves: &[Fr; 2]) -> [u8; 32] {
    let mut out = [0u8; 32];
    out[..31].copy_from_slice(&halves[1].as_le_bytes()[..31]);
    out[31] = halves[0].as_le_bytes()[0];
    out
}

/// The private inputs to a zswap spend.
#[derive(Clone, Debug)]
pub struct SpendWitness {
    /// Whether the coin is user-owned (`true`) or contract-owned (`false`).
    pub is_user: bool,
    /// The coin secret key, split per [`split_bytes32`]. Zero for
    /// contract-owned coins.
    pub sk: [Fr; 2],
    /// The owning contract address, split per [`split_bytes32`]. Zero for
    /// user-owned coins.
    pub contract_address: [Fr; 2],
    /// The coin commitment (the Merkle tree leaf), split per
    /// [`split_bytes32`].
    pub commitment: [Fr; 2],
    /// The Merkle path from the leaf to the root, leaf-level first. Each
    /// entry is the sibling hash and whether the path continues to the left.
    pub path: [(Fr, bool); ZSWAP_TREE_HEIGHT],
    /// The coin nonce, split per [`split_bytes32`].
    pub nonce: [Fr; 2],
    /// The coin token type, split per [`split_bytes32`].
    pub token_type: [Fr; 2],
    /// The coin value (a 128-bit quantity).
    pub value: Fr,
    /// The Pedersen commitment randomness (an embedded-curve scalar).
    pub rc: Fr,
    /// The binding input committing to the rest of the transaction.
    pub binding_input: Fr,
    /// The result of the transcript's historic root membership check, read
    /// from the public transcript outputs. `1` for any valid spend.
    pub check_root_result: Fr,
    /// The transaction segment, read from the public transcript outputs.
    pub segment: Fr,
}

impl SpendWitness {
    /// The primary input vector, in IR memory order.
    pub fn flatten(&self) -> Vec<Fr> {
        let mut inputs = Vec::with_capacity(NUM_INPUTS);
        inputs.push((self.is_user as u64).into()); // v0
        inputs.extend(self.sk); // v1, v2
        inputs.extend(self.contract_address); // v3, v4
        inputs.extend(self.commitment); // v5, v6
        for (sibling, goes_left) in &self.path {
            inputs.push(*sibling); // v7, v9, ..., v69
            inputs.push((*goes_left as u64).into()); // v8, v10, ..., v70
        }
        inputs.extend(self.nonce); // v71, v72
        inputs.extend(self.token_type); // v73, v74
        inputs.push(self.value); // v75
        inputs.push(self.rc); // v76
        debug_assert_eq!(inputs.len(), NUM_INPUTS);
        inputs
    }

    /// The Merkle root implied by the leaf and path.
    pub fn merkle_root(&self) -> Fr {
        self.path
            .iter()
            .fold(self.commitment[1], |acc, (sibling, goes_left)| {
                if *goes_left {
                    transient_hash(&[acc, *sibling])
                } else {
                    transient_hash(&[*sibling, acc])
                }
            })
    }

    /// The hash preimage shared by the commitment and nullifier, minus the
    /// domain separator and trailing key material.
    fn coin_bytes(&self) -> Vec<u8> {
        let mut data = Vec::with_capacity(32 + 32 + 16 + 1);
        data.extend(join_bytes32(&self.nonce));
        data.extend(join_bytes32(&self.token_type));
        data.extend(&self.value.as_le_bytes()[..16]);
        data.push(self.is_user as u8);
        data
    }

    fn keyed_coin_hash(&self, domain_sep: &[u8; 21], key: &[Fr; 2]) -> [Fr; 2] {
        let mut data = Vec::with_capacity(21 + 32 + 32 + 16 + 1 + 32);
        data.extend(domain_sep);
        data.extend(self.coin_bytes());
        data.extend(join_bytes32(key));
        split_bytes32(&base_crypto::hash::persistent_hash(&data).0)
    }

    /// The commitment target: the coin public key for user-owned coins, or
    /// the contract address.
    pub fn target(&self) -> [Fr; 2] {
        if self.is_user {
            let mut data = Vec::with_capacity(21 + 32);
            data.extend(PK_DOMAIN_SEP);
            data.extend(join_bytes32(&self.sk));
            split_bytes32(&base_crypto::hash::persistent_hash(&data).0)
        } else {
            self.contract_address
        }
    }

    /// The coin commitment implied by the witness, split per
    /// [`split_bytes32`]. For a satisfiable witness this must equal
    /// `self.commitment`.
    pub fn implied_commitment(&self) -> [Fr; 2] {
        self.keyed_coin_hash(CC_DOMAIN_SEP, &self.target())
    }

    /// The coin nullifier, split per [`split_bytes32`].
    pub fn nullifier(&self) -> [Fr; 2] {
        let sender = if self.is_user {
            self.sk
        } else {
            self.contract_address
        };
        self.keyed_coin_hash(CN_DOMAIN_SEP, &sender)
    }

    /// The Pedersen value commitment
    /// `value * hash_to_curve(type, segment) + rc * G`, as (x, y).
    pub fn value_commitment(&self) -> [Fr; 2] {
        let base = hash_to_curve(&[self.token_type[0], self.token_type[1], self.segment]);
        let point = EmbeddedGroupAffine::generator() * self.rc + base * self.value;
        [point.x().unwrap_or(0.into()), point.y().unwrap_or(0.into())]
    }

    /// The full public input vector this witness satisfies, including the
    /// binding input, in circuit order.
    pub fn public_inputs(&self) -> Vec<Fr> {
        let fr = |v: u64| Fr::from(v);
        let is_contract: Fr = ((!self.is_user) as u64).into();
        // `x` if the coin is contract-owned, `0` otherwise.
        let if_contract = |v: Fr| if self.is_user { 0.into() } else { v };
        let [cn_hi, cn_lo] = self.nullifier();
        let [vc_x, vc_y] = self.value_commitment();
        let neg_two = -Fr::from(2);

        let mut pis = Vec::with_capacity(NUM_PUBLIC_INPUTS);
        pis.push(self.binding_input);
        // HistoricMerkleTree_check_root([0], root): Dup 0, Idx [0], Idx [2],
        // Push root, Member, Popeq(cached) result.
        pis.push(fr(OP_DUP_0));
        pis.extend([fr(OP_IDX_1), fr(1), fr(1), fr(0)]);
        pis.extend([fr(OP_IDX_1), fr(1), fr(1), fr(2)]);
        pis.extend([fr(OP_PUSH), fr(1), fr(1), neg_two, self.merkle_root()]);
        pis.push(fr(OP_MEMBER));
        pis.extend([fr(OP_POPEQ_C), fr(1), fr(1), self.check_root_result]);
        // Set_insert([1], nullifier): Idx(push_path) [1], Push nullifier,
        // Push(storage) null, Ins, Ins(cached).
        pis.extend([fr(OP_IDX_PP_1), fr(1), fr(1), fr(1)]);
        pis.extend([fr(OP_PUSH), fr(1), fr(1), fr(0x20), cn_hi, cn_lo]);
        pis.extend([fr(OP_PUSH_S), fr(0)]);
        pis.push(fr(OP_INS_1));
        pis.push(fr(OP_INS_C_1));
        // Cell_write([3], contract_address), active for contract-owned coins
        // only; inactive spends zero the block (its guard is part of the
        // statement).
        pis.extend([
            if_contract(fr(OP_PUSH)),
            is_contract,
            is_contract,
            is_contract,
            if_contract(fr(3)),
            if_contract(fr(OP_PUSH_S)),
            is_contract,
            is_contract,
            if_contract(fr(0x20)),
            if_contract(self.contract_address[0]),
            if_contract(self.contract_address[1]),
            if_contract(fr(OP_INS_1)),
        ]);
        // Cell_read([5], segment): Dup 0, Idx [5], Popeq result.
        pis.push(fr(OP_DUP_0));
        pis.extend([fr(OP_IDX_1), fr(1), fr(1), fr(5)]);
        pis.extend([fr(OP_POPEQ), fr(1), fr(2), self.segment]);
        // Cell_write([2], value_commitment): Idx [2]? encoded as Push ... Ins.
        pis.extend([fr(OP_PUSH), fr(1), fr(1), fr(1), fr(2)]);
        pis.extend([fr(OP_PUSH_S), fr(1), fr(2), neg_two, neg_two, vc_x, vc_y]);
        pis.push(fr(OP_INS_1));
        debug_assert_eq!(pis.len(), NUM_PUBLIC_INPUTS);
        pis
    }
}

/// The zswap spend relation. See the module documentation.
#[derive(Clone, Debug, Default)]
pub struct ZswapSpend;

type Assigned = AssignedNative<OldScalar>;

/// Mirrors the IR interpreter's lowering of `cond_select`:
/// `bit != 0 ? a : b`.
fn cond_select(
    std: &ZkStdLib,
    layouter: &mut impl Layouter<OldScalar>,
    bit: &Assigned,
    a: &Assigned,
    b: &Assigned,
) -> Result<Assigned, Error> {
    let bit_is_zero = std.is_zero(layouter, bit)?;
    std.select(layouter, &bit_is_zero, b, a)
}

/// Mirrors the IR interpreter's lowering of `persistent_hash`: decode the
/// field-aligned inputs to bytes, SHA256 them, and return the result split as
/// (last byte, first 31 bytes).
fn persistent_hash(
    std: &ZkStdLib,
    layouter: &mut impl Layouter<OldScalar>,
    alignment: &Alignment,
    inputs: &[Assigned],
) -> Result<(Assigned, Assigned), Error> {
    let bytes = v1_fab_decode_to_bytes(std, layouter, alignment, inputs)?;
    let res_bytes = std.sha2_256(layouter, &bytes)?;
    let hi = std.convert(layouter, &res_bytes[31])?;
    let lo = v1_assemble_bytes(std, layouter, &res_bytes[..31])?;
    Ok((hi, lo))
}

/// The alignment of a sequence of plain byte strings.
fn bytes_alignment(lengths: &[u32]) -> Alignment {
    Alignment(
        lengths
            .iter()
            .map(|length| AlignmentSegment::Atom(AlignmentAtom::Bytes { length: *length }))
            .collect(),
    )
}

impl Relation for ZswapSpend {
    type Instance = Vec<OldScalar>;

    type Witness = SpendWitness;

    fn format_instance(instance: &Self::Instance) -> Result<Vec<OldScalar>, Error> {
        Ok(instance.clone())
    }

    // IR variable indices (vNNN) refer to `zswap/static/spend.zkir`, and
    // pi[N] to positions in the public input vector; they are cited so this
    // implementation can be audited against the IR side-by-side.
    fn circuit(
        &self,
        std: &ZkStdLib,
        layouter: &mut impl Layouter<OldScalar>,
        _instance: Value<Self::Instance>,
        witness: Value<Self::Witness>,
    ) -> Result<(), Error> {
        let fixed = |layouter: &mut _, v: u64| -> Result<Assigned, Error> {
            std.assign_fixed(layouter, OldScalar::from(v))
        };

        // Primary inputs (v0..v76), assigned as one batch.
        let input_values = witness.as_ref().map(|w| {
            w.flatten()
                .into_iter()
                .map(|f| cvt(f.0))
                .collect::<Vec<_>>()
        });
        let memory = std.assign_many(layouter, &input_values.transpose_vec(NUM_INPUTS))?;

        let is_user = &memory[0];
        let sk = [&memory[1], &memory[2]];
        let contract_address = [&memory[3], &memory[4]];
        let commitment = [&memory[5], &memory[6]];
        let path = (0..ZSWAP_TREE_HEIGHT)
            .map(|i| (&memory[7 + 2 * i], &memory[8 + 2 * i]))
            .collect::<Vec<_>>();
        let nonce = [&memory[71], &memory[72]];
        let token_type = [&memory[73], &memory[74]];
        let value = &memory[75];
        let rc = &memory[76];

        let binding_input =
            std.assign(layouter, witness.as_ref().map(|w| cvt(w.binding_input.0)))?;
        let mut pis = vec![binding_input]; // pi[0]

        // Range constraints on the primary inputs.
        let _: AssignedBit<_> = std.convert(layouter, is_user)?;
        for [hi, lo] in [sk, contract_address, commitment] {
            drop(std.assigned_to_le_bits(layouter, hi, Some(8), false)?);
            drop(std.assigned_to_le_bits(layouter, lo, Some(248), false)?);
        }
        for (_, goes_left) in &path {
            let _: AssignedBit<_> = std.convert(layouter, *goes_left)?;
        }
        for [hi, lo] in [nonce, token_type] {
            drop(std.assigned_to_le_bits(layouter, hi, Some(8), false)?);
            drop(std.assigned_to_le_bits(layouter, lo, Some(248), false)?);
        }
        drop(std.assigned_to_le_bits(layouter, value, Some(128), false)?);

        let one = fixed(layouter, 1)?; // v77

        // Merkle path ascent: the leaf is the low half of the commitment;
        // each level Poseidon-hashes the running node with its sibling,
        // ordered by the direction bit.
        let mut node = commitment[1].clone();
        for (sibling, goes_left) in &path {
            let left = cond_select(std, layouter, goes_left, &node, sibling)?;
            let right = cond_select(std, layouter, goes_left, sibling, &node)?;
            node = std.poseidon(layouter, &[left, right])?;
        }
        let root = node; // v173

        // Transcript: HistoricMerkleTree_check_root([0], root).
        let op_dup_0 = fixed(layouter, OP_DUP_0)?; // v174
        pis.push(op_dup_0.clone()); // pi[1]
        let op_idx_1 = fixed(layouter, OP_IDX_1)?; // v175
        let zero = fixed(layouter, 0)?; // v176
        pis.extend([op_idx_1.clone(), one.clone(), one.clone(), zero.clone()]); // pi[2..6]
        let two = fixed(layouter, 2)?; // v177
        pis.extend([op_idx_1.clone(), one.clone(), one.clone(), two.clone()]); // pi[6..10]
        let op_push = fixed(layouter, OP_PUSH)?; // v178
        let neg_two: Assigned = std.assign_fixed(layouter, -OldScalar::from(2))?; // v179
        pis.extend([
            op_push.clone(),
            one.clone(),
            one.clone(),
            neg_two.clone(),
            root,
        ]); // pi[10..15]
        let op_member = fixed(layouter, OP_MEMBER)?; // v180
        pis.push(op_member); // pi[15]
        let check_root_result = std.assign(
            layouter,
            witness.as_ref().map(|w| cvt(w.check_root_result.0)),
        )?; // v181
        let op_popeq_c = fixed(layouter, OP_POPEQ_C)?; // v182
        pis.extend([op_popeq_c, one.clone(), one.clone(), check_root_result]); // pi[16..20]

        // Commitment opening: derive the coin public key from the secret key,
        // select the commitment target, and check the commitment equals the
        // Merkle leaf.
        let ds_pk = std.assign_fixed(
            layouter,
            cvt(Fr::from_le_bytes(PK_DOMAIN_SEP)
                .expect("21 bytes fit in Fr")
                .0),
        )?; // v183
        let (pk_hi, pk_lo) = persistent_hash(
            std,
            layouter,
            &bytes_alignment(&[21, 32]),
            &[ds_pk, sk[0].clone(), sk[1].clone()],
        )?; // v184, v185
        let is_contract = cond_select(std, layouter, is_user, &zero, &one)?; // v186
        let target_hi = cond_select(std, layouter, is_user, &pk_hi, contract_address[0])?; // v187
        let target_lo = cond_select(std, layouter, is_user, &pk_lo, contract_address[1])?; // v188
        let ds_cc = std.assign_fixed(
            layouter,
            cvt(Fr::from_le_bytes(CC_DOMAIN_SEP)
                .expect("21 bytes fit in Fr")
                .0),
        )?; // v189
        let coin_alignment = bytes_alignment(&[21, 32, 32, 16, 1, 32]);
        let (cc_hi, cc_lo) = persistent_hash(
            std,
            layouter,
            &coin_alignment,
            &[
                ds_cc,
                nonce[0].clone(),
                nonce[1].clone(),
                token_type[0].clone(),
                token_type[1].clone(),
                value.clone(),
                is_user.clone(),
                target_hi,
                target_lo,
            ],
        )?; // v190, v191
        let eq_hi = {
            let bit = std.is_equal(layouter, &cc_hi, commitment[0])?;
            std.convert(layouter, &bit)?
        }; // v192
        let eq_lo = {
            let bit = std.is_equal(layouter, &cc_lo, commitment[1])?;
            std.convert(layouter, &bit)?
        }; // v193
        let eq_both = cond_select(std, layouter, &eq_hi, &eq_lo, &zero)?; // v194
        std.assert_non_zero(layouter, &eq_both)?;

        // Nullifier: as the commitment, but keyed with the sender evidence
        // (the secret key itself for user-owned coins).
        let sender_hi = cond_select(std, layouter, is_user, sk[0], contract_address[0])?; // v195
        let sender_lo = cond_select(std, layouter, is_user, sk[1], contract_address[1])?; // v196
        let ds_cn = std.assign_fixed(
            layouter,
            cvt(Fr::from_le_bytes(CN_DOMAIN_SEP)
                .expect("21 bytes fit in Fr")
                .0),
        )?; // v197
        let (cn_hi, cn_lo) = persistent_hash(
            std,
            layouter,
            &coin_alignment,
            &[
                ds_cn,
                nonce[0].clone(),
                nonce[1].clone(),
                token_type[0].clone(),
                token_type[1].clone(),
                value.clone(),
                is_user.clone(),
                sender_hi,
                sender_lo,
            ],
        )?; // v198, v199

        // Transcript: Set_insert([1], nullifier).
        let op_idx_pp_1 = fixed(layouter, OP_IDX_PP_1)?; // v200
        pis.extend([op_idx_pp_1, one.clone(), one.clone(), one.clone()]); // pi[20..24]
        let bytes32_len = fixed(layouter, 0x20)?; // v201
        pis.extend([
            op_push.clone(),
            one.clone(),
            one.clone(),
            bytes32_len.clone(),
            cn_hi,
            cn_lo,
        ]); // pi[24..30]
        let op_push_s = fixed(layouter, OP_PUSH_S)?; // v202
        pis.extend([op_push_s.clone(), zero.clone()]); // pi[30..32]
        let op_ins_1 = fixed(layouter, OP_INS_1)?; // v203
        pis.push(op_ins_1.clone()); // pi[32]
        let op_ins_c_1 = fixed(layouter, OP_INS_C_1)?; // v204
        pis.push(op_ins_c_1); // pi[33]

        // Transcript: Cell_write([3], contract_address), guarded by
        // `is_contract`; user spends zero the whole block.
        let three = fixed(layouter, 3)?; // v205
        let select_contract =
            |std: &ZkStdLib, layouter: &mut _, var: &Assigned| -> Result<Assigned, Error> {
                cond_select(std, layouter, &is_contract, var, &zero)
            };
        let g_push = select_contract(std, layouter, &op_push)?; // v206
        pis.extend([
            g_push,
            is_contract.clone(),
            is_contract.clone(),
            is_contract.clone(),
        ]); // pi[34..38]
        let g_three = select_contract(std, layouter, &three)?; // v207
        pis.push(g_three); // pi[38]
        let g_push_s = select_contract(std, layouter, &op_push_s)?; // v208
        pis.extend([g_push_s, is_contract.clone(), is_contract.clone()]); // pi[39..42]
        let g_bytes32_len = select_contract(std, layouter, &bytes32_len)?; // v209
        pis.push(g_bytes32_len); // pi[42]
        let g_addr_hi = select_contract(std, layouter, contract_address[0])?; // v210
        pis.push(g_addr_hi); // pi[43]
        let g_addr_lo = select_contract(std, layouter, contract_address[1])?; // v211
        pis.push(g_addr_lo); // pi[44]
        let g_ins = select_contract(std, layouter, &op_ins_1)?; // v212
        pis.push(g_ins); // pi[45]

        // Transcript: Cell_read([5], segment).
        pis.push(op_dup_0); // pi[46]
        let five = fixed(layouter, 5)?; // v213
        pis.extend([op_idx_1, one.clone(), one.clone(), five]); // pi[47..51]
        let segment: Assigned = std.assign(layouter, witness.as_ref().map(|w| cvt(w.segment.0)))?; // v214
        let op_popeq = fixed(layouter, OP_POPEQ)?; // v215
        pis.extend([op_popeq, one.clone(), two.clone(), segment.clone()]); // pi[51..55]

        // Pedersen value commitment:
        // value * hash_to_curve(type, segment) + rc * G.
        let base_point = std.hash_to_curve(
            layouter,
            &[token_type[0].clone(), token_type[1].clone(), segment],
        )?;
        let base_x = std.jubjub().x_coordinate(&base_point); // v216
        let base_y = std.jubjub().y_coordinate(&base_point); // v217
        let generator: midnight_circuits_v1::types::AssignedNativePoint<OldEmbeddedAffineExt> = std
            .jubjub()
            .assign_fixed(layouter, OldEmbeddedAffine::generator())?;
        let rc_scalar = std.jubjub().convert(layouter, rc)?;
        let blind = std.jubjub().msm(layouter, &[rc_scalar], &[generator])?;
        let blind_x = std.jubjub().x_coordinate(&blind); // v218
        let blind_y = std.jubjub().y_coordinate(&blind); // v219
        let base = v1_ecc_from_parts(std, layouter, &base_x, &base_y)?;
        let value_scalar = std.jubjub().convert(layouter, value)?;
        let committed = std.jubjub().msm(layouter, &[value_scalar], &[base])?;
        let committed_x = std.jubjub().x_coordinate(&committed); // v220
        let committed_y = std.jubjub().y_coordinate(&committed); // v221
        let blind_pt = v1_ecc_from_parts(std, layouter, &blind_x, &blind_y)?;
        let committed_pt = v1_ecc_from_parts(std, layouter, &committed_x, &committed_y)?;
        let vc = std.jubjub().add(layouter, &blind_pt, &committed_pt)?;
        let vc_x = std.jubjub().x_coordinate(&vc); // v222
        let vc_y = std.jubjub().y_coordinate(&vc); // v223

        // Transcript: Cell_write([2], value_commitment).
        pis.extend([op_push, one.clone(), one.clone(), one.clone(), two.clone()]); // pi[55..60]
        pis.extend([op_push_s, one, two, neg_two.clone(), neg_two, vc_x, vc_y]); // pi[60..67]
        pis.push(op_ins_1); // pi[67]

        debug_assert_eq!(pis.len(), NUM_PUBLIC_INPUTS);
        pis.iter()
            .try_for_each(|pi| std.constrain_as_public_input(layouter, pi))
    }

    fn used_chips(&self) -> ZkStdLibArch {
        ZkStdLibArch {
            jubjub: true,
            poseidon: true,
            sha2_256: true,
            sha2_512: false,
            sha3_256: false,
            keccak_256: false,
            blake2b: false,
            nr_pow2range_cols: 1,
            secp256k1: false,
            bls12_381: false,
            base64: false,
            automaton: false,
        }
    }

    fn write_relation<W: std::io::Write>(&self, _writer: &mut W) -> std::io::Result<()> {
        Ok(())
    }

    fn read_relation<R: std::io::Read>(_reader: &mut R) -> std::io::Result<Self> {
        Ok(ZswapSpend)
    }
}
