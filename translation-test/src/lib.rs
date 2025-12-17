use base_crypto::cost_model::CostDuration;
use derive_where::derive_where;
use serialize::{Deserializable, Serializable, Tagged};
use std::any::Any;
use std::borrow::Cow;
use std::io;
use std::marker::PhantomData;
use storage::Storable;
use storage::arena::{ArenaKey, BackendLoader, Sp};
use storage::db::{DB, InMemoryDB, ParityDb};
use storage::merkle_patricia_trie::{self, Annotation, MerklePatriciaTrie, Monoid, Semigroup};
use storage::storable::{Loader, SizeAnn};
use storage::storage::{HashMap, Map, default_storage};

pub mod mechanism;

// mod ledger_tl;

use mechanism::*;

type TestDb = ParityDb;

#[derive(Storable)]
#[derive_where(Clone, Debug)]
#[storable(db = D)]
#[tag = "foo"]
pub struct Foo<D: DB> {
    foo: Map<u64, FooEntry, D>,
    #[storable(child)]
    baz: Sp<Baz<D>, D>,
}

#[derive(Storable)]
#[derive_where(Clone, Debug)]
#[storable(db = D)]
#[tag = "bar"]
pub struct Bar<D: DB> {
    bar: Map<u64, BarEntry, D>,
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
        Node(
            u32,
            #[storable(child)] Sp<Nesty<D>, D>,
            #[storable(child)] Sp<Nesty<D>, D>,
        ),
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
        vec![TranslationId(lr::Nesty::<D>::tag(), rl::Nesty::<D>::tag())]
    }
    fn child_translations(
        source: &lr::Nesty<D>,
    ) -> Vec<(TranslationId, Sp<dyn Any + Send + Sync, D>)> {
        let tlid = || TranslationId(lr::Nesty::<D>::tag(), rl::Nesty::<D>::tag());
        match source {
            lr::Nesty::Empty => vec![],
            lr::Nesty::Node(_, a, b) => vec![(tlid(), a.upcast()), (tlid(), b.upcast())],
        }
    }
    fn finalize(
        source: &lr::Nesty<D>,
        _limit: &mut CostDuration,
        cache: &TranslationCache<D>,
    ) -> io::Result<Option<rl::Nesty<D>>> {
        let tlid = TranslationId(lr::Nesty::<D>::tag(), rl::Nesty::<D>::tag());
        match source {
            lr::Nesty::Empty => Ok(Some(rl::Nesty::Empty)),
            lr::Nesty::Node(n, a, b) => {
                let Some(atrans) = cache.lookup(&tlid, a.as_child()) else {
                    return Ok(None);
                };
                let Some(btrans) = cache.lookup(&tlid, b.as_child()) else {
                    return Ok(None);
                };
                let res = Ok(Some(rl::Nesty::Node(
                    *n,
                    btrans.force_downcast(),
                    atrans.force_downcast(),
                )));
                res
            }
        }
    }
}

struct FooToBarTranslation;

impl<D: DB> DirectTranslation<Foo<D>, Bar<D>, D> for FooToBarTranslation {
    fn required_translations() -> Vec<TranslationId> {
        vec![TranslationId(
            MerklePatriciaTrie::<FooEntry, D, SizeAnn>::tag(),
            MerklePatriciaTrie::<BarEntry, D, SizeAnn>::tag(),
        )]
    }
    fn child_translations(source: &Foo<D>) -> Vec<(TranslationId, Sp<dyn Any + Send + Sync, D>)> {
        vec![
            // HashMap<u64, FooEntry, D> -> HashMap<u64, BarEntry, D>
            // => Map(ArenaKey<D::Hasher>, (Sp<u64, D>, Sp<FooEntry, D>)) -> Map(ArenaKey<D::Hasher>, (Sp<u64, D>, Sp<BarEntry, D>)) ->
            // => Sp<MerklePatriciaTrie<(Sp<u64, D>, Sp<FooEntry, D>), SizeAnn>, D> -> Sp<MerklePatriciaTrie<(Sp<u64, D>, Sp<BarEntry, D>), SizeAnn>, D>
            (
                TranslationId(
                    MerklePatriciaTrie::<FooEntry, D, SizeAnn>::tag(),
                    MerklePatriciaTrie::<BarEntry, D, SizeAnn>::tag(),
                ),
                source.foo.mpt.upcast(),
            ),
        ]
    }
    fn finalize(
        source: &Foo<D>,
        _limit: &mut CostDuration,
        cache: &TranslationCache<D>,
    ) -> io::Result<Option<Bar<D>>> {
        let foo_tlid = TranslationId(
            MerklePatriciaTrie::<FooEntry, D, SizeAnn>::tag(),
            MerklePatriciaTrie::<BarEntry, D, SizeAnn>::tag(),
        );
        let Some(footl) = cache.lookup(&foo_tlid, source.foo.mpt.as_child()) else {
            return Ok(None);
        };
        let bar = Map {
            mpt: footl.force_downcast(),
            key_type: PhantomData,
        };
        Ok(Some(Bar {
            bar,
            baz: source.baz.clone(),
        }))
    }
}

