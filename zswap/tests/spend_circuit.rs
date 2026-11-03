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

//! Tests the spend circuits: the plain circuit proves today's statement (its
//! public inputs match those the IR virtual machine computes for
//! `zswap/static/spend.zkir`) and re-derives the coin's type; the guarded
//! circuit also verifies the coin's guard, against a privately witnessed key.

#![cfg(feature = "proof-verifying")]

use base_crypto::hash::HashOutput;
use coin_structure::coin::{Info as CoinInfo, QualifiedInfo as QualifiedCoinInfo};
use coin_structure::contract::ContractAddress;
use coin_structure::custom_spend::{ShieldedTokenPreimage, SpendRule};
use coin_structure::transfer::{Recipient, SenderEvidence};
use midnight_zswap::spend_circuit::zk::{midnight_proofs, midnight_zk_stdlib};
use midnight_zswap::spend_circuit::{
    AcceptAll, F, GUARD_K, ZswapSpendIr, guard_vk_hash, verify_spend, write_guard_vk,
};
use midnight_zswap::{
    GUARD_ARGS_LEN, GuardProof, GuardedSpend, Input, SpendRecipient, guard_args,
};
use midnight_proofs::poly::kzg::params::ParamsKZG;
use midnight_proofs::utils::SerdeFormat;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha20Rng;
use std::borrow::Cow;
use std::fs::File;
use std::io::BufReader;
use storage::db::InMemoryDB;
use transient_crypto::curve::Fr;
use transient_crypto::merkle_tree::MerkleTree;
use transient_crypto::proofs::{
    KeyLocation, ParamsProver, ParamsProverProvider, Proof, ProofPreimage,
};
use zkir_v2::IrSource;

const PARAMS_DIR: &str = env!("MIDNIGHT_PP");
const ZSWAP_STATIC: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/static");

struct Params;

impl ParamsProverProvider for Params {
    async fn get_params(&self, k: u8) -> std::io::Result<ParamsProver> {
        ParamsProver::read(BufReader::new(File::open(format!(
            "{PARAMS_DIR}/bls_midnight_2p{k}"
        ))?))
    }
}

fn guard_params() -> ParamsKZG<midnight_curves::Bls12> {
    ParamsKZG::read_custom(
        &mut BufReader::new(
            File::open(format!("{PARAMS_DIR}/bls_midnight_2p{GUARD_K}"))
                .expect("params file must exist"),
        ),
        SerdeFormat::RawBytesUnchecked,
    )
    .expect("params must parse")
}

use midnight_zswap::spend_circuit::zk::midnight_curves;

/// A guard that accepts every spend: `ACCEPT_ALL` itself, keyed afresh, so a
/// rule can name it.
struct Guard<R: Relation<Instance = Vec<F>, Witness = (), Error = Error>> {
    relation: R,
    vk: midnight_zk_stdlib::MidnightVK,
    pk: midnight_zk_stdlib::MidnightPK<R>,
}

impl<R: Relation<Instance = Vec<F>, Witness = (), Error = Error>> Guard<R> {
    fn new(relation: R) -> Self {
        let vk = midnight_zk_stdlib::setup_vk(&guard_params(), &relation);
        let pk = midnight_zk_stdlib::setup_pk(&relation, &vk);
        Guard { relation, vk, pk }
    }

    fn rule(&self) -> SpendRule {
        SpendRule::guard(guard_vk_hash(&self.vk))
    }

    /// A guard proof on `args`.
    fn prove(&self, args: &[Fr]) -> GuardProof {
        let args: Vec<F> = args
            .iter()
            .map(|f| midnight_zswap::spend_circuit::to_f(*f))
            .collect();
        let proof = midnight_zk_stdlib::prove::<
            _,
            midnight_circuits::hash::poseidon::PoseidonState<F>,
        >(
            &guard_params(),
            &self.pk,
            &self.relation,
            &args,
            (),
            rand::rngs::OsRng,
        )
        .expect("guard proving");
        GuardProof {
            verifier_key: write_guard_vk(&self.vk),
            proof,
        }
    }
}

use midnight_zswap::spend_circuit::zk::midnight_circuits;
use midnight_zswap::spend_circuit::zk::midnight_proofs::circuit::{Layouter, Value};
use midnight_zswap::spend_circuit::zk::midnight_proofs::plonk::Error;
use midnight_zswap::spend_circuit::zk::midnight_zk_stdlib::{Relation, ZkStdLib, ZkStdLibArch};

