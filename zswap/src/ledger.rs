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

use crate::ZSWAP_TREE_HEIGHT;
use crate::error::TransactionInvalid;
use crate::structure::*;
use base_crypto::time::{Duration, Timestamp};
use coin_structure::coin::{Commitment, Nullifier};
use coin_structure::contract::ContractAddress;
use derive_where::derive_where;
use serde::Serialize;
use serialize::{Deserializable, Serializable, Tagged, tag_enforcement_test};
use std::fmt::Debug;
use std::ops::Deref;
use storage::arena::Sp;
use storage::db::DB;
use storage::storage::default_storage;
use storage::storage::{HashMap, Map};
use storage::storage::{Identity, TimeFilterMap};
use storage::{
    Storable,
    arena::{ArenaHash, ArenaKey},
    storable::Loader,
};
use transient_crypto::merkle_tree::{MerkleTree, MerkleTreeDigest};

#[derive(Storable)]
#[derive_where(Clone, PartialEq, Debug, Eq)]
#[storable(db = D)]
#[tag = "zswap-ledger-state[v5]"]
#[must_use]
pub struct State<D: DB> {
    pub coin_coms: MerkleTree<Option<Sp<ContractAddress, D>>, D>,
    pub coin_coms_set: HashMap<Commitment, (), D>,
    pub first_free: u64,
    pub nullifiers: HashMap<Nullifier, (), D>,
    pub past_roots: TimeFilterMap<Identity<MerkleTreeDigest>, D>,
}
tag_enforcement_test!(State<storage::db::InMemoryDB>);

impl<D: DB> Default for State<D> {
    fn default() -> Self {
        State {
            coin_coms: MerkleTree::blank(ZSWAP_TREE_HEIGHT),
            coin_coms_set: HashMap::new(),
            first_free: 0,
            nullifiers: HashMap::new(),
            past_roots: TimeFilterMap::new(),
        }
    }
}

impl<D: DB> State<D> {
    pub fn new() -> Self {
        Default::default()
    }
    // ── the apply primitives ────────────────────────────────────────────────────────────
    //
    // Each takes the fields it reads rather than the struct they arrived in, so that applying
    // an `Offer` and applying a `ZswapEffects` run the same code. A second implementation of
    // "what applying does" is how the two could quietly disagree.

    fn apply_spend(
        mut self,
        merkle_tree_root: MerkleTreeDigest,
        nullifier: Nullifier,
        contract_address: Option<Sp<ContractAddress, D>>,
        whitelist: &Option<Map<ContractAddress, ()>>,
    ) -> Result<Self, TransactionInvalid> {
        if !self.past_roots.contains(&merkle_tree_root) {
            warn!(
                ?merkle_tree_root,
                "attempted spend with unknown Merkle tree"
            );
            return Err(TransactionInvalid::UnknownMerkleRoot(merkle_tree_root));
        };

        if self.nullifiers.contains_key(&nullifier) {
            warn!(?nullifier, "attempted double spend");
            return Err(TransactionInvalid::NullifierAlreadyPresent(nullifier));
        }

        if Self::on_whitelist(whitelist, &(contract_address.as_ref().map(|x| *x.deref()))) {
            self.nullifiers = self.nullifiers.insert(nullifier, ());
        }
        Ok(self)
    }

    fn apply_create(
        mut self,
        coin_com: Commitment,
        contract_address: Option<Sp<ContractAddress, D>>,
        whitelist: &Option<Map<ContractAddress, ()>>,
    ) -> Result<(Self, Commitment, u64), TransactionInvalid> {
        if self.coin_coms_set.contains_key(&coin_com) {
            warn!(?coin_com, "attempted faerie gold");
            return Err(TransactionInvalid::CommitmentAlreadyPresent(coin_com));
        }
        self.coin_coms_set = self.coin_coms_set.insert(coin_com, ());
        let first_free = self.first_free;
        self.coin_coms = self
            .coin_coms
            .try_update_hash(
                first_free,
                coin_com.0,
                contract_address.as_ref().map(|x| Sp::new(*x.deref())),
            )
            .map_err(TransactionInvalid::MerkleTreeError)?;

        if !Self::on_whitelist(whitelist, &contract_address.as_ref().map(|x| *x.deref())) {
            self.coin_coms = self.coin_coms.collapse(first_free, first_free);
        }

        self.first_free = first_free + 1;
        Ok((self, coin_com, first_free)) // Different from the spec because I'm referring to the pre-plus-1 value
    }

