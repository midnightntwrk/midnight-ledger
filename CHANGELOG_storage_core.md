# `storage-core` Changelog

## Version `1.2.0`

- feat: add incremental garbage collector, running in a time-bounded way. This requires databases to support a new scan operation.
- feat: add GC-root metadata (behind `gc-v1`): `Sp::persist_with_metadata` / `StorageBackend::persist_with_metadata` attach an opaque value (e.g. a block hash) when the tag is known at persist time; `StorageBackend::set_root_metadata` tags an already-persisted root without changing its persist count. `StorageBackend::unpersist_by_metadata` later releases matching roots given only those values -- zero-clamped, in one backend borrow, with no caller-side index. Metadata is staged in the write cache and committed in the same `batch_update` as any persist-count change, and deleted on flush when the count drops to zero. Backed by a new `root_metadata` table in SQLite, and the previously reserved column 2 in ParityDb.
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
