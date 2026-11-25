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

//! State translation from ledger v9 to ledger v10.
//!
//! ## Usage
//!
//! ```ignore
//! use storage_v10::state_translation::TypedTranslationState;
//! use v9_to_v10_state_translation::StateTranslationTable;
//!
//! let mut tl = TypedTranslationState::<
//!     ledger_v9::structure::LedgerState<D>,
//!     ledger_v10::structure::LedgerState<D>,
//!     StateTranslationTable,
//!     D,
//! >::start(Sp::new(v9_state))?;
//! loop {
//!     tl = tl.run(cost_per_iteration)?;
//!     if let Some(result) = tl.result()? { /* done */ break; }
//! }
//! ```
//!
//! ## State shape differences (only stored types listed)
//!
//! | type              | v9 tag                   | v10 tag                  | change |
//! | ----------------- | ------------------------ | ------------------------ | ------ |
//! | LedgerState       | `ledger-state[v18]`      | `ledger-state[v19]`      | reflows `ContractState` |
//! | ContractState     | `contract-state[v8]`     | `contract-state[v9]`     | reflows `ContractOperation`; canonicalises `charged_keys.rc_0` |
//! | ContractOperation | `contract-operation[v6]` | `contract-operation[v7]` | `{ v2, v3, ir }` → `{ v2, v3, v4, ir }`; `v4` stays empty |
//!
//! ## Canonicalisation
//!
//! Storage 4 (ledger 10) rejects tries holding a *short extension chain*: an
//! `Extension` shorter than 255 nibbles whose child is another `Extension`.
//! Storage 3's `insert` produced them when splitting a 255-nibble extension,
//! which needs a key longer than 255 nibbles. Of the tries in ledger state,
//! only `RcMap::rc_0` has such keys: it is keyed by `ArenaKey`, whose `Direct`
//! form inlines a node of up to 1 KiB. Every other trie is keyed by a hash, an
//! address, a token type, a timestamp or an index, and is recast unchanged;
//! the `state_value_tries_stay_canonical` property backs this for contract
//! state.
//!
//! `rc_0` is rebuilt node by node, merging each short chain on the way up
//! (see [`MptTl`]). Its entries are unchanged, and since contract state keeps
//! its hashes, `charged_keys` still accounts for exactly the same nodes.
//!
//! Everything else in `LedgerState` is tag-stable and passes through `recast`.

use base_crypto::cost_model::CostDuration;
use serialize::Tagged;
use std::{any::Any, borrow::Cow, io, marker::PhantomData};
use storage_v9::{
    Storable,
    arena::Sp,
    db::DB,
    delta_tracking::ChildRef as ChildRefV9,
    merkle_patricia_trie::{
        Annotation as AnnotationV9, MerklePatriciaTrie as MptV9, Node as NodeV9,
    },
    storable::SizeAnn,
    storage::default_storage,
};
use storage_v10::{
    delta_tracking::{ChildRef as ChildRefV10, RcMap as RcMapV10},
    merkle_patricia_trie::{
        Annotation as AnnotationV10, MerklePatriciaTrie as MptV10, Node as NodeV10,
        canonical_extension,
    },
    state_translation::*,
    storage::{HashMap, Map},
};

use ledger_v9::annotation::NightAnn as NightAnnV9;
use ledger_v10::annotation::NightAnn as NightAnnV10;
use onchain_state_v9::state as cs9;
use onchain_state_v10::state as cs10;

/// Reinterpret a stored object as another type with the same tag, keeping its
/// bytes and hash.
fn recast<A: Storable<D> + Tagged, B: Storable<D> + Tagged, D: DB>(
    a: &Sp<A, D>,
) -> io::Result<Sp<B, D>> {
    if A::tag() != B::tag() {
        return Err(io::Error::other(format!(
            "tags do not match: {} vs {}",
            A::tag(),
            B::tag()
        )));
    }
    default_storage::<D>().get_lazy(&a.as_child().into())
}

/// [`recast`] for tag-stable types held by value.
fn recast_base<A: Tagged + serialize::Serializable, B: Tagged + serialize::Deserializable>(
    a: &A,
) -> io::Result<B> {
    if A::tag() != B::tag() {
        return Err(io::Error::other(format!(
            "tags do not match: {} vs {}",
            A::tag(),
            B::tag()
        )));
    }
    let mut buf = Vec::new();
    a.serialize(&mut buf)?;
    B::deserialize(&mut &buf[..], 0)
}