/// Another guard in the guard shape, which also accepts everything, but whose
/// key differs from `ACCEPT_ALL`'s.
#[derive(Clone, Debug)]
struct OtherGuard;

impl Relation for OtherGuard {
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
        use midnight_circuits::instructions::{AssignmentInstructions, PublicInputInstructions};
        let mut cells = Vec::new();
        for field in instance.transpose_vec(GUARD_ARGS_LEN) {
            let cell: midnight_circuits::types::AssignedNative<F> = std.assign(layouter, field)?;
            std.constrain_as_public_input(layouter, &cell)?;
            cells.push(cell);
        }
        std.poseidon(layouter, &cells[..2])?;
        Ok(())
    }

    fn used_chips(&self) -> ZkStdLibArch {
        midnight_zswap::spend_circuit::guard_arch()
    }

    fn write_relation<W: std::io::Write>(&self, _writer: &mut W) -> std::io::Result<()> {
        Ok(())
    }

    fn read_relation<R: std::io::Read>(_reader: &mut R) -> std::io::Result<Self> {
        Ok(OtherGuard)
    }
}

/// A contract-held coin of the type `preimage` derives, at index 0 of a
/// one-leaf tree.
fn contract_coin(
    rng: &mut ChaCha20Rng,
    preimage: &ShieldedTokenPreimage,
    contract: ContractAddress,
) -> (QualifiedCoinInfo, MerkleTree<(), InMemoryDB>) {
    let coin = CoinInfo {
        nonce: rng.r#gen(),
        type_: preimage.token_type(),
        value: rng.r#gen::<u64>() as u128,
    };
    let tree = MerkleTree::<(), InMemoryDB>::blank(32)
        .try_update_hash(0, coin.commitment(&Recipient::Contract(contract)).0, ())
        .expect("fresh tree")
        .rehash();
    (coin.qualify(0), tree)
}

async fn prove(guarded: bool, preimage: &ProofPreimage) -> anyhow::Result<(Proof, Vec<Fr>)> {
    ZswapSpendIr { guarded }
        .prove(ChaCha20Rng::from_seed([7; 32]), &Params, preimage)
        .await
}

