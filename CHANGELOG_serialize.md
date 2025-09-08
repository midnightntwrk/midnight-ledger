# `serialize` Changelog

## Version `1.0.0`

- version bump in preparation for full stablisation

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
