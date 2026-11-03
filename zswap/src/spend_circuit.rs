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

//! The zswap spend circuits, written directly against zk-stdlib.
//!
//! Following the custom spend logic MIP, every `Input` carries a proof from
//! one of two spend circuits, which replace today's `spend`
//! (`zswap/zswap.compact`, compiled to `zswap/static/spend.zkir`):
//!
//! - the *plain circuit*, whose descriptor is always empty, and
//! - the *guarded circuit*, which also enforces the coin's spend rule, and
//!   spends coins with or without one.
//!
//! # Statement
//!
//! Both circuits prove today's statement, for a coin `(nonce, type, value)`:
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
//! and both re-derive the coin's type from its preimage, with no colour
//! exempt:
//!
//! 6. *Type re-derivation*: `type = SHA256(inner || domain_sep || contract_address [|| descriptor])`;
//!    the plain circuit always with the empty descriptor.
//!
//! The guarded circuit takes the descriptor as a `Maybe`. One flag, whether
//! it is present, decides both which hash the type must match and which guard
//! is verified:
//!
//! 7. *Spend rule*: for a guarded type (mode 2, version 1; the descriptor's
//!    payload is the guard's verifying-key hash), a guard proof is verified
//!    recursively against a verifying key the prover witnesses privately,
//!    bound in-circuit to the payload, on the guard's arguments
//!    ([`construct::guard_args`](crate::guard_args)). For a coin without a
//!    rule, the published `ACCEPT_ALL` proof is verified on fixed arguments.
//!    The resulting accumulator is multiplied by a random factor, by
//!    accumulating it with a witnessed random multiple of itself, and
//!    exposed; the verifier finishes its pairing check.
//! 8. *Named output*: if the spend names an output, it pays the declared
//!    recipient, and its commitment is disclosed.
//!
//! # Public inputs
//!
//! The plain circuit's are today's: the binding input, then the field
//! representation of the Impact program the ledger replays against the zswap
//! contract state ([`SpendTranscript`]). The guarded circuit's transcript also
//! reads the guard's `reserved` inputs (before the segment), and its public
//! inputs continue with the named output (`is_some`, then its commitment),
//! then the accumulator.
//!
//! # 32-byte values in the witness
//!
//! 32-byte quantities (hashes, keys, addresses) do not fit in a BLS12-381
//! scalar; following `FieldRepr` for `[u8; 32]` they are split into two field
//! elements: the final byte (`hi`), then the first 31 bytes interpreted
//! little-endian (`lo`). [`split_bytes32`] implements this split.
//!
//! # Proving library
//!
//! The circuits are written against midnight-zk `main` (patched in the
//! workspace `Cargo.toml`), re-exported as [`zk`]: only it verifies a proof
//! against a privately witnessed key.

use base_crypto::fab::AlignedValue;
use base_crypto::hash::HashOutput;
use coin_structure::custom_spend::Descriptor;
use coin_structure::{coin::Nullifier, contract::ContractAddress};
use ff::{Field, PrimeField};
use group::{Curve, Group, GroupEncoding};
use midnight_circuits::hash::poseidon::PoseidonState;
use midnight_circuits::instructions::{
    ArithInstructions, AssertionInstructions, AssignmentInstructions, ControlFlowInstructions,
    ConversionInstructions, DecompositionInstructions, EccInstructions, PublicInputInstructions,
    BinaryInstructions, ZeroInstructions,
};
use midnight_circuits::types::{
    AssignedBit, AssignedByte, AssignedNative, AssignedNativePoint, Instantiable,
};
use midnight_circuits::verifier::{
    Accumulator, AssignedAccumulator, BlstrsEmulation, Msm, Point, SelfEmulation, vk_hash,
};
use midnight_onchain_runtime::{
    ops::{Key, Op},
    program_fragments::*,
    result_mode::ResultModeGather,
    state::StateValue,
};
use midnight_proofs::{
    circuit::{Layouter, Value},
    plonk::{self, ConstraintSystem, Error},
    poly::{
        PolynomialLabel,
        kzg::params::{ParamsKZG, ParamsVerifierKZG},
    },
    transcript::{CircuitTranscript, Transcript},
    utils::SerdeFormat,
};
use midnight_zk_stdlib::{DefaultPCS, MidnightPK, MidnightVK, Relation, ZkStdLib, ZkStdLibArch};
use rand::{CryptoRng, Rng};
use serialize::{Deserializable, Serializable, Tagged};
use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap, hash_map::Entry};
use std::io::{self, Read, Write};
use std::sync::{Arc, Mutex, OnceLock};
use storage::{arena::Sp, db::InMemoryDB};
use transient_crypto::curve::{EmbeddedGroupAffine, Fr, outer};
use transient_crypto::hash::{hash_to_curve, transient_hash};
use transient_crypto::proofs::{
    DeferredAccumulator, InnerProofWitness, ParamsProverProvider, Proof, ProofPreimage,
    ProvingError,
};
use transient_crypto::repr::FieldRepr;

use crate::construct::{GUARD_ARGS_LEN, RESERVED_SLOT, reserved_value};
use crate::verify::{CADDR_OP_LEN, with_outputs};
use crate::{GUARDED_SPEND_KEY_LOCATION, PLAIN_SPEND_KEY_LOCATION};

/// The midnight-zk libraries the spend circuits and their guards are written
/// against.
pub mod zk {
    pub use midnight_circuits;
    pub use midnight_curves;
    pub use midnight_proofs;
    pub use midnight_zk_stdlib;
}

/// The circuits' native field (the BLS12-381 scalar field).
pub type F = midnight_curves::Fq;
type S = BlstrsEmulation;
type C = <S as SelfEmulation>::C;
type Bls12 = midnight_curves::Bls12;

/// The height of the zswap coin commitment Merkle tree.
pub const ZSWAP_TREE_HEIGHT: usize = 32;

/// The number of witness inputs of today's (IR) spend statement.
const NUM_LEGACY_INPUTS: usize = 77;

/// The number of legacy inputs preceding each circuit's extra inputs:
/// ownership, commitment, and Merkle path. The coin and `rc` follow them.
const NUM_INPUTS_BEFORE_EXTRA: usize = 71;

/// The plain circuit's extra inputs: the domain separator and contract
/// address of the coin's type (two fields each).
const PLAIN_EXTRA_INPUTS: usize = 4;

/// The guarded circuit's extra inputs: those of the plain circuit, then
/// `guarded`, the payload's key hash, the recipient (7) and the named output
/// (6).
const GUARDED_EXTRA_INPUTS: usize = 4 + 2 + 7 + 6;

/// The number of public inputs of today's spend statement, including the
/// binding input: those of the plain circuit.
pub const NUM_LEGACY_PUBLIC_INPUTS: usize = 68;

/// Domain separator for deriving a coin public key from a coin secret key.
pub const PK_DOMAIN_SEP: &[u8; 21] = b"midnight:zswap-pk[v1]";
/// Domain separator for coin commitments.
pub const CC_DOMAIN_SEP: &[u8; 21] = b"midnight:zswap-cc[v1]";
/// Domain separator for coin nullifiers.
pub const CN_DOMAIN_SEP: &[u8; 21] = b"midnight:zswap-cn[v1]";
/// The inner domain separator of shielded token type derivation.
const DERIVE_TOKEN_DOMAIN_SEP: &[u8; 32] = b"midnight:derive_token\0\0\0\0\0\0\0\0\0\0\0";

/// The log2 size of every guard. Version 1 fixes one circuit shape for
/// guards: this size, with [`guard_arch`], and [`GUARD_ARGS_LEN`] public
/// inputs.
pub const GUARD_K: u8 = 12;

/// The zk-stdlib architecture every guard must use.
pub fn guard_arch() -> ZkStdLibArch {
    ZkStdLibArch {
        poseidon: true,
        ..ZkStdLibArch::default()
    }
}