/// The plain circuit's public inputs match the IR virtual machine running
/// today's spend circuit, on today's inputs.
#[tokio::test]
async fn plain_public_inputs_match_ir() {
    let mut rng = ChaCha20Rng::from_seed([0x5e; 32]);
    let ir = IrSource::load_from_tagged(
        &mut File::open(format!("{ZSWAP_STATIC}/spend.bzkir")).expect("spend.bzkir must exist"),
    )
    .expect("spend.bzkir must parse");
    let contract = ContractAddress(HashOutput(rng.r#gen()));
    let preimage = ShieldedTokenPreimage::genesis(HashOutput(rng.r#gen()), SpendRule::Empty);
    let (qcoin, tree) = contract_coin(&mut rng, &preimage, contract);
    let input: Input<ProofPreimage, InMemoryDB> =
        Input::new_contract_owned(&mut rng, &qcoin, Some(3), contract, &tree, &preimage, None)
            .expect("spend construction");
    let (_, statement) = prove(false, &input.proof).await.expect("plain proving");

    // Today's preimage: the extra inputs removed.
    let mut legacy = (*input.proof).clone();
    legacy.inputs.drain(71..75);
    legacy.key_location = KeyLocation(Cow::Borrowed("midnight/zswap/spend"));
    let preprocessed = ir.preprocess(&legacy).expect("IR run must succeed");
    assert_eq!(
        preprocessed.pis,
        statement.iter().map(|f| f.0).collect::<Vec<_>>(),
        "the plain circuit's public inputs must be today's"
    );
}

#[tokio::test]
async fn spends_verify_exactly_with_their_type_and_rule() {
    let mut rng = ChaCha20Rng::from_seed([0x5f; 32]);
    let contract = ContractAddress(HashOutput(rng.r#gen()));
    let issuer = ContractAddress(HashOutput(rng.r#gen()));
    let guard = Guard::new(AcceptAll);
    let plain = ShieldedTokenPreimage {
        domain_sep: HashOutput(rng.r#gen()),
        contract_address: issuer,
        rule: SpendRule::Empty,
    };
    let gated = ShieldedTokenPreimage {
        rule: guard.rule(),
        ..plain
    };
    let recipient = SpendRecipient::Contract(contract);

    // An unguarded coin spends through either circuit.
    let (qcoin, tree) = contract_coin(&mut rng, &plain, contract);
    for guarded in [false, true] {
        let input: Input<ProofPreimage, InMemoryDB> = Input::new_contract_owned(
            &mut rng,
            &qcoin,
            None,
            contract,
            &tree,
            &plain,
            guarded.then(|| GuardedSpend {
                recipient,
                output: None,
                guard: None,
            }),
        )
        .expect("spend construction");
        let (proof, statement) = prove(guarded, &input.proof).await.expect("proving");
        assert_eq!(proof.accumulators.len(), guarded as usize);
        verify_spend(&proof, &statement).expect("an unguarded spend verifies");

        // The statement binds the spend.
        let mut wrong = statement.clone();
        wrong[0] = wrong[0] + Fr::from(1u64);
        assert!(verify_spend(&proof, &wrong).is_err());
    }

    // The type's preimage must derive the coin's type.
    let wrong_type = ShieldedTokenPreimage {
        domain_sep: HashOutput(rng.r#gen()),
        ..plain
    };
    assert!(
        Input::<ProofPreimage, InMemoryDB>::new_contract_owned(
            &mut rng,
            &qcoin,
            None,
            contract,
            &tree,
            &wrong_type,
            None
        )
        .is_err()
    );

    // A guarded coin spends only through the guarded circuit, with a proof of
    // its guard on its spend's arguments, and names its output.
    let (gcoin, gtree) = contract_coin(&mut rng, &gated, contract);
    let sender = SenderEvidence::Contract(contract);
    let output = CoinInfo {
        nonce: rng.r#gen(),
        type_: gated.token_type(),
        value: gcoin.value,
    };
    let args = guard_args(&gcoin, &sender, &recipient, Some(&output));
    assert_eq!(args.len(), GUARD_ARGS_LEN);
    let spend = |guard: GuardProof, rng: &mut ChaCha20Rng| {
        Input::<ProofPreimage, InMemoryDB>::new_contract_owned(
            rng,
            &gcoin,
            None,
            contract,
            &gtree,
            &gated,
            Some(GuardedSpend {
                recipient,
                output: Some(output),
                guard: Some(guard),
            }),
        )
    };
    assert!(
        Input::<ProofPreimage, InMemoryDB>::new_contract_owned(
            &mut rng, &gcoin, None, contract, &gtree, &gated, None
        )
        .is_err(),
        "a guarded coin needs the guarded circuit"
    );
    let input = spend(guard.prove(&args), &mut rng).expect("spend construction");
    assert_eq!(
        input.named_output.as_deref().copied(),
        Some(output.commitment(&Recipient::Contract(contract)))
    );
    let (proof, statement) = prove(true, &input.proof).await.expect("guarded proving");
    verify_spend(&proof, &statement).expect("a guarded spend verifies");

    // Tampering with the disclosed named output fails.
    let mut wrong = statement.clone();
    let last = wrong.len() - 1;
    wrong[last] = wrong[last] + Fr::from(1u64);
    assert!(verify_spend(&proof, &wrong).is_err());

    // A proof with the identity accumulator, or with two, is refused.
    let mut identity = proof.clone();
    identity.accumulators = vec![transient_crypto::proofs::DeferredAccumulator::from_points(
        Default::default(),
        Default::default(),
    )];
    assert!(verify_spend(&identity, &statement).is_err());
    let mut two = proof.clone();
    two.accumulators.push(two.accumulators[0]);
    assert!(verify_spend(&two, &statement).is_err());

    // A guard proof on other arguments, or of another guard, does not prove
    // the spend.
    let mut other_args = args.clone();
    other_args[0] = other_args[0] + Fr::from(1u64);
    let input = spend(guard.prove(&other_args), &mut rng).expect("spend construction");
    assert!(prove(true, &input.proof).await.is_err());
    let input = spend(Guard::new(OtherGuard).prove(&args), &mut rng).expect("spend construction");
    assert!(prove(true, &input.proof).await.is_err());
}
