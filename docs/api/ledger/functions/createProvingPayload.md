[**@midnight/ledger v1.0.0-rc.4**](../README.md)

***

[@midnight/ledger](../globals.md) / createProvingPayload

# Function: createProvingPayload()

```ts
function createProvingPayload(
   serializedPreimage, 
   overwriteBindingInput, 
   keyMaterial?): Uint8Array;
```

Creates a payload for proving a specific proof through the proof server

## Parameters

### serializedPreimage

`Uint8Array`

### overwriteBindingInput

`bigint` | `undefined`

### keyMaterial?

[`ProvingKeyMaterial`](../type-aliases/ProvingKeyMaterial.md)

## Returns

`Uint8Array`
