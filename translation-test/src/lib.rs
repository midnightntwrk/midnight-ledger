use base_crypto::cost_model::CostDuration;
use derive_where::derive_where;
use serialize::{Deserializable, Serializable, Tagged};
use std::borrow::Cow;
use std::io;
use std::marker::PhantomData;
use storage::Storable;
use storage::arena::{ArenaKey, BackendLoader, Sp};
use storage::db::{DB, InMemoryDB};
use storage::merkle_patricia_trie::{self, Annotation, MerklePatriciaTrie, Monoid, Semigroup};
use storage::storable::{Loader, SizeAnn};
use storage::storage::{HashMap, Map, default_storage};

pub mod mechanism;

//mod ledger_tl;

use mechanism::*;

#[derive(Storable)]
#[derive_where(Clone, Debug)]
#[storable(db = D)]
#[tag = "foo"]
pub struct Foo<D: DB> {
    foo: HashMap<u64, FooEntry, D>,
    #[storable(child)]
    baz: Sp<Baz<D>, D>,
}

#[derive(Storable)]
#[derive_where(Clone, Debug)]
#[storable(db = D)]
#[tag = "bar"]
pub struct Bar<D: DB> {
    bar: HashMap<u64, BarEntry, D>,
    #[storable(child)]
    baz: Sp<Baz<D>, D>,
}

#[derive(Serializable, Storable, Clone, Debug)]
#[storable(base)]
#[tag = "foo-entry"]
pub struct FooEntry(u64);

#[derive(Serializable, Storable, Clone, Debug)]
#[storable(base)]
#[tag = "bar-entry"]
pub struct BarEntry(u64);

#[derive(Storable)]
#[derive_where(Clone, Debug)]
#[storable(db = D)]
#[tag = "baz"]
pub struct Baz<D: DB> {
    baz: HashMap<u64, (), D>,
}

mod lr {
    use super::*;
    #[derive(Storable)]
    #[derive_where(Clone, Debug)]
    #[storable(db = D)]
    #[tag = "nesty-lr"]
    pub enum Nesty<D: DB> {
        Empty,
        Node(u32, Sp<Nesty<D>, D>, Sp<Nesty<D>, D>),
    }
}

mod rl {
    use super::*;
    #[derive(Storable)]
    #[derive_where(Clone, Debug)]
    #[storable(db = D)]
    #[tag = "nesty-rl"]
    pub enum Nesty<D: DB> {
        Empty,
        Node(
            u32,
            #[storable(child)] Sp<Nesty<D>, D>,
            #[storable(child)] Sp<Nesty<D>, D>,
        ),
    }
}

struct NestyLrToRlTranslation;

impl<D: DB> DirectTranslation<lr::Nesty<D>, rl::Nesty<D>, D> for NestyLrToRlTranslation {
    fn required_translations() -> Vec<TranslationId> {
        eprintln!("nesty required_translations 1");
        let res = vec![TranslationId(lr::Nesty::<D>::tag(), rl::Nesty::<D>::tag())];
        eprintln!("nesty required_translations 2");
        res
    }
    fn child_translations(source: &lr::Nesty<D>) -> Vec<(TranslationId, RawNode<D>)> {
        eprintln!("nesty child_translations 1");
        let tlid = || TranslationId(lr::Nesty::<D>::tag(), rl::Nesty::<D>::tag());
        eprintln!("nesty child_translations 2");
        let res = match source {
            lr::Nesty::Empty => vec![],
            lr::Nesty::Node(_, a, b) => vec![(tlid(), a.as_child()), (tlid(), b.as_child())],
        };
        eprintln!("nesty child_translations 3");
        res
    }
    fn finalize(
        source: &lr::Nesty<D>,
        limit: &mut CostDuration,
        cache: &TranslationCache<D>,
    ) -> io::Result<Option<rl::Nesty<D>>> {
        eprintln!("nesty finalise 1");
        let tlid = TranslationId(lr::Nesty::<D>::tag(), rl::Nesty::<D>::tag());
        eprintln!("nesty finalise 2");
        let storage = default_storage::<D>();
        eprintln!("nesty finalise 3");
        // TODO: adjust limit
        match source {
            lr::Nesty::Empty => Ok(Some(rl::Nesty::Empty)),
            lr::Nesty::Node(n, a, b) => {
                eprintln!("nesty finalise 3a");
                let Some(atrans) = cache.lookup(&tlid, a.as_child()) else {
                    return Ok(None);
                };
                eprintln!("nesty finalise 3b");
                let Some(btrans) = cache.lookup(&tlid, b.as_child()) else {
                    return Ok(None);
                };
                eprintln!("nesty finalise 3c");
                let res = Ok(Some(rl::Nesty::Node(
                    *n,
                    storage
                        .get_lazy(&btrans.child.clone().into())
                        .expect("translated node must be in storage"),
                    storage
                        .get_lazy(&atrans.child.clone().into())
                        .expect("translated node must be in storage"),
                )));
                eprintln!("nesty finalise 3d");
                res
            }
        }
    }
}

