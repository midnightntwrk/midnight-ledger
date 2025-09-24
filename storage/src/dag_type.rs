//! dag_type.rs
#![allow(missing_docs)]

use std::{marker::PhantomData, sync::Mutex};

use crate::{
    Storable,
    arena::{Arena, ArenaKey, BackendLoader, Sp},
    db::DB,
    merkle_patricia_trie::{Annotation, MerklePatriciaTrie, Node, ResumeToken},
};
use derive_where::derive_where;
use serialize::Deserializable;
use serialize::Serializable;

/// Walkthrough 1: Unique numeric global type representation
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serializable)]
pub struct TypeRep(pub u32);

/// Walkthrough: Typed wrapper for MPT roots. Allows discovery step to build root -> type mappings.
#[derive_where(Clone)]
pub struct TypedSubtrieRoot<D: DB> {
    pub rep: TypeRep,
    pub root: ArenaKey<D::Hasher>,
}

impl<D: DB> Storable<D> for TypedSubtrieRoot<D> {
    fn children(&self) -> Vec<ArenaKey<D::Hasher>> {
        vec![self.root.clone()]
    }

    fn to_binary_repr<W: std::io::Write>(&self, w: &mut W) -> std::io::Result<()> {
        self.rep.serialize(w)
    }

    fn from_binary_repr<R: std::io::Read>(
        r: &mut R,
        child_hashes: &mut impl Iterator<Item = ArenaKey<D::Hasher>>,
        _loader: &impl crate::storable::Loader<D>,
    ) -> std::io::Result<Self> {
        let rep = TypeRep::deserialize(r, 0)?;
        let root = child_hashes.next().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "TypedSubtrieRoot missing child",
            )
        })?;
        Ok(TypedSubtrieRoot { rep, root })
    }
}

/// Walkthrough 2: For type erasure of `MptTypeHandler`
pub trait DagHandler<D: DB>: Send + Sync {
    fn rep(&self) -> TypeRep;

    /// Does the object at `key` decode as this handler’s MPT node
    /// type (Node<V, D, A>) in this arena?
    fn probe_root(&self, arena: &Arena<D>, key: &ArenaKey<D::Hasher>) -> bool;

    /// Probably not the best name, but we run `budget_values`-many transformations (visit `budget_values` values)
    /// of a given object
    fn step(
        &self,
        arena: &Arena<D>,
        root: &ArenaKey<D::Hasher>,
        prior_token: Option<&[u8]>,
        budget_values: usize,
    ) -> Result<WalkOutcome<Option<Vec<u8>>>, ResumeError<D>>;
}

/// Walkthrough: A concrete type -> handler mapping for an MPT whose leaf values are `V` annotated by `A`
pub struct MptHandler<V, A, D: DB, F>
// Another one of these per kind of DAG entity (and implement MptHandler)
where
    V: Storable<D> + 'static,
    A: Storable<D> + Annotation<V>,
    F: FnMut(&Sp<V, D>) + Send + 'static,
{
    rep: TypeRep,
    // Walkthrough: This is just a no-op during discovery...so really this should be split into two types if we want to go with this design (one without a handler field).
    //              Or maybe just make it optional, but I still think that's misleading.
    handler: Mutex<F>,
    _pd: PhantomData<(V, A, D)>,
}

impl<V, A, D, F> MptHandler<V, A, D, F>
where
    V: Storable<D> + 'static,
    A: Storable<D> + Annotation<V>,
    D: DB,
    F: FnMut(&Sp<V, D>) + Send + 'static,
{
    pub fn new(rep: TypeRep, handler: F) -> Self {
        Self {
            rep,
            handler: Mutex::new(handler),
            _pd: PhantomData,
        }
    }
}

