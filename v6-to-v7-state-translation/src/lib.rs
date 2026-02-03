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
    storage::{HashMap, Map, default_storage},
};

struct LedgerV6ToV7Translation;

impl LedgerV6ToV7Translation {
    fn contract_tlid<D: DB>() -> TranslationId {
        TranslationId(
            MerklePatriciaTrie::<
                onchain_state_v6::state::ContractState<D>,
                D,
                ledger_v6::annotation::NightAnn,
            >::tag(),
            MerklePatriciaTrie::<
                onchain_state_v7::state::ContractState<D>,
                D,
                ledger_v7::annotation::NightAnn,
            >::tag(),
        )
    }
}

// copied from `storage` crate, as we have to pin an older version of that crate
fn recast<A: Storable<D> + Tagged, B: Storable<D> + Tagged, D: DB>(
    a: &Sp<A, D>,
) -> io::Result<Sp<B, D>> {
    if A::tag() != B::tag() {
        return io::Result::Err(io::Error::new(io::ErrorKind::Other, "tags do not match"));
    }

    default_storage::<D>().get_lazy(&a.as_child().into())
}

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
                    .fold(AnnB::empty(), |acc, x| acc.append(&x.ann()));
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
                let ann = child.ann();
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
            merkle_patricia_trie::Node::MidBranchLeaf { ann, value, child } => {
                let value = try_resopt!(cache.resolve(&tls[0].0, tls[0].1.as_child()));
                let child: Sp<merkle_patricia_trie::Node<B, D, AnnB>, D> =
                    try_resopt!(cache.resolve(&tls[1].0, tls[1].1.as_child()));
                let ann = AnnB::from_value(&value).append(&child.ann());
                merkle_patricia_trie::Node::MidBranchLeaf { ann, value, child }
            }
        }))
    }
}

impl<D: DB>
    DirectTranslation<ledger_v6::structure::LedgerState<D>, ledger_v7::structure::LedgerState<D>, D>
    for LedgerV6ToV7Translation
{
    fn required_translations() -> Vec<TranslationId> {
        vec![Self::contract_tlid::<D>()]
    }

    fn child_translations(
        source: &ledger_v6::structure::LedgerState<D>,
    ) -> Vec<(TranslationId, Sp<dyn Any + Send + Sync, D>)> {
        vec![(Self::contract_tlid::<D>(), source.contract.mpt.upcast())]
    }

    fn finalize(
        source: &ledger_v6::structure::LedgerState<D>,
        _limit: &mut CostDuration,
        cache: &TranslationCache<D>,
    ) -> io::Result<Option<ledger_v7::structure::LedgerState<D>>> {
        let Some(contract) =
            cache.lookup(&Self::contract_tlid::<D>(), source.contract.mpt.as_child())
        else {
            return io::Result::Ok(None);
        };

        Ok(Some(ledger_v7::structure::LedgerState {
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
                mpt: contract.force_downcast(),
                key_type: PhantomData,
            },
            utxo: recast(&source.utxo)?,
            replay_protection: recast(&source.replay_protection)?,
            dust: recast(&source.dust)?,
        }))
    }
}

struct ContractStateTranslation;

impl<D: DB>
    DirectTranslation<
        onchain_state_v6::state::ContractState<D>,
        onchain_state_v7::state::ContractState<D>,
        D,
    > for ContractStateTranslation
{
    fn required_translations() -> Vec<TranslationId> {
        Vec::new()
    }
    fn child_translations(
        _source: &onchain_state_v6::state::ContractState<D>,
    ) -> Vec<(TranslationId, Sp<dyn Any + Send + Sync, D>)> {
        Vec::new()
    }
    fn finalize(
        source: &onchain_state_v6::state::ContractState<D>,
        _limit: &mut CostDuration,
        _cache: &TranslationCache<D>,
    ) -> io::Result<Option<onchain_state_v7::state::ContractState<D>>> {
        Ok(Some(onchain_state_v7::state::ContractState::<D> {
            data: recast::<
                onchain_state_v6::state::ChargedState<D>,
                onchain_state_v7::state::ChargedState<D>,
                D,
            >(&Sp::new(source.data.clone()))?
            .deref()
            .clone(),
            operations: source
                .operations
                .iter()
                .map(|sp| {
                    (
                        recast::<
                            onchain_state_v6::state::EntryPointBuf,
                            onchain_state_v7::state::EntryPointBuf,
                            D,
                        >(&sp.0)
                        .unwrap()
                        .deref()
                        .clone(),
                        onchain_state_v7::state::ContractOperation::new(None),
                    )
                })
                .collect(),
            maintenance_authority: recast::<
                onchain_state_v6::state::ContractMaintenanceAuthority,
                onchain_state_v7::state::ContractMaintenanceAuthority,
                D,
            >(&Sp::new(source.maintenance_authority.clone()))?
            .deref()
            .clone(),
            balance: HashMap(Map {
                mpt: recast(&source.balance.0.mpt)?,
                key_type: PhantomData,
            }),
        }))
    }
}

pub struct StateTranslationTable;

impl<D: DB> TranslationTable<D> for StateTranslationTable {
    const TABLE: &[(TranslationId, &dyn TypelessTranslation<D>)] = &[
        (
            TranslationId(
                Cow::Borrowed("ledger-state[v12]"),
                Cow::Borrowed("ledger-state[v13]"),
            ),
            &DirectSpTranslation::<_, _, LedgerV6ToV7Translation, _>(PhantomData),
        ),
        (
            TranslationId(
                Cow::Borrowed("contract-state[v5]"),
                Cow::Borrowed("contract-state[v6]"),
            ),
            &DirectSpTranslation::<_, _, ContractStateTranslation, _>(PhantomData),
        ),
        (
            TranslationId(
                Cow::Borrowed("mpt(contract-state[v5],night-annotation)"),
                Cow::Borrowed("mpt(contract-state[v6],night-annotation)"),
            ),
            &DirectSpTranslation::<
                MerklePatriciaTrie<
                    onchain_state_v6::state::ContractState<D>,
                    D,
                    ledger_v6::annotation::NightAnn,
                >,
                MerklePatriciaTrie<
                    onchain_state_v7::state::ContractState<D>,
                    D,
                    ledger_v7::annotation::NightAnn,
                >,
                MptTl<
                    onchain_state_v6::state::ContractState<D>,
                    onchain_state_v7::state::ContractState<D>,
                    ledger_v6::annotation::NightAnn,
                    ledger_v7::annotation::NightAnn,
                >,
                _,
            >(PhantomData),
        ),
        (
            TranslationId(
                Cow::Borrowed("mpt-node(contract-state[v5],night-annotation)"),
                Cow::Borrowed("mpt-node(contract-state[v6],night-annotation)"),
            ),
            &DirectSpTranslation::<
                merkle_patricia_trie::Node<
                    onchain_state_v6::state::ContractState<D>,
                    D,
                    ledger_v6::annotation::NightAnn,
                >,
                merkle_patricia_trie::Node<
                    onchain_state_v7::state::ContractState<D>,
                    D,
                    ledger_v7::annotation::NightAnn,
                >,
                MptTl<
                    onchain_state_v6::state::ContractState<D>,
                    onchain_state_v7::state::ContractState<D>,
                    ledger_v6::annotation::NightAnn,
                    ledger_v7::annotation::NightAnn,
                >,
                _,
            >(PhantomData),
        ),
    ];
}
