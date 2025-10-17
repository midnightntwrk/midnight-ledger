[**@midnight/ledger v6.1.0-alpha.4**](../README.md)

***

[@midnight/ledger](../globals.md) / entryPointHash

# Function: entryPointHash()

```ts
function entryPointHash(entryPoint): string;
```

Computes the (hex-encoded) hash of a given contract entry point. Used in
composable contracts to reference the called contract's entry point ID
in-circuit.

## Parameters

### entryPoint

`string` | `Uint8Array`\<`ArrayBufferLike`\>

## Returns

`string`
