// This file is part of midnight-ledger.
// Copyright (C) 2026 Midnight Foundation
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

//! State translation from ledger v8 to ledger v9.
//!
//! Differences in the stored state shape between v8 and v9:
//!
//! | type                            | v8 tag                           | v9 tag                           | change |
//! | ------------------------------- | -------------------------------- | -------------------------------- | ------ |
//! | LedgerState                     | `ledger-state[v13]`              | `ledger-state[v16]`              | `bridge_receiving` gains `NightAnn` |
//! | LedgerParameters                | `ledger-parameters[v5]`         | `ledger-parameters[v6]`         | new field `min_block_price` |
//! | ContractState                   | `contract-state[v6]`             | `contract-state[v7]`             | propagates from CMA change |
//! | ContractMaintenanceAuthority    | `contract-maintenance-authority[v1]` | `contract-maintenance-authority[v2]` | `committee: Vec<VerifyingKey>` → `Vec<ContractMaintenanceVerifyingKey>` |
//!
//! Everything else (zswap, utxo, dust, replay_protection, unclaimed_block_rewards,
//! treasury) is tag-stable and can be `recast`.

use base_crypto::cost_model::CostDuration;
use serialize::Tagged;
use std::ops::Deref;
use std::{any::Any, borrow::Cow, io, marker::PhantomData};
use storage::{
    Storable,
    arena::Sp,
    db::DB,
    merkle_patricia_trie::{self, Annotation, MerklePatriciaTrie},
    state_translation::*,
    storable::SizeAnn,
    storage::{HashMap, Map, default_storage},
};

// ---------- Generic helpers (copied from the v6→v7 reference) ----------

/// Recast a stored object from one type to another, requiring matching tags.
/// Used for subtrees whose tag is unchanged between v8 and v9.
fn recast<A: Storable<D> + Tagged, B: Storable<D> + Tagged, D: DB>(
    a: &Sp<A, D>,
) -> io::Result<Sp<B, D>> {
    if A::tag() != B::tag() {
        return io::Result::Err(io::Error::new(io::ErrorKind::Other, "tags do not match"));
    }
    default_storage::<D>().get_lazy(&a.as_child().into())
}

/// Generic MPT translation: walks the trie, translating each entry via the
/// table-registered translation for `A→B`, and recomputes annotations under
/// `AnnB` from the new values.
struct MptTl<A, B, AnnA, AnnB>(PhantomData<(A, B, AnnA, AnnB)>);

impl<
    A: Storable<D> + Tagged,
    B: Storable<D> + Tagged,
    AnnA: Annotation<A> + Storable<D> + Tagged,
    AnnB: Annotation<B> + Storable<D> + Tagged,
    D: DB,
> DirectTranslation<MerklePatriciaTrie<A, D, AnnA>, MerklePatriciaTrie<B, D, AnnB>, D>
    for MptTl<A, B, AnnA, AnnB>
{
    fn required_translations() -> Vec<TranslationId> {
        vec![TranslationId(
            merkle_patricia_trie::Node::<A, D, AnnA>::tag(),
            merkle_patricia_trie::Node::<B, D, AnnB>::tag(),
        )]
    }
    fn child_translations(
        source: &MerklePatriciaTrie<A, D, AnnA>,
    ) -> Vec<(TranslationId, Sp<dyn Any + Send + Sync, D>)> {
        let tlids = <Self as DirectTranslation<MerklePatriciaTrie<A, D, AnnA>, _, D>>::required_translations();
        vec![(tlids[0].clone(), source.0.upcast())]
    }
    fn finalize(
        source: &MerklePatriciaTrie<A, D, AnnA>,
        _limit: &mut CostDuration,
        cache: &TranslationCache<D>,
    ) -> io::Result<Option<MerklePatriciaTrie<B, D, AnnB>>> {
        let tls = Self::child_translations(source);
        Ok(Some(MerklePatriciaTrie(try_resopt!(
            cache.resolve(&tls[0].0, tls[0].1.as_child())
        ))))
    }
}

impl<
    A: Storable<D> + Tagged,
    B: Storable<D> + Tagged,
    AnnA: Storable<D> + Tagged + Annotation<A>,
    AnnB: Storable<D> + Tagged + Annotation<B>,
    D: DB,