/// The label of the negated generator, the one fixed base a guard proof's
/// accumulator leaves when its key is witnessed.
fn neg_g_label() -> PolynomialLabel {
    PolynomialLabel::Custom("-G".into())
}

/// Unwraps a field element to the proving library's.
pub fn to_f(x: Fr) -> F {
    x.0
}

/// Wraps a field element of the proving library.
pub fn from_f(x: F) -> Fr {
    Fr(x)
}

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
    split_bytes32(&base_crypto::hash::persistent_hash(&parts.concat()).0)
}

/// The guard constraint system, as `MidnightVK::read` reconstructs it for a
/// guard's key, finalized.
fn guard_cs() -> &'static ConstraintSystem<F> {
    static CS: OnceLock<ConstraintSystem<F>> = OnceLock::new();
    CS.get_or_init(|| {
        let mut cs = ConstraintSystem::default();
        let _ = ZkStdLib::configure(&mut cs, (guard_arch(), GUARD_K - 1));
        cs.into_finalized()
    })
}

/// Reads a guard's verifying key, if it has the guard shape version 1 fixes.
pub fn read_guard_vk(bytes: &[u8]) -> Option<MidnightVK> {
    let mut reader = bytes;
    let vk = MidnightVK::read(&mut reader, SerdeFormat::Processed).ok()?;
    let arch = ZkStdLibArch::read(&mut &bytes[..]).ok()?;
    (reader.is_empty() && arch == guard_arch() && vk.k() == GUARD_K).then_some(vk)
}

/// Writes a guard's verifying key as a [`GuardProof`](crate::GuardProof)
/// carries it.
pub fn write_guard_vk(vk: &MidnightVK) -> Vec<u8> {
    let mut bytes = Vec::new();
    vk.write(&mut bytes, SerdeFormat::Processed)
        .expect("in-memory serialization");
    bytes
}

/// The hash of a guard's verifying key, as a mode 2 descriptor's payload
/// holds it: midnight-zk's `vk_hash`, little-endian.
pub fn guard_vk_hash(vk: &MidnightVK) -> HashOutput {
    let mut out = [0u8; 32];
    out.copy_from_slice(vk_hash::<S>(vk.vk()).to_repr().as_ref());
    HashOutput(out)
}

/// Verifies a guard proof, of a key in the guard shape, on `args`: what the
/// guarded circuit checks recursively, run in full.
pub fn verify_guard_proof(verifier_key: &[u8], proof: &[u8], args: &[Fr]) -> anyhow::Result<()> {
    let vk = read_guard_vk(verifier_key)
        .ok_or_else(|| anyhow::anyhow!("not a verifying key of the guard shape"))?;
    anyhow::ensure!(
        args.len() == GUARD_ARGS_LEN,
        "a guard takes {GUARD_ARGS_LEN} arguments, found {}",
        args.len()
    );
    // Every guard's instance is its arguments, as `AcceptAll` formats it.
    let args: Vec<F> = args.iter().copied().map(to_f).collect();
    midnight_zk_stdlib::verify::<AcceptAll, PoseidonState<F>>(
        params_verifier(),
        &vk,
        &args,
        None,
        proof,
    )
    .map_err(|_| anyhow::anyhow!("invalid guard proof"))
}

/// The public transcript of a spend: the Impact program that the ledger
/// replays against the zswap contract state, together with the outputs it is
/// expected to produce.
///
/// This is the same program `Input::well_formed` builds to verify a spend. The
/// state slots (`0` merkle tree, `1` nullifiers, `2` value commitment,
/// `3` contract address, `5` segment, `6` reserved) are those of the
/// `zswap.compact` ledger.
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
    /// The guard's reserved inputs the ledger supplies; read by the guarded
    /// circuit only.
    pub reserved: Option<[Fr; 4]>,
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
        let mut outputs = vec![self.root_check_passed.into()];
        if let Some([a, b, c, d]) = self.reserved {
            prog.extend(Cell_read!(
                [Key::Value(RESERVED_SLOT.into())],
                false,
                (Fr, Fr, Fr, Fr)
            ));
            outputs.push((a, b, c, d).into());
        }
        prog.extend(Cell_read!([Key::Value(5u8.into())], false, u16));
        outputs.push(self.segment.into());
        prog.extend(Cell_write!(
            [Key::Value(2u8.into())],
            false,
            (Fr, Fr),
            self.value_commitment
        ));

        let mut elements = Vec::new();
        for op in with_outputs(prog.into_iter(), outputs.into_iter()) {
            op.field_repr(&mut elements);
        }
        elements
    }
}

/// How one public input of the transcript is obtained in-circuit.
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
    /// One of the guard's reserved inputs.
    Reserved(usize),
    ValueCommitment(usize),
    Segment,
}

/// Derives a circuit's transcript layout from [`SpendTranscript`]: the
/// plain circuit's, or the guarded circuit's (with the reserved read).
///
/// A position is a *variable* if it changes when exactly that field of the
/// transcript changes, *contract-guarded* if it only differs between a
/// contract-owned and a user-owned spend, and a constant otherwise.
fn spend_layout(guarded: bool) -> &'static [PI] {
    // A transcript field: how to copy it between transcripts, and how the
    // circuit refers to the `k`th element it occupies.
    type Field = (fn(&mut SpendTranscript, &SpendTranscript), fn(usize) -> PI);
    const FIELDS: [Field; 7] = [
        (|t, s| t.merkle_root = s.merkle_root, |_| PI::MerkleRoot),
        (
            |t, s| t.root_check_passed = s.root_check_passed,
            |_| PI::RootCheck,
        ),
        (|t, s| t.nullifier = s.nullifier, PI::Nullifier),
        (|t, s| t.contract_address = s.contract_address, PI::Address),
        (|t, s| t.reserved = s.reserved, PI::Reserved),
        (
            |t, s| t.value_commitment = s.value_commitment,
            PI::ValueCommitment,
        ),
        (|t, s| t.segment = s.segment, |_| PI::Segment),
    ];

    fn layout(guarded: bool) -> Vec<PI> {
        // Two contract-owned transcripts that differ in every field.
        let sample = |seed: u8| SpendTranscript {
            merkle_root: Fr::from(100 + seed as u64),
            root_check_passed: seed != 0,
            nullifier: [1 + seed; 32],
            contract_address: Some([3 + seed; 32]),
            reserved: guarded.then(|| core::array::from_fn(|i| Fr::from(400 + 10 * seed as u64 + i as u64))),
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
    }

    static PLAIN: OnceLock<Vec<PI>> = OnceLock::new();
    static GUARDED: OnceLock<Vec<PI>> = OnceLock::new();
    if guarded {
        GUARDED.get_or_init(|| layout(true))
    } else {
        PLAIN.get_or_init(|| layout(false))
    }
}

/// A guard's verifying key and proof, as the guarded circuit witnesses them.
#[derive(Clone, Debug)]
pub struct GuardWitness {
    /// The guard's verifying key.
    pub vk: MidnightVK,
    /// The guard proof, over a Poseidon transcript.
    pub proof: Vec<u8>,
}