struct MptFooToBarTranslation;

impl<D: DB> DirectTranslation<MerklePatriciaTrie<FooEntry, D>, MerklePatriciaTrie<BarEntry, D>, D>
    for MptFooToBarTranslation
{
    fn required_translations() -> Vec<TranslationId> {
        vec![TranslationId(
            merkle_patricia_trie::Node::<FooEntry, D>::tag(),
            merkle_patricia_trie::Node::<BarEntry, D>::tag(),
        )]
    }
    fn child_translations(
        source: &MerklePatriciaTrie<FooEntry, D>,
    ) -> Vec<(TranslationId, Sp<dyn Any + Send + Sync, D>)> {
        vec![(
            TranslationId(
                merkle_patricia_trie::Node::<FooEntry, D>::tag(),
                merkle_patricia_trie::Node::<BarEntry, D>::tag(),
            ),
            source.0.upcast(),
        )]
    }
    fn finalize(
        source: &MerklePatriciaTrie<FooEntry, D>,
        _limit: &mut CostDuration,
        cache: &TranslationCache<D>,
    ) -> io::Result<Option<MerklePatriciaTrie<BarEntry, D>>> {
        let tlid = TranslationId(
            merkle_patricia_trie::Node::<FooEntry, D>::tag(),
            merkle_patricia_trie::Node::<BarEntry, D>::tag(),
        );
        let Some(tl) = cache.lookup(&tlid, source.0.as_child()) else {
            return Ok(None);
        };
        Ok(Some(MerklePatriciaTrie(tl.force_downcast())))
    }
}

struct MptNodeFooToBarTranslation;

impl<D: DB>
    DirectTranslation<
        merkle_patricia_trie::Node<FooEntry, D>,
        merkle_patricia_trie::Node<BarEntry, D>,
        D,
    > for MptNodeFooToBarTranslation
{
    fn required_translations() -> Vec<TranslationId> {
        let entry_tl = TranslationId(FooEntry::tag(), BarEntry::tag());
        let self_tl = TranslationId(
            merkle_patricia_trie::Node::<FooEntry, D>::tag(),
            merkle_patricia_trie::Node::<BarEntry, D>::tag(),
        );
        vec![entry_tl, self_tl]
    }
    fn child_translations(
        source: &merkle_patricia_trie::Node<FooEntry, D>,
    ) -> Vec<(TranslationId, Sp<dyn Any + Send + Sync, D>)> {
        let entry_tl = TranslationId(FooEntry::tag(), BarEntry::tag());
        let self_tl = TranslationId(
            merkle_patricia_trie::Node::<FooEntry, D>::tag(),
            merkle_patricia_trie::Node::<BarEntry, D>::tag(),
        );
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
        source: &merkle_patricia_trie::Node<FooEntry, D>,
        _limit: &mut CostDuration,
        cache: &TranslationCache<D>,
    ) -> io::Result<Option<merkle_patricia_trie::Node<BarEntry, D>>> {
        let entry_tl = TranslationId(FooEntry::tag(), BarEntry::tag());
        let self_tl = TranslationId(
            merkle_patricia_trie::Node::<FooEntry, D>::tag(),
            merkle_patricia_trie::Node::<BarEntry, D>::tag(),
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
                    *new_child = entry.force_downcast();
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
                let child: Sp<merkle_patricia_trie::Node<BarEntry, D>, D> = entry.force_downcast();
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
                let value: Sp<BarEntry, D> = entry.force_downcast();
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
                let value: Sp<BarEntry, D> = value_entry.force_downcast();
                let child: Sp<merkle_patricia_trie::Node<BarEntry, D>, D> =
                    child_entry.force_downcast();
                let ann = SizeAnn::from_value(&value).append(&child.ann());
                merkle_patricia_trie::Node::MidBranchLeaf { ann, value, child }
            }
        }))
    }
}

struct FooEntryToBarEntryTranslation;

impl<D: DB> DirectTranslation<FooEntry, BarEntry, D> for FooEntryToBarEntryTranslation {
    fn required_translations() -> Vec<TranslationId> {
        vec![]
    }
    fn child_translations(source: &FooEntry) -> Vec<(TranslationId, Sp<dyn Any + Send + Sync, D>)> {
        vec![]
    }
    fn finalize(
        source: &FooEntry,
        _limit: &mut CostDuration,
        _cache: &TranslationCache<D>,
    ) -> io::Result<Option<BarEntry>> {
        Ok(Some(BarEntry(source.0.wrapping_mul(42))))
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
                Cow::Borrowed("mpt(foo-entry,size-annotation)"),
                Cow::Borrowed("mpt(bar-entry,size-annotation)"),
            ),
            &DirectSpTranslation::<_, _, MptFooToBarTranslation, _>(PhantomData),
        ),
        (
            TranslationId(
                Cow::Borrowed("mpt-node(foo-entry,size-annotation)"),
                Cow::Borrowed("mpt-node(bar-entry,size-annotation)"),
            ),
            &DirectSpTranslation::<_, _, MptNodeFooToBarTranslation, _>(PhantomData),
        ),
        (
            TranslationId(Cow::Borrowed("foo-entry"), Cow::Borrowed("bar-entry")),
            &DirectSpTranslation::<_, _, FooEntryToBarEntryTranslation, _>(PhantomData),
        ),
    ];
}

