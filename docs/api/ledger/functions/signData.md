[**@midnight/ledger v6.1.0-alpha.4**](../README.md)

***

[@midnight/ledger](../globals.md) / signData

# Function: signData()

```ts
function signData(key, data): string;
```

Signs arbitrary data with the given signing key.

WARNING: Do not expose access to this function for valuable keys for data
that is not strictly controlled!

## Parameters

### key

`string`

### data

`Uint8Array`

## Returns

`string`
