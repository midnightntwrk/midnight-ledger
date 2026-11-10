[**@midnight/ledger v1.0.2-rc.0**](../README.md)

***

[@midnight/ledger](../globals.md) / partitionTranscripts

# Function: partitionTranscripts()

```ts
function partitionTranscripts(
   calls, 
   params, 
   expectedTxShape?): PartitionedTranscript[];
```

Finalizes a set of programs against their initial contexts,
resulting in guaranteed and fallible [Transcript](../type-aliases/Transcript.md)s, optimally
allocated, and heuristically covered for gas fees.

`expectedTxShape` replaces the shape [TransactionShape.forCalls](../classes/TransactionShape.md#forcalls) would
derive for `calls`.

## Parameters

### calls

[`PreTranscript`](../classes/PreTranscript.md)[]

### params

[`LedgerParameters`](../classes/LedgerParameters.md)

### expectedTxShape?

[`TransactionShape`](../classes/TransactionShape.md)

## Returns

[`PartitionedTranscript`](../type-aliases/PartitionedTranscript.md)[]