>
    DirectTranslation<
        merkle_patricia_trie::Node<A, D, AnnA>,
        merkle_patricia_trie::Node<B, D, AnnB>,
        D,
    > for MptTl<A, B, AnnA, AnnB>
{
    fn required_translations() -> Vec<TranslationId> {
        let entry_tl = TranslationId(A::tag(), B::tag());
        let self_tl = TranslationId(
            merkle_patricia_trie::Node::<A, D, AnnA>::tag(),
            merkle_patricia_trie::Node::<B, D, AnnB>::tag(),
        );
        vec![entry_tl, self_tl]
    }
    fn child_translations(
        source: &merkle_patricia_trie::Node<A, D, AnnA>,
    ) -> Vec<(TranslationId, Sp<dyn Any + Send + Sync, D>)> {
        let tls = <Self as DirectTranslation<merkle_patricia_trie::Node::<A, D, AnnA>, _, D>>::required_translations();
        let entry_tl = tls[0].clone();
        let self_tl = tls[1].clone();
        match source {
            merkle_patricia_trie::Node::Empty => vec![],
            merkle_patricia_trie::Node::Branch { children, .. } => children
                .iter()
                .map(|child| (self_tl.clone(), child.upcast()))
                .collect(),
            merkle_patricia_trie::Node::Extension { child, .. } => {
                vec![(self_tl, child.upcast())]
            }
            merkle_patricia_trie::Node::MidBranchLeaf { value, child, .. } => {
                vec![(entry_tl, value.upcast()), (self_tl, child.upcast())]
            }
            merkle_patricia_trie::Node::Leaf { value, .. } => vec![(entry_tl, value.upcast())],
        }
    }
    fn finalize(
        source: &merkle_patricia_trie::Node<A, D, AnnA>,
        _limit: &mut CostDuration,
        cache: &TranslationCache<D>,
    ) -> io::Result<Option<merkle_patricia_trie::Node<B, D, AnnB>>> {
        let tls = Self::child_translations(source);
        Ok(Some(match source {
            merkle_patricia_trie::Node::Empty => merkle_patricia_trie::Node::Empty,
            merkle_patricia_trie::Node::Branch { .. } => {
                let mut new_children =
                    core::array::from_fn(|_| Sp::new(merkle_patricia_trie::Node::Empty));
                for (child, new_child) in tls.iter().zip(new_children.iter_mut()) {
                    *new_child = try_resopt!(cache.resolve(&child.0, child.1.as_child()));
                }
                let ann = new_children
                    .iter()
                    .fold(AnnB::empty(), |acc, x| {
                        acc.append(&merkle_patricia_trie::Node::<B, D, AnnB>::ann(x))
                    });
                merkle_patricia_trie::Node::Branch {
                    ann,
                    children: Box::new(new_children),
                }
            }
            merkle_patricia_trie::Node::Extension {
                compressed_path, ..
            } => {
                let child: Sp<merkle_patricia_trie::Node<B, D, AnnB>, D> =
                    try_resopt!(cache.resolve(&tls[0].0, tls[0].1.as_child()));
                let ann = merkle_patricia_trie::Node::<B, D, AnnB>::ann(&child);
                merkle_patricia_trie::Node::Extension {
                    ann,
                    compressed_path: compressed_path.clone(),
                    child,
                }
            }
            merkle_patricia_trie::Node::Leaf { .. } => {
                let value = try_resopt!(cache.resolve(&tls[0].0, tls[0].1.as_child()));
                let ann = AnnB::from_value(&value);
                merkle_patricia_trie::Node::Leaf { ann, value }
            }
            merkle_patricia_trie::Node::MidBranchLeaf { .. } => {
                let value = try_resopt!(cache.resolve(&tls[0].0, tls[0].1.as_child()));
                let child: Sp<merkle_patricia_trie::Node<B, D, AnnB>, D> =
                    try_resopt!(cache.resolve(&tls[1].0, tls[1].1.as_child()));
                let ann = AnnB::from_value(&value)
                    .append(&merkle_patricia_trie::Node::<B, D, AnnB>::ann(&child));
                merkle_patricia_trie::Node::MidBranchLeaf { ann, value, child }
            }
        }))
    }
}

/// Identity translation for a type whose serialization is unchanged across
/// versions. Needed when an MPT's entries are tag-stable but its annotation
/// changes (e.g. `bridge_receiving`).
struct IdentityTl<T>(PhantomData<T>);