    fn apply_transient_effect(
        mut self,
        nullifier: Nullifier,
        coin_com: Commitment,
        contract_address: Option<Sp<ContractAddress, D>>,
        whitelist: &Option<Map<ContractAddress, ()>>,
    ) -> Result<(Self, Commitment, u64), TransactionInvalid> {
        // Checked here as well as in `apply_create`, deliberately: a transient that is
        // both a faerie gold and a double spend must report the commitment, as it did
        // before this was factored. One extra lookup buys unchanged error ordering.
        if self.coin_coms_set.contains_key(&coin_com) {
            warn!(?coin_com, "attempted faerie gold");
            return Err(TransactionInvalid::CommitmentAlreadyPresent(coin_com));
        }

        if self.nullifiers.contains_key(&nullifier) {
            return Err(TransactionInvalid::NullifierAlreadyPresent(nullifier));
        } else if Self::on_whitelist(whitelist, &contract_address.as_ref().map(|x| *x.deref())) {
            self.nullifiers = self.nullifiers.insert(nullifier, ());
        }

        self.apply_create(coin_com, contract_address, whitelist)
    }

    fn apply_input<P: Storable<D>, B: Clone + Storable<D>>(
        self,
        inp: Input<P, D, B>,
        whitelist: &Option<Map<ContractAddress, ()>>,
    ) -> Result<Self, TransactionInvalid> {
        self.apply_spend(
            inp.merkle_tree_root,
            inp.nullifier,
            inp.contract_address,
            whitelist,
        )
    }

    fn apply_output<P: Storable<D>, B: Clone + Storable<D>>(
        self,
        out: Output<P, D, B>,
        whitelist: &Option<Map<ContractAddress, ()>>,
    ) -> Result<(Self, Commitment, u64), TransactionInvalid> {
        self.apply_create(out.coin_com, out.contract_address, whitelist)
    }

    fn apply_transient<P: Storable<D>, B: Clone + Storable<D>>(
        self,
        trans: Transient<P, D, B>,
        whitelist: &Option<Map<ContractAddress, ()>>,
    ) -> Result<(Self, Commitment, u64), TransactionInvalid> {
        self.apply_transient_effect(
            trans.nullifier,
            trans.coin_com,
            trans.contract_address,
            whitelist,
        )
    }

    #[instrument(skip(whitelist))]
    fn on_whitelist(
        whitelist: &Option<Map<ContractAddress, ()>>,
        contract: &Option<ContractAddress>,
    ) -> bool {
        match (whitelist, contract) {
            (Some(list), Some(addr)) => list.contains_key(addr),
            // If we have a contract whitelist, the assumption is that we're
            // tracking a contract, *not* a user state!
            (Some(_), None) => false,
            (None, None) | (None, Some(_)) => true,
        }
    }

    #[instrument(skip(self, offer, whitelist))]
    pub fn try_apply<P: Storable<D> + Deserializable, B: Clone + Storable<D>>(
        &self,
        offer: &Offer<P, D, B>,
        whitelist: Option<Map<ContractAddress, ()>>,
    ) -> Result<(Self, Map<Commitment, u64>), TransactionInvalid> {
        let mut com_indicies = Map::new();
        let mut new_st = offer
            .inputs
            .iter_deref()
            .try_fold(self.clone(), |state, inp| {
                state.apply_input(inp.clone(), &whitelist)
            })?;
        (new_st, com_indicies) = offer.outputs.iter_deref().try_fold(
            (new_st, com_indicies),
            |(state, indicies), output| {
                let (state, com, index) = state.apply_output(output.clone(), &whitelist)?;
                Ok((state, indicies.insert(com, index)))
            },
        )?;
        (new_st, com_indicies) = offer.transient.iter_deref().try_fold(
            (new_st, com_indicies),
            |(state, indicies), trans| {
                let (state, com, index) = state.apply_transient(trans.clone(), &whitelist)?;
                Ok((state, indicies.insert(com, index)))
            },
        )?;
        Ok((new_st, com_indicies))
    }

