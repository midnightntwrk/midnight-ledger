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
//! This is a hand-written counterpart of the `spend` circuit in
//! `zswap/zswap.compact` (compiled to `zswap/static/spend.zkir`). It proves the
//! same statement and exposes the same public inputs, which
//! `zswap/tests/spend_circuit.rs` checks against the IR virtual machine. It is
//! *not* constrained to synthesise the same constraint system, so its verifier
//! key differs from the shipped `zswap/static/spend.verifier`.
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
//! # Public transcript
//!
//! The public inputs are the binding input followed by the field
//! representation of the Impact program the ledger replays against the zswap
//! contract state. That program is defined once, as a [`SpendTranscript`],
//! using the same program fragments the ledger uses to verify spends; the
//! circuit derives its public-input layout from that definition rather than
//! spelling out opcodes and indices (see `spend_layout`).
//!
//! # 32-byte values in the witness
//!
//! 32-byte quantities (hashes, keys, addresses) do not fit in a BLS12-381
//! scalar; following `FieldRepr` for `[u8; 32]` they are split into two field
//! elements: the final byte (`hi`), then the first 31 bytes interpreted
//! little-endian (`lo`). [`split_bytes32`] implements this split.

use base_crypto::fab::AlignedValue;
use base_crypto::hash::{HashOutput, persistent_hash};
use coin_structure::{coin::Nullifier, contract::ContractAddress};
use group::Group;
use midnight_circuits_v1::instructions::{
    ArithInstructions, AssignmentInstructions, ControlFlowInstructions, ConversionInstructions,
    DecompositionInstructions, EccInstructions, PublicInputInstructions,
};
use midnight_circuits_v1::types::{AssignedBit, AssignedByte, AssignedNative, AssignedNativePoint};
use midnight_onchain_runtime::{
    ops::{Key, Op},
    program_fragments::*,
    result_mode::ResultModeGather,
    state::StateValue,
};
use midnight_proofs_v1::{
    circuit::{Layouter, Value},
    plonk::Error,
};
use midnight_zk_stdlib_v1::{Relation, ZkStdLib, ZkStdLibArch};
use std::collections::{HashMap, hash_map::Entry};
use std::sync::OnceLock;
use storage::{arena::Sp, db::InMemoryDB};
use transient_crypto::curve::{EmbeddedGroupAffine, Fr};
use transient_crypto::hash::{hash_to_curve, transient_hash};
use transient_crypto::repr::FieldRepr;
use zkir_v2::ir_v1::{OldEmbeddedAffine, OldEmbeddedAffineExt, OldScalar, cvt};

use crate::verify::{CADDR_OP_LEN, with_outputs};

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

/// `SHA256(parts[0] || parts[1] || ...)`, split per [`split_bytes32`].
fn hash_parts(parts: &[&[u8]]) -> [Fr; 2] {
    split_bytes32(&persistent_hash(&parts.concat()).0)
}

/// The public transcript of a spend: the Impact program that the ledger
/// replays against the zswap contract state, together with the outputs it is
/// expected to produce.
///
/// This is the same program `Input::well_formed` builds to verify a spend. The
/// state slots (`0` merkle tree, `1` nullifiers, `2` value commitment,
/// `3` contract address, `5` segment) are those of the `zswap.compact` ledger.
#[derive(Clone, Debug)]
pub struct SpendTranscript {
    /// The Merkle tree root the spent commitment is a member of.
    pub merkle_root: Fr,
    /// The result of the historic root membership check; `true` for any valid
    /// spend.
    pub root_check_passed: bool,
    /// The coin nullifier.
    pub nullifier: [u8; 32],
    /// The owning contract, for contract-owned coins.
    pub contract_address: Option<[u8; 32]>,
    /// The Pedersen value commitment, as (x, y).
    pub value_commitment: (Fr, Fr),
    /// The transaction segment, read back from the contract state.
    pub segment: u16,
}