/// The guarded circuit's witness beyond the plain circuit's.
#[derive(Clone, Debug)]
pub struct GuardedWitness {
    /// Whether the coin's type has a descriptor (mode 2, version 1).
    pub guarded: bool,
    /// The descriptor's payload, the guard's key hash, as a field element.
    /// Zero without a descriptor.
    pub vk_hash: Fr,
    /// `recipient: Either<ShieldedAddress, ContractAddress>`, as
    /// [`recipient_fields`](crate::construct::recipient_fields) encodes it.
    pub recipient: [Fr; 7],
    /// `output: Maybe<ShieldedCoinInfo>`, as
    /// [`output_fields`](crate::construct::output_fields) encodes it.
    pub output: [Fr; 6],
    /// The guard's reserved inputs, read from the public transcript.
    pub reserved: [Fr; 4],
    /// The guard key and proof: the coin's guard's, or `ACCEPT_ALL`'s.
    pub guard: GuardWitness,
    /// The factor of the mask the accumulator is accumulated with, which
    /// randomises it.
    pub randomizer: Fr,
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
    /// The type's domain separator, split per [`split_bytes32`].
    pub domain_sep: [Fr; 2],
    /// The type's minting contract, split per [`split_bytes32`].
    pub type_contract: [Fr; 2],
    /// The guarded circuit's further witness; `None` for the plain circuit.
    pub guarded: Option<GuardedWitness>,
}

impl SpendWitness {
    /// The primary input vector of today's statement, in IR memory order.
    pub fn flatten(&self) -> Vec<Fr> {
        let mut inputs = vec![Fr::from(self.is_user as u64)];
        inputs.extend([self.sk, self.contract_address, self.commitment].concat());
        for (sibling, goes_left) in &self.path {
            inputs.extend([*sibling, Fr::from(*goes_left as u64)]);
        }
        inputs.extend([self.nonce, self.token_type].concat());
        inputs.extend([self.value, self.rc]);
        debug_assert_eq!(inputs.len(), NUM_LEGACY_INPUTS);
        inputs
    }

    /// The circuit's extra inputs, in proof preimage order.
    pub fn extra_inputs(&self) -> Vec<Fr> {
        let mut inputs = [self.domain_sep, self.type_contract].concat();
        if let Some(g) = &self.guarded {
            inputs.extend([Fr::from(g.guarded as u64), g.vk_hash]);
            inputs.extend(g.recipient);
            inputs.extend(g.output);
        }
        inputs
    }

    /// The proof preimage inputs: today's inputs, with the extra inputs
    /// inserted after the Merkle path (so the coin and `rc` stay last).
    pub fn preimage_inputs(&self) -> Vec<Fr> {
        let legacy = self.flatten();
        let mut inputs = legacy[..NUM_INPUTS_BEFORE_EXTRA].to_vec();
        inputs.extend(self.extra_inputs());
        inputs.extend_from_slice(&legacy[NUM_INPUTS_BEFORE_EXTRA..]);
        inputs
    }