    /// Apply an offer's effects without the offer.
    ///
    /// [`try_apply`] needs a whole [`Offer`], which carries what is needed to *verify* it as
    /// well as what is needed to apply it — and the two sets barely overlap. Every `Input` and
    /// `Output` holds a `Pedersen` value commitment, and every `Output`'s `CoinCiphertext`
    /// holds a second curve point; the apply path reads none of them. Decoding them is not
    /// free: `EmbeddedGroupAffine` deserialises through `embedded::Affine::from_bytes`, a
    /// point decompression with subgroup validation, and it dominates decoding the offer.
    ///
    /// So a consumer that has already verified an offer — or that never held its proofs — can
    /// apply it from [`ZswapEffects`] alone, and never carry or decode the rest.
    ///
    /// Identical to [`try_apply`] in what it checks: the same three primitives, in the same
    /// order, over the same state. `Offer::effects` derives the input; the pair is meant to be
    /// read together.
    ///
    /// [`try_apply`]: Self::try_apply
    /// [`Offer`]: crate::structure::Offer
    #[instrument(skip(self, effects, whitelist))]
    pub fn try_apply_effects(
        &self,
        effects: &ZswapEffects<D>,
        whitelist: Option<Map<ContractAddress, ()>>,
    ) -> Result<(Self, Map<Commitment, u64>), TransactionInvalid> {
        let mut com_indicies = Map::new();
        let mut new_st = effects
            .spends
            .iter_deref()
            .try_fold(self.clone(), |state, s| {
                state.apply_spend(
                    s.merkle_tree_root,
                    s.nullifier,
                    s.contract_address.clone(),
                    &whitelist,
                )
            })?;
        (new_st, com_indicies) = effects.creates.iter_deref().try_fold(
            (new_st, com_indicies),
            |(state, indicies), c| {
                let (state, com, index) =
                    state.apply_create(c.coin_com, c.contract_address.clone(), &whitelist)?;
                Ok((state, indicies.insert(com, index)))
            },
        )?;
        (new_st, com_indicies) = effects.transients.iter_deref().try_fold(
            (new_st, com_indicies),
            |(state, indicies), t| {
                let (state, com, index) = state.apply_transient_effect(
                    t.nullifier,
                    t.coin_com,
                    t.contract_address.clone(),
                    &whitelist,
                )?;
                Ok((state, indicies.insert(com, index)))
            },
        )?;
        Ok((new_st, com_indicies))
    }

    pub fn filter(
        &self,
        filter: &[ContractAddress],
    ) -> MerkleTree<Option<Sp<ContractAddress, D>>, D> {
        let retained_indices: Vec<u64> = self
            .coin_coms
            .iter_aux()
            .filter(|(_index, (_hash, opt_aux))| match opt_aux {
                Some(aux) => filter.contains(aux),
                None => false,
            })
            .map(|(index, ..)| index)
            .collect();
        let mut tree = self.coin_coms.clone();
        let mut p = 0;
        for i in retained_indices {
            if i > 0 {
                tree = tree.collapse(p, i - 1);
            }
            if i < u64::MAX {
                p = i + 1;
            }
        }
        if self.first_free > 0 {
            tree.collapse(p, self.first_free - 1)
        } else {
            tree
        }
    }

    pub fn post_block_update(&self, tblock: Timestamp) -> Self {
        let mut new_st = self.clone();
        new_st.coin_coms = new_st.coin_coms.rehash();
        new_st.past_roots = new_st.past_roots.insert(
            tblock,
            new_st
                .coin_coms
                .root()
                .expect("rehashed tree must have root"),
        );
        new_st.past_roots = new_st
            .past_roots
            .filter(tblock - (Duration::from_secs(3600)));

        new_st
    }
}

#[cfg(test)]
mod tests {
    use super::State;
    use crate::{DB, Delta};
    use crate::{Input, Offer, Output};
    use coin_structure::coin::{Info as CoinInfo, ShieldedTokenType, TokenType};
    use coin_structure::contract::ContractAddress;
    use coin_structure::transfer::Recipient;
    use rand::rngs::ThreadRng;
    use rand::{CryptoRng, Rng};
    use storage::db::InMemoryDB;