impl SpendTranscript {
    /// The field representation of the transcript, excluding the binding
    /// input.
    pub fn to_field_elements(&self) -> Vec<Fr> {
        let mut prog = Vec::new();
        prog.extend::<[Op<ResultModeGather, InMemoryDB>; 6]>(HistoricMerkleTree_check_root!(
            [Key::Value(0u8.into())],
            false,
            32,
            Fr,
            self.merkle_root
        ));
        prog.extend(Set_insert!(
            [Key::Value(1u8.into())],
            false,
            [u8; 32],
            Nullifier(HashOutput(self.nullifier))
        ));
        match self.contract_address {
            Some(addr) => prog.extend(Cell_write!(
                [Key::Value(3u8.into())],
                false,
                ContractAddress,
                ContractAddress(HashOutput(addr))
            )),
            None => prog.push(Op::Noop { n: CADDR_OP_LEN }),
        }
        prog.extend(Cell_read!([Key::Value(5u8.into())], false, u16));
        prog.extend(Cell_write!(
            [Key::Value(2u8.into())],
            false,
            (Fr, Fr),
            self.value_commitment
        ));

        let outputs = [self.root_check_passed.into(), self.segment.into()];
        let mut elements = Vec::new();
        for op in with_outputs(prog.into_iter(), outputs.into_iter()) {
            op.field_repr(&mut elements);
        }
        elements
    }
}

/// How one public input (after the binding input) is obtained in-circuit.
#[derive(Clone, Copy, Debug)]
enum PI {
    /// Fixed by the shape of the program (opcodes, paths, alignments).
    Const(Fr),
    /// `c` for contract-owned spends, `0` for user-owned spends.
    IfContract(Fr),
    MerkleRoot,
    RootCheck,
    /// Half of the nullifier, in `split_bytes32` order.
    Nullifier(usize),
    /// Half of the contract address, in `split_bytes32` order; zero for user
    /// spends.
    Address(usize),
    ValueCommitment(usize),
    Segment,
}

/// Derives the circuit's public-input layout from [`SpendTranscript`].
///
/// A position is a *variable* if it changes when exactly that field of the
/// transcript changes, *contract-guarded* if it only differs between a
/// contract-owned and a user-owned spend, and a constant otherwise.
fn spend_layout() -> &'static [PI] {
    // A transcript field: how to copy it between transcripts, and how the
    // circuit refers to the `k`th element it occupies.
    type Field = (fn(&mut SpendTranscript, &SpendTranscript), fn(usize) -> PI);
    const FIELDS: [Field; 6] = [
        (|t, s| t.merkle_root = s.merkle_root, |_| PI::MerkleRoot),
        (
            |t, s| t.root_check_passed = s.root_check_passed,
            |_| PI::RootCheck,
        ),
        (|t, s| t.nullifier = s.nullifier, PI::Nullifier),
        (|t, s| t.contract_address = s.contract_address, PI::Address),
        (
            |t, s| t.value_commitment = s.value_commitment,
            PI::ValueCommitment,
        ),
        (|t, s| t.segment = s.segment, |_| PI::Segment),
    ];

    static LAYOUT: OnceLock<Vec<PI>> = OnceLock::new();
    LAYOUT.get_or_init(|| {
        // Two contract-owned transcripts that differ in every field.
        let sample = |seed: u8| SpendTranscript {
            merkle_root: Fr::from(100 + seed as u64),
            root_check_passed: seed != 0,
            nullifier: [1 + seed; 32],
            contract_address: Some([3 + seed; 32]),
            value_commitment: (Fr::from(200 + seed as u64), Fr::from(300 + seed as u64)),
            segment: seed as u16,
        };
        let (a, b) = (sample(0), sample(1));
        let contract = a.to_field_elements();
        let user = SpendTranscript {
            contract_address: None,
            ..a.clone()
        }
        .to_field_elements();

        let mut layout: Vec<_> = contract
            .iter()
            .zip(&user)
            .map(|(c, u)| {
                if c == u {
                    PI::Const(*c)
                } else {
                    PI::IfContract(*c)
                }
            })
            .collect();
        for (copy_field, pi) in FIELDS {
            let mut probe = a.clone();
            copy_field(&mut probe, &b);
            let probed = probe.to_field_elements();
            let changed = (0..layout.len()).filter(|&i| probed[i] != contract[i]);
            for (k, i) in changed.enumerate() {
                assert!(
                    matches!(layout[i], PI::Const(_) | PI::IfContract(_)),
                    "transcript fields overlap"
                );
                layout[i] = pi(k);
            }
        }

        // The contract guard must zero exactly the contract-address write.
        for (i, pi) in layout.iter().enumerate() {
            let guarded = matches!(pi, PI::IfContract(_) | PI::Address(_));
            let expected = if guarded { Fr::from(0u64) } else { contract[i] };
            assert!(user[i] == expected, "unexpected contract guard");
        }
        layout
    })
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
    /// from the public transcript outputs. `true` for any valid spend.
    pub check_root_result: bool,
    /// The transaction segment, read from the public transcript outputs.
    pub segment: u16,
}