struct FooToBarTranslation;

impl<D: DB> DirectTranslation<Foo<D>, Bar<D>, D> for FooToBarTranslation {
    fn required_translations() -> Vec<TranslationId> {
        vec![TranslationId(
            MerklePatriciaTrie::<(Sp<u64, D>, Sp<FooEntry, D>), D, SizeAnn>::tag(),
            MerklePatriciaTrie::<(Sp<u64, D>, Sp<BarEntry, D>), D, SizeAnn>::tag(),
        )]
    }
    fn child_translations(source: &Foo<D>) -> Vec<(TranslationId, RawNode<D>)> {
        vec![
            // HashMap<u64, FooEntry, D> -> HashMap<u64, BarEntry, D>
            // => Map(ArenaKey<D::Hasher>, (Sp<u64, D>, Sp<FooEntry, D>)) -> Map(ArenaKey<D::Hasher>, (Sp<u64, D>, Sp<BarEntry, D>)) ->
            // => Sp<MerklePatriciaTrie<(Sp<u64, D>, Sp<FooEntry, D>), SizeAnn>, D> -> Sp<MerklePatriciaTrie<(Sp<u64, D>, Sp<BarEntry, D>), SizeAnn>, D>
            (
                TranslationId(
                    MerklePatriciaTrie::<(Sp<u64, D>, Sp<FooEntry, D>), D, SizeAnn>::tag(),
                    MerklePatriciaTrie::<(Sp<u64, D>, Sp<BarEntry, D>), D, SizeAnn>::tag(),
                ),
                source.foo.0.mpt.as_child(),
            ),
        ]
    }
    fn finalize(
        source: &Foo<D>,
        _limit: &mut CostDuration,
        cache: &TranslationCache<D>,
    ) -> io::Result<Option<Bar<D>>> {
        let foo_tlid = TranslationId(
            MerklePatriciaTrie::<(Sp<u64, D>, Sp<FooEntry, D>), D, SizeAnn>::tag(),
            MerklePatriciaTrie::<(Sp<u64, D>, Sp<BarEntry, D>), D, SizeAnn>::tag(),
        );
        let Some(footl) = cache.lookup(&foo_tlid, source.foo.0.mpt.as_child()) else {
            return Ok(None);
        };
        let bar = HashMap(Map {
            mpt: default_storage::<D>().get_lazy(&footl.child.clone().into())?,
            key_type: PhantomData,
        });
        Ok(Some(Bar {
            bar,
            baz: source.baz.clone(),
        }))
    }
}

struct MptFooToBarTranslation;

