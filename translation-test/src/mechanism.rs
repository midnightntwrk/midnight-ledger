use base_crypto::cost_model::CostDuration;
use derive_where::derive_where;
use serialize::{Deserializable, Serializable, Tagged};
use std::any::Any;
use std::borrow::Cow;
use std::fmt::Debug;
use std::io;
use std::marker::PhantomData;
use std::sync::atomic::AtomicU64;
use std::time::Instant;
use storage::Storable;
use storage::arena::{ArenaHash, ArenaKey, BackendLoader, Sp};
use storage::db::DB;
use storage::storable::Loader;
use storage::storage::{HashMap, Map, default_storage};

pub type RawNode<D> = ArenaKey<<D as DB>::Hasher>;

pub enum StepResult<D: DB> {
    Finished(Sp<dyn Any + Send + Sync, D>),
    NotEnoughTime,
    Suspended,
    Depends(Vec<(TranslationId, Sp<dyn Any + Send + Sync, D>)>),
}

#[derive(Serializable, Storable)]
#[derive_where(Clone, Debug, PartialEq, Eq, Hash)]
#[storable(base)]
#[phantom(D)]
struct TranslationCacheKey<D: DB> {
    tlid: TranslationId,
    hash: ArenaHash<D::Hasher>,
    persist: bool,
}

impl<D: DB> TranslationCacheKey<D> {
    fn from_key(tlid: &TranslationId, key: &ArenaKey<D::Hasher>) -> Self {
        TranslationCacheKey { tlid: tlid.clone(), hash: key.hash().clone(), persist: matches!(key, ArenaKey::Ref(_)) }
    }
}

//#[derive(Storable)]
#[derive_where(Clone)]
//#[storable(db = D)]
pub struct TranslationCache<D: DB> {
    //map: HashMap<TranslationCacheKey<D>, ChildRef<D>, D>,
    //map: rpds::map::hash_trie_map::HashTrieMapSync<TranslationCacheKey<D>, Sp<dyn Any + Send + Sync, D>>,
    persistent_map: HashMap<TranslationCacheKey<D>, Sp<dyn Any + Send + Sync, D>, D>,
    transient_map: hashbrown::HashMap<TranslationCacheKey<D>, Sp<dyn Any + Send + Sync, D>>,
}

impl<D: DB> From<HashMap<TranslationCacheKey<D>, Sp<dyn Any + Send + Sync, D>, D>> for TranslationCache<D> {
    fn from(persistent_map: HashMap<TranslationCacheKey<D>, Sp<dyn Any + Send + Sync, D>, D>) -> Self {
        TranslationCache { persistent_map, transient_map: hashbrown::HashMap::new() }
    }
}

impl<D: DB> From<TranslationCache<D>> for HashMap<TranslationCacheKey<D>, Sp<dyn Any + Send + Sync, D>, D> {
    fn from(value: TranslationCache<D>) -> Self {
        value.transient_map.into_iter().filter(|(k, _)| k.persist).fold(value.persistent_map, |pm, (k, v)| pm.insert(k, v))
    }
}