/// Translates a trie node for node, translating entries through the table
/// and recomputing annotations. An extension over a translated extension is
/// merged and re-chunked by storage 4's [`canonical_extension`], which removes
/// short extension chains; translated children are canonical already, so no
/// other node changes shape.
struct MptTl<A, B, AnnA, AnnB>(PhantomData<(A, B, AnnA, AnnB)>);

impl<
    A: Storable<D> + Tagged,
    B: Storable<D> + Tagged,
    AnnA: AnnotationV9<A> + Storable<D> + Tagged,
    AnnB: AnnotationV10<B> + Storable<D> + Tagged,
    D: DB,
> DirectTranslation<MptV9<A, D, AnnA>, MptV10<B, D, AnnB>, D> for MptTl<A, B, AnnA, AnnB>
{
    fn required_translations() -> Vec<TranslationId> {
        vec![TranslationId(
            NodeV9::<A, D, AnnA>::tag(),
            NodeV10::<B, D, AnnB>::tag(),
        )]
    }
    fn child_translations(
        source: &MptV9<A, D, AnnA>,
    ) -> Vec<(TranslationId, Sp<dyn Any + Send + Sync, D>)> {
        let tlids = <Self as DirectTranslation<MptV9<A, D, AnnA>, _, D>>::required_translations();
        vec![(tlids[0].clone(), source.0.upcast())]
    }
    fn finalize(
        source: &MptV9<A, D, AnnA>,
        _limit: &mut CostDuration,
        cache: &TranslationCache<D>,
    ) -> io::Result<Option<MptV10<B, D, AnnB>>> {
        let tls = Self::child_translations(source);
        Ok(Some(MptV10(try_resopt!(
            cache.resolve(&tls[0].0, tls[0].1.as_child())
        ))))
    }
}

impl<
    A: Storable<D> + Tagged,
    B: Storable<D> + Tagged,
    AnnA: AnnotationV9<A> + Storable<D> + Tagged,
    AnnB: AnnotationV10<B> + Storable<D> + Tagged,
    D: DB,
> DirectTranslation<NodeV9<A, D, AnnA>, NodeV10<B, D, AnnB>, D> for MptTl<A, B, AnnA, AnnB>
{
    fn required_translations() -> Vec<TranslationId> {
        vec![
            TranslationId(A::tag(), B::tag()),
            TranslationId(NodeV9::<A, D, AnnA>::tag(), NodeV10::<B, D, AnnB>::tag()),
        ]
    }
    fn child_translations(
        source: &NodeV9<A, D, AnnA>,
    ) -> Vec<(TranslationId, Sp<dyn Any + Send + Sync, D>)> {
        let tls = <Self as DirectTranslation<NodeV9<A, D, AnnA>, _, D>>::required_translations();
        let entry_tl = tls[0].clone();
        let self_tl = tls[1].clone();
        match source {
            NodeV9::Empty => vec![],
            NodeV9::Branch { children, .. } => children
                .iter()
                .map(|child| (self_tl.clone(), child.upcast()))
                .collect(),
            NodeV9::Extension { child, .. } => vec![(self_tl, child.upcast())],
            NodeV9::MidBranchLeaf { value, child, .. } => {
                vec![(entry_tl, value.upcast()), (self_tl, child.upcast())]
            }
            NodeV9::Leaf { value, .. } => vec![(entry_tl, value.upcast())],
        }
    }
    fn finalize(
        source: &NodeV9<A, D, AnnA>,
        _limit: &mut CostDuration,
        cache: &TranslationCache<D>,
    ) -> io::Result<Option<NodeV10<B, D, AnnB>>> {
        let tls = Self::child_translations(source);
        Ok(Some(match source {
            NodeV9::Empty => NodeV10::Empty,
            NodeV9::Branch { .. } => {
                let mut new_children = core::array::from_fn(|_| Sp::new(NodeV10::Empty));
                for (child, new_child) in tls.iter().zip(new_children.iter_mut()) {
                    *new_child = try_resopt!(cache.resolve(&child.0, child.1.as_child()));
                }
                let ann = new_children.iter().fold(AnnB::empty(), |acc, x| {
                    acc.append(&NodeV10::<B, D, AnnB>::ann(x))
                });
                NodeV10::Branch {
                    ann,
                    children: Box::new(new_children),
                }
            }
            NodeV9::Extension {
                compressed_path, ..
            } => {
                let child: Sp<NodeV10<B, D, AnnB>, D> =
                    try_resopt!(cache.resolve(&tls[0].0, tls[0].1.as_child()));
                (*canonical_extension(compressed_path.clone(), child)).clone()
            }
            NodeV9::Leaf { .. } => {
                let value = try_resopt!(cache.resolve(&tls[0].0, tls[0].1.as_child()));
                let ann = AnnB::from_value(&value);
                NodeV10::Leaf { ann, value }
            }
            NodeV9::MidBranchLeaf { .. } => {
                let value = try_resopt!(cache.resolve(&tls[0].0, tls[0].1.as_child()));
                let child: Sp<NodeV10<B, D, AnnB>, D> =
                    try_resopt!(cache.resolve(&tls[1].0, tls[1].1.as_child()));
                let ann = AnnB::from_value(&value).append(&NodeV10::<B, D, AnnB>::ann(&child));
                NodeV10::MidBranchLeaf { ann, value, child }
            }
        }))
    }
}