    /// Reads a spend witness from its proof preimage. `guard` is the guard key
    /// and proof, and `randomizer` the accumulator's factor, for the guarded
    /// circuit.
    pub fn from_preimage(
        preimage: &ProofPreimage,
        guard: Option<(GuardWitness, Fr)>,
    ) -> anyhow::Result<Self> {
        let inputs = &preimage.inputs;
        let extra = match guard {
            Some(_) => GUARDED_EXTRA_INPUTS,
            None => PLAIN_EXTRA_INPUTS,
        };
        anyhow::ensure!(
            inputs.len() == NUM_LEGACY_INPUTS + extra,
            "a spend preimage has {} inputs, found {}",
            NUM_LEGACY_INPUTS + extra,
            inputs.len()
        );
        let bit = |f: Fr| -> anyhow::Result<bool> {
            if f == Fr::from(0u64) {
                Ok(false)
            } else if f == Fr::from(1u64) {
                Ok(true)
            } else {
                anyhow::bail!("expected a bit in the spend preimage")
            }
        };
        let pair = |i: usize| [inputs[i], inputs[i + 1]];
        let mut path = [(Fr::from(0u64), false); ZSWAP_TREE_HEIGHT];
        for (i, entry) in path.iter_mut().enumerate() {
            *entry = (inputs[7 + 2 * i], bit(inputs[8 + 2 * i])?);
        }
        let o = NUM_INPUTS_BEFORE_EXTRA;
        let c = o + extra;
        let outputs = &preimage.public_transcript_outputs;
        let expected_outputs = 2 + if guard.is_some() { 4 } else { 0 };
        anyhow::ensure!(
            outputs.len() == expected_outputs,
            "a spend preimage has {expected_outputs} public transcript outputs"
        );
        let guarded = match guard {
            None => None,
            Some((guard, randomizer)) => Some(GuardedWitness {
                guarded: bit(inputs[o + 4])?,
                vk_hash: inputs[o + 5],
                recipient: core::array::from_fn(|i| inputs[o + 6 + i]),
                output: core::array::from_fn(|i| inputs[o + 13 + i]),
                reserved: core::array::from_fn(|i| outputs[1 + i]),
                guard,
                randomizer,
            }),
        };
        Ok(SpendWitness {
            is_user: bit(inputs[0])?,
            sk: pair(1),
            contract_address: pair(3),
            commitment: pair(5),
            path,
            domain_sep: pair(o),
            type_contract: pair(o + 2),
            nonce: pair(c),
            token_type: pair(c + 2),
            value: inputs[c + 4],
            rc: inputs[c + 5],
            binding_input: preimage.binding_input,
            check_root_result: bit(outputs[0])?,
            segment: u16::try_from(*outputs.last().expect("checked above"))
                .map_err(|_| anyhow::anyhow!("segment is not a u16"))?,
            guarded,
        })
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

    /// The coin's index in the commitment tree, which its path's directions
    /// spell out (leaf level least significant; a left turn is a 0 bit).
    pub fn mt_index(&self) -> u64 {
        self.path
            .iter()
            .enumerate()
            .map(|(i, (_, goes_left))| (!goes_left as u64) << i)
            .sum()
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

    /// The coin public key for user-owned coins.
    fn public_key(&self) -> [Fr; 2] {
        hash_parts(&[PK_DOMAIN_SEP, &join_bytes32(&self.sk)])
    }

    /// The commitment target: the coin public key for user-owned coins, or
    /// the contract address.
    pub fn target(&self) -> [Fr; 2] {
        if self.is_user {
            self.public_key()
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

    /// The token type the preimage derives, split per [`split_bytes32`]. For
    /// a satisfiable witness this must equal `self.token_type`.
    pub fn derived_token_type(&self) -> [Fr; 2] {
        let mut preimage = [
            &DERIVE_TOKEN_DOMAIN_SEP[..],
            &join_bytes32(&self.domain_sep),
            &join_bytes32(&self.type_contract),
        ]
        .concat();
        if let Some(g) = self.guarded.as_ref().filter(|g| g.guarded) {
            let payload = HashOutput(
                g.vk_hash.as_le_bytes()[..32]
                    .try_into()
                    .expect("a field element has 32 bytes"),
            );
            preimage.extend(Descriptor::guard(payload).to_bytes());
        }
        hash_parts(&[&preimage])
    }

    /// The arguments the guard is handed, in the ledger's field encoding: the
    /// coin's guard's for a guarded type, or `ACCEPT_ALL`'s (zeros).
    pub fn guard_args(&self) -> Vec<Fr> {
        let Some(g) = self.guarded.as_ref().filter(|g| g.guarded) else {
            return vec![Fr::from(0u64); GUARD_ARGS_LEN];
        };
        let mut args = Vec::with_capacity(GUARD_ARGS_LEN);
        args.extend(self.nonce);
        args.extend(self.token_type);
        args.extend([self.value, Fr::from(self.mt_index())]);
        args.extend(self.nullifier());
        let zero = [Fr::from(0u64); 2];
        args.push(Fr::from(self.is_user as u64));
        args.extend(if self.is_user { self.public_key() } else { zero });
        args.extend(if self.is_user { zero } else { self.contract_address });
        args.extend(g.recipient);
        args.extend(g.output);
        args.extend(g.reserved);
        args
    }

    /// The named output's disclosure: `is_some`, then the commitment of the
    /// output to the recipient (zero if absent).
    pub fn named_output(&self) -> [Fr; 3] {
        let Some(g) = &self.guarded else {
            return [Fr::from(0u64); 3];
        };
        if g.output[0] == Fr::from(0u64) {
            return [Fr::from(0u64); 3];
        }
        let [is_left, ck0, ck1, _, _, c0, c1] = g.recipient;
        let target = if is_left == Fr::from(1u64) {
            [ck0, ck1]
        } else {
            [c0, c1]
        };
        let [_, n0, n1, t0, t1, v] = g.output;
        let [hi, lo] = hash_parts(&[
            CC_DOMAIN_SEP,
            &join_bytes32(&[n0, n1]),
            &join_bytes32(&[t0, t1]),
            &v.as_le_bytes()[..16],
            &[(is_left == Fr::from(1u64)) as u8],
            &join_bytes32(&target),
        ]);
        [Fr::from(1u64), hi, lo]
    }

    /// The guard proof's accumulator, fully collapsed, as the guarded
    /// circuit computes it before randomising it.
    fn guard_accumulator(&self) -> anyhow::Result<Accumulator<S>> {
        let g = self
            .guarded
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("the plain circuit exposes no accumulator"))?;
        let args: Vec<F> = self.guard_args().into_iter().map(to_f).collect();
        let mut transcript =
            CircuitTranscript::<PoseidonState<F>>::init_from_bytes(&g.guard.proof);
        let dual_msm = plonk::prepare::<F, DefaultPCS, CircuitTranscript<PoseidonState<F>>>(
            g.guard.vk.vk(),
            &[],
            &[&[], &args],
            &mut transcript,
        )?;
        let fixed_bases = BTreeMap::from([(neg_g_label(), -C::generator())]);
        let mut acc = Accumulator::<S>::from_dual_msm(dual_msm, &fixed_bases);
        acc.collapse();
        acc.resolve_fixed_bases(&fixed_bases);
        acc.collapse();
        Ok(acc)
    }

    /// The mask the guarded circuit accumulates the guard's accumulator
    /// with: the accumulator multiplied by the randomizer.
    pub fn mask_accumulator(&self) -> anyhow::Result<Accumulator<S>> {
        let g = self
            .guarded
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("the plain circuit exposes no accumulator"))?;
        let acc = self.guard_accumulator()?;
        let r = to_f(g.randomizer);
        let side = |msm: Msm<S>| msm.eval(&BTreeMap::new()) * r;
        Ok(collapsed(side(acc.lhs()), side(acc.rhs())))
    }

    /// The accumulator the guarded circuit exposes: the guard proof's,
    /// accumulated with its mask, so a random multiple of it.
    pub fn accumulator(&self) -> anyhow::Result<Accumulator<S>> {
        let mut acc = Accumulator::accumulate(&[self.mask_accumulator()?, self.guard_accumulator()?]);
        acc.collapse();
        Ok(acc)
    }

    /// The public transcript this witness satisfies.
    pub fn transcript(&self) -> SpendTranscript {
        SpendTranscript {
            merkle_root: self.merkle_root(),
            root_check_passed: self.check_root_result,
            nullifier: join_bytes32(&self.nullifier()),
            contract_address: (!self.is_user).then(|| join_bytes32(&self.contract_address)),
            reserved: self.guarded.as_ref().map(|g| g.reserved),
            value_commitment: self.value_commitment().into(),
            segment: self.segment,
        }
    }

    /// The binding input and the transcript: the plain circuit's public
    /// inputs.
    pub fn transcript_public_inputs(&self) -> Vec<Fr> {
        let mut pis = vec![self.binding_input];
        pis.extend(self.transcript().to_field_elements());
        pis
    }

    /// The circuit's public inputs, excluding the accumulator.
    pub fn public_inputs(&self) -> Vec<Fr> {
        let mut pis = self.transcript_public_inputs();
        if self.guarded.is_some() {
            pis.extend(self.named_output());
        }
        pis
    }

    /// The circuit's full instance: [`public_inputs`](Self::public_inputs),
    /// then the accumulator for the guarded circuit.
    pub fn instance(&self) -> anyhow::Result<Vec<F>> {
        let mut instance: Vec<F> = self.public_inputs().into_iter().map(to_f).collect();
        if self.guarded.is_some() {
            instance.extend(AssignedAccumulator::<S>::as_public_input(&self.accumulator()?));
        }
        Ok(instance)
    }

    /// The public transcript inputs of the proof preimage: the declared public
    /// inputs minus the binding input, with the contract-address block removed
    /// when it is skipped (user-owned spends).
    pub fn public_transcript_inputs(&self) -> Vec<Fr> {
        self.transcript()
            .to_field_elements()
            .into_iter()
            .zip(spend_layout(self.guarded.is_some()))
            .filter(|(_, pi)| !self.is_user || !matches!(pi, PI::IfContract(_) | PI::Address(_)))
            .map(|(element, _)| element)
            .collect()
    }
}

/// `ACCEPT_ALL`: a guard accepting everything. It exposes its arguments and
/// asserts nothing.
///
/// The guarded circuit verifies a fixed, published proof of it (on all-zero
/// arguments) for a coin without a rule, so its circuit shape does not depend
/// on the rule.
#[derive(Clone, Debug, Default)]
pub struct AcceptAll;

impl Relation for AcceptAll {
    type Instance = Vec<F>;
    type Witness = ();
    type Error = Error;

    fn format_instance(instance: &Self::Instance) -> Result<Vec<F>, Error> {
        Ok(instance.clone())
    }

    fn circuit(
        &self,
        std: &ZkStdLib,
        layouter: &mut impl Layouter<F>,
        instance: Value<Self::Instance>,
        _witness: Value<()>,
    ) -> Result<(), Error> {
        for field in instance.transpose_vec(GUARD_ARGS_LEN) {
            let cell: AssignedNative<F> = std.assign(layouter, field)?;
            std.constrain_as_public_input(layouter, &cell)?;
        }
        Ok(())
    }

    fn used_chips(&self) -> ZkStdLibArch {
        guard_arch()
    }

    fn write_relation<W: io::Write>(&self, _writer: &mut W) -> io::Result<()> {
        Ok(())
    }

    fn read_relation<R: io::Read>(_reader: &mut R) -> io::Result<Self> {
        Ok(AcceptAll)
    }
}

const ACCEPT_ALL_VK_RAW: &[u8] = include_bytes!("../static/accept-all.verifier");
const ACCEPT_ALL_PROOF_RAW: &[u8] = include_bytes!("../static/accept-all.proof");

/// `ACCEPT_ALL`'s published verifying key and its fixed proof, on all-zero
/// arguments.
pub fn accept_all() -> &'static GuardWitness {
    static ACCEPT_ALL: OnceLock<GuardWitness> = OnceLock::new();
    ACCEPT_ALL.get_or_init(|| GuardWitness {
        vk: read_guard_vk(ACCEPT_ALL_VK_RAW)
            .expect("the shipped accept-all verifying key must parse; regenerate it"),
        proof: ACCEPT_ALL_PROOF_RAW.to_vec(),
    })
}

/// `ACCEPT_ALL`'s key hash, which the guarded circuit fixes.
fn accept_all_hash() -> F {
    static HASH: OnceLock<F> = OnceLock::new();
    *HASH.get_or_init(|| vk_hash::<S>(accept_all().vk.vk()))
}

/// The zswap spend relations: the plain circuit, or the guarded circuit.
#[derive(Clone, Debug, Default)]
pub struct ZswapSpend {
    pub guarded: bool,
}

type Assigned = AssignedNative<F>;
type Bit = AssignedBit<F>;
type Byte = AssignedByte<F>;
/// A 32-byte value split per [`split_bytes32`], in-circuit.
type Pair = [Assigned; 2];

/// The standard library and layouter that gadgets are built with.
struct Gadgets<'a, L> {
    std: &'a ZkStdLib,
    layouter: &'a mut L,
}