impl SpendWitness {
    /// The primary input vector, in IR memory order.
    pub fn flatten(&self) -> Vec<Fr> {
        let mut inputs = vec![Fr::from(self.is_user as u64)];
        inputs.extend([self.sk, self.contract_address, self.commitment].concat());
        for (sibling, goes_left) in &self.path {
            inputs.extend([*sibling, Fr::from(*goes_left as u64)]);
        }
        inputs.extend([self.nonce, self.token_type].concat());
        inputs.extend([self.value, self.rc]);
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

    /// `SHA256(domain_sep || nonce || type || value || is_user || key)`.
    fn keyed_coin_hash(&self, domain_sep: &[u8; 21], key: &[Fr; 2]) -> [Fr; 2] {
        hash_parts(&[
            domain_sep,
            &join_bytes32(&self.nonce),
            &join_bytes32(&self.token_type),
            &self.value.as_le_bytes()[..16],
            &[self.is_user as u8],
            &join_bytes32(key),
        ])
    }

    /// The commitment target: the coin public key for user-owned coins, or
    /// the contract address.
    pub fn target(&self) -> [Fr; 2] {
        if self.is_user {
            hash_parts(&[PK_DOMAIN_SEP, &join_bytes32(&self.sk)])
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
            &self.sk
        } else {
            &self.contract_address
        };
        self.keyed_coin_hash(CN_DOMAIN_SEP, sender)
    }

    /// The Pedersen value commitment
    /// `value * hash_to_curve(type, segment) + rc * G`, as (x, y).
    pub fn value_commitment(&self) -> [Fr; 2] {
        let [hi, lo] = self.token_type;
        let base = hash_to_curve(&[hi, lo, Fr::from(self.segment as u64)]);
        let point = EmbeddedGroupAffine::generator() * self.rc + base * self.value;
        [point.x().unwrap_or(0.into()), point.y().unwrap_or(0.into())]
    }

    /// The public transcript this witness satisfies.
    pub fn transcript(&self) -> SpendTranscript {
        SpendTranscript {
            merkle_root: self.merkle_root(),
            root_check_passed: self.check_root_result,
            nullifier: join_bytes32(&self.nullifier()),
            contract_address: (!self.is_user).then(|| join_bytes32(&self.contract_address)),
            value_commitment: self.value_commitment().into(),
            segment: self.segment,
        }
    }

    /// The full public input vector this witness satisfies, including the
    /// binding input, in circuit order.
    pub fn public_inputs(&self) -> Vec<Fr> {
        let mut pis = vec![self.binding_input];
        pis.extend(self.transcript().to_field_elements());
        pis
    }

    /// The public transcript inputs of the proof preimage: the declared public
    /// inputs minus the binding input, with the contract-address block removed
    /// when it is skipped (user-owned spends).
    pub fn public_transcript_inputs(&self) -> Vec<Fr> {
        self.transcript()
            .to_field_elements()
            .into_iter()
            .zip(spend_layout())
            .filter(|(_, pi)| !self.is_user || !matches!(pi, PI::IfContract(_) | PI::Address(_)))
            .map(|(element, _)| element)
            .collect()
    }
}

/// The zswap spend relation. See the module documentation.
#[derive(Clone, Debug, Default)]
pub struct ZswapSpend;

type Assigned = AssignedNative<OldScalar>;
type Bit = AssignedBit<OldScalar>;
type Byte = AssignedByte<OldScalar>;
/// A 32-byte value split per [`split_bytes32`], in-circuit.
type Pair = [Assigned; 2];

/// The coin fields shared by the commitment and the nullifier, decoded once.
struct Coin {
    /// `nonce || type || value || is_user`, as hashed.
    bytes: Vec<Byte>,
    /// The 16 little-endian bytes of `value`, a sub-slice of `bytes`.
    value: Vec<Byte>,
}

/// The standard library and layouter that gadgets are built with.
struct Gadgets<'a, L> {
    std: &'a ZkStdLib,
    layouter: &'a mut L,
}