impl<V, A, D, F> DagHandler<D> for MptHandler<V, A, D, F>
where
    V: Storable<D> + 'static,
    A: Storable<D> + Annotation<V>,
    D: DB,
    F: FnMut(&Sp<V, D>) + Send + 'static,
{
    fn rep(&self) -> TypeRep {
        self.rep
    }

    fn probe_root(&self, arena: &Arena<D>, key: &ArenaKey<D::Hasher>) -> bool {
        let (children, bytes) = {
            let obj = match arena.with_backend(|be| be.get(key).cloned()) {
                Some(o) => o,
                None => return false,
            };
            if obj.data.is_empty() {
                return false;
            }
            (obj.children.clone(), obj.data.clone())
        };

        let mut reader = std::io::Cursor::new(bytes);
        let mut child_iter = children.into_iter();
        let loader = BackendLoader::new(arena, Some(0));

        <Node<V, D, A> as Storable<D>>::from_binary_repr(&mut reader, &mut child_iter, &loader)
            .is_ok()
    }

    fn step(
        &self,
        arena: &Arena<D>,
        root: &ArenaKey<D::Hasher>,
        prior_token: Option<&[u8]>,
        budget_values: usize,
    ) -> Result<WalkOutcome<Option<Vec<u8>>>, ResumeError<D>> {
        let root_sp = arena
            .get_lazy_unversioned::<Node<V, D, A>>(root)
            .map_err(ResumeError::Io)?;
        let trie = MerklePatriciaTrie::<V, D, A>(root_sp);

        let prior = prior_token.map(|b| ResumeToken::from_bytes(b.to_vec()));
        let mut h = self.handler.lock().unwrap();

        // Walkthrough: where we drop into the MPT specific stuff
        match trie.resumable_map(prior.as_ref(), budget_values, |v| (h)(v))? {
            WalkOutcome::Finished { visited } => Ok(WalkOutcome::Finished { visited }),
            WalkOutcome::Suspended { visited, snapshot } => Ok(WalkOutcome::Suspended {
                visited,
                snapshot: Some(snapshot.as_bytes().to_vec()),
            }),
        }
    }
}

/// @Thomas: This is the "other than MPTs" category
pub struct ObjectHandler<T, D: DB, F>
where
    T: Storable<D> + 'static,
    F: FnMut(&Sp<T, D>) + Send + 'static,
{
    rep: TypeRep,
    f: std::sync::Mutex<F>,
    _pd: std::marker::PhantomData<(T, D)>,
}

impl<T, D, F> ObjectHandler<T, D, F>
where
    T: Storable<D> + 'static,
    D: DB,
    F: FnMut(&Sp<T, D>) + Send + 'static,
{
    pub fn new(rep: TypeRep, f: F) -> Self {
        Self {
            rep,
            f: std::sync::Mutex::new(f),
            _pd: std::marker::PhantomData,
        }
    }
}

impl<T, D, F> DagHandler<D> for ObjectHandler<T, D, F>
where
    T: Storable<D> + 'static,
    D: DB,
    F: FnMut(&Sp<T, D>) + Send + 'static,
{
    fn rep(&self) -> TypeRep {
        self.rep
    }

    fn probe_root(&self, arena: &Arena<D>, key: &ArenaKey<D::Hasher>) -> bool {
        let (children, bytes) = match arena.with_backend(|be| be.get(key).cloned()) {
            Some(o) => (o.children.clone(), o.data.clone()),
            None => return false,
        };

        let mut rdr = std::io::Cursor::new(bytes);
        let mut child_iter = children.into_iter();
        let loader = crate::arena::BackendLoader::new(arena, Some(0));

        <T as Storable<D>>::from_binary_repr(&mut rdr, &mut child_iter, &loader).is_ok()
    }

    fn step(
        &self,
        arena: &Arena<D>,
        root: &ArenaKey<D::Hasher>,
        prior: Option<&[u8]>,
        budget_values: usize,
    ) -> Result<WalkOutcome<Option<Vec<u8>>>, ResumeError<D>> {
        if prior.is_some() {
            return Ok(WalkOutcome::Finished { visited: 0 });
        }
        if budget_values == 0 {
            return Ok(WalkOutcome::Suspended {
                visited: 0,
                snapshot: Some(vec![1]),
            });
        }
        let sp = arena
            .get_lazy_unversioned::<T>(root)
            .map_err(ResumeError::Io)?;
        (self.f.lock().unwrap())(&sp);
        Ok(WalkOutcome::Finished { visited: 1 })
    }
}

pub enum WalkOutcome<Resume> {
    Finished { visited: usize },
    Suspended { visited: usize, snapshot: Resume },
}

#[derive(Debug)]
/// Something went wrong during suspendable tree-walk resumption
pub enum ResumeError<D: DB> {
    /// Some IO error
    Io(std::io::Error),
    /// Source root mismatch
    WrongSourceRoot {
        /// The snapshot expected this source root…
        expected: ArenaKey<<D as DB>::Hasher>,
        /// …but the live trie has this root.
        found: ArenaKey<<D as DB>::Hasher>,
    },
}

impl<D: DB> From<std::io::Error> for ResumeError<D> {
    fn from(value: std::io::Error) -> Self {
        ResumeError::Io(value)
    }
}