impl<L: Layouter<F>> Gadgets<'_, L> {
    fn assign(&mut self, value: Value<Fr>) -> Result<Assigned, Error> {
        self.std.assign(self.layouter, value.map(to_f))
    }

    fn fixed(&mut self, constant: Fr) -> Result<Assigned, Error> {
        self.std.assign_fixed(self.layouter, to_f(constant))
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

    /// `bit ? x : 0`.
    fn gate(&mut self, bit: &Bit, x: &Assigned) -> Result<Assigned, Error> {
        let bit: Assigned = bit.clone().into();
        self.std.mul(self.layouter, &bit, x, None)
    }

    /// `1 - bit`.
    fn not(&mut self, bit: &Bit) -> Result<Bit, Error> {
        self.std.not(self.layouter, bit)
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

    fn const_bytes(&mut self, bytes: &[u8]) -> Result<Vec<Byte>, Error> {
        bytes
            .iter()
            .map(|byte| self.std.assign_fixed(self.layouter, *byte))
            .collect()
    }

    /// `SHA256(sep || nonce || type || value || is_user || key)`, from the
    /// coin's bytes `nonce || type || value || is_user`.
    fn coin_hash(
        &mut self,
        domain_sep: &[u8; 21],
        coin_bytes: &[Byte],
        key: &Pair,
    ) -> Result<Pair, Error> {
        let domain_sep = self.const_bytes(domain_sep)?;
        let key = self.bytes32(key)?;
        self.sha256(&[&domain_sep, coin_bytes, &key])
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

    /// `value * hash_to_curve(type, segment) + rc * G`, as (x, y), from the
    /// value's 16 little-endian bytes.
    fn value_commitment(
        &mut self,
        token_type: &Pair,
        value_bytes: &[Byte],
        segment: &Assigned,
        rc: &Assigned,
    ) -> Result<Pair, Error> {
        let jubjub = self.std.jubjub();
        let [hi, lo] = token_type.clone();
        let base = self
            .std
            .hash_to_curve(self.layouter, &[hi, lo, segment.clone()])?;
        let generator: AssignedNativePoint<midnight_curves::JubjubExtended> = jubjub
            .assign_fixed(
                self.layouter,
                midnight_curves::JubjubSubgroup::generator(),
            )?;
        let rc = jubjub.convert(self.layouter, rc)?;
        // Built from the hashed value bytes, not from `convert(value)`: this
        // gives 128 bits, not 255, and ties the committed value to the hashed
        // one. `value < 2^128 < r_J`, so no reduction occurs.
        let value = jubjub.scalar_from_le_bytes(self.layouter, value_bytes)?;
        let blinding = jubjub.msm(self.layouter, &[rc], &[generator])?;
        let committed = jubjub.msm(self.layouter, &[value], &[base])?;
        let sum = EccInstructions::add(jubjub, self.layouter, &blinding, &committed)?;
        Ok([jubjub.x_coordinate(&sum), jubjub.y_coordinate(&sum)])
    }
}

impl Relation for ZswapSpend {
    type Instance = Vec<F>;

    type Witness = SpendWitness;

    type Error = Error;

    fn format_instance(instance: &Self::Instance) -> Result<Vec<F>, Error> {
        Ok(instance.clone())
    }

    fn circuit(
        &self,
        std: &ZkStdLib,
        layouter: &mut impl Layouter<F>,
        _instance: Value<Self::Instance>,
        witness: Value<Self::Witness>,
    ) -> Result<(), Error> {
        let mut g = Gadgets { std, layouter };

        // The primary inputs, in `SpendWitness::preimage_inputs` order.
        let n_inputs = NUM_LEGACY_INPUTS
            + if self.guarded {
                GUARDED_EXTRA_INPUTS
            } else {
                PLAIN_EXTRA_INPUTS
            };
        let inputs = witness
            .as_ref()
            .map(|w| w.preimage_inputs().into_iter().map(to_f).collect::<Vec<_>>());
        let mut cells = std
            .assign_many(g.layouter, &inputs.transpose_vec(n_inputs))?
            .into_iter();
        let mut next = || cells.next().expect("one cell per witness input");
        let is_user = next();
        let sk = [next(), next()];
        let contract_address = [next(), next()];
        // Unused: the Merkle leaf is the commitment computed below, so this
        // witness would only be constrained to equal it.
        let _commitment = [next(), next()];
        let path: Vec<_> = (0..ZSWAP_TREE_HEIGHT).map(|_| (next(), next())).collect();
        let domain_sep = [next(), next()];
        let type_contract = [next(), next()];
        let guarded_inputs = self.guarded.then(|| {
            (
                next(),
                next(),
                core::array::from_fn::<_, 7, _>(|_| next()),
                core::array::from_fn::<_, 6, _>(|_| next()),
            )
        });
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
        let reserved = if self.guarded {
            let values = witness
                .as_ref()
                .map(|w| w.guarded.as_ref().map(|g| g.reserved).unwrap_or_default());
            let mut cells = Vec::with_capacity(4);
            for i in 0..4 {
                cells.push(g.assign(values.map(|r| r[i]))?);
            }
            cells
        } else {
            Vec::new()
        };
        let segment = g.assign(witness.as_ref().map(|w| Fr::from(w.segment as u64)))?;

        let is_user_bit = g.bit(&is_user)?;
        let path = path
            .into_iter()
            .map(|(sibling, goes_left)| Ok((sibling, g.bit(&goes_left)?)))
            .collect::<Result<Vec<_>, Error>>()?;

        // The byte decompositions in this function are the only range checks
        // on `sk`, `nonce`, `token_type`, `value`, and the type's preimage,
        // and on `contract_address` through the selected target and sender.
        // For user spends `contract_address` stays unchecked, but its only
        // uses are in values multiplied by `is_contract = 0`.
        let mut coin_bytes = g.bytes32(&nonce)?;
        let type_bytes = g.bytes32(&token_type)?;
        coin_bytes.extend(type_bytes);
        let value_bytes = g.bytes(&value, 16)?;
        coin_bytes.extend(value_bytes.iter().cloned());
        coin_bytes.extend(g.bytes(&is_user, 1)?);

        // Commitment opening: the coin's commitment, with its target (the
        // public key for users, the contract address otherwise), is the leaf.
        let ds_pk = g.const_bytes(PK_DOMAIN_SEP)?;
        let sk_bytes = g.bytes32(&sk)?;
        let public_key = g.sha256(&[&ds_pk, &sk_bytes])?;
        let target = g.select_pair(&is_user_bit, &public_key, &contract_address)?;
        let commitment = g.coin_hash(CC_DOMAIN_SEP, &coin_bytes, &target)?;

        // Membership: the leaf is the low half of the commitment.
        let merkle_root = g.merkle_root(&commitment[1], &path)?;

        // Nullifier: keyed with the sender (the secret key for users).
        let sender = g.select_pair(&is_user_bit, &sk, &contract_address)?;
        let nullifier = g.coin_hash(CN_DOMAIN_SEP, &coin_bytes, &sender)?;

        let value_commitment = g.value_commitment(&token_type, &value_bytes, &segment, &rc)?;

        // Type re-derivation, on every spend: the empty descriptor's hash.
        let inner = g.const_bytes(DERIVE_TOKEN_DOMAIN_SEP)?;
        let ds_bytes = g.bytes32(&domain_sep)?;
        let contract_bytes = g.bytes32(&type_contract)?;
        let plain_type = g.sha256(&[&inner, &ds_bytes, &contract_bytes])?;

        let is_contract = g.not(&is_user_bit)?;
        let mut extra_pis = Vec::new();
        let mut accumulator = None;
        match guarded_inputs {
            None => {
                for (expected, actual) in plain_type.iter().zip(&token_type) {
                    std.assert_equal(g.layouter, expected, actual)?;
                }
            }
            Some((guarded, vk_hash, recipient, output)) => {
                let guarded = g.bit(&guarded)?;

                // One flag decides the type's hash and the guard verified. A
                // descriptor present is mode 2, version 1, with the guard's
                // key hash, canonically encoded, as its payload.
                let descriptor_head = g.const_bytes(&[Descriptor::MODE_GUARD, Descriptor::VERSION_1])?;
                let payload = std.assigned_to_le_bytes(g.layouter, &vk_hash, None)?;
                let guarded_type =
                    g.sha256(&[&inner, &ds_bytes, &contract_bytes, &descriptor_head, &payload])?;
                let expected = g.select_pair(&guarded, &guarded_type, &plain_type)?;
                for (expected, actual) in expected.iter().zip(&token_type) {
                    std.assert_equal(g.layouter, expected, actual)?;
                }

                // The guard's arguments. Each `Either`'s other branch, and an
                // absent output, is the default.
                let [rec_is_left, ck0, ck1, ex, ey, rc0, rc1] = recipient;
                let rec_is_left = g.bit(&rec_is_left)?;
                let rec_is_right = g.not(&rec_is_left)?;
                let [out_is_some, on0, on1, ot0, ot1, ov] = output;
                let out_is_some = g.bit(&out_is_some)?;
                let zero = g.fixed(Fr::from(0u64))?;
                let one = g.fixed(Fr::from(1u64))?;
                let mt_index = {
                    let terms: Vec<(F, Assigned)> = path
                        .iter()
                        .enumerate()
                        .map(|(i, (_, goes_left))| {
                            (F::from(1u64 << i), goes_left.clone().into())
                        })
                        .collect();
                    // Σ (1 - goes_left_i) 2^i
                    let all_ones = F::from((1u64 << ZSWAP_TREE_HEIGHT) - 1);
                    let negated: Vec<(F, Assigned)> =
                        terms.into_iter().map(|(c, x)| (-c, x)).collect();
                    std.linear_combination(g.layouter, &negated, all_ones)?
                };
                let is_user_native: Assigned = is_user_bit.clone().into();
                let recipient_args = [
                    rec_is_left.clone().into(),
                    g.gate(&rec_is_left, &ck0)?,
                    g.gate(&rec_is_left, &ck1)?,
                    g.select(&rec_is_left, &ex, &zero)?,
                    g.select(&rec_is_left, &ey, &one)?,
                    g.gate(&rec_is_right, &rc0)?,
                    g.gate(&rec_is_right, &rc1)?,
                ];
                let output_args = [
                    out_is_some.clone().into(),
                    g.gate(&out_is_some, &on0)?,
                    g.gate(&out_is_some, &on1)?,
                    g.gate(&out_is_some, &ot0)?,
                    g.gate(&out_is_some, &ot1)?,
                    g.gate(&out_is_some, &ov)?,
                ];
                let mut guard_args: Vec<Assigned> = vec![
                    nonce[0].clone(),
                    nonce[1].clone(),
                    token_type[0].clone(),
                    token_type[1].clone(),
                    value.clone(),
                    mt_index,
                    nullifier[0].clone(),
                    nullifier[1].clone(),
                    is_user_native,
                    g.gate(&is_user_bit, &public_key[0])?,
                    g.gate(&is_user_bit, &public_key[1])?,
                    g.gate(&is_contract, &contract_address[0])?,
                    g.gate(&is_contract, &contract_address[1])?,
                ];
                guard_args.extend(recipient_args);
                guard_args.extend(output_args);
                guard_args.extend(reserved.iter().cloned());
                debug_assert_eq!(guard_args.len(), GUARD_ARGS_LEN);
                let args = guard_args
                    .iter()
                    .map(|arg| g.gate(&guarded, arg))
                    .collect::<Result<Vec<_>, Error>>()?;

                // Verify the guard proof against the witnessed key, bound to
                // the payload, or to `ACCEPT_ALL`'s hash.
                let accept_all_hash = g.std.assign_fixed(
                    g.layouter,
                    accept_all_hash(),
                )?;
                let key_hash = g.select(&guarded, &vk_hash, &accept_all_hash)?;
                let verifier = std.verifier();
                let bls = std.bls12_381();
                let scalar_chip = bls.scalar_field_chip();
                let unbound = verifier.assign_unbound_vk(
                    g.layouter,
                    witness.as_ref().map(|w| {
                        w.guarded
                            .as_ref()
                            .map(|g| g.guard.vk.vk())
                            .unwrap_or(accept_all().vk.vk())
                    }),
                    guard_cs(),
                )?;
                let guard_vk = verifier.bind_vk_to_hash(g.layouter, unbound, &key_hash)?;
                let mut acc = verifier.prepare(
                    g.layouter,
                    &guard_vk,
                    &[],
                    &[&[], &args],
                    witness.as_ref().map(|w| {
                        w.guarded
                            .as_ref()
                            .map(|g| g.guard.proof.clone())
                            .unwrap_or_default()
                    }),
                )?;
                // Collapse before resolving, mirroring the off-circuit
                // computation; the key's commitments are variable bases, so
                // only `-G` is fixed.
                acc.collapse(g.layouter, bls, scalar_chip)?;
                let neg_g = bls.assign_fixed(g.layouter, -C::generator())?;
                acc.resolve_fixed_bases(&BTreeMap::from([(neg_g_label(), neg_g)]));
                acc.collapse(g.layouter, bls, scalar_chip)?;
                // Randomise the accumulator: accumulating it with the mask
                // `r * acc` gives `(r + h) * acc`, for the challenge `h` both
                // fix. Like any accumulation, the result is valid only if both
                // are, and it is a uniformly random multiple of `acc` for a
                // uniform `r`.
                let mask = verifier.assign_collapsed_accumulator(
                    g.layouter,
                    &[],
                    witness.as_ref().map(|w| {
                        w.mask_accumulator()
                            .expect("the guard proof prepares, as checked before proving")
                    }),
                )?;
                let mut acc = verifier.accumulate(g.layouter, &[mask, acc])?;
                acc.collapse(g.layouter, bls, scalar_chip)?;
                accumulator = Some(acc);

                // The named output pays the recipient; its commitment is
                // disclosed.
                let rec_target = g.select_pair(&rec_is_left, &[ck0, ck1], &[rc0, rc1])?;
                let mut out_bytes = g.bytes32(&[on0, on1])?;
                out_bytes.extend(g.bytes32(&[ot0, ot1])?);
                out_bytes.extend(g.bytes(&ov, 16)?);
                let rec_is_left_native: Assigned = rec_is_left.clone().into();
                out_bytes.extend(g.bytes(&rec_is_left_native, 1)?);
                let out_com = g.coin_hash(CC_DOMAIN_SEP, &out_bytes, &rec_target)?;
                extra_pis = vec![
                    out_is_some.clone().into(),
                    g.gate(&out_is_some, &out_com[0])?,
                    g.gate(&out_is_some, &out_com[1])?,
                ];
            }
        }

        // Public transcript: its shape comes from `SpendTranscript`; here we
        // only say where each variable comes from.
        let is_contract: Assigned = is_contract.into();
        let mut constants: HashMap<Vec<u8>, Assigned> = HashMap::new();
        let mut pis = vec![binding_input];
        for pi in spend_layout(self.guarded) {
            pis.push(match *pi {
                PI::Const(c) => match constants.entry(c.as_le_bytes().to_vec()) {
                    Entry::Occupied(cell) => cell.get().clone(),
                    Entry::Vacant(slot) => slot.insert(g.fixed(c)?).clone(),
                },
                PI::IfContract(c) if c == Fr::from(1u64) => is_contract.clone(),
                PI::IfContract(c) => std.mul_by_constant(g.layouter, &is_contract, to_f(c))?,
                PI::MerkleRoot => merkle_root.clone(),
                PI::RootCheck => root_check_passed.clone(),
                PI::Nullifier(half) => nullifier[half].clone(),
                PI::Address(half) => {
                    std.mul(g.layouter, &is_contract, &contract_address[half], None)?
                }
                PI::Reserved(i) => reserved[i].clone(),
                PI::ValueCommitment(coord) => value_commitment[coord].clone(),
                PI::Segment => segment.clone(),
            });
        }
        pis.extend(extra_pis);
        pis.iter()
            .try_for_each(|pi| std.constrain_as_public_input(g.layouter, pi))?;
        if let Some(acc) = accumulator {
            std.verifier().constrain_as_public_input(g.layouter, &acc)?;
        }
        Ok(())
    }

    fn used_chips(&self) -> ZkStdLibArch {
        ZkStdLibArch {
            jubjub: true,
            poseidon: true,
            sha2_256: true,
            bls12_381: self.guarded,
            // The in-circuit verifier's emulated arithmetic is dominated by
            // range checks: four parallel lookups take the guarded circuit
            // from 2^20 rows to 2^18, quartering its proving memory.
            nr_pow2range_cols: if self.guarded { 4 } else { 1 },
            ..ZkStdLibArch::default()
        }
    }

    fn write_relation<W: io::Write>(&self, writer: &mut W) -> io::Result<()> {
        writer.write_all(&[self.guarded as u8])
    }

    fn read_relation<R: io::Read>(reader: &mut R) -> io::Result<Self> {
        let mut byte = [0u8];
        reader.read_exact(&mut byte)?;
        Ok(ZswapSpend {
            guarded: byte[0] != 0,
        })
    }
}

const PLAIN_SPEND_VK_RAW: &[u8] = include_bytes!("../static/plain-spend.verifier");
const GUARDED_SPEND_VK_RAW: &[u8] = include_bytes!("../static/guarded-spend.verifier");

/// A spend circuit's verifying key.
///
/// Shipped in `zswap/static/{plain,guarded}-spend.verifier`; regenerate them
/// with `cargo test -p midnight-zswap --release -- --ignored generate_spend_keys`
/// whenever a circuit changes.
pub fn spend_vk(guarded: bool) -> &'static MidnightVK {
    static PLAIN: OnceLock<MidnightVK> = OnceLock::new();
    static GUARDED: OnceLock<MidnightVK> = OnceLock::new();
    let (cell, raw) = if guarded {
        (&GUARDED, GUARDED_SPEND_VK_RAW)
    } else {
        (&PLAIN, PLAIN_SPEND_VK_RAW)
    };
    cell.get_or_init(|| {
        MidnightVK::read(&mut &raw[..], SerdeFormat::Processed)
            .expect("the shipped spend verifying key must parse; regenerate it")
    })
}

/// A spend circuit's proving key, derived from [`spend_vk`] once per process.
fn spend_pk(guarded: bool) -> &'static MidnightPK<ZswapSpend> {
    static PLAIN: OnceLock<MidnightPK<ZswapSpend>> = OnceLock::new();
    static GUARDED: OnceLock<MidnightPK<ZswapSpend>> = OnceLock::new();
    let cell = if guarded { &GUARDED } else { &PLAIN };
    cell.get_or_init(|| {
        midnight_zk_stdlib::setup_pk(&ZswapSpend { guarded }, spend_vk(guarded))
    })
}