impl<T: Storable<D> + Clone, D: DB> DirectTranslation<T, T, D> for IdentityTl<T> {
    fn required_translations() -> Vec<TranslationId> {
        Vec::new()
    }
    fn child_translations(_: &T) -> Vec<(TranslationId, Sp<dyn Any + Send + Sync, D>)> {
        Vec::new()
    }
    fn finalize(
        source: &T,
        _limit: &mut CostDuration,
        _cache: &TranslationCache<D>,
    ) -> io::Result<Option<T>> {
        Ok(Some(source.clone()))
    }
}

// ---------- Translation IDs (shorthand) ----------

struct Ids;

impl Ids {
    fn contract_mpt<D: DB>() -> TranslationId {
        TranslationId(
            MerklePatriciaTrie::<
                onchain_state_v8::state::ContractState<D>,
                D,
                ledger_v8::annotation::NightAnn,
            >::tag(),
            MerklePatriciaTrie::<
                onchain_state_v9::state::ContractState<D>,
                D,
                ledger_v9::annotation::NightAnn,
            >::tag(),
        )
    }

    fn bridge_receiving_mpt<D: DB>() -> TranslationId {
        TranslationId(
            MerklePatriciaTrie::<u128, D, SizeAnn>::tag(),
            MerklePatriciaTrie::<u128, D, ledger_v9::annotation::NightAnn>::tag(),
        )
    }

    fn parameters<D: DB>() -> TranslationId {
        TranslationId(
            ledger_v8::structure::LedgerParameters::tag(),
            ledger_v9::structure::LedgerParameters::tag(),
        )
    }
}

// ---------- Top-level: LedgerState v8 → v9 ----------

struct LedgerStateTl;

impl<D: DB>
    DirectTranslation<ledger_v8::structure::LedgerState<D>, ledger_v9::structure::LedgerState<D>, D>
    for LedgerStateTl
{
    fn required_translations() -> Vec<TranslationId> {
        vec![
            Ids::parameters::<D>(),
            Ids::bridge_receiving_mpt::<D>(),
            Ids::contract_mpt::<D>(),
        ]
    }

    fn child_translations(
        source: &ledger_v8::structure::LedgerState<D>,
    ) -> Vec<(TranslationId, Sp<dyn Any + Send + Sync, D>)> {
        vec![
            (Ids::parameters::<D>(), source.parameters.upcast()),
            (
                Ids::bridge_receiving_mpt::<D>(),
                source.bridge_receiving.mpt.upcast(),
            ),
            (Ids::contract_mpt::<D>(), source.contract.mpt.upcast()),
        ]
    }

    fn finalize(
        source: &ledger_v8::structure::LedgerState<D>,
        _limit: &mut CostDuration,
        cache: &TranslationCache<D>,
    ) -> io::Result<Option<ledger_v9::structure::LedgerState<D>>> {
        let Some(parameters) = cache.lookup(&Ids::parameters::<D>(), source.parameters.as_child())
        else {
            return Ok(None);
        };
        let Some(bridge_recv_mpt) =
            cache.lookup(&Ids::bridge_receiving_mpt::<D>(), source.bridge_receiving.mpt.as_child())
        else {
            return Ok(None);
        };
        let Some(contract_mpt) =
            cache.lookup(&Ids::contract_mpt::<D>(), source.contract.mpt.as_child())
        else {
            return Ok(None);
        };

        Ok(Some(ledger_v9::structure::LedgerState {
            network_id: source.network_id.clone(),
            parameters: parameters.force_downcast(),
            locked_pool: source.locked_pool,
            bridge_receiving: Map {
                mpt: bridge_recv_mpt.force_downcast(),
                key_type: PhantomData,
            },
            reserve_pool: source.reserve_pool,
            block_reward_pool: source.block_reward_pool,
            unclaimed_block_rewards: Map {
                mpt: recast(&source.unclaimed_block_rewards.mpt)?,
                key_type: PhantomData,
            },
            treasury: Map {
                mpt: recast(&source.treasury.mpt)?,
                key_type: PhantomData,
            },
            zswap: recast(&source.zswap)?,
            contract: Map {
                mpt: contract_mpt.force_downcast(),
                key_type: PhantomData,
            },
            utxo: recast(&source.utxo)?,
            replay_protection: recast(&source.replay_protection)?,
            dust: recast(&source.dust)?,
        }))
    }
}

// ---------- LedgerParameters v8 → v9 ----------

struct LedgerParametersTl;