impl<D: DB>
    DirectTranslation<
        MerklePatriciaTrie<(Sp<u64, D>, Sp<FooEntry, D>), D>,
        MerklePatriciaTrie<(Sp<u64, D>, Sp<BarEntry, D>), D>,
        D,
    > for MptFooToBarTranslation
{
    fn required_translations() -> Vec<TranslationId> {
        vec![TranslationId(
            merkle_patricia_trie::Node::<(Sp<u64, D>, Sp<FooEntry, D>), D>::tag(),
            merkle_patricia_trie::Node::<(Sp<u64, D>, Sp<BarEntry, D>), D>::tag(),
        )]
    }
    fn child_translations(
        source: &MerklePatriciaTrie<(Sp<u64, D>, Sp<FooEntry, D>), D>,
    ) -> Vec<(TranslationId, RawNode<D>)> {
        vec![(
            TranslationId(
                merkle_patricia_trie::Node::<(Sp<u64, D>, Sp<FooEntry, D>), D>::tag(),
                merkle_patricia_trie::Node::<(Sp<u64, D>, Sp<BarEntry, D>), D>::tag(),
            ),
            source.0.as_child(),
        )]
    }
    fn finalize(
        source: &MerklePatriciaTrie<(Sp<u64, D>, Sp<FooEntry, D>), D>,
        _limit: &mut CostDuration,
        cache: &TranslationCache<D>,
    ) -> io::Result<Option<MerklePatriciaTrie<(Sp<u64, D>, Sp<BarEntry, D>), D>>> {
        let tlid = TranslationId(
            merkle_patricia_trie::Node::<(Sp<u64, D>, Sp<FooEntry, D>), D>::tag(),
            merkle_patricia_trie::Node::<(Sp<u64, D>, Sp<BarEntry, D>), D>::tag(),
        );
        let Some(tl) = cache.lookup(&tlid, source.0.as_child()) else {
            return Ok(None);
        };
        Ok(Some(MerklePatriciaTrie(
            default_storage::<D>().get_lazy(&tl.child.clone().into())?,
        )))
    }
}

struct MptNodeFooToBarTranslation;

