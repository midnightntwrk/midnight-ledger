//! migration_plan.rs
#![allow(missing_docs)]

use crate::{
    arena::{Arena, ArenaKey, Sp},
    dag_type::{DagHandler, MptHandler, ObjectHandler, ResumeError, TypeRep, WalkOutcome},
    db::DB,
    merkle_patricia_trie::Annotation,
    storable::Storable,
};
use std::marker::PhantomData;

/// Resume token for `Plan::step`
///
/// `root_idx`: which MPT root in the plan is to be migrated next
/// `inner`: MPT `ResumeToken` bytes (if we're resuming part-way through an MPT walk)
// Walkthrough:
// Discovery cursor: Cursor over the dag during discovery
// Plan cursor: Cursor over the ordered vec of discovered roots
// Resume token: Cursor over a specific MPT
#[derive(Clone, Debug)]
pub struct PlanToken<H> {
    root_idx: u32,
    inner: Option<Vec<u8>>,
    _phantom: PhantomData<H>,
}

impl<H> PlanToken<H> {
    fn new(root_idx: u32, inner: Option<Vec<u8>>) -> Self {
        Self {
            root_idx,
            inner,
            _phantom: PhantomData,
        }
    }
}

// Walkthrough 5:
struct Entry<D: DB> {
    root: ArenaKey<D::Hasher>,
    ty: Box<dyn DagHandler<D>>,
}

/// A built migration plan: iterate known roots and apply type handlers
// Walkthrough 5.5: The list of mappings of root -> ty
pub struct Plan<D: DB> {
    entries: Vec<Entry<D>>,
}

impl<D: DB> Plan<D> {
    /// Start a new plan for a given DAG root (kept only for provenance/logging).
    pub fn new() -> Self {
        Self { entries: vec![] }
    }

    // Walkthrough 6: This is where we say how to handle an MPT of a given type
    /// For MPTs with values `(Sp<K>, Sp<V>)` with annotation `A`.
    pub fn register_mpt_kv_handler<K, V, A, F>(
        mut self,
        mpt_root: ArenaKey<D::Hasher>,
        rep: TypeRep,
        handler: F,
    ) -> Self
    where
        K: Storable<D> + serialize::Serializable + serialize::Deserializable + Clone + 'static,
        V: Storable<D> + serialize::Serializable + serialize::Deserializable + Clone + 'static,
        A: Storable<D> + Annotation<(Sp<K, D>, Sp<V, D>)> + 'static,
        F: FnMut(&Sp<(Sp<K, D>, Sp<V, D>), D>) + Send + 'static,
    {
        let ty = MptHandler::<(Sp<K, D>, Sp<V, D>), A, D, F>::new(rep, handler);
        self.entries.push(Entry {
            root: mpt_root,
            ty: Box::new(ty),
        });
        self
    }

    pub fn register_mpt_value_handler<V, A, F>(
        mut self,
        mpt_root: ArenaKey<D::Hasher>,
        rep: TypeRep,
        handler: F,
    ) -> Self
    where
        V: Storable<D> + serialize::Serializable + serialize::Deserializable + Clone + 'static,
        A: Storable<D> + Annotation<V> + 'static,
        F: FnMut(&Sp<V, D>) + Send + 'static,
    {
        let ty = MptHandler::<V, A, D, F>::new(rep, handler);
        self.entries.push(Entry {
            root: mpt_root,
            ty: Box::new(ty),
        });
        self
    }

    pub fn register_object_handler<T, F>(
        mut self,
        root: ArenaKey<D::Hasher>,
        rep: TypeRep,
        f: F,
    ) -> Self
    where
        T: Storable<D> + serialize::Serializable + serialize::Deserializable + Clone + 'static,
        F: FnMut(&Sp<T, D>) + Send + 'static,
    {
        let h = ObjectHandler::<T, D, F>::new(rep, f);
        self.entries.push(Entry {
            root,
            ty: Box::new(h),
        });
        self
    }

    // Walkthrough 7:
    /// Run a budgeted step across all registered MPT roots, resuming from `prior` if provided.
    pub fn step(
        &self,
        arena: Arena<D>,
        prior: Option<&PlanToken<D::Hasher>>,
        budget_values: usize,
    ) -> Result<WalkOutcome<PlanToken<D::Hasher>>, ResumeError<D>> {
        let mut idx: usize = prior.map(|t| t.root_idx as usize).unwrap_or(0);
        let mut inner: Option<Vec<u8>> = prior.and_then(|t| t.inner.clone());

        if budget_values == 0 {
            return Ok(WalkOutcome::Suspended {
                visited: 0,
                snapshot: PlanToken::new(idx as u32, inner),
            });
        }

        let mut visited = 0usize;

        while idx < self.entries.len() && visited < budget_values {
            let entry = &self.entries[idx];
            // Original MPT walk happens here. Budget is shared.
            let step = entry.ty.step(
                &arena,
                &entry.root,
                inner.as_deref(),
                budget_values - visited,
            )?;

            match step {
                WalkOutcome::Finished { visited: v } => {
                    visited += v;
                    inner = None;
                    idx += 1;
                }
                WalkOutcome::Suspended {
                    visited: v,
                    snapshot,
                } => {
                    visited += v;
                    inner = snapshot;
                    return Ok(WalkOutcome::Suspended {
                        visited,
                        snapshot: PlanToken::new(idx as u32, inner),
                    });
                }
            }
        }

        if idx >= self.entries.len() {
            Ok(WalkOutcome::Finished { visited })
        } else {
            Ok(WalkOutcome::Suspended {
                visited,
                snapshot: PlanToken::new(idx as u32, inner),
            })
        }
    }
}
