---
title: Storage IO write/delete cost modelling
author: Nathan Collins <nathan.collins@gmail.com>
date: 2025-08-28
---

This document describes how to cost model io. This is separated into write+delete costing, which happens globally/at-the-end at the level of whole tx/contract evaluations, and read costing, which happens locally/incrementally at the level of individual contract eval steps including vm operations.

Below we distinguish between "global" costing, meaning at the end of a tx, before deciding whether to commit, and "local" costing, meaning incrementally while the tx is being evaluated, e.g. during each vm operation. All cpu and io read costing happens locally, whereas all io write+delete costing happens globally.

## Summary of clarifications vs [cost model spec](./cost-model.md)

- io write+delete costing *only* happens *globally*, never locally.
- io read costing *only* happens *locally*, never globally.
- in all cases for cpu costing, and whenever possible for reads (everywhere except `idx`? see details in [costing reads](#costing-reads)) we want to cost *before* evaluating the vm op, and fail fast if the incremental cost would exceed the budget.
- crypto operations: there is no io read costing, only cpu costing, associated with crypto operations (e.g. `CostModel::proof_verify`, `::verifier_key_load`, `::transient_hash`).
- `tree_copy`: nothing to model! There is no io read costing or cpu costing of the `tree_copy` operation. Rather, it will only be costed globally as a write (the implicit cpu cost of setting up this write will be covered by the relatively expensive write cost itself, and will only happen after committing the tx anyway). The cost model should still have a `tree_copy` costing function, but it will return the "identity" cost.
- `time_filter_map`: unlike for the container data structures (`bmt`, `array`, `map`), the `time_filter_map_*` costing is not related to vm operations:
  - rather, we need to create separate benchmarks to learn the cpu cost-model of these operations.
  - but like for the other data structures, the io read costing of these operations will be determined by estimating the number and size of sync reads, with some possible run time input (as in the case of container lookups with `idx` described above).

# Costing writes and deletes

Here's a solution to our "bounding bytes written and deleted" problem[^problem], that allows for zero cost rollbacks to earlier committed chain states[^rollback]. The key insight is that we can completely separate logical gc for costing from actual gc that mutates db: in the tx we estimate the gc that will happen later in the future once the current tx falls out of the rollback window, but actually performing that gc is outside of any tx and not part of consensus (I think we prefer this anyway).

[^problem]: The high-level cost-model spec in PR#141 assumed that we could do *incremental* costing of writes and deletes. However, because our state data structures are implemented using shared Merkle dags -- to allow for cheap state copies, for example -- it's not possible to efficiently predict the incremental writes+deletes effect of a state-changing vm operation, because the writes+deletes effect depends on the current state of the *full* underlying Merkle dag. So, we moved to a solution that does whole-tx write+delete costing, using metadata about the full Merkle dag referenced by the tx to achieve sufficient time efficiency.

[^rollback]: I'm assuming a "rollback window" of some previous blocks as the possible states to rollback to. For each of these blocks, the only requirement is that the roots of the contract states at the end of those blocks are still in the db. This implies that outside of consensus, nodes maintain a set of gc roots for their underlying db, corresponding to state roots in blocks the node wants to be able to rollback to.

## Assumptions

I'm assuming our current storage, as currently implemented:
- Merkle dags, where hashing captures graph structure, but not ref counts; indeed, the underlying db having ref counts is not relevant here for costing, since we compute our own ref counts from the point of view of isolated contract states, whereas the underlying db shares nodes that occur in the storage of distinct contracts.
- persisted gc roots, which prevent the underlying (out of consensus) gc from cleaning up any nodes reachable from these roots.
- possible tmp objects -- i.e. nodes that are not reachable from any currently persisted gc roots; an earlier version of this proposal needed to disallow tmp objects.

I'm assuming txs can only lookup keys in their local state, and that local state must be computed by the contract itself, locally. In particular, I'm assuming txs cannot lookup arbitrary keys in the underlying global storage.

I also need to make one addition to the consensus state of each contract:
- a set `K` of all keys that the contract is currently "charged for", along with some metadata about these "charged" keys.

But first before describing the costing solution, let's motivate why we need `K`, by considering an idealized version of our problem.

## Idealized solution and motivation for `K`

Suppose we evaluate a new tx for some contract with current root set `R0`, and at the end of the tx we have the new root set `R1`. By "root set" here, we mean the roots of the Merkle dags corresponding to the contract state. Our goal is to compute how many bytes would be written and deleted if we updated the underlying storage to store everything reachable from `R1`, and then deleted anything no longer reachable, assuming the storage only contained nodes reachable from `R0` or `R1`.

More precisely, let `reachable(keys: Set<Key>) -> Set<Key>` be a function that computes the Merkle dag closure of the key set `keys`. I.e., `reachable(keys)` is the set all keys reachable by following zero or more edges from keys in `keys`[^keypun]. Then our goal is to compute

    written_keys = reachable(R1) \ reachable(R0)
    deleted_keys = reachable(R0) \ reachable(R1)
    written_bytes = sum k.node_size for k in written_keys
    deleted_bytes = sum k.node_size for k in deleted_keys

[^keypun]: Here and elsewhere we pun the distinction between keys and nodes, since they're in 1-to-1 correspondence. I.e., here, by "following an edge from a key" we of course mean following an edge from the node with that key.

Unfortunately, this simple goal is not feasible in practice, because a tx can induce an unbounded number of deleted nodes by dropping a very large contract state that was built up over many prior txs. I.e., we can't in general hope to compute the set `deleted_keys`. So, we need to approximate, and because we need to protect against malicious txs, we over approximate, meaning that at all times a contract is charged for *at least* as many bytes as it's currently storing.

So, we introduce a *consensus* set `K` of keys the contract is currently charged for, meaning all keys whose nodes have been costed as writes, but not credited as deletes. This set can be large -- it includes all keys in the state of the contract -- but it's only a fraction of the size of the state itself (no payloads or children), so hopefully this additional storage is acceptable. We can represent `K` using a Merkle dag, and store it in the underlying db just like our other Merkle dags, and making `K` part of consensus amounts to making the root key of `K` part of consensus. But in the discussion that follows, any mention of root keys (e.g. `R`, `R0`, `R1`) refers to the root keys corresponding to the contract state itself, not this additional metadata in `K`.

Because any new nodes in the final state of a tx must have been computed by the tx itself -- a tx cannot insert arbitrary keys into its state, but instead must compute its final state from its starting state using simple vm operations -- we can compute a sound approximation to new keys `written_keys` in `O(time taken by tx)` by simple graph search from roots in `R1`, truncating our search whenever we reach a key in the currently charged set `K`, which itself contains `R0`: this may skip some keys that were not reachable from `R0`, but since they're in `K` they're currently charged and don't need to be charged again. But when trying to approximate `deleted_keys`, not only can't we compute the whole set, but worse, we don't a priori have any efficient test to tell if some key in `reachable(R0)` is not in `reachable(R1)`. To get an efficient test, we extend `K` with metadata about currently costed keys, including their reference counts from the point of view of `K`.

I.e., the metadata we need for the keys in `K` are:
- contract-local ref counts (explained in "invariants" below), and so `K: RcMap` where we can lookup the contract-local ref count of key `k` as `K[k]: RefCount`, where `RefCount` is some unsigned int type.
- a way to efficiently find the keys in `K` with ref count zero, i.e. keys `k` s.t. `K[k] == 0`, which we call "unreachable keys"; for simplicity in the pseudo code presentation below, we pretend `K` is a simple map, but in the implementation we'll need something more complex[^rcmap-opt][^rcmap-ref].

[^rcmap-opt]: The simplest thing with sufficient performance would be something like `struct RcMap { rcs: Map<Key, RefCount>, unreachable: Set<Key> }`. However, if we also want to optimize the storage size of `RcMap`s -- they are part of consensus -- something like

    ```
    struct RcMap {
      // Keys in the map with rc == 0
      rc_zero: Set<Key>,
      // Keys in the map with rc == 1
      rc_one: Set<Key>,
      // Map from keys to their rcs, when rc >= 2
      rc_ge_two: Map<Key, RefCount>
    }
    ```

    would be better: we don't waste space storing rcs of zero or one, which are the most common cases, and only store actual rcs which are at least 2, which should be relatively rare.

[^rcmap-ref]: The `RcMap` implementation requires careful handling to ensure that the dag node info for charged keys remain accessible. There are two approaches to address this:

    1. Backend persistence approach: ensure that storage of a key in the `RcMap` implies it remains in the backend storage. By using an `ArenaKey` wrapper with a custom `Storable` implementation, we can do this with zero storage overhead in the `RcMap`. The first implementation took this approach.

    2. Self-contained approach: avoid backend dependency entirely by storing the key, its children, and node size directly in the `RcMap`. This allows garbage collection to operate purely on the `RcMap` data without backend lookups, but significantly increases the size of the `RcMap`.

We also need some invariants on `K`, where we pun `K` as a simple set of keys as needed:
- `K` is **child-closed**, which we define by `reachable(K) == K`.
- the ref counts in `K` correspond to the graph induced by `K`: if `k` in `K`, then `K[k] = |{ l in K: k in l.children }|.
- `K` soundly approximates the contract state; i.e. if the contract state has root set `R`, then we require `reachable(R) ⊆ K`.

## Real solution using `K`

Suppose we evaluate a new tx for some contract with current charged keys `K0: RcMap` and current root set `R0: Set<Key>`, and at the end of the tx we have the new root set `R1: Set<Key>`. Our goal is
- compute a new key set `K1: RcMap` satisfying the invariants w.r.t `R1`, in particular s.t. `reachable(R1) ⊆ K1`.
- compute newly written nodes as `K1 \ K0`, and charge the tx for their bytes.
- compute newly deleted nodes as `K0 \ K1`, and credit the tx for their bytes.

Towards this end, we define some helper functions for costing, which will be explained in detail below:

    // Compute keys reachable from r1 not costed in k0
    fn get_writes(k0: RcMap, r1: Set<Key>) -> Set<Key>;
    // Compute update of k0 by extending with keys in get_writes()
    fn update_rcmap(k0: RcMap, writes: Set<Key>) -> RcMap;
    // Iteratively gc the updated rcmap, preserving keys reachable from r1,
    // returning gc'd keys and updated rc map with gc'd keys removed
    fn gc_rcmap(updated_rcmap: RcMap, r1: Set<Key>) -> (RcMap, Set<Key>);

We can then compute the write+delete counts and new charged-key set `K1` as follows:

    keys_added = get_writes(K0, R1)
    K = update_rcmap(K0, keys_added)
    (K1, keys_removed) = gc_rcmap(K, R1)
    // Account for bytes that would be written and deleted by
    // storing new state and releasing old state
    bytes_written = sum k.node_size for k in keys_added
    bytes_deleted = sum k.node_size for k in keys_removed
    // Account for bytes written and deleted as part of `K` itself.
    // May need to further take into account bookkeeping related
    // to efficient computation of zero-ref-count keys in `K`.
    bytes_per_key_in_K = /* TBD: some function of `K` */
    bytes_written += keys_added.length() * bytes_per_key_in_K
    bytes_deleted += keys_removed.length() * bytes_per_key_in_K

To implement `get_writes(K0, R1)`, note that
- since `K0` is child-closed, and all keys in `K0` are charged, the uncharged writes are `reachable(R1) \ K0`, which we can compute by graph search from `R1`, truncating the search at nodes in `K0`.
- any new nodes in `reachable(R1) \ K0` were constructed while the tx was running, so we have time to run this graph search to completion.

So, we define `get_writes` by

```
// Returns keys in `reachable(R1) \ K0`.
//
// The returned keys are the uncharged nodes written by the current tx.
fn get_writes(K0: RcMap, R1: Set<Key>) -> Set<Key>:
  let Q = R1
  let keys_added = empty set
  while k = pop Q:
    if k not in (K0 + keys_added):
      keys_added += { k }
      Q += k.children
  return keys_added
```

Next, to implement `update_rcmap(K0, keys_added)`, we first need to clarify what exactly it does: assuming `K0` contains accurate ref counts for all keys in `K0`, i.e.

    K0[k] = |{ l in K0: k in l.children }|

for all `k` in `K0`, we want to compute the corresponding `RcMap` for all keys in `K0 ∪ keys_added`. Since we assume `K0` is child-closed, and because we computed `keys_added` by exhaustive search until truncation at nodes in `K0`, we see that `K0 ∪ keys_added` is also child-closed.

Let's first consider how to compute an accurate `RcMap` from scratch for a child-closed set of keys `keys`, and then figure out how to adapt this to extending an existing `RcMap` for a child-closed set `K0` to a new child-closed set `K0 ∪ keys_added`. To compute `m: RcMap` for the child-closed key set `keys` from scratch is simple:

```
m = empty RcMap
for k in keys:
  m[k] = 0
for k in keys:
  for c in k.children:
    m[c] += 1
```

Why is this correct:
- every key in `m` needs an entry in `m`, so we initialize all entries to zero.
- since `keys` is child-closed, we know that for all `k` in `keys`, all keys in `k.children` are also in `keys`. So, the lookups `m[c]` in the increments are well defined.
- the `m[c] += 1` increments the ref count of `c` for each incoming edge to `c` induced by keys in `keys`, so each edge in the graph induced by `keys` is accounted for.

Now, to incrementally update the existing `K0` by the keys `keys_added`, we just observe that the initializations `m[k] = 0` above can be interspersed with the updates `m[c] += 1`, as long as we respect topological ordering: i.e. we don't update a ref count for a key we haven't initialized yet, or reinitialize a ref count for key we've already incremented. But all keys in `keys_added` are topologically prior to all keys in `K0`, and `keys_added` is disjoint from `K0`, since `K0` is child-closed and `keys_added` was produced by a search that truncates at `K0`. So, we define `update_rcmap` by

```
// Return `RcMap` gotten by updating `K0` with rc updates induced by nodes in `keys_added`.
//
// Assumes:
// - `K0` satisfies the `RcMap` invariants
// - there is no edge from `K0` to `keys_added`
// - `K0` and `keys_added` are disjoint
// - `K0 ∪ keys_added` is child-closed
fn update_rcmap(K0: RcMap, keys_added: Set<Key>) -> RcMap:
  for k in keys_added:
    K0[k] = 0
  for k in keys_added:
    for c in k.children:
      K0[c] += 1
  return K0
```

Finally, we need to define `gc_rcmap`:
- given `K: RcMap`, the goal of `gc_rcmap(K, R1)` is to compute nodes in the graph induced by `K` that are not reachable from `R1`, remove those nodes from `K`, and return the removed nodes along with the updated `K`.
- because it might not be possible to compute all such nodes fast enough, `gc_rcmap` need to work iteratively and quit early if resource limits are reached.
- because `K` contains accurate ref counts for the graph induced by its keys, we proceed by simulating incremental gc on `K`, removing zero ref-count nodes which aren't in `R1`, and decrementing the ref counts of their children.
- because `reachable(R1) ⊆ K`, and we avoid removing nodes in `R1`, we also avoid removing nodes in `reachable(R1)`, since such nodes are either in `R1`, or have non-zero ref counts induced by `reachable(R1)`.

So, we define `gc_rcmap` by

```
// Returns subset of keys in `K \ reachable(R1)`, along with an
// update of `K` with those keys removed.
//
// This is an incremental gc on `K`:
// - it is resource bounded, and so not guaranteed to remove all
//   possible "garbage" in `K`
// - running it again against the same root set, but on the
//   rcmap output by the previous run would make more progress
//   if possible, e.g.
//   `k = gc_rcmap(k, r1); k = gc_rcmap(k, r1); k = gc_rcmap(k, r1); ...`
//
// Assumes:
// - `K` is child-closed
// - `K` contains accurate ref counts for the graph induced by its keys
// - `reachable(R1) ⊆ K`
fn gc_rcmap(K: RcMap, R1: Set<Key>) -> (RcMap, Set<Key>):
  // Invariant: keys in `Q` have ref count zero and aren't in `R1`
  let Q = { k in K: K[k] == 0 and k not in R1 }
  // Invariant: `keys_removed` is all garbage identified so far
  let keys_removed = empty set
  // Make sure we don't run too long; could be dynamically bounded by
  // remaining gas, but here I've assumed a global limit for simplicity;
  // Anything deterministic will be consistent with consensus.
  let step = 0
  while k = pop Q and step < GC_STEP_LIMIT:
    step += 1
    keys_removed += { k }
    K -= { k }
    for c in k.children:
      K[c] -= 1
      if K[c] == 0 and c not in R1:
        Q += { c }
  return (K, keys_removed)
```

And that's it!

Some notes:
- when we call `gc_rcmap(K, R1)` in the cost calcuation, everything in `K0` is currently charged, and we're in the process of charging the keys `keys_added`, so the keys in `K = update_rcmap(K0, keys_added)` are considered charged.
- a later tx may add back some keys to the state `reachable(R1)` which are not in `reachable(R0)` without paying for them, but this is ok, because those keys are still in `K0`, meaning still charged but not credited; as mentioned before, the full `gc_rcmap()` computation can be `Omega(|state reachable from R0|)` in time, which is not bounded by `O(time taken by tx)`, and so we can't assume we can run it to completion.

## Implementation notes

Above we described ideas for implementing the `RcMap` data structure, including optimizations. There is also the question of *where* the global write+delete cost calculations will happen. The answer is that these should happen at the level where the vm is called: the input to the vm includes a starting stack with the starting contract state, and the output of the vm is a final stack which includes the ending contract state. So assuming we've plumbed the rcmap `K0` to the place where the vm is run, we can do the write+delete computations described above in the same place.

# Costing reads

At a high level, the io read costing will be in terms of number and size of sync reads. A sync read means one that the contract eval will block on. The size will be measured in 4k blocks. These sync reads will be converted to synthetic time based on their size (in 4k blocks), presumably with some per-read overhead. I.e. the cost of a sync read of `b` 4k blocks will have a cost like

    cost = time_cost_per_sync_read + b * time_cost_per_4k_block

with the idea that a sync read itself is expensive, but then adding more blocks to an existing read is relatively cheap.

For io read costing container ops (e.g. map insert, delete, lookup), we want to estimate the total number and size of these sync reads. We can think about this in terms of paths in the underlying mpt: to lookup, insert, or delete, we need to traverse a path in the mpt, and read the nodes on that path one-by-one in sync reads. Finally, in the case of lookup, we need to read the root node of the result.
- for the path traversal, we'll assume all nodes fit in a single block and charge one sync read of one block per node.
- for lookup (`idx`) specifically, we'll cost the value looked up as the size in blocks (i.e. `ceil(size in bytes / 4k)`) of the root node of the result. If the result is a cell, then the root node will be the whole cell, and may be up to 32KiB (as defined by `onchain-state::state::CELL_BOUND`). If the result is a container, then we can just assume the root node fits in a single block (altho if we're measuring cell results anyway, might be simpler implementation wise to just measure uniformly, indep of result type).

The vm has a notion of cached reads, and for these we assume they're cached in memory and induce *no* io read cost. The vm knows what's cached and can compute accordingly. TODO: clarify implications for cost-modeling with per-container-op costing - does this costing depend on vm state, or is it separate from vm op costing and assumes no caching?

In the vm, for per-vm-op costing, we'll compute cpu cost and io read cost separately: the cpu cost will be computed *before* evaluating the vm op, but the io read cost will be computed *after* for `idx`, since we need the actual value looked up in the case of `idx`. In most (all?) other cases we can compute both cpu and io-read cost *before* executing the vm op (for path lengths we just estimate by the log size of the container, ignoring the actual key/path being looked up).
