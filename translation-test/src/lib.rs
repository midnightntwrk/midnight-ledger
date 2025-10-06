use base_crypto::cost_model::CostDuration;
use storage::db::{InMemoryDB, DB};
use storage::arena::{ArenaKey, BackendLoader, Sp};
use storage::Storable;
use storage::storable::Loader;
use storage::storage::{default_storage, HashMap, Map};
use serialize::{Tagged, Serializable, Deserializable};
use derive_where::derive_where;
use std::io;
use std::borrow::Cow;
use std::marker::PhantomData;

pub mod mechanism;

use mechanism::*;

#[derive(Storable)]
#[derive_where(Clone)]
#[storable(db = D)]
#[tag = "foo"]
pub struct Foo<D: DB> {
    foo: HashMap<FooKey, (), D>,
    baz: Baz<D>,
}

#[derive(Storable)]
#[derive_where(Clone)]
#[storable(db = D)]
#[tag = "bar"]
pub struct Bar<D: DB> {
    bar: HashMap<BarKey, (), D>,
    baz: Baz<D>,
}

#[derive(Serializable, Storable, Clone)]
#[storable(base)]
#[tag = "foo-key"]
pub struct FooKey(u64);

#[derive(Serializable, Storable, Clone)]
#[storable(base)]
#[tag = "bar-key"]
pub struct BarKey(u64);

#[derive(Storable)]
#[derive_where(Clone)]
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
        Node(u32, #[storable(child)] Sp<Nesty<D>, D>, #[storable(child)] Sp<Nesty<D>, D>),
    }
}

struct NestyLrToRlTranslation;

impl<D: DB> DirectTranslation<lr::Nesty<D>, rl::Nesty<D>, D> for NestyLrToRlTranslation {
    fn child_translations(source: &lr::Nesty<D>) -> Vec<(TranslationId, RawNode<D>)> {
        let tlid = || TranslationId(lr::Nesty::<D>::tag(), rl::Nesty::<D>::tag());
        match source {
            lr::Nesty::Empty => vec![],
            lr::Nesty::Node(_, a, b) => vec![(tlid(), a.hash().into()), (tlid(), b.hash().into())],
        }
    }
    fn finalize(source: &lr::Nesty<D>, limit: &mut CostDuration, cache: &TranslationCache<D>) -> io::Result<Option<rl::Nesty<D>>> {
        let tlid = TranslationId(lr::Nesty::<D>::tag(), rl::Nesty::<D>::tag());
        let storage = default_storage::<D>();
        // TODO: adjust limit
        match source {
            lr::Nesty::Empty => Ok(Some(rl::Nesty::Empty)),
            lr::Nesty::Node(n, a, b) => {
                let Some(atrans) = cache.lookup(&tlid, a.hash().into()) else { return Ok(None); };
                let Some(btrans) = cache.lookup(&tlid, b.hash().into()) else { return Ok(None); };
                Ok(Some(rl::Nesty::Node(
                    *n,
                    storage.get_lazy(&btrans.key.into()).expect("translated node must be in storage"),
                    storage.get_lazy(&atrans.key.into()).expect("translated node must be in storage"),
                )))
            }
        }
    }
}

struct TestTable;

impl<D: DB> TranslationTable<D> for TestTable {
    const TABLE: &[(TranslationId, &dyn TypelessTranslation<D>)] = &[
        (TranslationId(Cow::Borrowed("nesty-lr"), Cow::Borrowed("nesty-rl")), &DirectSpTranslation::<_, _, NestyLrToRlTranslation, _>(PhantomData)),
    ];
}

#[cfg(test)]
mod tests {
    use serialize::Tagged;
    use storage::{arena::Sp, db::InMemoryDB};
    use super::*;

    fn mk_nesty(depth: usize, offset: u32) -> Sp<lr::Nesty<InMemoryDB>> {
        if depth == 0 {
            return Sp::new(lr::Nesty::Empty);
        }
        let left = mk_nesty(depth - 1, offset);
        let right = mk_nesty(depth - 1, offset + 1 << depth);
        Sp::new(lr::Nesty::Node(offset, left, right))
    }

    #[test]
    fn test_tl() {
        let before = mk_nesty(20, 0);
        let tlid = TranslationId(lr::Nesty::<InMemoryDB>::tag(), rl::Nesty::<InMemoryDB>::tag());
        let mut tl_state = TranslationState::<TestTable, InMemoryDB>::start(&tlid, before.hash().into()).unwrap();
        let mut cost = CostDuration::from_picoseconds(1_000_000_000_000);
        let after = loop {
            match tl_state.step(&mut cost).unwrap() {
                Either::Left(next) => tl_state = next,
                Either::Right(res) => break res,
            }
        };
        let _after = default_storage::<InMemoryDB>().get::<rl::Nesty<InMemoryDB>>(&after.key.clone().into()).unwrap();
        //dbg!(before);
        //dbg!(after);
        //assert!(false);
    }
}
