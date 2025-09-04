# `storage` Changelog

## Version `1.0.0`

- Add `storage::delta_tracking::RcMap` for reference-counted key tracking in
  write+delete I/O cost modeling

## Version `0.5.0`

- breaking: adopt breaking serialization changes
- breaking: Remove `base_storable!` macro, in favor of `#[storable(base)]`
  attribute for `#[derive(Storable)]`
- breaking: `#[derive(Storable)]` now also derives `Serializable` based on the
  `Sp` serialization of the type
- breaking: Switch `Sp` serialization encoding to use indexes instead of hashes
  for a more compact encoding, and ensure it is canonical on decoding.
- feat: Add enum and unit type support for `Storable`.
- bugfix: Fix bounds on Array and Merkle Tree

## Version `0.5.0`

- Remove array indexing syntax support (`array[index]`) for `Array` type. Use `array.get(index)` instead, which returns `Option<&T>` for safe access.
- Change storage `Array` type to support arbitrary size (before it was restricted to length at most 16 elements).
- Change storage `Array` API:
  - Change `Array::insert` to only supports *existing* indexes, returning `None` if index is out of bounds.
  - Add `Array::push` to append an element to the end of the array.
- Remove `Vec` storage type, replacing internal usage with storage `Array` type. The two types seemed to serve the same purpose, and without the length restriction on `Array`, `Array` can be used in all the same places as `Vec` was used before. In particular, note that `Vec::{from,to}_std_vec` correspond to `impl From<std::Vec> for Array` and `impl From<Array> for std::Vec` now.

## Version `0.4.1`

- Add `Vec` storage type on top of `Map`
- Add `HashSet` storage type on top of `Map`
- Add iterator for `HashMap`
- Add `fn keys` and `fn values` for `HashMap`
- Add into_iterator for `Map`

## Version `0.4.0`

- `MerklePatriciaTrie`'s store leaf values as `Sp`s
- Add macro to derive `Storable`
- Extend `default_storage` to include any requested `DB` type, with default storages initialized on demand.
- Add `set_default_storage` and `drop_default_storage` to allow changing the default storage.
- Make Merkle-ized data structures use implicit `default_storage`, instead of explicitly passing arenas.
- Add `WrappedDB` newtype to allow creating distinct default storages for the same underlying `DB` type.
- Add `Arena::with_backend` to allow access to the `StorageBackend` via the arena.
- Remove pass-thru functions in arena that passed thru to the backend, since now those backend functions can be called directly via `Arena::with_backend`.
- Change various `StorageBackend` functions from `pub` to `pub(crate)`, since they were never intended for external use.
- Remove public `Arena` and `StorageBackend` constructors: now lib users must construct `Storage` and then access the arena and backend via that.
- Make `Arena::get*` return `None` instead of crashing on unknown key.
- breaking: Renamed `MerklePatriciaTrie::lookup_arc` to `MerklePatriciaTrie::lookup_sp`
- Add `Sp::is_lazy` to allow checking if an `Sp` is lazy. This is useful for skipping sanity checks in `Storable::from_binary_repr` when the sanity check would otherwise force loading of a child `Sp`.
- Remove `MerkleTree` (moved to `transient-crypto`)
- Add `StorageBackend::get_stats()` that returns a new `StorageBackendStats` struct with stats about the backend, for use in performance tuning by backend users.
* breaking: `Storable` impl for `Vec` removed as unsafe
* `Backend` read cache can be unbounded.

## Version `0.3.5`
- Version bump to pull in breaking crypto changes.
- breaking: `Storable::from_binary_repr` now takes an argument of type `&mut impl Iterator<Item = ArenaKey>` instead of `&[ArenaKey]`

## Version `0.3.4`
- bugfix: Sp deserialization always forces a complete deserialization,
  and no longer depends on the current Arena state

## Version `0.3.3`

- Remove `Storable::Child` type, and change `<T as Storable>::children` to return a `Vec<ArenaKey>` instead of `Vec<T::Child>`.
- Remove `Storable::to_binary_repr` API, since all callers were eliminated.
- Add `Storable` impls for `(K,V)` and `Either<K,V>`, for `K, V: Storable`.
- Remove support for casting to/from `Sp<dyn Any + Send + Sync>`.
- Remove `DynStorable` trait.
- Extend `Sp` recursion tracking to mpt Node::MidBranchLeaf

## Version `0.3.2`

