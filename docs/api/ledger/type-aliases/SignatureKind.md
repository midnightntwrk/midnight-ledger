[**@midnight/ledger v0.1.0-alpha.1**](../README.md)

***

[@midnight/ledger](../globals.md) / SignatureKind

# Type Alias: SignatureKind

```ts
type SignatureKind = "schnorr" | "ecdsa";
```

The algorithm used for a particular signature.

- `schnorr` corresponds to BIP-340 Schnorr signatures
- `ecdsa` corresponds to ECDSA signatures over secp256k1
