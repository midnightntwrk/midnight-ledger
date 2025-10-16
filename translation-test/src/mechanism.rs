use base_crypto::cost_model::CostDuration;
use derive_where::derive_where;
use serialize::{Deserializable, Serializable, Tagged};
use std::borrow::Cow;
use std::fmt::Debug;
use std::io;
use std::marker::PhantomData;
use storage::Storable;
use storage::arena::{ArenaKey, BackendLoader, Sp};
use storage::db::{DB, InMemoryDB};
use storage::delta_tracking::KeyRef;
use storage::storable::Loader;
use storage::storage::{HashMap, Map, default_storage};

pub type RawNode<D> = ArenaKey<<D as DB>::Hasher>;

pub enum StepResult<D: DB> {
    Finished(Sp<KeyRef<D>, D>),
    NotEnoughTime,
    Suspended,
    Depends {
        id: TranslationId,
        child: RawNode<D>,
    },
}

#[derive(Serializable, Storable)]
#[derive_where(Clone, Debug)]
#[storable(base)]
#[phantom(D)]
struct TranslationCacheKey<D: DB>(TranslationId, RawNode<D>);

#[derive(Storable)]
#[derive_where(Clone, Debug)]
#[storable(db = D)]
pub struct TranslationCache<D: DB> {
    map: HashMap<TranslationCacheKey<D>, KeyRef<D>, D>,
}

impl<D: DB> TranslationCache<D> {
    fn new() -> Self {
        TranslationCache {
            map: HashMap::new(),
        }
    }
    fn insert(&self, id: TranslationId, from: RawNode<D>, to: KeyRef<D>) -> Self {
        Self {
            map: self.map.insert(TranslationCacheKey(id, from), to),
        }
    }
    pub fn lookup(&self, id: &TranslationId, child: RawNode<D>) -> Option<KeyRef<D>> {
        self.map
            .get(&TranslationCacheKey(id.clone(), child))
            .map(|v| (&*v).clone())
    }
    pub fn resolve<T: Storable<D> + Tagged>(
        &self,
        id: &TranslationId,
        child: RawNode<D>,
    ) -> io::Result<Option<Sp<T, D>>> {
        let Some(keyref) = self.lookup(id, child) else {
            return Ok(None);
        };
        default_storage()
            .get_lazy(&keyref.key.clone().into())
            .map(Some)
    }
}

#[macro_export]
macro_rules! try_resopt {
    ($x:expr) => {
        match $x? {
            Some(x) => x,
            None => return Ok(None),
        }
    };
}

pub use try_resopt;

trait AsBackendLoader<D: DB> {
    fn as_backend_loader<'a>(&'a self) -> io::Result<&'a BackendLoader<'a, D>>;
}

impl<T: Loader<D>, D: DB> AsBackendLoader<D> for T {
    fn as_backend_loader<'a>(&'a self) -> io::Result<&'a BackendLoader<'a, D>> {
        // NOTE: This is wildly unsafe, but in a controlled way.
        // Ideally we'd like to cast if type IDs match here, but that's a no-go because
        // TypeId requires 'static, and BackendLoader *isn't*. That's not
        // actually a concern, becaues we know it can't outlive the lifetime of
        // &self here.
        // However, it means that we need to fall back to `type_name` to test if
        // we have a backend loader, which *is not* guaranteed to be unique.
        // That said, we have a limited set of loaders, defined by us, and there
        // is no danger that this will be abused.
        if std::any::type_name::<Self>() == std::any::type_name::<BackendLoader<D>>() {
            Ok(unsafe { &*(self as *const Self as *const BackendLoader<D>) })
        } else {
            Err(io::Error::new(
                io::ErrorKind::Other,
                "requiered backend loader for translation machinery due to dyn trait shenanigans",
            ))
        }
    }
}

pub trait TypelessTranslation<D: DB> {
    fn required_translations(&self) -> Vec<TranslationId>;
    fn start(&self, raw: RawNode<D>) -> Box<dyn TypelessTranslationState<D>>;
    fn from_binary_repr(
        &self,
        reader: &mut dyn std::io::Read,
        child_hashes: &mut dyn Iterator<Item = RawNode<D>>,
        loader: &BackendLoader<D>,
    ) -> Result<Box<dyn TypelessTranslationState<D>>, std::io::Error>;
}

pub trait TypelessTranslationState<D: DB>: Send + Sync + std::fmt::Debug {
    fn boxed_clone(&self) -> Box<dyn TypelessTranslationState<D>>;
    fn step(
        &mut self,
        limit: &mut CostDuration,
        cache: &TranslationCache<D>,
    ) -> io::Result<StepResult<D>>;

    fn children(&self) -> std::vec::Vec<RawNode<D>>;
    fn to_binary_repr(&self, writer: &mut dyn std::io::Write) -> Result<(), std::io::Error>;
}

