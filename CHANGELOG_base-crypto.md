# `base-crypto` Changelog

## Unreleased

- feat: add the `fetch` feature (off by default). Downloading missing public
  parameters in `data_provider` now requires it; without it
  `MidnightDataProvider` still serves files already present on disk and
  returns an `io::Error` of kind `Unsupported` when a download would be
  needed. `reqwest`, `atomic-write-file` and `futures` became optional
  dependencies, so verification-only builds (wasm, VM targets) no longer link
  an HTTP client or TLS stack. Consumers that fetch parameters (proof server,
  toolkit, tests) must enable `midnight-base-crypto/fetch`.
- breaking: `MidnightDataProvider::base_url` is now a `url::Url` (previously
  the identical type re-exported through `reqwest`).
- feat: add `rng::derive_crypto_rng`, deriving a `StdRng` from any `Rng`.
- fix: the `Distribution<Standard>` samplers for `schnorr::{VerifyingKey,
  Signature}` and `ecdsa::{VerifyingKey, Signature}` derive their keys from
  the RNG they are given instead of `OsRng`, so seeded sampling is
  deterministic and no library code path reaches operating-system randomness.

## Version `1.1.0`

- feat: add `within_bounds` on `RunningCost`
- feat: add `Envelope` trait derive macro, for building signing envelopes
- feat: add `Mul` implementation of `CostDuration` and `FixedPoint`

## Version `1.0.0`

- version bump in preparation for full stablisation
- feat: add cost model abstractions and primitives
- addressed audit issues:
  - bugfix: `HashOutput` equality has been replaced with a constant-time comparison
  - bugfix: The cursor advancement in this branch of `repr_traverse` has been fixed.
  - bugfix: Field elements permitted in the low-level encoding would cause a
  higher-level panic
  - bugfix: For the `Option` case, we were incorrectly ommitting the discriminant
  from field and binary representations.

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