impl<L: Layouter<OldScalar>> Gadgets<'_, L> {
    fn assign(&mut self, value: Value<Fr>) -> Result<Assigned, Error> {
        self.std.assign(self.layouter, value.map(|f| cvt(f.0)))
    }

    fn fixed(&mut self, constant: Fr) -> Result<Assigned, Error> {
        self.std.assign_fixed(self.layouter, cvt(constant.0))
    }

    /// Constrains `x` to be a bit.
    fn bit(&mut self, x: &Assigned) -> Result<Bit, Error> {
        self.std.convert(self.layouter, x)
    }

    /// `bit ? a : b`.
    fn select(&mut self, bit: &Bit, a: &Assigned, b: &Assigned) -> Result<Assigned, Error> {
        self.std.select(self.layouter, bit, a, b)
    }

    fn select_pair(&mut self, bit: &Bit, a: &Pair, b: &Pair) -> Result<Pair, Error> {
        Ok([
            self.select(bit, &a[0], &b[0])?,
            self.select(bit, &a[1], &b[1])?,
        ])
    }

    /// The `n` little-endian bytes of `x`. Constrains `x < 2^(8n)`.
    fn bytes(&mut self, x: &Assigned, n: usize) -> Result<Vec<Byte>, Error> {
        self.std.assigned_to_le_bytes(self.layouter, x, Some(n))
    }

    /// The bytes of a [`split_bytes32`] pair, as [`join_bytes32`] orders them.
    fn bytes32(&mut self, [hi, lo]: &Pair) -> Result<Vec<Byte>, Error> {
        let mut bytes = self.bytes(lo, 31)?;
        bytes.extend(self.bytes(hi, 1)?);
        Ok(bytes)
    }

    /// `SHA256(parts[0] || parts[1] || ...)`, split per [`split_bytes32`].
    fn sha256(&mut self, parts: &[&[Byte]]) -> Result<Pair, Error> {
        let digest = self.std.sha2_256(self.layouter, &parts.concat())?;
        Ok([
            self.std.convert(self.layouter, &digest[31])?,
            self.std
                .assigned_from_le_bytes(self.layouter, &digest[..31])?,
        ])
    }

    fn domain_sep(&mut self, domain_sep: &[u8; 21]) -> Result<Vec<Byte>, Error> {
        domain_sep
            .iter()
            .map(|byte| self.std.assign_fixed(self.layouter, *byte))
            .collect()
    }

    /// `SHA256(domain_sep || nonce || type || value || is_user || key)`.
    fn coin_hash(&mut self, domain_sep: &[u8; 21], coin: &Coin, key: &Pair) -> Result<Pair, Error> {
        let domain_sep = self.domain_sep(domain_sep)?;
        let key = self.bytes32(key)?;
        self.sha256(&[&domain_sep, &coin.bytes, &key])
    }

    /// Poseidon-hashes the leaf up the Merkle path, each level ordered by the
    /// direction bit.
    fn merkle_root(
        &mut self,
        leaf: &Assigned,
        path: &[(Assigned, Bit)],
    ) -> Result<Assigned, Error> {
        path.iter()
            .try_fold(leaf.clone(), |node, (sibling, goes_left)| {
                let left = self.select(goes_left, &node, sibling)?;
                let right = self.select(goes_left, sibling, &node)?;
                self.std.poseidon(self.layouter, &[left, right])
            })
    }

    /// `value * hash_to_curve(type, segment) + rc * G`, as (x, y).
    fn value_commitment(
        &mut self,
        token_type: &Pair,
        coin: &Coin,
        segment: &Assigned,
        rc: &Assigned,
    ) -> Result<Pair, Error> {
        let jubjub = self.std.jubjub();
        let [hi, lo] = token_type.clone();
        let base = self
            .std
            .hash_to_curve(self.layouter, &[hi, lo, segment.clone()])?;
        let generator: AssignedNativePoint<OldEmbeddedAffineExt> =
            jubjub.assign_fixed(self.layouter, OldEmbeddedAffine::generator())?;
        let rc = jubjub.convert(self.layouter, rc)?;
        // Built from the hashed value bytes, not from `convert(value)`: this
        // gives 128 bits, not 255, and ties the committed value to the hashed
        // one. `value < 2^128 < r_J`, so no reduction occurs.
        let value = jubjub.scalar_from_le_bytes(self.layouter, &coin.value)?;
        let blinding = jubjub.msm(self.layouter, &[rc], &[generator])?;
        let committed = jubjub.msm(self.layouter, &[value], &[base])?;
        let sum = jubjub.add(self.layouter, &blinding, &committed)?;
        Ok([jubjub.x_coordinate(&sum), jubjub.y_coordinate(&sum)])
    }
}

