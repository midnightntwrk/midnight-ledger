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

//! Tests for custom spend logic, mode 2 (guards): a shielded token type whose
//! descriptor names a guard must only be spendable by also proving that
//! guard, on the spend's arguments.
//!
//! The guard here checks knowledge of an additional secret key: it proves
//! `poseidon(sk) = pk` for a `pk` fixed in the circuit, so its verifying key,
//! and hence the token type, is bound to that key.

// As this test proves real zswap spends.
#![cfg(feature = "proving")]

use base_crypto::hash::HashOutput;
use base_crypto::rng::SplittableRng;
use coin_structure::coin::{Info as CoinInfo, QualifiedInfo as QualifiedCoinInfo};
use coin_structure::contract::ContractAddress;
use coin_structure::custom_spend::{ShieldedTokenPreimage, SpendRule};
use coin_structure::transfer::SenderEvidence;
use lazy_static::lazy_static;
use midnight_ledger::semantics::TransactionResult;
use midnight_ledger::structure::{ProofPreimageMarker, Signature, Transaction};
use midnight_ledger::test_utilities::{Resolver, TestState, test_resolver, tx_prove};
use midnight_ledger::verify::WellFormedStrictness;
use midnight_ledger_v10 as midnight_ledger;
use rand::{Rng, SeedableRng, rngs::StdRng};
use std::borrow::Cow;
use std::fs::File;
use std::io::BufReader;
use storage::db::InMemoryDB;
use transient_crypto::commitment::PedersenRandomness;
use transient_crypto::curve::Fr;
use zk::midnight_circuits::hash::poseidon::{PoseidonChip, PoseidonState};
use zk::midnight_circuits::instructions::{
    AssertionInstructions, AssignmentInstructions, PublicInputInstructions, hash::HashCPU,
};
use zk::midnight_circuits::types::AssignedNative;
use zk::midnight_proofs::{
    circuit::{Layouter, Value},
    plonk::Error,
    poly::kzg::params::ParamsKZG,
    utils::SerdeFormat,
};
use zk::midnight_zk_stdlib::{MidnightPK, MidnightVK, Relation, ZkStdLib, ZkStdLibArch};
use zswap::spend_circuit::{
    F, GUARD_K, from_f, guard_arch, guard_vk_hash, to_f, write_guard_vk, zk,
};
use zswap::{
    GUARD_ARGS_LEN, GuardProof, GuardedSpend, Offer, Output, SpendRecipient, guard_args,
};

lazy_static! {
    static ref RESOLVER: Resolver = test_resolver("");
}

/// A guard in the guard shape: it exposes the spend's [`GUARD_ARGS_LEN`]
/// arguments as its public inputs, and proves knowledge of a secret key `sk`
/// with `poseidon(sk) = pk`, for the `pk` fixed in the circuit.
#[derive(Clone, Debug)]
struct SecretKeyGuard {
    pk: F,
}

impl Relation for SecretKeyGuard {
    /// The spend's guard arguments.
    type Instance = Vec<F>;
    /// `sk`.
    type Witness = F;
    type Error = Error;

    fn format_instance(instance: &Self::Instance) -> Result<Vec<F>, Error> {
        Ok(instance.clone())
    }

    fn circuit(
        &self,
        std: &ZkStdLib,
        layouter: &mut impl Layouter<F>,
        instance: Value<Self::Instance>,
        witness: Value<Self::Witness>,
    ) -> Result<(), Error> {
        for field in instance.transpose_vec(GUARD_ARGS_LEN) {
            let cell: AssignedNative<F> = std.assign(layouter, field)?;
            std.constrain_as_public_input(layouter, &cell)?;
        }
        let sk = std.assign(layouter, witness)?;
        let pk = std.poseidon(layouter, &[sk])?;
        std.assert_equal_to_fixed(layouter, &pk, self.pk)
    }

    fn used_chips(&self) -> ZkStdLibArch {
        guard_arch()
    }

    fn write_relation<W: std::io::Write>(&self, writer: &mut W) -> std::io::Result<()> {
        writer.write_all(&from_f(self.pk).as_le_bytes())
    }

    fn read_relation<R: std::io::Read>(reader: &mut R) -> std::io::Result<Self> {
        let mut bytes = [0u8; 32];
        reader.read_exact(&mut bytes)?;
        Fr::from_le_bytes(&bytes)
            .map(|pk| SecretKeyGuard { pk: to_f(pk) })
            .ok_or_else(|| std::io::Error::other("invalid guard public key"))
    }
}

