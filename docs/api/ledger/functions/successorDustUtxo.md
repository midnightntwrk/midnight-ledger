[**@midnight/ledger v1.0.0-rc.4**](../README.md)

***

[@midnight/ledger](../globals.md) / successorDustUtxo

# Function: successorDustUtxo()

```ts
function successorDustUtxo(
   qdo, 
   now, 
   subtractFee, 
   newCommitmentIndex, 
   genInfo, 
   sk, 
   dustParams): QualifiedDustOutput;
```

Returns a new Dust UTXO with a reduced value and the sequential nonce

## Parameters

### qdo

[`QualifiedDustOutput`](../type-aliases/QualifiedDustOutput.md)

### now

`Date`

### subtractFee

`bigint`

### newCommitmentIndex

`bigint`

### genInfo

[`DustGenerationInfo`](../type-aliases/DustGenerationInfo.md)

### sk

[`DustSecretKey`](../classes/DustSecretKey.md)

### dustParams

[`DustParameters`](../classes/DustParameters.md)

## Returns

[`QualifiedDustOutput`](../type-aliases/QualifiedDustOutput.md)
