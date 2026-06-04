[**@midnight/ledger v0.1.0-alpha.1**](../README.md)

***

[@midnight/ledger](../globals.md) / sampleSigningKey

# Function: sampleSigningKey()

```ts
function sampleSigningKey(kind?): SigningKey;
```

Randomly samples a [SigningKey](../type-aliases/SigningKey.md). If `kind` is not supplied, assumes
`schnorr`.

## Parameters

### kind?

[`SignatureKind`](../type-aliases/SignatureKind.md)

## Returns

[`SigningKey`](../type-aliases/SigningKey.md)