pub trait DirectTranslation<A: Storable<D>, B: Storable<D>, D: DB>: Send + Sync + 'static {
    fn required_translations() -> Vec<TranslationId>;
    fn child_translations(source: &A) -> Vec<(TranslationId, RawNode<D>)>;
    fn finalize(
        source: &A,
        limit: &mut CostDuration,
        cache: &TranslationCache<D>,
    ) -> io::Result<Option<B>>;
}

pub struct DirectSpTranslation<A: Storable<D>, B: Storable<D>, T: DirectTranslation<A, B, D>, D: DB>(
    pub PhantomData<(A, B, T, D)>,
);

#[derive(Storable)]
#[storable(db = D)]
#[derive_where(Clone)]
#[derive_where(Debug; A)]
#[phantom(T)]
#[tag = "direct-sp-translation-state"]
pub struct DirectSpTranslationState<
    A: Storable<D>,
    B: Storable<D>,
    T: DirectTranslation<A, B, D>,
    D: DB,
> {
    children_processed: u32,
    value: Sp<A, D>,
    _phantom1: PhantomData<B>,
    _phantom2: PhantomData<T>,
}

impl<A: Storable<D> + std::fmt::Debug, B: Storable<D>, T: DirectTranslation<A, B, D>, D: DB>
    TypelessTranslation<D> for DirectSpTranslation<A, B, T, D>
{
    fn required_translations(&self) -> Vec<TranslationId> {
        T::required_translations()
    }
    fn start(&self, raw: RawNode<D>) -> Box<dyn TypelessTranslationState<D>> {
        let state: DirectSpTranslationState<A, B, T, D> = DirectSpTranslationState {
            children_processed: 0,
            value: default_storage()
                .get(&raw.into())
                .expect("translation target must be present"),
            _phantom1: PhantomData,
            _phantom2: PhantomData,
        };
        Box::new(state)
    }
    fn from_binary_repr(
        &self,
        mut reader: &mut dyn std::io::Read,
        mut child_hashes: &mut dyn Iterator<Item = RawNode<D>>,
        loader: &BackendLoader<D>,
    ) -> Result<Box<dyn TypelessTranslationState<D>>, std::io::Error> {
        Ok(Box::new(
            DirectSpTranslationState::<A, B, T, D>::from_binary_repr(
                &mut reader,
                &mut child_hashes,
                loader,
            )?,
        ))
    }
}

impl<A: Storable<D> + std::fmt::Debug, B: Storable<D>, T: DirectTranslation<A, B, D>, D: DB>
    TypelessTranslationState<D> for DirectSpTranslationState<A, B, T, D>
{
    fn boxed_clone(&self) -> Box<dyn TypelessTranslationState<D>> {
        Box::new(self.clone())
    }
    fn step(
        &mut self,
        limit: &mut CostDuration,
        cache: &TranslationCache<D>,
    ) -> io::Result<StepResult<D>> {
        let mut proc_children = T::child_translations(&self.value);
        if (self.children_processed as usize) < proc_children.len() {
            let child = proc_children.swap_remove(self.children_processed as usize);
            self.children_processed += 1;
            Ok(StepResult::Depends {
                id: child.0,
                child: child.1,
            })
        } else {
            let res = T::finalize(&self.value, limit, cache)?;
            match res {
                // TODO: Here and elsewhere, we destroy the Sp for the hash,
                // which will actually deallocate it. Use something that keeps
                // the ref alive, like in the rcmap.
                Some(res) => {
                    let sp = Sp::new(res);
                    let keyref = Sp::new(KeyRef::new(sp.hash().into()));
                    drop(sp);
                    Ok(StepResult::Finished(keyref))
                }
                None => Ok(StepResult::Suspended),
            }
        }
    }
    fn children(&self) -> std::vec::Vec<RawNode<D>> {
        vec![self.value.hash().into()]
    }
    fn to_binary_repr(&self, mut writer: &mut dyn std::io::Write) -> Result<(), std::io::Error> {
        self.children_processed.serialize(&mut writer)
    }
}

// Problem: How to make Box<dyn Foo> (for translation state) Storable?
// Really should also be Sp<dyn Foo, D> (Or Sp<Box<dyn Foo>, D>)
// Problem is that we need a unique tag table to deserialize, and tags in the dyn states.
// Maybe better to encode as an enum than a dyn? Downside is that enum type
// infects everything, breaking abstractions (the translation tooling needs to know the translation specifics)
//
// Alternatively, build up the TL table _somehow_, and auto-derive a storable instance off that.

#[derive(PartialEq, Eq, Serializable, Clone)]
pub struct TranslationId(pub Cow<'static, str>, pub Cow<'static, str>);