/// The verifier parameters, read from the same SRS prefix
/// `transient_crypto::proofs::PARAMS_VERIFIER` embeds.
fn params_verifier() -> &'static ParamsVerifierKZG<Bls12> {
    static PARAMS: OnceLock<ParamsVerifierKZG<Bls12>> = OnceLock::new();
    PARAMS.get_or_init(|| {
        const RAW: &[u8] = include_bytes!("../../transient-crypto/static/bls_midnight_2p14");
        ParamsKZG::<Bls12>::read_custom(&mut &RAW[..], SerdeFormat::RawBytesUnchecked)
            .expect("the embedded verifier parameters must parse")
            .verifier_params()
    })
}

fn to_outer(point: C) -> outer::Affine {
    point.to_affine()
}

fn from_outer(point: outer::Affine) -> Option<C> {
    Some(point.into())
}

/// The number of public inputs a guarded spend's accumulator occupies.
pub fn accumulator_public_inputs_len() -> usize {
    AssignedAccumulator::<S>::as_public_input(&collapsed(C::generator(), C::generator())).len()
}

/// The collapsed accumulator with sides `lhs` and `rhs`.
fn collapsed(lhs: C, rhs: C) -> Accumulator<S> {
    let side = |p: C| Msm::new(&[Point::Variable(p)], &[F::ONE], &[PolynomialLabel::NoLabel]);
    Accumulator::new(side(lhs), side(rhs))
}

