# `storage-core` Changelog

## Version `1.2.0`

- feat: add incremental garbage collector, running in a time-bounded way. This requires databases to support a new scan operation.
- feat: allow parityDB to use existing instance
- fix: removed race condition from `force_as_arc`
- fix: prevent a panic in `Sp` serialization with a mix of 'promoted' and 'unpromoted' keys.
- fix: correct `Sp::into_tracked` behaviour
- feat: allow shared parity_db backend through generic Dere
- fix: remove pending Update from memory before cache_insert_new_key in get()
- fix: Respect lock ordering in `force_as_arc`

## Version `1.1.0`

- feat: add layout version 2, which removes reference counting. For now, it disables garbage collection as well.

## Version `1.0.2`

- feat: optimised Sp allocations to minimise cache use and disk interactions

## Version `1.0.1`

- fix: lazy loading of embedded small nodes

## Version `1.0.0`

- Initial tracked release