/// The guard's prover parameters, of the size every guard has.
fn guard_params() -> ParamsKZG<zk::midnight_curves::Bls12> {
    ParamsKZG::read_custom(
        &mut BufReader::new(
            File::open(format!("{}/bls_midnight_2p{GUARD_K}", env!("MIDNIGHT_PP")))
                .expect("params file must exist"),
        ),
        SerdeFormat::RawBytesUnchecked,
    )
    .expect("params must parse")
}

struct Guard {
    params: ParamsKZG<zk::midnight_curves::Bls12>,
    relation: SecretKeyGuard,
    pk: MidnightPK<SecretKeyGuard>,
    vk: MidnightVK,
}

impl Guard {
    /// Keys the guard for the holder of `sk`.
    fn setup(sk: Fr) -> Self {
        let params = guard_params();
        let relation = SecretKeyGuard {
            pk: <PoseidonChip<F> as HashCPU<F, F>>::hash(&[to_f(sk)]),
        };
        let vk = zk::midnight_zk_stdlib::setup_vk(&params, &relation);
        let pk = zk::midnight_zk_stdlib::setup_pk(&relation, &vk);
        Guard {
            params,
            relation,
            pk,
            vk,
        }
    }

    /// The mode 2 spend rule naming this guard: its verifying key's hash.
    fn rule(&self) -> SpendRule {
        SpendRule::guard(guard_vk_hash(&self.vk))
    }

    /// A guard proof with `sk` on `args`, if the guard accepts `sk`.
    fn prove(&self, rng: &mut StdRng, args: &[Fr], sk: Fr) -> Option<GuardProof> {
        let args: Vec<F> = args.iter().copied().map(to_f).collect();
        let proof = zk::midnight_zk_stdlib::prove::<_, PoseidonState<F>>(
            &self.params,
            &self.pk,
            &self.relation,
            &args,
            to_f(sk),
            rng,
        )
        .ok()?;
        zk::midnight_zk_stdlib::verify::<SecretKeyGuard, PoseidonState<F>>(
            &self.params.verifier_params(),
            &self.vk,
            &args,
            None,
            &proof,
        )
        .ok()?;
        Some(GuardProof {
            verifier_key: write_guard_vk(&self.vk),
            proof,
        })
    }
}

/// How a test spends its coin.
enum Spend<'a> {
    /// Through the plain circuit.
    Plain,
    /// Through the guarded circuit, with the guard proof `guard` returns for
    /// the spend's arguments, if any.
    Guarded(&'a mut dyn FnMut(&[Fr]) -> Option<GuardProof>),
}

/// Spends the state's coin of the type `preimage` derives back to itself,
/// and applies the transaction. Returns whether the spend was accepted:
/// refusing to construct or prove it counts as rejection.
///
/// The spender knows the type's preimage, as it must to spend it: every
/// spend circuit re-derives the type from it. It learns it privately, here
/// by construction.
async fn spend_to_self(
    state: &mut TestState<InMemoryDB>,
    rng: &mut StdRng,
    preimage: &ShieldedTokenPreimage,
    spend: Spend<'_>,
) -> bool {
    let token = preimage.token_type();
    let coin: QualifiedCoinInfo = *state
        .zswap
        .coins
        .iter()
        .find(|kv| kv.1.type_ == token)
        .expect("state must hold a coin of the token type")
        .1;
    let new_coin = CoinInfo {
        nonce: rng.r#gen(),
        value: coin.value,
        type_: token,
    };
    let guarded = match spend {
        Spend::Plain => None,
        Spend::Guarded(guard) => {
            // A self-transfer declares the spender's own address, and names
            // the output paying it.
            let recipient = SpendRecipient::User {
                coin_key: state.zswap_keys.coin_public_key(),
                encryption_key: state.zswap_keys.enc_public_key(),
            };
            let sender = SenderEvidence::User(Cow::Borrowed(&state.zswap_keys.coin_secret_key));
            let args = guard_args(&coin, &sender, &recipient, Some(&new_coin));
            Some(GuardedSpend {
                recipient,
                output: Some(new_coin),
                guard: guard(&args),
            })
        }
    };
    let Ok((_, input)) =
        state
            .zswap
            .spend(rng, &state.zswap_keys, &coin, Some(0), preimage, guarded)
    else {
        return false;
    };
    let output = Output::new(
        rng,
        &new_coin,
        Some(0),
        &state.zswap_keys.coin_public_key(),
        Some(state.zswap_keys.enc_public_key()),
    )
    .expect("output creation must succeed");
    let offer = Offer {
        inputs: vec![input].into(),
        outputs: vec![output].into(),
        transient: vec![].into(),
        deltas: vec![].into(),
    };
    let tx: Transaction<Signature, ProofPreimageMarker, PedersenRandomness, InMemoryDB> =
        Transaction::new(
            "local-test",
            Default::default(),
            Some(offer),
            Default::default(),
        );
    let Ok(tx) = tx_prove(rng.split(), &tx, &RESOLVER).await else {
        return false;
    };
    // Fees are out of scope: only the spend itself is under test.
    let mut strictness = WellFormedStrictness::default();
    strictness.enforce_balancing = false;
    matches!(
        state.apply(&tx, strictness),
        Ok(TransactionResult::Success(..))
    )
}