impl Relation for ZswapSpend {
    type Instance = Vec<OldScalar>;

    type Witness = SpendWitness;

    fn format_instance(instance: &Self::Instance) -> Result<Vec<OldScalar>, Error> {
        Ok(instance.clone())
    }

    fn circuit(
        &self,
        std: &ZkStdLib,
        layouter: &mut impl Layouter<OldScalar>,
        _instance: Value<Self::Instance>,
        witness: Value<Self::Witness>,
    ) -> Result<(), Error> {
        let mut g = Gadgets { std, layouter };

        // The primary inputs, in `SpendWitness::flatten` order.
        let inputs = witness.as_ref().map(|w| {
            w.flatten()
                .into_iter()
                .map(|f| cvt(f.0))
                .collect::<Vec<_>>()
        });
        let mut cells = std
            .assign_many(g.layouter, &inputs.transpose_vec(NUM_INPUTS))?
            .into_iter();
        let mut next = || cells.next().expect("one cell per witness input");
        let is_user = next();
        let sk = [next(), next()];
        let contract_address = [next(), next()];
        // Unused: the Merkle leaf is the commitment computed below, so this
        // witness would only be constrained to equal it.
        let _commitment = [next(), next()];
        let path: Vec<_> = (0..ZSWAP_TREE_HEIGHT).map(|_| (next(), next())).collect();
        let nonce = [next(), next()];
        let token_type = [next(), next()];
        let value = next();
        let rc = next();

        // The values only the public transcript carries.
        let binding_input = g.assign(witness.as_ref().map(|w| w.binding_input))?;
        let root_check_passed = g.assign(
            witness
                .as_ref()
                .map(|w| Fr::from(w.check_root_result as u64)),
        )?;
        let segment = g.assign(witness.as_ref().map(|w| Fr::from(w.segment as u64)))?;

        let is_user_bit = g.bit(&is_user)?;
        let path = path
            .into_iter()
            .map(|(sibling, goes_left)| Ok((sibling, g.bit(&goes_left)?)))
            .collect::<Result<Vec<_>, Error>>()?;

        // The byte decompositions in this function are the only range checks
        // on `sk`, `nonce`, `token_type` and `value`, and on `contract_address`
        // through the selected target and sender. For user spends
        // `contract_address` stays unchecked, but its only use is in public
        // inputs that are multiplied by `is_contract = 0`.
        let mut coin_bytes = g.bytes32(&nonce)?;
        coin_bytes.extend(g.bytes32(&token_type)?);
        let value_bytes = g.bytes(&value, 16)?;
        coin_bytes.extend(value_bytes.iter().cloned());
        coin_bytes.extend(g.bytes(&is_user, 1)?);
        let coin = Coin {
            bytes: coin_bytes,
            value: value_bytes,
        };

        // Commitment opening: the coin's commitment, with its target (the
        // public key for users, the contract address otherwise), is the leaf.
        let ds_pk = g.domain_sep(PK_DOMAIN_SEP)?;
        let sk_bytes = g.bytes32(&sk)?;
        let public_key = g.sha256(&[&ds_pk, &sk_bytes])?;
        let target = g.select_pair(&is_user_bit, &public_key, &contract_address)?;
        let commitment = g.coin_hash(CC_DOMAIN_SEP, &coin, &target)?;

        // Membership: the leaf is the low half of the commitment.
        let merkle_root = g.merkle_root(&commitment[1], &path)?;

        // Nullifier: keyed with the sender (the secret key for users).
        let sender = g.select_pair(&is_user_bit, &sk, &contract_address)?;
        let nullifier = g.coin_hash(CN_DOMAIN_SEP, &coin, &sender)?;

        let value_commitment = g.value_commitment(&token_type, &coin, &segment, &rc)?;

        // Public transcript: its shape comes from `SpendTranscript`; here we
        // only say where each variable comes from.
        let is_contract = std.linear_combination(
            g.layouter,
            &[(-OldScalar::from(1u64), is_user)],
            OldScalar::from(1u64),
        )?;
        let mut constants: HashMap<Vec<u8>, Assigned> = HashMap::new();
        let mut pis = vec![binding_input];
        for pi in spend_layout() {
            pis.push(match *pi {
                PI::Const(c) => match constants.entry(c.as_le_bytes().to_vec()) {
                    Entry::Occupied(cell) => cell.get().clone(),
                    Entry::Vacant(slot) => slot.insert(g.fixed(c)?).clone(),
                },
                PI::IfContract(c) if c == Fr::from(1u64) => is_contract.clone(),
                PI::IfContract(c) => std.mul_by_constant(g.layouter, &is_contract, cvt(c.0))?,
                PI::MerkleRoot => merkle_root.clone(),
                PI::RootCheck => root_check_passed.clone(),
                PI::Nullifier(half) => nullifier[half].clone(),
                PI::Address(half) => {
                    std.mul(g.layouter, &is_contract, &contract_address[half], None)?
                }
                PI::ValueCommitment(coord) => value_commitment[coord].clone(),
                PI::Segment => segment.clone(),
            });
        }
        debug_assert_eq!(pis.len(), NUM_PUBLIC_INPUTS);
        pis.iter()
            .try_for_each(|pi| std.constrain_as_public_input(g.layouter, pi))
    }