impl Debug for TranslationId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} => {}", &self.0, &self.1)
    }
}

#[derive(Storable)]
#[derive_where(Clone)]
#[derive_where(Debug; T)]
#[storable(db = D)]
pub struct Queue<T: Storable<D>, D: DB> {
    pub start: u64,
    pub end: u64,
    pub queue: Map<u64, T, D>,
}

impl<T: Storable<D>, D: DB> Queue<T, D> {
    pub fn empty() -> Self {
        Queue {
            start: 0,
            end: 0,
            queue: Map::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }

    pub fn front(&self) -> Option<&T> {
        self.queue.get(&self.start)
    }

    pub fn back(&self) -> Option<&T> {
        self.queue.get(&self.end.wrapping_sub(1))
    }

    pub fn push_front(&self, value: T) -> Self {
        let start = self.start.wrapping_sub(1);
        Self {
            start,
            end: self.end,
            queue: self.queue.insert(start, value),
        }
    }

    pub fn push_back(&self, value: T) -> Self {
        Self {
            start: self.start,
            end: self.end.wrapping_add(1),
            queue: self.queue.insert(self.end, value),
        }
    }

    pub fn remove_front(&self) -> Self {
        if self.start == self.end {
            return self.clone();
        }
        Self {
            start: self.start.wrapping_add(1),
            end: self.end,
            queue: self.queue.remove(&self.start),
        }
    }

    pub fn remove_back(&self) -> Self {
        if self.start == self.end {
            return self.clone();
        }
        Self {
            start: self.start,
            end: self.end.wrapping_sub(1),
            queue: self.queue.remove(&self.end.wrapping_sub(1)),
        }
    }
}

pub trait TranslationTable<D: DB>: Send + Sync + 'static {
    const TABLE: &[(TranslationId, &dyn TypelessTranslation<D>)];
    fn assert_closure() {
        let mut missing_tls = vec![];
        for (outer, tl) in Self::TABLE.iter() {
            for inner in tl.required_translations() {
                if let Err(_) = Self::get(&inner) {
                    missing_tls.push((outer.clone(), inner));
                }
            }
        }
        for (outer, inner) in missing_tls.iter() {
            eprintln!("missing translation required for {outer:?}: {inner:?}");
        }
        assert!(missing_tls.is_empty());
    }
    fn get(id: &TranslationId) -> io::Result<&'static dyn TypelessTranslation<D>> {
        Self::TABLE
            .iter()
            .find(|(id2, _)| id == id2)
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("unknown translation ID: {id:?}"),
                )
            })
            .map(|tl| tl.1)
    }
    fn start(id: &TranslationId, raw: RawNode<D>) -> io::Result<TaggedTranslationState<Self, D>>
    where
        Self: Sized,
    {
        let tl = Self::get(id)?;
        Ok(TaggedTranslationState {
            id: id.clone(),
            from: raw.clone(),
            typeless_state: tl.start(raw),
            _phantom: PhantomData,
        })
    }
}

#[derive_where(Debug)]
pub struct TaggedTranslationState<TABLE: TranslationTable<D>, D: DB> {
    pub id: TranslationId,
    pub from: RawNode<D>,
    pub typeless_state: Box<dyn TypelessTranslationState<D>>,
    _phantom: PhantomData<TABLE>,
}

impl<TABLE: TranslationTable<D>, D: DB> Clone for TaggedTranslationState<TABLE, D> {
    fn clone(&self) -> Self {
        Self {
            id: self.id.clone(),
            from: self.from.clone(),
            typeless_state: self.typeless_state.boxed_clone(),
            _phantom: PhantomData,
        }
    }
}

impl<TABLE: TranslationTable<D>, D: DB> Storable<D> for TaggedTranslationState<TABLE, D> {
    fn to_binary_repr<W: std::io::Write>(&self, writer: &mut W) -> Result<(), std::io::Error>
    where
        Self: Sized,
    {
        self.id.serialize(writer)?;
        self.from.serialize(writer)?;
        self.typeless_state.to_binary_repr(writer)
    }
    fn children(&self) -> std::vec::Vec<RawNode<D>> {
        self.typeless_state.children()
    }
    fn from_binary_repr<R: std::io::Read>(
        reader: &mut R,
        child_hashes: &mut impl Iterator<Item = RawNode<D>>,
        loader: &impl Loader<D>,
    ) -> Result<Self, std::io::Error>
    where
        Self: Sized,
    {
        let id = TranslationId::deserialize(reader, 0)?;
        let from = RawNode::<D>::deserialize(reader, 0)?;
        let tl = TABLE::get(&id)?;
        let typeless_state =
            tl.from_binary_repr(reader, child_hashes, loader.as_backend_loader()?)?;
        Ok(TaggedTranslationState {
            id,
            from,
            typeless_state,
            _phantom: PhantomData,
        })
    }
}

