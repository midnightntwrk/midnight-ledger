[**@midnight/zswap v4.0.0-rc**](../README.md)

***

[@midnight/zswap](../globals.md) / ProofErasedTransaction

# Class: ProofErasedTransaction

[Transaction](Transaction.md), with all proof information erased

Primarily for use in testing, or handling data known to be correct from
external information

## Properties

### fallibleCoins

```ts
readonly fallibleCoins: undefined | ProofErasedOffer;
```

The fallible Zswap offer

***

### guaranteedCoins

```ts
readonly guaranteedCoins: undefined | ProofErasedOffer;
```

The guaranteed Zswap offer

***

### mint

```ts
readonly mint: undefined | ProofErasedAuthorizedMint;
```

The mint this transaction represents, if applicable

## Methods

### fees()

```ts
fees(params): bigint
```

The cost of this transaction, in the atomic unit of the base token

#### Parameters

##### params

[`LedgerParameters`](LedgerParameters.md)

#### Returns

`bigint`

***

### identifiers()

```ts
identifiers(): string[]
```

Returns the set of identifiers contained within this transaction. Any of
these *may* be used to watch for a specific transaction.

#### Returns

`string`[]

***

### imbalances()

```ts
imbalances(guaranteed, fees?): Map<string, bigint>
```

For given fees, and a given section (guaranteed/fallible), what the
surplus or deficit of this transaction in any token type is.

#### Parameters

##### guaranteed

`boolean`

##### fees?

`bigint`

#### Returns

`Map`\<`string`, `bigint`\>

***

### merge()

```ts
merge(other): ProofErasedTransaction
```

Merges this transaction with another

#### Parameters

##### other

[`ProofErasedTransaction`](ProofErasedTransaction.md)

#### Returns

[`ProofErasedTransaction`](ProofErasedTransaction.md)

#### Throws

If both transactions have contract interactions, or they spend the
same coins

***

### serialize()

```ts
serialize(netid): Uint8Array<ArrayBufferLike>
```

#### Parameters

##### netid

[`NetworkId`](../enumerations/NetworkId.md)

#### Returns

`Uint8Array`\<`ArrayBufferLike`\>

***

### toString()

```ts
toString(compact?): string
```

#### Parameters

##### compact?

`boolean`

#### Returns

`string`

***

### deserialize()

```ts
static deserialize(raw, netid): ProofErasedTransaction
```

#### Parameters

##### raw

`Uint8Array`\<`ArrayBufferLike`\>

##### netid

[`NetworkId`](../enumerations/NetworkId.md)

#### Returns

[`ProofErasedTransaction`](ProofErasedTransaction.md)