impl<D: DB>
    DirectTranslation<
        merkle_patricia_trie::Node<(Sp<u64, D>, Sp<FooEntry, D>), D>,
        merkle_patricia_trie::Node<(Sp<u64, D>, Sp<BarEntry, D>), D>,
        D,
    > for MptNodeFooToBarTranslation
{
    fn required_translations() -> Vec<TranslationId> {
        let entry_tl = TranslationId(
            <(Sp<u64, D>, Sp<FooEntry, D>)>::tag(),
            <(Sp<u64, D>, Sp<BarEntry, D>)>::tag(),
        );
        let self_tl = TranslationId(
            merkle_patricia_trie::Node::<(Sp<u64, D>, Sp<FooEntry, D>), D>::tag(),
            merkle_patricia_trie::Node::<(Sp<u64, D>, Sp<BarEntry, D>), D>::tag(),
        );
        vec![entry_tl, self_tl]
    }
    fn child_translations(
        source: &merkle_patricia_trie::Node<(Sp<u64, D>, Sp<FooEntry, D>), D>,
    ) -> Vec<(TranslationId, RawNode<D>)> {
        let entry_tl = TranslationId(
            <(Sp<u64, D>, Sp<FooEntry, D>)>::tag(),
            <(Sp<u64, D>, Sp<BarEntry, D>)>::tag(),
        );
        let self_tl = TranslationId(
            merkle_patricia_trie::Node::<(Sp<u64, D>, Sp<FooEntry, D>), D>::tag(),
            merkle_patricia_trie::Node::<(Sp<u64, D>, Sp<BarEntry, D>), D>::tag(),
        );
        match source {
            merkle_patricia_trie::Node::Empty => vec![],
            merkle_patricia_trie::Node::Branch { children, .. } => children
                .iter()
                .map(|child| (self_tl.clone(), child.as_child()))
                .collect(),
            merkle_patricia_trie::Node::Extension { child, .. } => {
                vec![(self_tl, child.as_child())]
            }
            merkle_patricia_trie::Node::MidBranchLeaf { value, child, .. } => vec![
                (entry_tl, value.as_child()),
                (self_tl, child.as_child()),
            ],
            merkle_patricia_trie::Node::Leaf { value, .. } => vec![(entry_tl, value.as_child())],
        }
    }
    fn finalize(
        source: &merkle_patricia_trie::Node<(Sp<u64, D>, Sp<FooEntry, D>), D>,
        _limit: &mut CostDuration,
        cache: &TranslationCache<D>,
    ) -> io::Result<Option<merkle_patricia_trie::Node<(Sp<u64, D>, Sp<BarEntry, D>), D>>> {
        let entry_tl = TranslationId(
            <(Sp<u64, D>, Sp<FooEntry, D>)>::tag(),
            <(Sp<u64, D>, Sp<BarEntry, D>)>::tag(),
        );
        let self_tl = TranslationId(
            merkle_patricia_trie::Node::<(Sp<u64, D>, Sp<FooEntry, D>), D>::tag(),
            merkle_patricia_trie::Node::<(Sp<u64, D>, Sp<BarEntry, D>), D>::tag(),
        );
        Ok(Some(match source {
            merkle_patricia_trie::Node::Empty => merkle_patricia_trie::Node::Empty,
            merkle_patricia_trie::Node::Branch { ann, children } => {
                let mut new_children =
                    core::array::from_fn(|_| Sp::new(merkle_patricia_trie::Node::Empty));
                for (child, new_child) in children.iter().zip(new_children.iter_mut()) {
                    let Some(entry) = cache.lookup(&self_tl, child.as_child()) else {
                        return Ok(None);
                    };
                    *new_child = default_storage::<D>().get_lazy(&entry.child.clone().into())?;
                }
                let ann = new_children
                    .iter()
                    .fold(SizeAnn::empty(), |acc, x| acc.append(&x.ann()));
                merkle_patricia_trie::Node::Branch {
                    ann,
                    children: new_children,
                }
            }
            merkle_patricia_trie::Node::Extension {
                ann,
                compressed_path,
                child,
            } => {
                let Some(entry) = cache.lookup(&self_tl, child.as_child()) else {
                    return Ok(None);
                };
                let child: Sp<merkle_patricia_trie::Node<(Sp<u64, D>, Sp<BarEntry, D>), D>, D> =
                    default_storage::<D>().get_lazy(&entry.child.clone().into())?;
                let ann = child.ann();
                merkle_patricia_trie::Node::Extension {
                    ann,
                    compressed_path: compressed_path.clone(),
                    child,
                }
            }
            merkle_patricia_trie::Node::Leaf { ann, value } => {
                let Some(entry) = cache.lookup(&entry_tl, value.as_child()) else {
                    return Ok(None);
                };
                let value: Sp<(Sp<u64, D>, Sp<BarEntry, D>), D> =
                    default_storage::<D>().get_lazy(&entry.child.clone().into())?;
                let ann = SizeAnn::from_value(&value);
                merkle_patricia_trie::Node::Leaf { ann, value }
            }
            merkle_patricia_trie::Node::MidBranchLeaf { ann, value, child } => {
                let Some(value_entry) = cache.lookup(&entry_tl, value.as_child()) else {
                    return Ok(None);
                };
                let Some(child_entry) = cache.lookup(&self_tl, child.as_child()) else {
                    return Ok(None);
                };
                let value: Sp<(Sp<u64, D>, Sp<BarEntry, D>), D> =
                    default_storage::<D>().get_lazy(&value_entry.child.clone().into())?;
                let child: Sp<merkle_patricia_trie::Node<(Sp<u64, D>, Sp<BarEntry, D>), D>, D> =
                    default_storage::<D>().get_lazy(&child_entry.child.clone().into())?;
                let ann = SizeAnn::from_value(&value).append(&child.ann());
                merkle_patricia_trie::Node::MidBranchLeaf { ann, value, child }
            }
        }))
    }
}

struct FooEntryToBarEntryTranslation;

impl<D: DB> DirectTranslation<(Sp<u64, D>, Sp<FooEntry, D>), (Sp<u64, D>, Sp<BarEntry, D>), D>
    for FooEntryToBarEntryTranslation
{
    fn required_translations() -> Vec<TranslationId> {
        vec![]
    }
    fn child_translations(
        source: &(Sp<u64, D>, Sp<FooEntry, D>),
    ) -> Vec<(TranslationId, RawNode<D>)> {
        vec![]
    }
    fn finalize(
        source: &(Sp<u64, D>, Sp<FooEntry, D>),
        _limit: &mut CostDuration,
        _cache: &TranslationCache<D>,
    ) -> io::Result<Option<(Sp<u64, D>, Sp<BarEntry, D>)>> {
        Ok(Some((
            source.0.clone(),
            Sp::new(BarEntry(source.1.0.wrapping_mul(42))),
        )))
    }
}