/// Verifies a spend proof against `statement`, its public inputs excluding
/// any accumulator: under the plain circuit's key if the proof carries no
/// accumulator, the guarded circuit's if it carries one, which must not be
/// the identity and must pass its pairing check.
pub fn verify_spend(proof: &Proof, statement: &[Fr]) -> anyhow::Result<()> {
    let mut instance: Vec<F> = statement.iter().copied().map(to_f).collect();
    let guarded = match &proof.accumulators[..] {
        [] => None,
        [acc] => {
            let (lhs, rhs) = acc.points();
            let (lhs, rhs) = from_outer(lhs)
                .zip(from_outer(rhs))
                .ok_or_else(|| anyhow::anyhow!("malformed spend accumulator"))?;
            anyhow::ensure!(
                !bool::from(lhs.is_identity()),
                "a guarded spend's accumulator must not be the identity"
            );
            let acc = collapsed(lhs, rhs);
            instance.extend(AssignedAccumulator::<S>::as_public_input(&acc));
            Some(acc)
        }
        _ => anyhow::bail!("a spend proof carries at most one accumulator"),
    };
    let relation = ZswapSpend {
        guarded: guarded.is_some(),
    };
    midnight_zk_stdlib::verify::<ZswapSpend, blake2b_simd::State>(
        params_verifier(),
        spend_vk(relation.guarded),
        &instance,
        None,
        &proof.bytes,
    )
    .map_err(|_| anyhow::anyhow!("invalid spend proof"))?;
    if let Some(acc) = guarded {
        anyhow::ensure!(
            acc.check(params_verifier(), &BTreeMap::new()),
            "the guard proof's accumulator failed its pairing check"
        );
    }
    Ok(())
}

/// The "IR" of a spend circuit: the circuits are native Rust, so this only
/// names one, for proving providers to dispatch on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ZswapSpendIr {
    pub guarded: bool,
}

impl Tagged for ZswapSpendIr {
    fn tag() -> Cow<'static, str> {
        Cow::Borrowed("zswap-spend-ir[v3]")
    }
    fn tag_unique_factor() -> String {
        "u8".into()
    }
}

impl Serializable for ZswapSpendIr {
    fn serialize(&self, writer: &mut impl Write) -> io::Result<()> {
        Serializable::serialize(&(self.guarded as u8), writer)
    }
    fn serialized_size(&self) -> usize {
        1
    }
}

impl Deserializable for ZswapSpendIr {
    fn deserialize(reader: &mut impl Read, recursion_depth: u32) -> io::Result<Self> {
        Ok(ZswapSpendIr {
            guarded: <u8 as Deserializable>::deserialize(reader, recursion_depth)? != 0,
        })
    }
}

/// Reads the guard a spend preimage carries (see
/// [`GuardProof::inner_proofs`](crate::GuardProof::inner_proofs)); `None` if
/// it carries none.
fn preimage_guard(preimage: &ProofPreimage) -> anyhow::Result<Option<GuardWitness>> {
    match &preimage.inner_proofs[..] {
        [] => Ok(None),
        [InnerProofWitness::Direct(proof), InnerProofWitness::Direct(vk)] => {
            let vk = read_guard_vk(vk)
                .ok_or_else(|| anyhow::anyhow!("the guard's verifying key has not the guard shape"))?;
            Ok(Some(GuardWitness {
                vk,
                proof: proof.clone(),
            }))
        }
        _ => anyhow::bail!("a spend preimage carries a guard proof and its key, or nothing"),
    }
}

