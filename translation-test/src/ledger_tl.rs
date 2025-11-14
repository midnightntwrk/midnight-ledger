use crate::mechanism::*;
use base_crypto::cost_model::{CostDuration, RunningCost};
use base_crypto::time::Timestamp;
use base_crypto::fab::AlignedValue;
use new_ledger::structure::INITIAL_TRANSACTION_COST_MODEL;
use serialize::{Deserializable, Serializable, Tagged};
use std::any::Any;
use std::borrow::Cow;
use std::io;
use std::marker::PhantomData;
use storage::Storable;
use storage::arena::{ArenaKey, Sp};
use storage::db::{DB, InMemoryDB};
use storage::delta_tracking::{RcMap, initial_write_delete_costs};
use storage::merkle_patricia_trie::{self, Annotation, MerklePatriciaTrie};
use storage::storage::{HashMap, HashSet, Map, TimeFilterMap, Array, default_storage};
use std::ops::Deref;

fn recast<A: Storable<D>, B: Storable<D> + Tagged, D: DB>(a: &Sp<A, D>) -> io::Result<Sp<B, D>> {
    default_storage::<D>().get_lazy(&a.as_child().into())
}

fn recast_from_ser<A: Serializable, B: Deserializable>(a: &A) -> io::Result<B> {
    let mut buf = Vec::new();
    a.serialize(&mut buf)?;
    B::deserialize(&mut &buf[..], 0)
}

struct ZswapStateTl;

impl<D: DB> DirectTranslation<old_zswap::ledger::State<D>, new_zswap::ledger::State<D>, D>
    for ZswapStateTl
{
    fn required_translations() -> Vec<TranslationId> {
        vec![TranslationId(
            old_transient_crypto::merkle_tree::MerkleTreeNode::<
                Option<Sp<old_coin_structure::contract::ContractAddress, D>>,
                D,
            >::tag(),
            new_transient_crypto::merkle_tree::MerkleTreeNode::<
                Option<Sp<new_coin_structure::contract::ContractAddress, D>>,
                D,
            >::tag(),
        )]
    }
    fn child_translations(
        source: &old_zswap::ledger::State<D>,
    ) -> Vec<(TranslationId, Sp<dyn Any + Send + Sync, D>)> {
        let tlids = <Self as DirectTranslation<_, _, D>>::required_translations();
        vec![(tlids[0].clone(), source.coin_coms.0.upcast())]
    }
    fn finalize(
        source: &old_zswap::ledger::State<D>,
        limit: &mut base_crypto::cost_model::CostDuration,
        cache: &TranslationCache<D>,
    ) -> std::io::Result<Option<new_zswap::ledger::State<D>>> {
        let tls = Self::child_translations(source);
        let coin_coms = new_transient_crypto::merkle_tree::MerkleTree(try_resopt!(
            cache.resolve(&tls[0].0, tls[0].1.as_child())
        ));
        let past_roots = TimeFilterMap::new().insert(
            Timestamp::from_secs(0),
            coin_coms
                .root()
                .expect("translated Merkle tree must be rehashed"),
        );
        Ok(Some(new_zswap::ledger::State {
            coin_coms_set: HashMap(Map {
                mpt: recast(&source.coin_coms_set.0.mpt)?,
                key_type: PhantomData,
            }),
            first_free: source.first_free,
            nullifiers: HashMap(Map {
                mpt: recast(&source.nullifiers.0.mpt)?,
                key_type: PhantomData,
            }),
            coin_coms,
            past_roots,
        }))
    }
}

struct LedgerStateTl;

