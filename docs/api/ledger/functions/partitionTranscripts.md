[**@midnight/ledger v6.1.0-alpha.1**](../README.md)

***

[@midnight/ledger](../globals.md) / partitionTranscripts

# Function: partitionTranscripts()

```ts
function partitionTranscripts(calls, params): [
  | undefined
  | Transcript<AlignedValue>, 
  | undefined
  | Transcript<AlignedValue>][];
```

Finalizes a set of programs against their initial contexts,
resulting in guaranteed and fallible [Transcript](../type-aliases/Transcript.md)s, optimally
allocated, and heuristically covered for gas fees.

## Parameters

### calls

[`PreTranscript`](../classes/PreTranscript.md)[]

### params

[`LedgerParameters`](../classes/LedgerParameters.md)

## Returns

\[
  \| `undefined`
  \| [`Transcript`](../type-aliases/Transcript.md)\<[`AlignedValue`](../type-aliases/AlignedValue.md)\>, 
  \| `undefined`
  \| [`Transcript`](../type-aliases/Transcript.md)\<[`AlignedValue`](../type-aliases/AlignedValue.md)\>\][]