struct TestTable;

impl<D: DB> TranslationTable<D> for TestTable {
    const TABLE: &[(TranslationId, &dyn TypelessTranslation<D>)] = &[
        (
            TranslationId(Cow::Borrowed("nesty-lr"), Cow::Borrowed("nesty-rl")),
            &DirectSpTranslation::<_, _, NestyLrToRlTranslation, _>(PhantomData),
        ),
        (
            TranslationId(Cow::Borrowed("foo"), Cow::Borrowed("bar")),
            &DirectSpTranslation::<_, _, FooToBarTranslation, _>(PhantomData),
        ),
        (
            TranslationId(
                Cow::Borrowed("mpt((u64,foo-entry),size-annotation)"),
                Cow::Borrowed("mpt((u64,bar-entry),size-annotation)"),
            ),
            &DirectSpTranslation::<_, _, MptFooToBarTranslation, _>(PhantomData),
        ),
        (
            TranslationId(
                Cow::Borrowed("mpt-node((u64,foo-entry),size-annotation)"),
                Cow::Borrowed("mpt-node((u64,bar-entry),size-annotation)"),
            ),
            &DirectSpTranslation::<_, _, MptNodeFooToBarTranslation, _>(PhantomData),
        ),
        (
            TranslationId(
                Cow::Borrowed("(u64,foo-entry)"),
                Cow::Borrowed("(u64,bar-entry)"),
            ),
            &DirectSpTranslation::<_, _, FooEntryToBarEntryTranslation, _>(PhantomData),
        ),
    ];
}

#[cfg(test)]
mod tests {
    use super::*;
    use serialize::Tagged;
    use storage::{arena::Sp, db::InMemoryDB};

    fn mk_nesty(depth: usize, offset: u32) -> Sp<lr::Nesty<InMemoryDB>> {
        if depth == 0 {
            return Sp::new(lr::Nesty::Empty);
        }
        let left = mk_nesty(depth - 1, offset);
        let right = mk_nesty(depth - 1, offset + 1 << depth);
        Sp::new(lr::Nesty::Node(offset, left, right))
    }

    fn mk_foo(depth: usize) -> Sp<Foo<InMemoryDB>> {
        let baz = Sp::new(Baz {
            baz: (0..(1 << depth)).map(|i| (i * 4, ())).collect(),
        });
        let foo = (0..(1 << depth)).map(|i| (i * 8, FooEntry(i))).collect();
        Sp::new(Foo { foo, baz })
    }

    #[test]
    fn test_nesty_tl() {
        let before = mk_nesty(10, 0);
        let tl_state = TypedTranslationState::<
            lr::Nesty<InMemoryDB>,
            rl::Nesty<InMemoryDB>,
            TestTable,
            InMemoryDB,
        >::start(before)
        .unwrap();
        let cost = CostDuration::from_picoseconds(1_000_000_000_000);
        let Either::Right(_after) = tl_state.run(cost).unwrap() else {
            panic!("didn't finish");
        };
    }

    #[test]
    fn test_foo_tl() {
        let before = mk_foo(10);
        let tl_state = TypedTranslationState::<
            Foo<InMemoryDB>,
            Bar<InMemoryDB>,
            TestTable,
            InMemoryDB,
        >::start(before)
        .unwrap();
        let cost = CostDuration::from_picoseconds(1_000_000_000_000);
        let Either::Right(_after) = tl_state.run(cost).unwrap() else {
            panic!("didn't finish");
        };
    }

    #[test]
    fn test_test_table_closed() {
        <TestTable as TranslationTable<InMemoryDB>>::assert_closure();
    }
}