/// `ChildRef` is tag-stable, but must be rebuilt rather than recast so that
/// the v10 value holds its own backend reference to the child.
struct ChildRefTl;

impl<D: DB> DirectTranslation<ChildRefV9<D>, ChildRefV10<D>, D> for ChildRefTl {
    fn required_translations() -> Vec<TranslationId> {
        Vec::new()
    }
    fn child_translations(_: &ChildRefV9<D>) -> Vec<(TranslationId, Sp<dyn Any + Send + Sync, D>)> {
        Vec::new()
    }
    fn finalize(
        source: &ChildRefV9<D>,
        _limit: &mut CostDuration,
        _cache: &TranslationCache<D>,
    ) -> io::Result<Option<ChildRefV10<D>>> {
        Ok(Some(ChildRefV10::new(source.child.clone())))
    }
}

// ---------- Translation IDs ----------

struct Ids;

impl Ids {
    fn contract_mpt<D: DB>() -> TranslationId {
        TranslationId(
            MptV9::<cs9::ContractState<D>, D, NightAnnV9>::tag(),
            MptV10::<cs10::ContractState<D>, D, NightAnnV10>::tag(),
        )
    }

    fn rc_0_mpt<D: DB>() -> TranslationId {
        TranslationId(
            MptV9::<ChildRefV9<D>, D, SizeAnn>::tag(),
            MptV10::<ChildRefV10<D>, D, storage_v10::storable::SizeAnn>::tag(),
        )
    }
}

// ---------- LedgerState ----------

struct LedgerStateTl;

impl<D: DB>
    DirectTranslation<
        ledger_v9::structure::LedgerState<D>,
        ledger_v10::structure::LedgerState<D>,
        D,
    > for LedgerStateTl
{
    fn required_translations() -> Vec<TranslationId> {
        vec![Ids::contract_mpt::<D>()]
    }

    fn child_translations(
        source: &ledger_v9::structure::LedgerState<D>,
    ) -> Vec<(TranslationId, Sp<dyn Any + Send + Sync, D>)> {
        vec![(Ids::contract_mpt::<D>(), source.contract.mpt.upcast())]
    }

    fn finalize(
        source: &ledger_v9::structure::LedgerState<D>,
        _limit: &mut CostDuration,
        cache: &TranslationCache<D>,
    ) -> io::Result<Option<ledger_v10::structure::LedgerState<D>>> {
        let contract_mpt =
            try_resopt!(cache.resolve(&Ids::contract_mpt::<D>(), source.contract.mpt.as_child()));
        Ok(Some(ledger_v10::structure::LedgerState {
            network_id: source.network_id.clone(),
            parameters: recast(&source.parameters)?,
            locked_pool: source.locked_pool,
            bridge_receiving: Map {
                mpt: recast(&source.bridge_receiving.mpt)?,
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
                mpt: contract_mpt,
                key_type: PhantomData,
            },
            utxo: recast(&source.utxo)?,
            replay_protection: recast(&source.replay_protection)?,
            dust: recast(&source.dust)?,
        }))
    }
}

// ---------- ContractState ----------

/// Copies the `v2` and `v3` keys and the IR; ledger 10's `v4` slot has no v9
/// equivalent and stays empty.
fn translate_contract_operation(
    source: &cs9::ContractOperation,
) -> io::Result<cs10::ContractOperation> {
    let ir = source.ir.as_ref().map(recast).transpose()?;
    let mut op = cs10::ContractOperation::new(None, ir);
    op.v2 = source.v2.clone();
    op.v3 = source.v3.clone();
    Ok(op)
}

struct ContractStateTl;