impl<D: DB>
    DirectTranslation<
        old_ledger::structure::LedgerState<D>,
        new_ledger::structure::LedgerState<D>,
        D,
    > for LedgerStateTl
{
    fn required_translations() -> Vec<TranslationId> {
        vec![
            TranslationId(
                old_zswap::ledger::State::<D>::tag(),
                new_zswap::ledger::State::<D>::tag(),
            ),
            TranslationId(
                MerklePatriciaTrie::<
                    old_onchain_state::state::ContractState<D>,
                    D,
                    old_ledger::annotation::NightAnn,
                >::tag(),
                MerklePatriciaTrie::<
                    new_onchain_state::state::ContractState<D>,
                    D,
                    new_ledger::annotation::NightAnn,
                >::tag(),
            ),
            TranslationId(
                old_ledger::structure::UtxoState::<D>::tag(),
                new_ledger::structure::UtxoState::<D>::tag(),
            ),
            TranslationId(
                old_ledger::dust::DustState::<D>::tag(),
                new_ledger::dust::DustState::<D>::tag(),
            ),
        ]
    }
    fn child_translations(
        source: &old_ledger::structure::LedgerState<D>,
    ) -> Vec<(TranslationId, Sp<dyn Any + Send + Sync, D>)> {
        vec![
            (
                TranslationId(
                    old_zswap::ledger::State::<D>::tag(),
                    new_zswap::ledger::State::<D>::tag(),
                ),
                source.zswap.upcast(),
            ),
            (
                TranslationId(
                    MerklePatriciaTrie::<
                        old_onchain_state::state::ContractState<D>,
                        D,
                        old_ledger::annotation::NightAnn,
                    >::tag(),
                    MerklePatriciaTrie::<
                        new_onchain_state::state::ContractState<D>,
                        D,
                        new_ledger::annotation::NightAnn,
                    >::tag(),
                ),
                source.contract.mpt.upcast(),
            ),
            (
                TranslationId(
                    old_ledger::structure::UtxoState::<D>::tag(),
                    new_ledger::structure::UtxoState::<D>::tag(),
                ),
                source.utxo.upcast(),
            ),
            (
                TranslationId(
                    old_ledger::dust::DustState::<D>::tag(),
                    new_ledger::dust::DustState::<D>::tag(),
                ),
                source.dust.upcast(),
            ),
        ]
    }
    fn finalize(
        source: &old_ledger::structure::LedgerState<D>,
        limit: &mut base_crypto::cost_model::CostDuration,
        cache: &TranslationCache<D>,
    ) -> std::io::Result<Option<new_ledger::structure::LedgerState<D>>> {
        let tls = Self::child_translations(source);
        let zswap = try_resopt!(cache.resolve(&tls[0].0, tls[0].1.as_child()));
        let contract = Map {
            mpt: try_resopt!(cache.resolve(&tls[1].0, tls[1].1.as_child())),
            key_type: PhantomData,
        };
        let utxo = try_resopt!(cache.resolve(&tls[2].0, tls[2].1.as_child()));
        let dust = try_resopt!(cache.resolve(&tls[3].0, tls[3].1.as_child()));
        Ok(Some(new_ledger::structure::LedgerState {
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
            replay_protection: recast(&source.replay_protection)?,
            // actually translated
            zswap,
            contract,
            utxo,
            dust,
        }))
    }
}

struct DustStateTl;

impl<D: DB> DirectTranslation<old_ledger::dust::DustState<D>, new_ledger::dust::DustState<D>, D>
    for DustStateTl
{
    fn required_translations() -> Vec<TranslationId> {
        vec![
            TranslationId(
                old_transient_crypto::merkle_tree::MerkleTreeNode::<(), D>::tag(),
                new_transient_crypto::merkle_tree::MerkleTreeNode::<(), D>::tag(),
            ),
            TranslationId(
                old_transient_crypto::merkle_tree::MerkleTreeNode::<
                    old_ledger::dust::DustGenerationInfo,
                    D,
                >::tag(),
                new_transient_crypto::merkle_tree::MerkleTreeNode::<
                    new_ledger::dust::DustGenerationInfo,
                    D,
                >::tag(),
            ),
        ]
    }
    fn child_translations(
        source: &old_ledger::dust::DustState<D>,
    ) -> Vec<(TranslationId, Sp<dyn Any + Send + Sync, D>)> {
        let tlids = <Self as DirectTranslation<_, _, D>>::required_translations();
        vec![
            (tlids[0].clone(), source.utxo.commitments.0.upcast()),
            (
                tlids[1].clone(),
                source.generation.generating_tree.0.upcast(),
            ),
        ]
    }
    fn finalize(
        source: &old_ledger::dust::DustState<D>,
        limit: &mut CostDuration,
        cache: &TranslationCache<D>,
    ) -> io::Result<Option<new_ledger::dust::DustState<D>>> {
        let tls = Self::child_translations(source);
        let commitments = new_transient_crypto::merkle_tree::MerkleTree(try_resopt!(
            cache.resolve(&tls[0].0, tls[0].1.as_child())
        ));
        let generating_tree = new_transient_crypto::merkle_tree::MerkleTree(try_resopt!(
            cache.resolve(&tls[1].0, tls[1].1.as_child())
        ));
        let commitment_root_history = TimeFilterMap::new().insert(
            Timestamp::from_secs(0),
            commitments
                .root()
                .expect("translated Merkle tree should have root"),
        );
        let generating_tree_root_history = TimeFilterMap::new().insert(
            Timestamp::from_secs(0),
            generating_tree
                .root()
                .expect("translated Merkle tree should have root"),
        );
        Ok(Some(new_ledger::dust::DustState {
            utxo: new_ledger::dust::DustUtxoState {
                commitments,
                commitments_first_free: source.utxo.commitments_first_free,
                nullifiers: HashSet(HashMap(Map {
                    mpt: recast(&source.utxo.nullifiers.0.0.mpt)?,
                    key_type: PhantomData,
                })),
                root_history: commitment_root_history,
            },
            generation: new_ledger::dust::DustGenerationState {
                address_delegation: Map {
                    mpt: recast(&source.generation.address_delegation.mpt)?,
                    key_type: PhantomData,
                },
                generating_tree,
                generating_tree_first_free: source.generation.generating_tree_first_free,
                generating_set: HashSet(HashMap(Map {
                    mpt: recast(&source.generation.generating_set.0.0.mpt)?,
                    key_type: PhantomData,
                })),
                night_indices: HashMap(Map {
                    mpt: recast(&source.generation.night_indices.0.mpt)?,
                    key_type: PhantomData,
                }),
                root_history: generating_tree_root_history,
            },
        }))
    }
}

