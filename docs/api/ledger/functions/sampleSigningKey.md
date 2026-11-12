[**@midnight/ledger v1.0.2-rc.0**](../README.md)

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
