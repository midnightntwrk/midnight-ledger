# `coin-structure` Changelog

## Version `2.0.0`

- breaking: pull in breaking transient-crypto changes

## Version `1.0.0`

- version bump in preparation for full stablisation
- addressed audit issues:
  - bugfix: Correct TokenType length in BinaryHashRepr to 33
  - bugfix: Check `TokenType` tag validity before length

## Version `0.5.0`

- Feature: Added Unshielded TokenType variant
- Feature: TokenType is now  an enum of `ShieldedTokenType` or `UnshieldedTokenType`
- Renamed `Address` to `ContractAddress`

## Version `0.4.0`

- breaking: bump to pull in breaking `0.5.0` `transient-crypto` release.

## Version `0.3.1`

- Use `transient-crypto`.

## Version `0.3.0`

- Updated storage to `0.3.0`

## Version `0.2.0`

- Updated storage to `0.2.0`

## Version `0.1.2`

- Updated storage to `0.1.4`

## Version `0.1.1`

- Updated storage to `0.1.3`

## Version `0.1.0`

- Initial tracked release