struct UtxoStateTl;

impl<D: DB>
    DirectTranslation<old_ledger::structure::UtxoState<D>, new_ledger::structure::UtxoState<D>, D>
    for UtxoStateTl
{
    fn required_translations() -> Vec<TranslationId> {
        vec![TranslationId(
            MerklePatriciaTrie::<
                (
                    Sp<old_ledger::structure::Utxo, D>,
                    Sp<old_ledger::structure::UtxoMeta, D>,
                ),
                D,
                old_ledger::annotation::NightAnn,
            >::tag(),
            MerklePatriciaTrie::<
                (
                    Sp<new_ledger::structure::Utxo, D>,
                    Sp<new_ledger::structure::UtxoMeta, D>,
                ),
                D,
                new_ledger::annotation::NightAnn,
            >::tag(),
        )]
    }
    fn child_translations(
        source: &old_ledger::structure::UtxoState<D>,
    ) -> Vec<(TranslationId, Sp<dyn Any + Send + Sync, D>)> {
        let tlids = <Self as DirectTranslation<_, _, D>>::required_translations();
        vec![(tlids[0].clone(), source.utxos.0.mpt.upcast())]
    }
    fn finalize(
        source: &old_ledger::structure::UtxoState<D>,
        _limit: &mut CostDuration,
        cache: &TranslationCache<D>,
    ) -> io::Result<Option<new_ledger::structure::UtxoState<D>>> {
        let tls = Self::child_translations(source);
        let utxo_mpt = try_resopt!(cache.resolve(&tls[0].0, tls[0].1.as_child()));
        Ok(Some(new_ledger::structure::UtxoState {
            utxos: HashMap(Map {
                mpt: utxo_mpt,
                key_type: PhantomData,
            }),
        }))
    }
}

struct MerkleTreeTl<A, B>(PhantomData<(A, B)>);