impl<D: DB> DirectTranslation<cs9::ContractState<D>, cs10::ContractState<D>, D>
    for ContractStateTl
{
    fn required_translations() -> Vec<TranslationId> {
        vec![Ids::rc_0_mpt::<D>()]
    }
    fn child_translations(
        source: &cs9::ContractState<D>,
    ) -> Vec<(TranslationId, Sp<dyn Any + Send + Sync, D>)> {
        vec![(
            Ids::rc_0_mpt::<D>(),
            source.data.charged_keys.rc_0.mpt.upcast(),
        )]
    }
    fn finalize(
        source: &cs9::ContractState<D>,
        _limit: &mut CostDuration,
        cache: &TranslationCache<D>,
    ) -> io::Result<Option<cs10::ContractState<D>>> {
        let charged_keys = &source.data.charged_keys;
        let rc_0 =
            try_resopt!(cache.resolve(&Ids::rc_0_mpt::<D>(), charged_keys.rc_0.mpt.as_child()));
        // A contract's operation set is small, so it is rebuilt in place.
        let mut operations = HashMap::new();
        for entry in source.operations.iter() {
            let (key, op) = &*entry;
            let key: cs10::EntryPointBuf = key[..].into();
            operations = operations.insert(key, translate_contract_operation(op)?);
        }
        Ok(Some(cs10::ContractState {
            data: cs10::ChargedState {
                state: recast(&source.data.state)?,
                charged_keys: RcMapV10 {
                    rc_ge_1: Map {
                        mpt: recast(&charged_keys.rc_ge_1.mpt)?,
                        key_type: PhantomData,
                    },
                    rc_0: Map {
                        mpt: rc_0,
                        key_type: PhantomData,
                    },
                },
            },
            operations,
            maintenance_authority: recast_base(&source.maintenance_authority)?,
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
        (
            TranslationId(
                Cow::Borrowed("ledger-state[v18]"),
                Cow::Borrowed("ledger-state[v19]"),
            ),
            &DirectSpTranslation::<_, _, LedgerStateTl, _>(PhantomData),
        ),
        (
            TranslationId(
                Cow::Borrowed("contract-state[v8]"),
                Cow::Borrowed("contract-state[v9]"),
            ),
            &DirectSpTranslation::<_, _, ContractStateTl, _>(PhantomData),
        ),
        (
            TranslationId(
                Cow::Borrowed("mpt(contract-state[v8],night-annotation)"),
                Cow::Borrowed("mpt(contract-state[v9],night-annotation)"),
            ),
            &DirectSpTranslation::<
                MptV9<cs9::ContractState<D>, D, NightAnnV9>,
                MptV10<cs10::ContractState<D>, D, NightAnnV10>,
                MptTl<cs9::ContractState<D>, cs10::ContractState<D>, NightAnnV9, NightAnnV10>,
                _,
            >(PhantomData),
        ),
        (
            TranslationId(
                Cow::Borrowed("mpt-node(contract-state[v8],night-annotation)"),
                Cow::Borrowed("mpt-node(contract-state[v9],night-annotation)"),
            ),
            &DirectSpTranslation::<
                NodeV9<cs9::ContractState<D>, D, NightAnnV9>,
                NodeV10<cs10::ContractState<D>, D, NightAnnV10>,
                MptTl<cs9::ContractState<D>, cs10::ContractState<D>, NightAnnV9, NightAnnV10>,
                _,
            >(PhantomData),
        ),
        (
            TranslationId(Cow::Borrowed("childref[v1]"), Cow::Borrowed("childref[v1]")),
            &DirectSpTranslation::<_, _, ChildRefTl, _>(PhantomData),
        ),
        (
            TranslationId(
                Cow::Borrowed("mpt(childref[v1],size-annotation)"),
                Cow::Borrowed("mpt(childref[v1],size-annotation)"),
            ),
            &DirectSpTranslation::<
                MptV9<ChildRefV9<D>, D, SizeAnn>,
                MptV10<ChildRefV10<D>, D, storage_v10::storable::SizeAnn>,
                MptTl<ChildRefV9<D>, ChildRefV10<D>, SizeAnn, storage_v10::storable::SizeAnn>,
                _,
            >(PhantomData),
        ),
        (
            TranslationId(
                Cow::Borrowed("mpt-node(childref[v1],size-annotation)"),
                Cow::Borrowed("mpt-node(childref[v1],size-annotation)"),
            ),
            &DirectSpTranslation::<
                NodeV9<ChildRefV9<D>, D, SizeAnn>,
                NodeV10<ChildRefV10<D>, D, storage_v10::storable::SizeAnn>,
                MptTl<ChildRefV9<D>, ChildRefV10<D>, SizeAnn, storage_v10::storable::SizeAnn>,
                _,
            >(PhantomData),
        ),
    ];
}