impl ZswapSpendIr {
    /// The spend circuit a key location names.
    pub fn for_key_location(location: &str) -> Option<Self> {
        match location {
            PLAIN_SPEND_KEY_LOCATION => Some(ZswapSpendIr { guarded: false }),
            GUARDED_SPEND_KEY_LOCATION => Some(ZswapSpendIr { guarded: true }),
            _ => None,
        }
    }

    /// Checks a spend proof preimage is well-formed.
    pub fn check(&self, preimage: &ProofPreimage) -> Result<Vec<Option<usize>>, ProvingError> {
        anyhow::ensure!(
            Self::for_key_location(&preimage.key_location.0) == Some(*self),
            "not a preimage of this spend circuit"
        );
        let guard = preimage_guard(preimage)?;
        anyhow::ensure!(
            self.guarded || guard.is_none(),
            "the plain spend circuit takes no guard proof"
        );
        let witness = SpendWitness::from_preimage(
            preimage,
            self.guarded
                .then(|| (guard.unwrap_or_else(|| accept_all().clone()), Fr::from(1u64))),
        )?;
        anyhow::ensure!(
            witness.derived_token_type() == witness.token_type,
            "the spend's type preimage does not derive the coin's type"
        );
        Ok(vec![])
    }

    /// Proves a spend, returning the proof and its statement (its public
    /// inputs excluding the accumulator). The proof is checked with
    /// [`verify_spend`] before it is returned.
    pub async fn prove(
        &self,
        mut rng: impl Rng + CryptoRng,
        params: &impl ParamsProverProvider,
        preimage: &ProofPreimage,
    ) -> Result<(Proof, Vec<Fr>), ProvingError> {
        self.check(preimage)?;
        let guard = self.guarded.then(|| {
            let guard = preimage_guard(preimage)
                .ok()
                .flatten()
                .unwrap_or_else(|| accept_all().clone());
            let randomizer = loop {
                let r: Fr = rng.r#gen();
                if r != Fr::from(0u64) {
                    break r;
                }
            };
            (guard, randomizer)
        });
        let witness = SpendWitness::from_preimage(preimage, guard)?;
        let instance = witness.instance()?;
        let statement = witness.public_inputs();
        let accumulators = match self.guarded {
            false => vec![],
            true => {
                let acc = witness.accumulator()?;
                let side = |msm: Msm<S>| msm.eval(&BTreeMap::new());
                vec![DeferredAccumulator::from_points(
                    to_outer(side(acc.lhs())),
                    to_outer(side(acc.rhs())),
                )]
            }
        };
        let relation = ZswapSpend {
            guarded: self.guarded,
        };
        let pk = spend_pk(self.guarded);
        let params_k = params.get_params(pk.k()).await?;
        let bytes = midnight_zk_stdlib::prove::<_, blake2b_simd::State>(
            params_k.as_ref(),
            pk,
            &relation,
            &instance,
            witness,
            rng,
        )?;
        let proof = Proof {
            bytes,
            accumulators,
        };
        verify_spend(&proof, &statement)
            .map_err(|e| anyhow::anyhow!("spend proof self-verification failed: {e}"))?;
        Ok((proof, statement))
    }

    /// The log2 size of the spend circuit.
    pub fn k(&self) -> u8 {
        spend_vk(self.guarded).k()
    }
}

/// Generates the keys a shipped verifying key belongs to, for tests and the
/// key generators: reads `MIDNIGHT_PP`'s parameters for `2^k`.
#[cfg(test)]
fn test_params(k: u32) -> ParamsKZG<Bls12> {
    let file = std::fs::File::open(format!("{}/bls_midnight_2p{k}", env!("MIDNIGHT_PP")))
        .expect("params file must exist");
    ParamsKZG::read_custom(&mut io::BufReader::new(file), SerdeFormat::RawBytesUnchecked)
        .expect("params must parse")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_shape() {
        for guarded in [false, true] {
            let layout = spend_layout(guarded);
            let count = |f: fn(&PI) -> bool| layout.iter().filter(|pi| f(pi)).count();
            let reserved = if guarded { 4 } else { 0 };
            if !guarded {
                assert_eq!(layout.len() + 1, NUM_LEGACY_PUBLIC_INPUTS);
            }
            assert_eq!(count(|pi| matches!(pi, PI::MerkleRoot)), 1);
            assert_eq!(count(|pi| matches!(pi, PI::RootCheck)), 1);
            assert_eq!(count(|pi| matches!(pi, PI::Nullifier(_))), 2);
            assert_eq!(count(|pi| matches!(pi, PI::Reserved(_))), reserved);
            assert_eq!(count(|pi| matches!(pi, PI::ValueCommitment(_))), 2);
            assert_eq!(count(|pi| matches!(pi, PI::Segment)), 1);
            // The contract-address write is the guarded block, `CADDR_OP_LEN` long.
            let guarded_block = count(|pi| matches!(pi, PI::Address(_) | PI::IfContract(_)));
            assert_eq!(guarded_block, CADDR_OP_LEN as usize);
            assert_eq!(count(|pi| matches!(pi, PI::Address(_))), 2);
        }
    }

    #[test]
    fn field_conversions_round_trip() {
        for x in [Fr::from(0u64), Fr::from(1u64), -Fr::from(1u64), Fr::from(u128::MAX)] {
            assert_eq!(from_f(to_f(x)), x);
        }
    }

    /// Writes `zswap/static/accept-all.{verifier,proof}`. Run before
    /// [`generate_spend_keys`]: the guarded circuit fixes `ACCEPT_ALL`'s key
    /// hash.
    #[test]
    #[ignore = "regenerates the shipped accept-all key and proof"]
    fn generate_accept_all() {
        let params = test_params(GUARD_K as u32);
        let vk = midnight_zk_stdlib::setup_vk(&params, &AcceptAll);
        let pk = midnight_zk_stdlib::setup_pk(&AcceptAll, &vk);
        let proof = midnight_zk_stdlib::prove::<_, PoseidonState<F>>(
            &params,
            &pk,
            &AcceptAll,
            &vec![F::ZERO; GUARD_ARGS_LEN],
            (),
            rand::rngs::OsRng,
        )
        .expect("accept-all proving");
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/static");
        std::fs::write(format!("{dir}/accept-all.verifier"), write_guard_vk(&vk)).unwrap();
        std::fs::write(format!("{dir}/accept-all.proof"), proof).unwrap();
    }

    /// Writes `zswap/static/{plain,guarded}-spend.verifier`. Slow: keys both
    /// spend circuits.
    #[test]
    #[ignore = "regenerates the shipped spend verifying keys"]
    fn generate_spend_keys() {
        for guarded in [false, true] {
            let relation = ZswapSpend { guarded };
            let k = midnight_zk_stdlib::optimal_k(&relation);
            let model = midnight_zk_stdlib::cost_model(&relation, Some(k), false);
            println!(
                "{} spend circuit: k = {k}, rows = {}",
                if guarded { "guarded" } else { "plain" },
                model.rows
            );
            let vk = midnight_zk_stdlib::setup_vk(&test_params(k), &relation);
            let mut out = Vec::new();
            vk.write(&mut out, SerdeFormat::Processed).unwrap();
            let name = if guarded { "guarded" } else { "plain" };
            std::fs::write(
                format!("{}/static/{name}-spend.verifier", env!("CARGO_MANIFEST_DIR")),
                out,
            )
            .unwrap();
        }
    }
}