    /// `try_apply_effects` must agree with `try_apply` on every observable: the resulting
    /// state, the commitment indices, and the errors.
    ///
    /// This is the property that makes the split safe. `Offer::effects` drops the value
    /// commitments, the ciphertexts and the proofs, and the claim is that apply never read
    /// them — so the two paths must be indistinguishable from outside, including when they
    /// fail. A second implementation of "what applying does" would be free to drift; this is
    /// what stops that.
    #[test]
    fn effects_apply_exactly_as_the_offer_does() {
        let mut rng = rand::thread_rng();
        let offer_of = |rng: &mut ThreadRng, n: usize| -> Offer<(), InMemoryDB> {
            let (mut outputs, mut deltas) = (Vec::new(), Vec::new());
            for _ in 0..n {
                let (type_, value): (ShieldedTokenType, u128) = (rng.r#gen(), rng.r#gen());
                let info = CoinInfo {
                    nonce: rng.r#gen(),
                    type_,
                    value,
                };
                let cpk = coin_structure::coin::PublicKey(rng.r#gen());
                outputs.push(
                    Output::new(rng, &info, None, &cpk, None)
                        .unwrap()
                        .erase_proof(),
                );
                deltas.push(Delta {
                    token_type: type_,
                    value: value as i128,
                });
            }
            Offer {
                inputs: vec![].into(),
                outputs: outputs.into(),
                transient: vec![].into(),
                deltas: deltas.into(),
            }
        };

        let base = State::<InMemoryDB>::new();
        let offer = offer_of(&mut rng, 4);

        let (via_offer, idx_offer) = base.try_apply(&offer, None).unwrap();
        let (via_effects, idx_effects) = base.try_apply_effects(&offer.effects(), None).unwrap();

        assert_eq!(idx_offer, idx_effects, "commitment indices must agree");
        assert_eq!(
            via_offer.coin_coms.root(),
            via_effects.coin_coms.root(),
            "the commitment tree root must agree"
        );
        assert_eq!(via_offer.first_free, via_effects.first_free);
        assert_eq!(via_offer.coin_coms_set, via_effects.coin_coms_set);
        assert_eq!(via_offer.nullifiers, via_effects.nullifiers);

        // ⌖ And they must agree when they *fail*. Replaying the same offer is faerie gold, and
        // an error that differed between the two paths would be a difference a caller could
        // observe — which is the whole thing this test exists to rule out.
        let replay_offer = via_offer.try_apply(&offer, None).unwrap_err();
        let replay_effects = via_effects
            .try_apply_effects(&offer.effects(), None)
            .unwrap_err();
        assert_eq!(
            format!("{replay_offer:?}"),
            format!("{replay_effects:?}"),
            "a rejected apply must fail identically either way"
        );
    }

    #[test]
    fn test_filtered_spend() {
        fn insert_dummy_outputs<R: Rng + CryptoRng, D: DB>(
            rng: &mut R,
            mut state: State<D>,
            n: usize,
        ) -> State<D> {
            for _ in 0..n {
                let (type_, value) = (rng.r#gen(), rng.r#gen());
                let delta = Delta {
                    token_type: type_,
                    value: value as i128,
                };
                let info = CoinInfo {
                    nonce: rng.r#gen(),
                    type_,
                    value,
                };
                let cpk = coin_structure::coin::PublicKey(rng.r#gen());
                let output = Output::new(rng, &info, None, &cpk, None).unwrap();
                state = state
                    .try_apply(
                        &Offer {
                            inputs: vec![].into(),
                            outputs: vec![output].into(),
                            transient: vec![].into(),
                            deltas: vec![delta].into(),
                        },
                        None,
                    )
                    .unwrap()
                    .0
                    .post_block_update(Default::default());
            }
            state
        }
        let mut state = State::<InMemoryDB>::new();
        let mut rng = rand::thread_rng();
        let coin = CoinInfo {
            nonce: rng.r#gen(),
            type_: rng.r#gen(),
            value: 500,
        };
        let addr = ContractAddress::default();
        state = insert_dummy_outputs(&mut rng, state, 25);
        let output = Output::new_contract_owned(&mut rng, &coin, None, addr).unwrap();
        let (new_state, indices) = state
            .try_apply(
                &Offer {
                    inputs: vec![].into(),
                    outputs: vec![output].into(),
                    transient: vec![].into(),
                    deltas: vec![Delta {
                        token_type: rng.r#gen(),
                        value: 500,
                    }]
                    .into(),
                },
                None,
            )
            .unwrap();
        state = new_state.post_block_update(Default::default());
        state = insert_dummy_outputs(&mut rng, state, 25);
        let qcoin = coin.qualify(
            *indices
                .get(&coin.commitment(&Recipient::Contract(addr)))
                .unwrap(),
        );
        Input::new_contract_owned(&mut rng, &qcoin, None, addr, &state.filter(&[addr])).unwrap();
    }
}
