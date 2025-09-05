[**@midnight/zswap v4.0.0-rc**](../README.md)

***

[@midnight/zswap](../globals.md) / UnprovenTransaction

# Class: UnprovenTransaction

[Transaction](Transaction.md), prior to being proven

All "shielded" information in the transaction can still be extracted at this
stage!

## Constructors

### new UnprovenTransaction()

```ts
new UnprovenTransaction(guaranteed, fallible?): UnprovenTransaction
```

Creates the transaction from guaranteed/fallible [UnprovenOffer](UnprovenOffer.md)s

#### Parameters

##### guaranteed

[`UnprovenOffer`](UnprovenOffer.md)

##### fallible?

[`UnprovenOffer`](UnprovenOffer.md)

#### Returns

[`UnprovenTransaction`](UnprovenTransaction.md)

## Properties

### fallibleCoins

```ts
readonly fallibleCoins: undefined | UnprovenOffer;
```

The fallible Zswap offer

***

### guaranteedCoins

```ts
readonly guaranteedCoins: undefined | UnprovenOffer;
```

The guaranteed Zswap offer

***

### mint

```ts
readonly mint: undefined | UnprovenAuthorizedMint;
```

The mint this transaction represents, if applicable

## Methods

### eraseProofs()

```ts
eraseProofs(): ProofErasedTransaction
```

Erases the proofs contained in this transaction

#### Returns

[`ProofErasedTransaction`](ProofErasedTransaction.md)

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

### merge()

```ts
merge(other): UnprovenTransaction
```

Merges this transaction with another

#### Parameters

##### other

[`UnprovenTransaction`](UnprovenTransaction.md)

#### Returns

[`UnprovenTransaction`](UnprovenTransaction.md)

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
static deserialize(raw, netid): UnprovenTransaction
```

#### Parameters

##### raw

`Uint8Array`\<`ArrayBufferLike`\>

##### netid

[`NetworkId`](../enumerations/NetworkId.md)

#### Returns

[`UnprovenTransaction`](UnprovenTransaction.md)

***

### fromMint()

```ts
static fromMint(mint): UnprovenTransaction
```

Creates a minting claim transaction, the funds claimed must have been
legitimately minted previously.

#### Parameters

##### mint

[`UnprovenAuthorizedMint`](UnprovenAuthorizedMint.md)

#### Returns

[`UnprovenTransaction`](UnprovenTransaction.md)
