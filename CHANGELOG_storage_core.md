# `storage-core` Changelog

## Unreleased

- feat(gc-v1): `DB::scan_keys` and `DB::batch_get_children`, with default implementations over `scan` and `batch_get_nodes`. The garbage collector's sweep now asks for keys only and its mark phase for child references only, so a backend that can serve those without loading objects can override them (#782, #783).
- perf(gc-v1): the incremental collector now honours its time budget inside the mark and sweep phases instead of only between whole batches. Database work is issued in chunks of at most 256 keys, with a budget check before each chunk and the last chunk sized from a recent-cost (EWMA) estimate; the in-cache mark is a depth-first work stack with a budget check every 1024 nodes and resumes where it stopped on the next call; the mark and root sets keep their allocation across cycles.
- feat(gc-v1): `StorageBackend::gc_with_limits` with `GcLimits { max_deletes }` caps the number of nodes one call deletes; the sweep resumes after the last processed key, so repeated calls still delete every unreachable node exactly once. `gc` is unchanged and applies no cap.
- fix(gc-v1): a call whose budget ran out right after finishing the mark phase, before any sweep chunk, no longer discards the finished mark and restarts the cycle.
- fix: a failed `Sp::downcast` no longer leaks a metadata reference count for its root, which kept the key out of reach of the garbage collector.

## Version `1.2.1`

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