impl<D: DB, A: Storable<D> + Tagged, B: Storable<D> + Tagged>
    DirectTranslation<
        old_transient_crypto::merkle_tree::MerkleTreeNode<A, D>,
        new_transient_crypto::merkle_tree::MerkleTreeNode<B, D>,
        D,
    > for MerkleTreeTl<A, B>
{
    fn required_translations() -> Vec<TranslationId> {
        vec![TranslationId(
            old_transient_crypto::merkle_tree::MerkleTreeNode::<A, D>::tag(),
            new_transient_crypto::merkle_tree::MerkleTreeNode::<B, D>::tag(),
        )]
    }
    fn child_translations(
        source: &old_transient_crypto::merkle_tree::MerkleTreeNode<A, D>,
    ) -> Vec<(TranslationId, Sp<dyn Any + Send + Sync, D>)> {
        if let old_transient_crypto::merkle_tree::MerkleTreeNode::Node { left, right, .. } = source
        {
            let tlid = <Self as DirectTranslation<_, _, D>>::required_translations();
            vec![
                (tlid[0].clone(), left.upcast()),
                (tlid[0].clone(), right.upcast()),
            ]
        } else {
            vec![]
        }
    }
    fn finalize(
        source: &old_transient_crypto::merkle_tree::MerkleTreeNode<A, D>,
        limit: &mut CostDuration,
        cache: &TranslationCache<D>,
    ) -> io::Result<Option<new_transient_crypto::merkle_tree::MerkleTreeNode<B, D>>> {
        use new_transient_crypto::merkle_tree::MerkleTreeNode as NewMT;
        use old_transient_crypto::merkle_tree::MerkleTreeNode as OldMT;
        Ok(Some(match source {
            OldMT::Leaf { hash, aux } => {
                let aux_sp = Sp::new(aux.clone());
                let aux = (*recast::<A, B, D>(&aux_sp)?).clone();
                NewMT::Leaf {
                    hash: hash.clone(),
                    aux,
                }
            }
            OldMT::Collapsed { hash, height } => {
                eprintln!(
                    "attempted to translate collapsed tree. That's impossible. Not modifying."
                );
                NewMT::Collapsed {
                    hash: recast_from_ser(hash)?,
                    height: *height,
                }
            }
            OldMT::Stub { height } => NewMT::Stub { height: *height },
            OldMT::Node {
                left,
                right,
                height,
                ..
            } => {
                // Account for Poseidon hashing cost
                *limit -= INITIAL_TRANSACTION_COST_MODEL.runtime_cost_model.transient_hash;
                let tls = Self::child_translations(source);
                let left = try_resopt!(cache.resolve(&tls[0].0, tls[0].1.as_child()));
                let right = try_resopt!(cache.resolve(&tls[1].0, tls[1].1.as_child()));
                NewMT::Node {
                    left: right,
                    right: left,
                    hash: None,
                    height: *height,
                }
                .rehash()
            }
        }))
    }
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
            merkle_patricia_trie::Node::MidBranchLeaf { value, child, .. } => vec![
                (entry_tl, value.upcast()),
                (self_tl, child.upcast()),
            ],
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
                    children: new_children,
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

struct UtxoTl;

impl<D: DB>
    DirectTranslation<
        (
            Sp<old_ledger::structure::Utxo, D>,
            Sp<old_ledger::structure::UtxoMeta, D>,
        ),
        (
            Sp<new_ledger::structure::Utxo, D>,
            Sp<new_ledger::structure::UtxoMeta, D>,
        ),
    D,
    > for UtxoTl
{
    fn required_translations() -> Vec<TranslationId> {
        vec![]
    }
    fn child_translations(
        _source: &(
            Sp<old_ledger::structure::Utxo, D>,
            Sp<old_ledger::structure::UtxoMeta, D>,
        ),
    ) -> Vec<(TranslationId, Sp<dyn Any + Send + Sync, D>)> {
        vec![]
    }
    fn finalize(
        source: &(
            Sp<old_ledger::structure::Utxo, D>,
            Sp<old_ledger::structure::UtxoMeta, D>,
        ),
        _limit: &mut CostDuration,
        _cache: &TranslationCache<D>,
    ) -> io::Result<
        Option<(
            Sp<new_ledger::structure::Utxo, D>,
            Sp<new_ledger::structure::UtxoMeta, D>,
        )>,
    > {
        Ok(Some((
            Sp::new(recast_from_ser(source.0.deref())?),
            Sp::new(new_ledger::structure::UtxoMeta {
                ctime: source.1.ctime,
                source: None,
            }),
        )))
    }
}

struct KeyValueValueTl<K, A, B>(PhantomData<(K, A, B)>);

impl<K: Storable<D> + Tagged, A: Storable<D> + Tagged, B: Storable<D> + Tagged, D: DB> DirectTranslation<(Sp<K, D>, Sp<A, D>), (Sp<K, D>, Sp<B, D>), D> for KeyValueValueTl<K, A, B> {
    fn required_translations() -> Vec<TranslationId> {
        vec![TranslationId(A::tag(), B::tag())]
    }
    fn child_translations(source: &(Sp<K, D>, Sp<A, D>)) -> Vec<(TranslationId, Sp<dyn Any + Send + Sync, D>)> {
        vec![(TranslationId(A::tag(), B::tag()), source.1.upcast())]
    }
    fn finalize(
            source: &(Sp<K, D>, Sp<A, D>),
            _limit: &mut CostDuration,
            cache: &TranslationCache<D>,
        ) -> io::Result<Option<(Sp<K, D>, Sp<B, D>)>> {
        let tls = Self::child_translations(source);
        let b = try_resopt!(cache.resolve(&tls[0].0, tls[0].1.as_child()));
        Ok(Some((recast(&source.0)?, b)))
    }
}

struct ContractStateTl;

impl<D: DB> DirectTranslation<old_onchain_state::state::ContractState<D>, new_onchain_state::state::ContractState<D>, D> for ContractStateTl {
    fn required_translations() -> Vec<TranslationId> {
        vec![
            TranslationId(old_onchain_state::state::StateValue::<D>::tag(), new_onchain_state::state::StateValue::<D>::tag())
        ]
    }
    fn child_translations(source: &old_onchain_state::state::ContractState<D>) -> Vec<(TranslationId, Sp<dyn Any + Send + Sync, D>)> {
        let tlids = <Self as DirectTranslation<_, _, D>>::required_translations();
        vec![
            (tlids[0].clone(), source.data.state.upcast())
        ]
    }
    fn finalize(
            source: &old_onchain_state::state::ContractState<D>,
            _limit: &mut CostDuration,
            cache: &TranslationCache<D>,
    ) -> io::Result<Option<new_onchain_state::state::ContractState<D>>> {
        let tls = Self::child_translations(source);
        let state: Sp<new_onchain_state::state::StateValue<D>, D> = try_resopt!(cache.resolve(&tls[0].0, tls[0].1.as_child()));
        let state_hash = state.hash().into();
        let data = new_onchain_state::state::ChargedState {
            state,
            // TODO make incremental
            charged_keys: initial_write_delete_costs(&[state_hash].into_iter().collect(), |_, _| RunningCost::compute(CostDuration::from_picoseconds(1_000_000_000_000_000))).updated_charged_keys,
        };
        Ok(Some(new_onchain_state::state::ContractState {
            data,
            operations: HashMap(Map {
                mpt: recast(&source.operations.0.mpt)?,
                key_type: PhantomData,
            }),
            balance: HashMap(Map {
                mpt: recast(&source.balance.0.mpt)?,
                key_type: PhantomData,
            }),
            maintenance_authority: recast_from_ser(&source.maintenance_authority)?,
        }))
    }
}

struct StateValueTl;

impl<D: DB> DirectTranslation<old_onchain_state::state::StateValue<D>, new_onchain_state::state::StateValue<D>, D> for StateValueTl {
    fn required_translations() -> Vec<TranslationId> {
        vec![
            TranslationId(MerklePatriciaTrie::<(Sp<AlignedValue, D>, Sp<old_onchain_state::state::StateValue<D>, D>), D>::tag(), MerklePatriciaTrie::<(Sp<AlignedValue, D>, Sp<new_onchain_state::state::StateValue<D>, D>), D>::tag()),
            TranslationId(MerklePatriciaTrie::<old_onchain_state::state::StateValue<D>, D>::tag(), MerklePatriciaTrie::<new_onchain_state::state::StateValue<D>, D>::tag()),
            TranslationId(old_transient_crypto::merkle_tree::MerkleTreeNode::<(), D>::tag(), new_transient_crypto::merkle_tree::MerkleTreeNode::<(), D>::tag()),
        ]
    }
    fn child_translations(source: &old_onchain_state::state::StateValue<D>) -> Vec<(TranslationId, Sp<dyn Any + Send + Sync, D>)> {
        let tlids = <Self as DirectTranslation<_, _, D>>::required_translations();
        use old_onchain_state::state::StateValue as OldSV;
        match source {
            OldSV::Map(map) => vec![(tlids[0].clone(), map.0.mpt.upcast())],
            OldSV::Array(arr) => vec![(tlids[1].clone(), arr.0.upcast())],
            OldSV::BoundedMerkleTree(mt) => vec![(tlids[2].clone(), mt.0.upcast())],
            _ => vec![],
        }
    }
    fn finalize(
            source: &old_onchain_state::state::StateValue<D>,
            limit: &mut CostDuration,
            cache: &TranslationCache<D>,
        ) -> io::Result<Option<new_onchain_state::state::StateValue<D>>> {
        use old_onchain_state::state::StateValue as OldSV;
        use new_onchain_state::state::StateValue as NewSV;
        let tls = Self::child_translations(source);
        Ok(Some(match source {
            OldSV::Null => NewSV::Null,
            OldSV::Cell(val) => NewSV::Cell(val.clone()),
            OldSV::Map(_) => NewSV::Map(HashMap(Map {
                mpt: try_resopt!(cache.resolve(&tls[0].0, tls[0].1.as_child())),
                key_type: PhantomData,
            })),
            OldSV::Array(_) => NewSV::Array(Array(try_resopt!(cache.resolve(&tls[0].0, tls[0].1.as_child())))),
            OldSV::BoundedMerkleTree(_) => NewSV::BoundedMerkleTree(new_transient_crypto::merkle_tree::MerkleTree(try_resopt!(cache.resolve(&tls[0].0, tls[0].1.as_child())))),
            _ => unreachable!(),
        }))
    }
}

struct LedgerTlTable;

impl<D: DB> TranslationTable<D> for LedgerTlTable {
    const TABLE: &[(TranslationId, &dyn TypelessTranslation<D>)] = &[
        (
            TranslationId(
                Cow::Borrowed("ledger-state[v9]"),
                Cow::Borrowed("ledger-state[v10]"),
            ),
            &DirectSpTranslation::<_, _, LedgerStateTl, _>(PhantomData),
        ),
        (
            TranslationId(
                Cow::Borrowed("zswap-ledger-state[v4]"),
                Cow::Borrowed("zswap-ledger-state[v5]"),
            ),
            &DirectSpTranslation::<_, _, ZswapStateTl, _>(PhantomData),
        ),
        (
            TranslationId(
                Cow::Borrowed("dust-state[v1]"),
                Cow::Borrowed("dust-state[v2]"),
            ),
            &DirectSpTranslation::<_, _, DustStateTl, _>(PhantomData),
        ),
        (
            TranslationId(
                Cow::Borrowed("unshielded-utxo-state[v2]"),
                Cow::Borrowed("unshielded-utxo-state[v3]"),
            ),
            &DirectSpTranslation::<_, _, UtxoStateTl, _>(PhantomData),
        ),
        (
            TranslationId(
                Cow::Borrowed("merkle-tree-node[v1](())"),
                Cow::Borrowed("merkle-tree-node[v2](())"),
            ),
            &DirectSpTranslation::<_, _, MerkleTreeTl<(), ()>, _>(PhantomData),
        ),
        (
            TranslationId(
                Cow::Borrowed("merkle-tree-node[v1](dust-generation-info[v1])"),
                Cow::Borrowed("merkle-tree-node[v2](dust-generation-info[v1])"),
            ),
            &DirectSpTranslation::<
                _,
                _,
                MerkleTreeTl<
                    old_ledger::dust::DustGenerationInfo,
                    new_ledger::dust::DustGenerationInfo,
                >,
                _,
            >(PhantomData),
        ),
        (
            TranslationId(
                Cow::Borrowed("merkle-tree-node[v1](option(contract-address[v2]))"),
                Cow::Borrowed("merkle-tree-node[v2](option(contract-address[v2]))"),
            ),
            &DirectSpTranslation::<
                _,
                _,
                MerkleTreeTl<
                    Option<Sp<old_coin_structure::contract::ContractAddress, D>>,
                    Option<Sp<new_coin_structure::contract::ContractAddress, D>>,
                >,
                _,
            >(PhantomData),
        ),
        (
            TranslationId(
                Cow::Borrowed("mpt(contract-state[v4],night-annotation)"),
                Cow::Borrowed("mpt(contract-state[v5],night-annotation)"),
            ),
            &DirectSpTranslation::<
                MerklePatriciaTrie<_, _, _>,
                _,
                MptTl<
                    old_onchain_state::state::ContractState<D>,
                    new_onchain_state::state::ContractState<D>,
                    old_ledger::annotation::NightAnn,
                    new_ledger::annotation::NightAnn,
                >,
                _,
            >(PhantomData),
        ),
        (
            TranslationId(
                Cow::Borrowed("mpt-node(contract-state[v4],night-annotation)"),
                Cow::Borrowed("mpt-node(contract-state[v5],night-annotation)"),
            ),
            &DirectSpTranslation::<
                merkle_patricia_trie::Node<_, _, _>,
                _,
                MptTl<
                    old_onchain_state::state::ContractState<D>,
                    new_onchain_state::state::ContractState<D>,
                    old_ledger::annotation::NightAnn,
                    new_ledger::annotation::NightAnn,
                >,
                _,
            >(PhantomData),
        ),
        (
            TranslationId(
                Cow::Borrowed("mpt((unshielded-utxo[v1],utxo-metadata[v1]),night-annotation)"),
                Cow::Borrowed("mpt((unshielded-utxo[v1],utxo-metadata[v2]),night-annotation)"),
            ),
            &DirectSpTranslation::<
                MerklePatriciaTrie<_, _, _>,
                _,
                MptTl<
                    (
                        Sp<old_ledger::structure::Utxo, D>,
                        Sp<old_ledger::structure::UtxoMeta, D>,
                    ),
                    (
                        Sp<new_ledger::structure::Utxo, D>,
                        Sp<new_ledger::structure::UtxoMeta, D>,
                    ),
                    old_ledger::annotation::NightAnn,
                    new_ledger::annotation::NightAnn,
                >,
                _,
            >(PhantomData),
        ),
        (
            TranslationId(
                Cow::Borrowed("mpt-node((unshielded-utxo[v1],utxo-metadata[v1]),night-annotation)"),
                Cow::Borrowed("mpt-node((unshielded-utxo[v1],utxo-metadata[v2]),night-annotation)"),
            ),
            &DirectSpTranslation::<
                merkle_patricia_trie::Node<_, _, _>,
                _,
                MptTl<
                    (
                        Sp<old_ledger::structure::Utxo, D>,
                        Sp<old_ledger::structure::UtxoMeta, D>,
                    ),
                    (
                        Sp<new_ledger::structure::Utxo, D>,
                        Sp<new_ledger::structure::UtxoMeta, D>,
                    ),
                    old_ledger::annotation::NightAnn,
                    new_ledger::annotation::NightAnn,
                >,
                _,
            >(PhantomData),
        ),
        (
            TranslationId(
                Cow::Borrowed("(unshielded-utxo[v1],utxo-metadata[v1])"),
                Cow::Borrowed("(unshielded-utxo[v1],utxo-metadata[v2])"),
            ),
            &DirectSpTranslation::<_, _, UtxoTl, _>(PhantomData),
        ),
        (
            TranslationId(
                Cow::Borrowed("contract-state[v4]"),
                Cow::Borrowed("contract-state[v5]"),
            ),
            &DirectSpTranslation::<_, _, ContractStateTl, _>(PhantomData),
        ),
        (
            TranslationId(
                Cow::Borrowed("impact-state-value[v2]"),
                Cow::Borrowed("impact-state-value[v3]"),
            ),
            &DirectSpTranslation::<_, _, StateValueTl, _>(PhantomData),
        ),
        (
            TranslationId(
                Cow::Borrowed("mpt(impact-state-value[v2],size-annotation)"),
                Cow::Borrowed("mpt(impact-state-value[v3],size-annotation)"),
            ),
            &DirectSpTranslation::<
                MerklePatriciaTrie<_, _, _>,
                _,
                MptTl<
                    old_onchain_state::state::StateValue<D>,
                    new_onchain_state::state::StateValue<D>,
                    storage::storable::SizeAnn,
                    storage::storable::SizeAnn,
                >,
                _,
            >(PhantomData),
        ),
        (
            TranslationId(
                Cow::Borrowed("mpt-node(impact-state-value[v2],size-annotation)"),
                Cow::Borrowed("mpt-node(impact-state-value[v3],size-annotation)"),
            ),
            &DirectSpTranslation::<
                merkle_patricia_trie::Node<_, _, _>,
                _,
                MptTl<
                    old_onchain_state::state::StateValue<D>,
                    new_onchain_state::state::StateValue<D>,
                    storage::storable::SizeAnn,
                    storage::storable::SizeAnn,
                >,
                _,
            >(PhantomData),
        ),
        (
            TranslationId(
                Cow::Borrowed("mpt((fab-aligned-value[v1],impact-state-value[v2]),size-annotation)"),
                Cow::Borrowed("mpt((fab-aligned-value[v1],impact-state-value[v3]),size-annotation)"),
            ),
            &DirectSpTranslation::<
                MerklePatriciaTrie<_, _, _>,
                _,
                MptTl<
                    (Sp<AlignedValue, D>, Sp<old_onchain_state::state::StateValue<D>, D>),
                    (Sp<AlignedValue, D>, Sp<new_onchain_state::state::StateValue<D>, D>),
                    storage::storable::SizeAnn,
                    storage::storable::SizeAnn,
                >,
                _,
            >(PhantomData),
        ),
        (
            TranslationId(
                Cow::Borrowed("mpt-node((fab-aligned-value[v1],impact-state-value[v2]),size-annotation)"),
                Cow::Borrowed("mpt-node((fab-aligned-value[v1],impact-state-value[v3]),size-annotation)"),
            ),
            &DirectSpTranslation::<
                merkle_patricia_trie::Node<_, _, _>,
                _,
                MptTl<
                    (Sp<AlignedValue, D>, Sp<old_onchain_state::state::StateValue<D>, D>),
                    (Sp<AlignedValue, D>, Sp<new_onchain_state::state::StateValue<D>, D>),
                    storage::storable::SizeAnn,
                    storage::storable::SizeAnn,
                >,
                _,
            >(PhantomData),
        ),
        (
            TranslationId(
                Cow::Borrowed("(fab-aligned-value[v1],impact-state-value[v2])"),
                Cow::Borrowed("(fab-aligned-value[v1],impact-state-value[v3])"),
            ),
            &DirectSpTranslation::<
                _,
                _,
                KeyValueValueTl<
                    AlignedValue,
                    old_onchain_state::state::StateValue<D>,
                    new_onchain_state::state::StateValue<D>,
                >,
                _,
            >(PhantomData),
        ),
    ];
}

#[cfg(test)]
mod tests {
    use old_transient_crypto::commitment::Pedersen;
    use storage::{db::{InMemoryDB, ParityDb}, storage::set_default_storage, Storage};

    use crate::{mechanism::*, TestDb};
    use storage::arena::Sp;
    use base_crypto::cost_model::CostDuration;
    use super::LedgerTlTable;
    use rand::Rng;
    use std::ops::Deref;

    #[test]
    fn test_test_table_closed() {
        <LedgerTlTable as TranslationTable<InMemoryDB>>::assert_closure();
    }

    fn mk_ledger(n: usize) -> old_ledger::structure::LedgerState<TestDb> {
        let mut state = old_ledger::structure::LedgerState::new("local-test");
        let mut rng = rand::rngs::OsRng;
        state.utxo = Sp::new((0..n).fold(state.utxo.deref().clone(), |utxo, _| utxo.insert(old_ledger::structure::Utxo {
            intent_hash: rng.r#gen(),
            output_no: rng.r#gen(),
            owner: rng.r#gen(),
            type_: rng.r#gen(),
            value: rng.r#gen(),
        }, old_ledger::structure::UtxoMeta {
            ctime: rng.r#gen(),
        })));
        state.zswap = Sp::new((0..n).fold(state.zswap.deref().clone(), |zswap, n| {
            let offer = old_zswap::Offer { inputs: vec![].into(), outputs: vec![old_zswap::Output {
                ciphertext: None,
                coin_com: rng.r#gen(),
                contract_address: None,
                value_commitment: Pedersen(rng.r#gen()),
                proof: (),
            }].into(), transient: vec![].into(), deltas: vec![].into(), };
            state.zswap.deref().try_apply(&offer, None).unwrap().0
        }));
        state
    }

    #[test]
    fn test_ledger_tl() {
        set_default_storage::<TestDb>(|| Storage::new(1024, ParityDb::open("test-db".as_ref()))).unwrap();
        let t0 = std::time::Instant::now();
        let n = 100_000;
        let mut before = Sp::<_, TestDb>::new(mk_ledger(n));
        dbg!(before.serialize_to_node_list().nodes.len());
        before.persist();
        before.unload();
        let t1 = std::time::Instant::now();
        let mut tl_state = Sp::new(TypedTranslationState::<
            old_ledger::structure::LedgerState<TestDb>,
            new_ledger::structure::LedgerState<TestDb>,
            LedgerTlTable,
            TestDb,
        >::start(before)
        .unwrap());
        tl_state.persist();
        // 350ms
        let per_block_cost = CostDuration::from_picoseconds(350_000_000_000);
        let mut i = 1;
        while tl_state.result().unwrap().is_none() {
            let tb0 = std::time::Instant::now();
            let mut new_tl_state = Sp::new(tl_state.run(per_block_cost).unwrap());
            let tb1 = std::time::Instant::now();
            eprintln!("block {i} processed in {:?} (target: 350ms)", tb1 - tb0);
            i += 1;
            new_tl_state.persist();
            tl_state.unpersist();
            tl_state = new_tl_state;
            tl_state.unload();
        }
        let _after = tl_state.result().unwrap().unwrap();
        eprintln!("done");
        let tfin0 = std::time::Instant::now();
        drop(_after);
        drop(tl_state);
        let tfin1 = std::time::Instant::now();
        let dt0 = tfin1 - t0;
        let dt1 = tfin0 - t0;
        let dt2 = tfin1 - t1;
        let dt3 = tfin0 - t1;
        let m = 2 * n;
        eprintln!("took {dt0:?} for {m} items ({} items per second) [incl construction, incl drop]", m as f64 / dt0.as_secs_f64());
        eprintln!("took {dt1:?} for {m} items ({} items per second) [incl construction, excl drop]", m as f64 / dt1.as_secs_f64());
        eprintln!("took {dt2:?} for {m} items ({} items per second) [excl construction, incl drop]", m as f64 / dt2.as_secs_f64());
        eprintln!("took {dt3:?} for {m} items ({} items per second) [excl construction, excl drop]", m as f64 / dt3.as_secs_f64());
        dbg!(&TUPDATE);
        dbg!(&TPROCESS);
        dbg!(&TDEP);
        dbg!(&TFIN);
        dbg!(&NPROC);
        dbg!(TUPDATE.load(std::sync::atomic::Ordering::SeqCst) as f64 / NPROC.load(std::sync::atomic::Ordering::SeqCst) as f64);
        dbg!(TPROCESS.load(std::sync::atomic::Ordering::SeqCst) as f64 / NPROC.load(std::sync::atomic::Ordering::SeqCst) as f64);
    }
}
