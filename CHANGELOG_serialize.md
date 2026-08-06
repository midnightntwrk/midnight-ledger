# `serialize` Changelog

## Version `1.1.1`

- security: `HashMap` and `HashSet` deserialization now requires a normalized
  encoding (sorted, without duplicate keys), rejecting the non-canonical
  encodings that previously decoded to the same value.
- security: reject non-canonical `ScaleBigInt` encodings that use the 4-byte
  form for a value that fits the smaller form.
- security: `Box<T>` deserialization now counts against the recursion depth
  budget.
- fix: `Vec::with_bounded_capacity` no longer divides by zero for zero-sized
  types.
- fix: `serialized_size` for `HashMap` and `HashSet` accounts for the actual
  width of the length prefix.
- note: `Deserializable for HashSet<T>` now additionally requires
  `T: PartialOrd`. `Serializable for HashSet<T>` already required `T: Ord`, so
  any consumer that round-trips a `HashSet` is unaffected.

## Version `1.1.0`

- feat: add `tagged_deserialize_sequence`, which deserializes a sequence of tagged values

## Version `1.0.0`

- version bump in preparation for full stablisation
- fix: removed undefined behaviour in fixed-length byte deserialization

## Version `0.4.0`

- breaking: replace versioning with type tags, which may include a data version
    - Types may now be `Tagged` rather than `Versioned`
    - `Tagged` types may use the top-level `tagged_serialize` and
      `tagged_deserialize`. These prepend a human-readable tag identifying the
       data type.
- breaking: Change `Serializable` and `Deserializable` trait interface.
- breaking: Remove `NetworkId`.
- breaking: switch low-level encoding from Borsh to Scale.

## Version `0.3.3`

- serde for `Version`, `Timestamp` and `Duration`

## Version `0.3.2`

- Reduced the recursive deserialization limit for debug builds
- Exporting `RECURSION_LIMIT`

## Version `0.3.1`

- Make `randomized_serialization_test` use `midnight-tokio::enter_guard`. This is hidden from the user via a `pub use midnight_tokio`, but the user still needs to add a `tokio` dep if they don't already have one.
- Macro for checking version alignment

## Version `0.3.0`

- Remove statefullness from serialization, now `NetworkId` is a parameter on
top-level serialization functions.
- Fixed a bug relating to the NetworkId serialization of `T` vs `&T`

## Version `0.2.1`

- Make `rand` dependency optional.

## Version `0.2.0`

- Added recursion limit with `Versioned::LIMIT_RECURSION` and an explicit
  recursion counter in deserialization.

## Version `0.1.1`
- Added randomised property testing on Serializable/Deserializable objects via `randomized_serialization_test` macro

## Version `0.1.0`

- Initial tracked release