#[cfg(test)]
mod tests {
    use super::*;
    use serialize::Tagged;
    use storage::{Storage, arena::Sp, storage::set_default_storage};

    fn mk_nesty(depth: usize, offset: u32) -> Sp<lr::Nesty<TestDb>, TestDb> {
        if depth == 0 {
            return Sp::new(lr::Nesty::Empty);
        }
        let left = mk_nesty(depth - 1, offset);
        let right = mk_nesty(depth - 1, offset + (1 << depth));
        Sp::new(lr::Nesty::Node(offset, left, right))
    }

    fn mk_foo(depth: usize) -> Sp<Foo<TestDb>, TestDb> {
        let baz = Sp::new(Baz {
            baz: (0..(1 << depth)).map(|i| (i * 4, ())).collect(),
        });
        let foo = (0..(1 << depth)).map(|i| (i * 8, FooEntry(i))).collect();
        Sp::new(Foo { foo, baz })
    }

    #[test]
    fn test_nesty_tl() {
        set_default_storage::<TestDb>(|| Storage::new(1024, ParityDb::open("test-db".as_ref())))
            .unwrap();
        let t0 = std::time::Instant::now();
        let n = 23;
        let mut before = mk_nesty(n, 0);
        dbg!(before.serialize_to_node_list().nodes.len());
        before.persist();
        before.unload();
        let t1 = std::time::Instant::now();
        let mut tl_state = Sp::new(TypedTranslationState::<
            lr::Nesty<TestDb>,
            rl::Nesty<TestDb>,
            TestTable,
            TestDb,
        >::start(before)
        .unwrap());
        let cost = CostDuration::from_picoseconds(1_000_000_000_000);
        while tl_state.result().unwrap().is_none() {
            tl_state = Sp::new(tl_state.run(cost).unwrap());
            tl_state.persist();
            tl_state.unload();
        }
        let _after = tl_state.result().unwrap().unwrap();
        let tfin0 = std::time::Instant::now();
        drop(_after);
        drop(tl_state);
        let tfin1 = std::time::Instant::now();
        let dt0 = tfin1 - t0;
        let dt1 = tfin0 - t1;
        let m = 1 << n;
        eprintln!(
            "took {dt0:?} for {m} items ({} items per second) [incl construction]",
            m as f64 / dt0.as_secs_f64()
        );
        eprintln!(
            "took {dt1:?} for {m} items ({} items per second) [excl construction]",
            m as f64 / dt1.as_secs_f64()
        );
        dbg!(&TUPDATE);
        dbg!(&TPROCESS);
        dbg!(&TDEP);
        dbg!(&TFIN);
        dbg!(&NPROC);
        dbg!(
            TUPDATE.load(std::sync::atomic::Ordering::SeqCst) as f64
                / NPROC.load(std::sync::atomic::Ordering::SeqCst) as f64
        );
        dbg!(
            TPROCESS.load(std::sync::atomic::Ordering::SeqCst) as f64
                / NPROC.load(std::sync::atomic::Ordering::SeqCst) as f64
        );
    }

    #[test]
    fn test_foo_tl() {
        let t0 = std::time::Instant::now();
        let n = 18;
        let before = mk_foo(n);
        dbg!(before.foo.mpt.serialize_to_node_list().nodes.len());
        let t1 = std::time::Instant::now();
        let tl_state =
            TypedTranslationState::<Foo<TestDb>, Bar<TestDb>, TestTable, TestDb>::start(before)
                .unwrap();
        let cost = CostDuration::from_picoseconds(1_000_000_000_000);
        let finished_state = tl_state.run(cost).unwrap();
        let Some(_after) = finished_state.result().unwrap() else {
            panic!("didn't finish");
        };
        let tfin0 = std::time::Instant::now();
        drop(_after);
        drop(finished_state);
        drop(tl_state);
        let tfin1 = std::time::Instant::now();
        let dt0 = tfin1 - t0;
        let dt1 = tfin0 - t0;
        let dt2 = tfin1 - t1;
        let dt3 = tfin0 - t1;
        let m = 1 << n;
        eprintln!(
            "took {dt0:?} for {m} items ({} items per second) [incl construction, incl drop]",
            m as f64 / dt0.as_secs_f64()
        );
        eprintln!(
            "took {dt1:?} for {m} items ({} items per second) [incl construction, excl drop]",
            m as f64 / dt1.as_secs_f64()
        );
        eprintln!(
            "took {dt2:?} for {m} items ({} items per second) [excl construction, incl drop]",
            m as f64 / dt2.as_secs_f64()
        );
        eprintln!(
            "took {dt3:?} for {m} items ({} items per second) [excl construction, excl drop]",
            m as f64 / dt3.as_secs_f64()
        );
    }

    #[test]
    fn test_test_table_closed() {
        <TestTable as TranslationTable<TestDb>>::assert_closure();
    }
}
