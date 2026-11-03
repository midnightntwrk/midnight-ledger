# `coin-structure` Changelog

## Unreleased

- breaking: `SpendRule` (`spend-rule[v2]`) is the empty rule or a fixed-width `Descriptor` (`spend-rule-descriptor[v1]`: `mode`, `version`, 32-byte `payload`), appended raw to the shielded token type derivation preimage; the empty rule keeps today's derivation. Its field-aligned encoding is `[mode, version, payload]`, with the empty rule as mode 0. `SpendRule::guard`/`Descriptor::guard` build a mode 2, version 1 rule, and `is_defined` says whether the ledger accepts one.
- feat: `ShieldedTokenPreimage`, the `(domain_sep, contract_address, rule)` preimage of a shielded token type, which every shielded spend re-derives the type from; and `ContractAddress::GENESIS`, the all-zero address genesis types are minted under.

## Version `2.1.0`

- feat: add conversions from ECDSA keys to user addresses
- breaking: updated ECDSA domain separator

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