#[derive(Storable)]
#[derive_where(Clone, Debug)]
#[storable(db = D)]
#[phantom(TABLE)]
pub struct TranslationState<TABLE: TranslationTable<D>, D: DB> {
    work_queue: Queue<TaggedTranslationState<TABLE, D>, D>,
    cache: TranslationCache<D>,
}

pub enum Either<A, B> {
    Left(A),
    Right(B),
}

impl<TABLE: TranslationTable<D>, D: DB> TranslationState<TABLE, D> {
    pub fn start(id: &TranslationId, raw: RawNode<D>) -> io::Result<Self> {
        let tl = TABLE::start(id, raw)?;
        let s0 = TranslationState {
            work_queue: Queue::empty().push_back(tl),
            cache: TranslationCache::new(),
        };
        Ok(s0)
    }

    pub fn change_target(&self, id: &TranslationId, raw: RawNode<D>) -> io::Result<Self> {
        let tl = TABLE::start(id, raw)?;
        let work_queue = self.work_queue.push_back(tl);
        Ok(TranslationState {
            work_queue,
            cache: self.cache.clone(),
        })
    }

    pub fn run(&self, mut limit: CostDuration) -> io::Result<Either<Self, Sp<KeyRef<D>, D>>> {
        let mut cur = self.clone();
        let result = loop {
            match cur.step(&mut limit)? {
                Either::Left((true, state)) => {
                    cur = state;
                    break None;
                }
                Either::Left((false, state)) => cur = state,
                Either::Right(res) => break Some(res),
            }
        };
        match result {
            Some(res) => Ok(Either::Right(res)),
            None => Ok(Either::Left(cur)),
        }
    }

    fn step(&self, limit: &mut CostDuration) -> io::Result<Either<(bool, Self), Sp<KeyRef<D>, D>>> {
        let mut cur = self
            .work_queue
            .front()
            .expect("work queue must not be empty")
            .clone();
        let mut work_queue = self.work_queue.remove_front();
        let mut cache = self.cache.clone();
        let mut finished = false;
        match cur.typeless_state.as_mut().step(limit, &self.cache)? {
            StepResult::Suspended => work_queue = work_queue.push_front(cur),
            StepResult::Depends { id, child } => {
                work_queue = work_queue.push_front(cur);
                work_queue = work_queue.push_front(TABLE::start(&id, child)?);
            }
            StepResult::Finished(res) => {
                if work_queue.is_empty() {
                    return Ok(Either::Right(res));
                } else {
                    cache = cache.insert(cur.id, cur.from, (&*res).clone());
                }
            }
            StepResult::NotEnoughTime => finished = true,
        }
        Ok(Either::Left((finished, Self { work_queue, cache })))
    }
}

#[derive(Storable)]
#[derive_where(Clone, Debug)]
#[storable(db = D)]
#[phantom(A, B, TABLE)]
pub struct TypedTranslationState<A: Storable<D>, B: Storable<D>, TABLE: TranslationTable<D>, D: DB>
{
    pub state: TranslationState<TABLE, D>,
    _phantom1: PhantomData<A>,
    _phantom2: PhantomData<B>,
}

impl<A: Storable<D> + Tagged, B: Storable<D> + Tagged, TABLE: TranslationTable<D>, D: DB>
    TypedTranslationState<A, B, TABLE, D>
{
    pub fn start(input: Sp<A, D>) -> io::Result<Self> {
        let tlid = TranslationId(A::tag(), B::tag());
        Ok(TypedTranslationState {
            state: TranslationState::start(&tlid, input.hash().into())?,
            _phantom1: PhantomData,
            _phantom2: PhantomData,
        })
    }

    pub fn change_target(&self, target: Sp<A, D>) -> io::Result<Self> {
        let tlid = TranslationId(A::tag(), B::tag());
        Ok(TypedTranslationState {
            state: self.state.change_target(&tlid, target.hash().into())?,
            _phantom1: PhantomData,
            _phantom2: PhantomData,
        })
    }

    pub fn last_state(&self) -> io::Result<Sp<A, D>> {
        let hash = self
            .state
            .work_queue
            .back()
            .expect("last state must exist")
            .from
            .clone();
        default_storage::<D>().get_lazy(&hash.into())
    }

    pub fn run(&self, limit: CostDuration) -> io::Result<Either<Self, Sp<B, D>>> {
        match self.state.run(limit)? {
            Either::Left(state) => Ok(Either::Left(TypedTranslationState {
                state,
                _phantom1: PhantomData,
                _phantom2: PhantomData,
            })),
            Either::Right(hash) => Ok(Either::Right(
                default_storage::<D>().get_lazy(&hash.key.clone().into())?,
            )),
        }
    }
}