impl<D: DB>
    DirectTranslation<ledger_v8::structure::LedgerParameters, ledger_v9::structure::LedgerParameters, D>
    for LedgerParametersTl
{
    fn required_translations() -> Vec<TranslationId> {
        Vec::new()
    }
    fn child_translations(
        _: &ledger_v8::structure::LedgerParameters,
    ) -> Vec<(TranslationId, Sp<dyn Any + Send + Sync, D>)> {
        Vec::new()
    }
    fn finalize(
        source: &ledger_v8::structure::LedgerParameters,
        _limit: &mut CostDuration,
        _cache: &TranslationCache<D>,
    ) -> io::Result<Option<ledger_v9::structure::LedgerParameters>> {
        // Base-crypto-backed fields (Duration, FixedPoint, primitives) are
        // assignable directly because `midnight-base-crypto` is unified across
        // v8 and v9 by workspace patches. Composite types defined in `ledger`
        // (TransactionCostModel, TransactionLimits, etc.) are tag-stable but
        // not identical types, so we go through the (de)serializer.
        Ok(Some(ledger_v9::structure::LedgerParameters {
            cost_model: recast_base(&source.cost_model)?,
            limits: recast_base(&source.limits)?,
            dust: recast_base(&source.dust)?,
            fee_prices: recast_base(&source.fee_prices)?,
            global_ttl: source.global_ttl,
            cost_dimension_min_ratio: source.cost_dimension_min_ratio,
            price_adjustment_a_parameter: source.price_adjustment_a_parameter,
            cardano_to_midnight_bridge_fee_basis_points:
                source.cardano_to_midnight_bridge_fee_basis_points,
            c_to_m_bridge_min_amount: source.c_to_m_bridge_min_amount,
            // NEW IN v9 — placeholder; the production value should match the
            // value chosen for the hardfork.
            min_block_price: ledger_v9::structure::INITIAL_PARAMETERS.min_block_price,
        }))
    }
}

/// Recast for tag-stable base types passed by value (cost model, limits, etc.).
/// Not the same as `recast` above which only works for `Sp`.
fn recast_base<A: Tagged + serialize::Serializable, B: Tagged + serialize::Deserializable>(
    a: &A,
) -> io::Result<B> {
    if A::tag() != B::tag() {
        return Err(io::Error::new(io::ErrorKind::Other, "tags do not match"));
    }
    let mut buf = Vec::new();
    a.serialize(&mut buf)?;
    B::deserialize(&mut &buf[..], 0)
}

// ---------- ContractState v8 → v9 ----------

struct ContractStateTl;

impl<D: DB>
    DirectTranslation<
        onchain_state_v8::state::ContractState<D>,
        onchain_state_v9::state::ContractState<D>,
        D,
    > for ContractStateTl
{
    fn required_translations() -> Vec<TranslationId> {
        Vec::new()
    }
    fn child_translations(
        _: &onchain_state_v8::state::ContractState<D>,
    ) -> Vec<(TranslationId, Sp<dyn Any + Send + Sync, D>)> {
        Vec::new()
    }
    fn finalize(
        source: &onchain_state_v8::state::ContractState<D>,
        _limit: &mut CostDuration,
        _cache: &TranslationCache<D>,
    ) -> io::Result<Option<onchain_state_v9::state::ContractState<D>>> {
        // ChargedState, ContractOperation, EntryPointBuf, balance HashMap —
        // all tag-stable, recast through.
        let committee_v9 = source
            .maintenance_authority
            .committee
            .iter()
            .map(|vk| {
                onchain_state_v9::state::ContractMaintenanceVerifyingKey::Schnorr(vk.clone())
            })
            .collect();
        let maintenance_authority = onchain_state_v9::state::ContractMaintenanceAuthority {
            committee: committee_v9,
            threshold: source.maintenance_authority.threshold,
            counter: source.maintenance_authority.counter,
        };
        Ok(Some(onchain_state_v9::state::ContractState::<D> {
            data: recast::<
                onchain_state_v8::state::ChargedState<D>,
                onchain_state_v9::state::ChargedState<D>,
                D,
            >(&Sp::new(source.data.clone()))?
            .deref()
            .clone(),
            operations: HashMap(Map {
                mpt: recast(&source.operations.0.mpt)?,
                key_type: PhantomData,
            }),
            maintenance_authority,
            balance: HashMap(Map {
                mpt: recast(&source.balance.0.mpt)?,
                key_type: PhantomData,
            }),
        }))
    }
}

