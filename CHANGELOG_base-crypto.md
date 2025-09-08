# `base-crypto` Changelog

## Version `1.0.0`

- version bump in preparation for full stablisation
- feat: add cost model abstractions and primitives

## Version `0.5.0`

- breaking: pull in breaking serialization changes

## Version `0.4.4`

- impl `BinaryHashRepr` for `VerifyingKey`
- add `from_bytes` method for `SigningKey`
- add custom `Timestamp` type
- add custom `Duration` type
- add ranging for `ValueSlice`

## Version `0.4.3`

- feat: Add a data provider to fetch key material for Midnight. The source of
  this may be overriden with the `MIDNIGHT_PARAM_SOURCE` environment variable.

## Version `0.4.2`

- Split `base-crypto` into `base-crypto` and `transient-crypto`.

## Version `0.4.1`

- Fix serde deserialization to ensure normal form of `AlignedValue`, and serialization to always output normal form.

## Version `0.4.0`

- Updates to midnight-circuits

## Version `0.3.2`

- Updated serialization to `serialize-0.3.0`

## Version `0.3.1`

- Updated serialization to `serialize-0.2.0`
- Created `simple_arbitrary` macro using `Standard<T>` to generate instances of `T`
- Update `midnight-circuits` dependency to improve performance and fix a
  corner-case bug.

## Version `0.3.0`

- Add Schnorr BIP340 signatures
- Change verifier key format to include a leading length signifier
- Change verifier key deserialization to defer constructing to key to point of use.
- Change `IrSource::model` to take an optional `k` value as input; the
  computation here is non-trivial for large `k`, and if a bound is known, this
  can be used to speed this up.

## Version `0.2.0`

- Initial tracked release
