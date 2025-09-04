[**@midnight/ledger v6.1.0-alpha.1**](../README.md)

***

[@midnight/ledger](../globals.md) / proofDataIntoSerializedPreimage

# Function: proofDataIntoSerializedPreimage()

```ts
function proofDataIntoSerializedPreimage(
   input, 
   output, 
   public_transcript, 
   private_transcript_outputs): Uint8Array;
```

Converts input, output, and transcript information into a proof preimage
suitable to pass to a `ProvingProvider`.

## Parameters

### input

[`AlignedValue`](../type-aliases/AlignedValue.md)

### output

[`AlignedValue`](../type-aliases/AlignedValue.md)

### public\_transcript

[`Op`](../type-aliases/Op.md)\<[`AlignedValue`](../type-aliases/AlignedValue.md)\>[]

### private\_transcript\_outputs

[`AlignedValue`](../type-aliases/AlignedValue.md)[]

## Returns

`Uint8Array`