#[tokio::test]
async fn guarded_spend_requires_guard_proof() {
    let mut rng = StdRng::seed_from_u64(0x6a7e);
    let guard_sk: Fr = rng.r#gen();
    let guard = Guard::setup(guard_sk);

    // The guard accepts the additional secret key, and only that key.
    let args = vec![Fr::from(0u64); GUARD_ARGS_LEN];
    assert!(guard.prove(&mut rng, &args, guard_sk).is_some());
    let wrong_sk: Fr = rng.r#gen();
    assert!(guard.prove(&mut rng, &args, wrong_sk).is_none());

    // The preimages of both token types, as the spender learns them privately.
    let contract_address = ContractAddress(HashOutput(rng.r#gen()));
    let domain_sep = HashOutput(rng.r#gen());
    let unguarded = ShieldedTokenPreimage {
        domain_sep,
        contract_address,
        rule: SpendRule::Empty,
    };
    let guarded = ShieldedTokenPreimage {
        rule: guard.rule(),
        ..unguarded
    };
    assert_eq!(
        unguarded.token_type(),
        contract_address.custom_shielded_token_type(domain_sep, &SpendRule::Empty)
    );
    assert_ne!(unguarded.token_type(), guarded.token_type());

    let mut state: TestState<InMemoryDB> = TestState::new(&mut rng);
    state.rewards_shielded(&mut rng, unguarded, 1_000);
    state.rewards_shielded(&mut rng, guarded, 1_000);

    // Control: the same spend of an unguarded token is accepted.
    assert!(
        spend_to_self(&mut state, &mut rng, &unguarded, Spend::Plain).await,
        "an ordinary spend of an unguarded token must be accepted"
    );

    // The owner holds the coin's zswap key, but not the guard's secret key,
    // and supplies no guard proof: neither circuit spends the coin.
    assert!(
        !spend_to_self(&mut state, &mut rng, &guarded, Spend::Plain).await,
        "a plain spend of a guarded token must be rejected"
    );
    assert!(
        !spend_to_self(&mut state, &mut rng, &guarded, Spend::Guarded(&mut |_| None)).await,
        "a spend of a guarded token without a guard proof must be rejected"
    );
}

#[tokio::test]
async fn guarded_spend_with_guard_proof() {
    let mut rng = StdRng::seed_from_u64(0x6a7f);
    let holder_sk: Fr = rng.r#gen();
    let guard = Guard::setup(holder_sk);
    let other_sk: Fr = rng.r#gen();
    let other_guard = Guard::setup(other_sk);

    let guarded = ShieldedTokenPreimage {
        domain_sep: HashOutput(rng.r#gen()),
        contract_address: ContractAddress(HashOutput(rng.r#gen())),
        rule: guard.rule(),
    };

    let mut state: TestState<InMemoryDB> = TestState::new(&mut rng);
    state.rewards_shielded(&mut rng, guarded, 1_000);
    let mut guard_rng = StdRng::seed_from_u64(0x9a7e);

    // A proof of a guard other than the one the descriptor names is
    // rejected, even though it verifies against its own key.
    assert!(
        !spend_to_self(
            &mut state,
            &mut rng,
            &guarded,
            Spend::Guarded(&mut |args| other_guard.prove(&mut guard_rng, args, other_sk)),
        )
        .await,
        "a spend authorised by a guard the descriptor does not name must be rejected"
    );

    // A proof of the named guard, by the holder of its secret key, on other
    // arguments than the spend's, is rejected.
    assert!(
        !spend_to_self(
            &mut state,
            &mut rng,
            &guarded,
            Spend::Guarded(&mut |args| {
                let mut other = args.to_vec();
                other[0] = other[0] + Fr::from(1u64);
                guard.prove(&mut guard_rng, &other, holder_sk)
            }),
        )
        .await,
        "a guard proof on other arguments must be rejected"
    );

    // A proof of the named guard, by the holder of its secret key, is accepted.
    assert!(
        spend_to_self(
            &mut state,
            &mut rng,
            &guarded,
            Spend::Guarded(&mut |args| guard.prove(&mut guard_rng, args, holder_sk)),
        )
        .await,
        "a spend authorised by the guard the descriptor names must be accepted"
    );
}