impl<D: DB> TranslationCache<D> {
    fn insert(&mut self, id: TranslationId, from: RawNode<D>, to: Sp<dyn Any + Send + Sync, D>) {
        self.transient_map.insert(TranslationCacheKey::from_key(&id, &from), to);
    }
    pub fn lookup(&self, id: &TranslationId, child: RawNode<D>) -> Option<Sp<dyn Any + Send + Sync, D>> {
        let key = TranslationCacheKey::from_key(id, &child);
        self.transient_map
            .get(&key)
            .map(|v| (&*v).clone())
            .or_else(|| self.persistent_map.get(&key).map(|v| (&*v).clone()))
    }
    pub fn resolve<T: Storable<D> + Tagged + Any + Send + Sync>(
        &self,
        id: &TranslationId,
        child: RawNode<D>,
    ) -> io::Result<Option<Sp<T, D>>> {
        let Some(dynsp) = self.lookup(id, child) else {
            return Ok(None);
        };
        Ok(Some(dynsp.force_downcast::<T>()))
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
    fn start(&self, node: Sp<dyn Any + Send + Sync, D>) -> Box<dyn TypelessTranslationState<D>>;
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
    fn child_translations(source: &A) -> Vec<(TranslationId, Sp<dyn Any + Send + Sync, D>)>;
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
    #[storable(child)]
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
    fn start(&self, value: Sp<dyn Any + Send + Sync, D>) -> Box<dyn TypelessTranslationState<D>> {
        let state: DirectSpTranslationState<A, B, T, D> = DirectSpTranslationState {
            value: value.force_downcast(),
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
        //let mut data = Vec::new();
        //reader.read_to_end(&mut data)?;
        //let hashes = child_hashes.collect::<Vec<_>>();
        //let a = loader.get::<A>(&hashes[0]).unwrap();
        let res: Result<Box<dyn TypelessTranslationState<D>>, _> = Ok(Box::new(
            DirectSpTranslationState::<A, B, T, D>::from_binary_repr(
                &mut reader,
                &mut child_hashes,
                loader,
            )?,
        ));
        res
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
        let t0 = Instant::now();
        let mut required_children = T::child_translations(&self.value);
        required_children.retain(|(tid, obj)| cache.lookup(tid, obj.as_child()).is_none());
        let t1 = Instant::now();
        TDEP.fetch_update(std::sync::atomic::Ordering::SeqCst, std::sync::atomic::Ordering::SeqCst, |x| Some(x + (t1 - t0).as_nanos() as u64)).unwrap();
        if !required_children.is_empty() {
            Ok(StepResult::Depends(required_children))
        } else {
            let t0 = Instant::now();
            let res = T::finalize(&self.value, limit, cache)?;
            // Heuristic for overhead of processing a node. Dominates in most cases.
            // Set at 20us.
            *limit -= CostDuration::from_picoseconds(20_000_000);
            let res = match res {
                // TODO: Here and elsewhere, we destroy the Sp for the hash,
                // which will actually deallocate it. Use something that keeps
                // the ref alive, like in the rcmap.
                Some(res) => {
                    let sp = Sp::new(res);
                    let upcast = sp.upcast();
                    drop(sp);
                    Ok(StepResult::Finished(upcast))
                }
                None =>
                if *limit == CostDuration::ZERO {
                    Ok(StepResult::NotEnoughTime)
                } else {
                    Ok(StepResult::Suspended)
                },
            };
            let t1 = Instant::now();
            TFIN.fetch_update(std::sync::atomic::Ordering::SeqCst, std::sync::atomic::Ordering::SeqCst, |x| Some(x + (t1 - t0).as_nanos() as u64)).unwrap();
            res
        }
    }
    fn children(&self) -> std::vec::Vec<RawNode<D>> {
        vec![self.value.as_child()]
    }
    fn to_binary_repr(&self, _writer: &mut dyn std::io::Write) -> Result<(), std::io::Error> {
        Ok(())
    }
}

// Problem: How to make Box<dyn Foo> (for translation state) Storable?
// Really should also be Sp<dyn Foo, D> (Or Sp<Box<dyn Foo>, D>)
// Problem is that we need a unique tag table to deserialize, and tags in the dyn states.
// Maybe better to encode as an enum than a dyn? Downside is that enum type
// infects everything, breaking abstractions (the translation tooling needs to know the translation specifics)
//
// Alternatively, build up the TL table _somehow_, and auto-derive a storable instance off that.

#[derive(PartialEq, Eq, Serializable, Clone, Hash)]
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
pub struct PersistentQueue<T: Storable<D>, D: DB> {
    pub start: u64,
    pub end: u64,
    pub queue: Map<u64, T, D>,
}

impl<T: Storable<D>, D: DB> PersistentQueue<T, D> {
    pub fn empty() -> Self {
        PersistentQueue { start: 0, end: 0, queue: Map::new() }
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


//#[derive(Storable)]
#[derive_where(Clone)]
#[derive_where(Debug; T)]
//#[storable(db = D)]
pub struct Queue<T: Storable<D>, D: DB> {
    //pub start: u64,
    //pub end: u64,
    //pub queue: Map<u64, T, D>,
    //pub queue: rpds::map::red_black_tree_map::RedBlackTreeMapSync<u64, T>,
    persistent_queue: PersistentQueue<T, D>,
    transient_queue: Vec<T>,
}

impl<T: Storable<D>, D: DB> From<PersistentQueue<T, D>> for Queue<T, D> {
    fn from(persistent_queue: PersistentQueue<T, D>) -> Self {
        Queue {
            persistent_queue,
            transient_queue: Vec::new(),
        }
    }
}

impl<T: Storable<D>, D: DB> From<Queue<T, D>> for PersistentQueue<T, D> {
    fn from(queue: Queue<T, D>) -> Self {
        queue.transient_queue.into_iter().fold(queue.persistent_queue, |pq, entry| pq.push_front(entry))
    }
}

impl<T: Storable<D>, D: DB> Queue<T, D> {
    pub fn is_empty(&self) -> bool {
        self.persistent_queue.is_empty() && self.transient_queue.is_empty()
    }

    pub fn front(&self) -> Option<&T> {
        self.transient_queue.last().or_else(|| self.persistent_queue.front())
    }

    pub fn back(&self) -> Option<&T> {
        self.persistent_queue.back().or_else(|| self.transient_queue.first())
    }

    pub fn push_front(&mut self, value: T) {
        self.transient_queue.push(value)
    }

    pub fn push_back(&mut self, value: T) {
        self.persistent_queue = self.persistent_queue.push_back(value);
    }

    pub fn remove_front(&mut self) -> Option<T> {
        if let Some(value) = self.transient_queue.pop() {
            return Some(value);
        }
        let res = self.persistent_queue.front().cloned();
        if res.is_some() {
            self.persistent_queue = self.persistent_queue.remove_front();
        }
        res
    }

    pub fn remove_back(&mut self) -> Option<T> {
        let persistent_back = self.persistent_queue.back().cloned();
        if persistent_back.is_some() {
            self.persistent_queue = self.persistent_queue.remove_back();
            return persistent_back;
        }
        // If we reach here, the persistent queue must be empty
        if self.transient_queue.is_empty() {
            None
        } else {
            Some(self.transient_queue.remove(0))
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
    fn start(id: &TranslationId, node: Sp<dyn Any + Send + Sync, D>) -> io::Result<TaggedTranslationState<Self, D>>
    where
        Self: Sized,
    {
        let tl = Self::get(id)?;
        Ok(TaggedTranslationState {
            id: id.clone(),
            from: node.as_child(),
            typeless_state: tl.start(node),
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

pub static TUPDATE: AtomicU64 = AtomicU64::new(0);
pub static TPROCESS: AtomicU64 = AtomicU64::new(0);
pub static TDEP: AtomicU64 = AtomicU64::new(0);
pub static TFIN: AtomicU64 = AtomicU64::new(0);
pub static NPROC: AtomicU64 = AtomicU64::new(0);

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
#[derive_where(Clone)]
#[storable(db = D)]
#[phantom(TABLE)]
pub struct TranslationState<TABLE: TranslationTable<D>, D: DB> {
    work_queue: PersistentQueue<TaggedTranslationState<TABLE, D>, D>,
    cache: HashMap<TranslationCacheKey<D>, Sp<dyn Any + Send + Sync, D>, D>,
    result: Option<Sp<dyn Any + Send + Sync, D>>,
}

struct InflightTranslationState<TABLE: TranslationTable<D>, D: DB> {
    work_queue: Queue<TaggedTranslationState<TABLE, D>, D>,
    cache: TranslationCache<D>,
    result: Option<Sp<dyn Any + Send + Sync, D>>,
}

impl<TABLE: TranslationTable<D>, D: DB> From<TranslationState<TABLE, D>> for InflightTranslationState<TABLE, D> {
    fn from(value: TranslationState<TABLE, D>) -> Self {
        InflightTranslationState { work_queue: value.work_queue.into(), cache: value.cache.into(), result: value.result.into() }
    }
}

impl<TABLE: TranslationTable<D>, D: DB> From<InflightTranslationState<TABLE, D>> for TranslationState<TABLE, D> {
    fn from(value: InflightTranslationState<TABLE, D>) -> Self {
        TranslationState { work_queue: value.work_queue.into(), cache: value.cache.into(), result: value.result.into() }
    }
}

pub enum Either<A, B> {
    Left(A),
    Right(B),
}

impl<TABLE: TranslationTable<D>, D: DB> TranslationState<TABLE, D> {
    pub fn start(id: &TranslationId, node: Sp<dyn Any + Send + Sync, D>) -> io::Result<Self> {
        let tl = TABLE::start(id, node)?;
        let work_queue = PersistentQueue::empty().push_back(tl);
        let s0 = TranslationState {
            work_queue,
            cache: HashMap::new(),
            result: None,
        };
        Ok(s0)
    }

    pub fn change_target(&self, id: &TranslationId, node: Sp<dyn Any + Send + Sync, D>) -> io::Result<Self> {
        let tl = TABLE::start(id, node)?;
        let work_queue = self.work_queue.push_back(tl);
        Ok(TranslationState {
            work_queue,
            cache: self.cache.clone(),
            result: None,
        })
    }

    pub fn run(&self, mut limit: CostDuration) -> io::Result<Self> {
        let mut cur = InflightTranslationState::from(self.clone());
        cur.result = loop {
            match cur.step(&mut limit)? {
                Either::Left(true) => break None,
                Either::Left(false) => {}
                Either::Right(res) => break Some(res),
            }
        };
        Ok(cur.into())
    }

    fn result(&self) -> Option<Sp<dyn Any + Send + Sync, D>> {
        self.result.clone()
    }
}

impl<TABLE: TranslationTable<D>, D: DB> InflightTranslationState<TABLE, D> {
    fn step(&mut self, limit: &mut CostDuration) -> io::Result<Either<bool, Sp<dyn Any + Send + Sync, D>>> {
        let t0 = Instant::now();
        let mut cur = self.work_queue.remove_front().expect("work queue must not be empty");
        let mut finished = false;
        let t1 = Instant::now();
        let step_res = cur.typeless_state.step(limit, &self.cache)?;
        let t2 = Instant::now();
        match step_res {
            StepResult::Suspended => self.work_queue.push_front(cur),
            StepResult::Depends(dependencies) => {
                self.work_queue.push_front(cur);
                for (id, child) in dependencies.into_iter() {
                    if self.cache.lookup(&id, child.as_child()).is_none() {
                        self.work_queue.push_front(TABLE::start(&id, child)?);
                    }
                }
            }
            StepResult::Finished(res) => {
                if self.work_queue.is_empty() {
                    return Ok(Either::Right(res));
                } else {
                    self.cache.insert(cur.id, cur.from, res);
                }
            }
            StepResult::NotEnoughTime => finished = true,
        }
        let res = Ok(Either::Left(finished || *limit == CostDuration::ZERO));
        let t3 = Instant::now();
        TPROCESS.fetch_update(std::sync::atomic::Ordering::SeqCst, std::sync::atomic::Ordering::SeqCst, |x| Some(x + (t2 - t1).as_nanos() as u64)).unwrap();
        TUPDATE.fetch_update(std::sync::atomic::Ordering::SeqCst, std::sync::atomic::Ordering::SeqCst, |x| Some(x + ((t3 - t2) + (t1 - t0)).as_nanos() as u64)).unwrap();
        NPROC.fetch_update(std::sync::atomic::Ordering::SeqCst, std::sync::atomic::Ordering::SeqCst, |x| Some(x + 1)).unwrap();
        res
    }
}

#[derive(Storable)]
#[derive_where(Clone)]
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
            state: TranslationState::start(&tlid, input.upcast())?,
            _phantom1: PhantomData,
            _phantom2: PhantomData,
        })
    }

    pub fn change_target(&self, target: Sp<A, D>) -> io::Result<Self> {
        let tlid = TranslationId(A::tag(), B::tag());
        Ok(TypedTranslationState {
            state: self.state.change_target(&tlid, target.upcast())?,
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

    pub fn run(&self, limit: CostDuration) -> io::Result<Self> {
        Ok(TypedTranslationState { state: self.state.run(limit)?, _phantom1: PhantomData, _phantom2: PhantomData })
    }

    pub fn result(&self) -> io::Result<Option<Sp<B, D>>> {
        Ok(self.state.result().map(|dynsp| dynsp.force_downcast::<B>()))
    }
}
