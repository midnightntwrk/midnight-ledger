//! discovery.rs
#![allow(missing_docs)]

use crate::{
    arena::{Arena, ArenaKey},
    dag_type::{DagHandler, TypeRep},
    db::DB,
};
use serialize::{Deserializable, Serializable};

/// A discovery yield: a root that matches a registered flavour.
pub struct DiscoveryYield<D: DB> {
    pub root: ArenaKey<D::Hasher>,
    pub rep: TypeRep,
}

/// Resumable token for discovery: DFS frontier + roots already emitted.
// Walkthrough 3:
// Discovery cursor: Cursor over the dag during discovery
// Plan cursor: Cursor over the ordered vec of discovered roots
// Resume token: Cursor over a specific MPT
#[derive(Clone, Debug)]
pub struct DiscoveryToken<D: DB> {
    dag_root: ArenaKey<D::Hasher>,
    /// Stack of (node, next_child_idx)
    stack: Vec<(ArenaKey<D::Hasher>, u8)>,
    /// Roots already yielded
    claimed_roots: Vec<ArenaKey<D::Hasher>>,
}

/// Walkthrough: Discovery of root -> type mappings
/// Note that this is DAG object-type agnostic
pub struct Discovery<D: DB> {
    arena: Arena<D>,
    dag_root: ArenaKey<D::Hasher>,
    flavours: Vec<Box<dyn DagHandler<D>>>,
}

impl<D: DB> Discovery<D> {
    pub fn new(
        arena: Arena<D>,
        dag_root: ArenaKey<D::Hasher>,
        flavours: Vec<Box<dyn DagHandler<D>>>,
    ) -> Self {
        Self {
            arena,
            dag_root,
            flavours,
        }
    }

    /// Run a budgeted discovery step
    pub fn step(
        &self,
        token: Option<&DiscoveryToken<D>>,
        mut budget: usize,
    ) -> std::io::Result<(Vec<DiscoveryYield<D>>, DiscoveryToken<D>)> {
        let mut stack = token
            .map(|t| t.stack.clone())
            .unwrap_or_else(|| vec![(self.dag_root.clone(), 0)]);
        let mut claimed: std::collections::HashSet<_> = token
            .map(|t| t.claimed_roots.iter().cloned().collect())
            .unwrap_or_default();

        let mut outputs = Vec::new();

        while let Some((key, idx)) = stack.pop() {
            if budget == 0 {
                stack.push((key, idx));
                break;
            }
            budget -= 1;

            if let Some((rep, root)) = classify(&self.arena, &self.flavours, &key) {
                if claimed.insert(root.clone()) {
                    outputs.push(DiscoveryYield { root, rep });
                }
            }

            let children = self.arena.children_of(&key)?;
            if (idx as usize) < children.len() {
                stack.push((key, idx + 1));
                stack.push((children[idx as usize].clone(), 0));
            }
        }

        let token = DiscoveryToken {
            dag_root: self.dag_root.clone(),
            stack,
            claimed_roots: claimed.into_iter().collect(),
        };
        Ok((outputs, token))
    }
}

fn classify<D: DB>(
    arena: &Arena<D>,
    flavours: &[Box<dyn DagHandler<D>>],
    key: &ArenaKey<D::Hasher>,
) -> Option<(TypeRep, ArenaKey<D::Hasher>)> {
    if let Some(obj) = arena.with_backend(|be| be.get(key).cloned()) {
        if obj.children.len() == 1 && obj.data.len() >= 4 {
            if let Ok(rep) = TypeRep::deserialize(&mut std::io::Cursor::new(&obj.data), 0) {
                let inner = obj.children[0].clone();
                if flavours
                    .iter()
                    .any(|f| f.rep() == rep && f.probe_root(arena, &inner))
                {
                    return Some((rep, inner));
                }
            }
        }
    }
    for f in flavours {
        // Ask each handler if this key decodes as this flavour's expected root type
        if f.probe_root(arena, key) {
            return Some((f.rep(), key.clone()));
        }
    }
    None
}

impl<D: DB> Serializable for DiscoveryToken<D>
where
    ArenaKey<D::Hasher>: Serializable,
{
    fn serialize(&self, w: &mut impl std::io::Write) -> std::io::Result<()> {
        self.dag_root.serialize(w)?;
        let n = u32::try_from(self.stack.len()).map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "frontier too large")
        })?;
        n.serialize(w)?;
        for (k, idx) in &self.stack {
            k.serialize(w)?;
            (*idx as u8).serialize(w)?;
        }
        let m = u32::try_from(self.claimed_roots.len()).map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "claimed_roots too large")
        })?;
        m.serialize(w)?;
        for k in &self.claimed_roots {
            k.serialize(w)?;
        }
        Ok(())
    }

    fn serialized_size(&self) -> usize {
        self.dag_root.serialized_size()
            + 4
            + self
                .stack
                .iter()
                .map(|(k, _)| k.serialized_size() + 1)
                .sum::<usize>()
            + 4
            + self
                .claimed_roots
                .iter()
                .map(|k| k.serialized_size())
                .sum::<usize>()
    }
}

impl<D: DB> Deserializable for DiscoveryToken<D>
where
    ArenaKey<D::Hasher>: Deserializable,
{
    fn deserialize(r: &mut impl std::io::Read, depth: u32) -> std::io::Result<Self> {
        let dag_root = ArenaKey::<D::Hasher>::deserialize(r, depth)?;

        let n = u32::deserialize(r, depth)? as usize;
        let mut frontier = Vec::with_capacity(n);
        for _ in 0..n {
            let k = ArenaKey::<D::Hasher>::deserialize(r, depth)?;
            let idx = u8::deserialize(r, depth)?;
            frontier.push((k, idx));
        }

        let m = u32::deserialize(r, depth)? as usize;
        let mut claimed_roots = Vec::with_capacity(m);
        for _ in 0..m {
            claimed_roots.push(ArenaKey::<D::Hasher>::deserialize(r, depth)?);
        }

        Ok(Self {
            dag_root,
            stack: frontier,
            claimed_roots,
        })
    }
}