// ---------- Translation table ----------

pub struct StateTranslationTable;

impl<D: DB> TranslationTable<D> for StateTranslationTable {
    const TABLE: &[(TranslationId, &dyn TypelessTranslation<D>)] = &[
        // Top-level
        (
            TranslationId(
                Cow::Borrowed("ledger-state[v13]"),
                Cow::Borrowed("ledger-state[v16]"),
            ),
            &DirectSpTranslation::<_, _, LedgerStateTl, _>(PhantomData),
        ),
        // LedgerParameters
        (
            TranslationId(
                Cow::Borrowed("ledger-parameters[v5]"),
                Cow::Borrowed("ledger-parameters[v6]"),
            ),
            &DirectSpTranslation::<_, _, LedgerParametersTl, _>(PhantomData),
        ),
        // ContractState
        (
            TranslationId(
                Cow::Borrowed("contract-state[v6]"),
                Cow::Borrowed("contract-state[v7]"),
            ),
            &DirectSpTranslation::<_, _, ContractStateTl, _>(PhantomData),
        ),
        // `contract` MPT in LedgerState — entries are ContractState
        (
            TranslationId(
                Cow::Borrowed("mpt(contract-state[v6],night-annotation)"),
                Cow::Borrowed("mpt(contract-state[v7],night-annotation)"),
            ),
            &DirectSpTranslation::<
                MerklePatriciaTrie<
                    onchain_state_v8::state::ContractState<D>,
                    D,
                    ledger_v8::annotation::NightAnn,
                >,
                MerklePatriciaTrie<
                    onchain_state_v9::state::ContractState<D>,
                    D,
                    ledger_v9::annotation::NightAnn,
                >,
                MptTl<
                    onchain_state_v8::state::ContractState<D>,
                    onchain_state_v9::state::ContractState<D>,
                    ledger_v8::annotation::NightAnn,
                    ledger_v9::annotation::NightAnn,
                >,
                _,
            >(PhantomData),
        ),
        (
            TranslationId(
                Cow::Borrowed("mpt-node(contract-state[v6],night-annotation)"),
                Cow::Borrowed("mpt-node(contract-state[v7],night-annotation)"),
            ),
            &DirectSpTranslation::<
                merkle_patricia_trie::Node<
                    onchain_state_v8::state::ContractState<D>,
                    D,
                    ledger_v8::annotation::NightAnn,
                >,
                merkle_patricia_trie::Node<
                    onchain_state_v9::state::ContractState<D>,
                    D,
                    ledger_v9::annotation::NightAnn,
                >,
                MptTl<
                    onchain_state_v8::state::ContractState<D>,
                    onchain_state_v9::state::ContractState<D>,
                    ledger_v8::annotation::NightAnn,
                    ledger_v9::annotation::NightAnn,
                >,
                _,
            >(PhantomData),
        ),
        // `bridge_receiving` MPT — entries unchanged (u128), annotation changes
        // from SizeAnn to NightAnn. Needs an identity entry translation and an
        // MptTl that re-annotates.
        (
            TranslationId(Cow::Borrowed("u128"), Cow::Borrowed("u128")),
            &DirectSpTranslation::<u128, u128, IdentityTl<u128>, _>(PhantomData),
        ),
        (
            TranslationId(
                Cow::Borrowed("mpt(u128,size-annotation)"),
                Cow::Borrowed("mpt(u128,night-annotation)"),
            ),
            &DirectSpTranslation::<
                MerklePatriciaTrie<u128, D, SizeAnn>,
                MerklePatriciaTrie<u128, D, ledger_v9::annotation::NightAnn>,
                MptTl<u128, u128, SizeAnn, ledger_v9::annotation::NightAnn>,
                _,
            >(PhantomData),
        ),
        (
            TranslationId(
                Cow::Borrowed("mpt-node(u128,size-annotation)"),
                Cow::Borrowed("mpt-node(u128,night-annotation)"),
            ),
            &DirectSpTranslation::<
                merkle_patricia_trie::Node<u128, D, SizeAnn>,
                merkle_patricia_trie::Node<u128, D, ledger_v9::annotation::NightAnn>,
                MptTl<u128, u128, SizeAnn, ledger_v9::annotation::NightAnn>,
                _,
            >(PhantomData),
        ),
    ];
}