    fn used_chips(&self) -> ZkStdLibArch {
        ZkStdLibArch {
            jubjub: true,
            poseidon: true,
            sha2_256: true,
            ..ZkStdLibArch::default()
        }
    }

    fn write_relation<W: std::io::Write>(&self, _writer: &mut W) -> std::io::Result<()> {
        Ok(())
    }

    fn read_relation<R: std::io::Read>(_reader: &mut R) -> std::io::Result<Self> {
        Ok(ZswapSpend)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_shape() {
        let layout = spend_layout();
        let count = |f: fn(&PI) -> bool| layout.iter().filter(|pi| f(pi)).count();
        assert_eq!(layout.len() + 1, NUM_PUBLIC_INPUTS);
        assert_eq!(count(|pi| matches!(pi, PI::MerkleRoot)), 1);
        assert_eq!(count(|pi| matches!(pi, PI::RootCheck)), 1);
        assert_eq!(count(|pi| matches!(pi, PI::Nullifier(_))), 2);
        assert_eq!(count(|pi| matches!(pi, PI::ValueCommitment(_))), 2);
        assert_eq!(count(|pi| matches!(pi, PI::Segment)), 1);
        // The contract-address write is the guarded block, `CADDR_OP_LEN` long.
        let guarded = count(|pi| matches!(pi, PI::Address(_) | PI::IfContract(_)));
        assert_eq!(guarded, CADDR_OP_LEN as usize);
        assert_eq!(count(|pi| matches!(pi, PI::Address(_))), 2);
    }
}
