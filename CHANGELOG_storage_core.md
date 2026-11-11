# `storage-core` Changelog

## Unreleased

- fix: a failed `Sp::downcast` no longer leaks a metadata reference count for its root, which kept the key out of reach of the garbage collector.

## Version `1.2.3`

- fix: revert to `sha2` 0.10 / `digest` 0.10. `1.2.2` exposed `digest` 0.11
  through `WellBehavedHasher` and `ArenaHash`, which broke published
  `storage` 2.0.x sharing this crate.
- feat: `DagMemo` caches a function's result per distinct node, and
  `Monoid` combines results, for walks over shared DAGs in linear time.
- feat: `Storable::check_invariant_memoized` shares memos across the checks of
  one decode, so a node reached by many paths is checked once. It defaults to
  `check_invariant`. `storage-macros` 1.0.1 derives it from
  `#[storable(invariant_memoized = ...)]`.
- feat: `map_key` tells `storage` how to check `Map` keys, and `ArenaHash`
  implements `EncodingShape`.
- security: in-lined (`ArenaKey::Direct`) children now count towards the
  recursion limit when deserializing a storage graph, so a deeply nested graph
  of small nodes can no longer overflow the stack. In-lined values are also
  memoized for the lifetime of the loader.
- security: reusing an already-deserialized (memoized) node now counts its
  height towards the recursion limit, so a graph that shares nodes can no
  longer decode to a value nested deeper than the limit.
- security: deserializing a storage graph hashes each node once, rather than
  once per reference to it, so a node referenced many times no longer costs
  time quadratic in the encoding.

## Version `1.2.2`

Skips `1.2.1`, published from the `ledger-8` line.

- pull in hardened `serialize` and `base-crypto` deserialization.

## Version `1.2.0`

- feat: add incremental garbage collector, running in a time-bounded way. This requires databases to support a new scan operation.
- feat: allow parityDB to use existing instance
- fix: removed race condition from `force_as_arc`
- fix: prevent a panic in `Sp` serialization with a mix of 'promoted' and 'unpromoted' keys.
- fix: correct `Sp::into_tracked` behaviour
- feat: allow shared parity_db backend through generic Dere
- fix: remove pending Update from memory before cache_insert_new_key in get()
- fix: Respect lock ordering in `force_as_arc`
- fix: hold metadata lock across `track_lazy` and `Sp::lazy` in `BackendLoader::get` lazy path to prevent a panic when a concurrent drop removes the ref-counted entry between the two calls

## Version `1.1.0`

- feat: add layout version 2, which removes reference counting. For now, it disables garbage collection as well.

## Version `1.0.2`

- feat: optimised Sp allocations to minimise cache use and disk interactions

## Version `1.0.1`

- fix: lazy loading of embedded small nodes

## Version `1.0.0`

- Initial tracked release
