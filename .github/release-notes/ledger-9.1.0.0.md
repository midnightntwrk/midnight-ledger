# Ledger 9.1.0.0 Release Notes

**Version:** 9.1.0.0
**Date:** <YYYY-MM-DD>

## High-level summary

<One or two paragraphs: what kind of release this is and the headline changes.>

## Audience

This release note is important for:

- <e.g. Node developers>
- <e.g. Developers using the ledger WASM bindings>

## Summary of updates

### <Component or area> Updates

- <feat/fix/breaking bullet>

## New features requiring configuration updates

*None in this release.*

## Deprecations

*None in this release.*

## Breaking changes or required actions for developers

*None in this release.*

## Fixed defect list

| **Component** | **Description** |
|---------------|-----------------|
| <component>   | <description>   |

<!-- Draft bullets from commits since the last tag: -->
- fix: let the release resume after the npm publish approval
- fix: release the proof server on its own tag, idempotently
- fix: flatten proof-server public key into manifest artifact
- fix: stop expanding untrusted input into workflow shell (GHSA-w369-9rqw-8hq8)
- fix: drop the unused http2 feature (RUSTSEC-2026-0258) (#722)
- fix: stop purging the Nix store cache on every run (#724)