- Add `storage::Loader` trait and change `Storable::from_binary_repr` to receive a `Loader` instead of a hash map from keys to IRs. The point is that the loader is more abstract, generalizing the IR-based loading, loading from the backend, and lazy loading.
- Internal: add `IrLoader` and `BackendLoader` implementers of `Loader`, which load from `IntermediateRepr`s and `StorageBackend`, respectively.
- Add lazy loading for `Sp` smart pointers. This allows creation of `Sp`s that load their content on demand, as its accessed via `deref`. Associated changes:
  - The `*Eq` and `*Ord` traits for `Sp` now use O(1) equality based on hash. We expect `Sp<T>` to be used with types `T` with `*Eq` and `*Ord` that respect hash equality, so this shouldn't be a semantic change in practice.
  - Various trait bound changes for `Sp` and data types that embed `Sp`s. In particular, the `*Eq` and `*Ord` instances for `Sp<T>` now require `T: Storable`, because comparisons will force loading of lazy sps.
  - Removed `Sp::children` API: the pass-thru implementation on `T` for `Sp<T>` via deref coercion should do the same thing.
  - Added `Arena::get_lazy` API that creates a lazy sp from a hash.
  - Added `Sp::unload` which takes an existing sp and unloads it, replacing its root data with a lazy placeholder.
  - Altho not visible in the APIs, along the way we added a cache to `Arena`, which prevents creation of multiple copies of the data payload for distinct `Sp`s with the same hash. This is transparent to users, but could improve memory usage in some cases.
- Reverted breaking API change in 0.3.1 requiring `T: Storable` on `Array<T>`

## Version `0.3.1`

- `Sp`s track recursion during deserialization
- `MerkleTree`s and `MerklePatriciaTree`s track recursion during deserialization

## Version `0.3.0`

- Add `DB::get_unreachable_keys` API.
- Add `StorageBackend::gc` API.
- Rename `StorageBackend::insert` -> `cache`.
- Rename `StorageBackend::release` -> `unpersist`.
- Breaking change: begin versioning `Map` and `Array`
- Rename public `SqlDB` file-based constructor to `SqlDB::exclusive_file`, and change semantics so that only one instance can be live for a given file.
- Remove `Clone` parent trait from `DB` trait.

## Version `0.2.9`

- Fixed complexity explosion in deserializing nested `Sp`s

## Version `0.2.8`

- Added `HashMap`
- Various preparations to help nesting alloc issues
- Implement new caching logic that minimizes writing of temp values to the db in `StorageBackend`.
- Implement parent->child reference counting in `StorageBackend`.
- Add support for persisting GC roots in `StorageBackend`.
  - Add GC-root related APIs to `DB` trait.
  - Extend `DB::batch_update` API to support GC-root updates.
- Remove `Ord` parent trait from `Storable` trait.
- Split `StorageBackend` caches into separate read and write caches.
- Add `batch_get_nodes` and `bfs_get_nodes` APIs to `DB` trait.
- Implement `StorageBackend::pre_fetch` and call automatically from
  `StorageBackend::get`.
- Remove all `async` usage, including `futures::executor::block_on`; we no longer depend on Tokio.
- Change `StorageBackend::database` to be a `D: DB` instead of an `Option<D>` for `D: DB`, with corresponding constructor change.
- Fixed a bug where the hash of `Sp<T>` can change mid deserialization if the
  binary representation of `T` changes due to a minor version change.

## Version `0.2.7`

- Reinstated:
	- Fixed a bug where extensions of length over 255 nibbles were not deserializable
	- Fixed a bug which caused a panic when Map keys were a prefix/extension on existing keys

## Version `0.2.6`

- Fixed a bug where extensions of length over 255 nibbles were not deserializable
- Fixed a bug which caused a panic when Map keys were a prefix/extension on existing keys

Reverted due to a regression.

## Version `0.2.5`

- Fix `Map::keys` iterator.

## Version `0.2.4`

- Hidden sqlite dep behind a feature flag

## Version `0.2.3`

- Swapped out AsyncMutex for Mutex to fix ledger tests running in tokio async environment
- Add `SqlDB`, an SQLite backed `storage::DB` implementation.

## Version `0.2.2`

- Extended `WellBehaved` trait with `Deserializable` to allow nested Maps
- Re-fix the tokio issue with async mutexes.

## Version `0.2.1`

- Fix infinite recursion in `FromIterator` implementation of `Map`.

## Version `0.2.0`

- Optimised Map inserts from O(n^2) -> O(log n)
- Update base-crypto to `0.4`

## Version `0.1.5`

- Reduce cache op complexity from O(n) to O(1) by swapping out home-rolled version for `lru` crate.

## Version `0.1.4`

- Updating base-crypto to `0.3.1`

## Version `0.1.3`

- Updated base-crypto to `0.2.1`

## Version `0.1.2`

- Fix tokio inconsistencies by abandoning async mutexes for now.

## Version `0.1.1`

- Fix race condition in allocation and reference counting.

## Version `0.1.0`

- Initial tracked release
