[**@midnight/ledger v6.1.0-alpha.1**](../README.md)

***

[@midnight/ledger](../globals.md) / Transaction

# Class: Transaction\<S, P, B\>

A Midnight transaction, consisting a section of [ContractAction](../type-aliases/ContractAction.md)s, and a guaranteed and fallible [ZswapOffer](ZswapOffer.md).

The guaranteed section are run first, and fee payment is taken during this
part. If it succeeds, the fallible section is also run, and atomically
rolled back if it fails.

## Type Parameters

### S

`S` *extends* [`Signaturish`](../type-aliases/Signaturish.md)

### P

`P` *extends* [`Proofish`](../type-aliases/Proofish.md)

### B

`B` *extends* [`Bindingish`](../type-aliases/Bindingish.md)

## Properties

### bindingRandomness

```ts
readonly bindingRandomness: bigint;
```

The binding randomness associated with this transaction

***

### fallibleOffer

```ts
fallibleOffer: undefined | Map<number, ZswapOffer<P>>;
```

The fallible Zswap offer

#### Throws

On writing if `B` is [Binding](Binding.md) or this is not a standard
transaction

***

### guaranteedOffer

```ts
guaranteedOffer: undefined | ZswapOffer<P>;
```

The guaranteed Zswap offer

#### Throws

On writing if `B` is [Binding](Binding.md) or this is not a standard
transaction

***

### intents

```ts
intents: undefined | Map<number, Intent<S, P, B>>;
```

The intents contained in this transaction

#### Throws

On writing if `B` is [Binding](Binding.md) or this is not a standard
transaction

***

### rewards

```ts
readonly rewards: 
  | undefined
| ClaimRewardsTransaction<S>;
```

The rewards this transaction represents, if applicable

## Methods

### bind()

```ts
bind(): Transaction<S, P, Binding>;
```

Enforces binding for this transaction. This is irreversible.

#### Returns

`Transaction`\<`S`, `P`, [`Binding`](Binding.md)\>

***

### cost()

```ts
cost(params): SyntheticCost;
```

The underlying resource cost of this transaction.

#### Parameters

##### params

[`LedgerParameters`](LedgerParameters.md)

#### Returns

[`SyntheticCost`](../type-aliases/SyntheticCost.md)

***

### eraseProofs()

```ts
eraseProofs(): Transaction<S, NoProof, NoBinding>;
```

Erases the proofs contained in this transaction

#### Returns

`Transaction`\<`S`, [`NoProof`](NoProof.md), [`NoBinding`](NoBinding.md)\>

***

### eraseSignatures()

```ts
eraseSignatures(): Transaction<SignatureErased, P, B>;
```

Removes signatures from this transaction.

#### Returns

`Transaction`\<[`SignatureErased`](SignatureErased.md), `P`, `B`\>

***

### fees()

```ts
fees(params): bigint;
```

The cost of this transaction, in SPECKs.

Note that this is *only* accurate when called with proven transactions.

#### Parameters

##### params

[`LedgerParameters`](LedgerParameters.md)

#### Returns

`bigint`

***

### identifiers()

```ts
identifiers(): string[];
```

Returns the set of identifiers contained within this transaction. Any of
these *may* be used to watch for a specific transaction.

#### Returns

`string`[]

***

### imbalances()

```ts
imbalances(segment, fees?): Map<TokenType, bigint>;
```

For given fees, and a given section (guaranteed/fallible), what the
surplus or deficit of this transaction in any token type is.

#### Parameters

##### segment

`number`

##### fees?

`bigint`

#### Returns

`Map`\<[`TokenType`](../type-aliases/TokenType.md), `bigint`\>

#### Throws

If `segment` is not a valid segment ID

***

### merge()

```ts
merge(other): Transaction<S, P, B>;
```

Merges this transaction with another

#### Parameters

##### other

`Transaction`\<`S`, `P`, `B`\>

#### Returns

`Transaction`\<`S`, `P`, `B`\>

#### Throws

If both transactions have contract interactions, or they spend the
same coins

***

### mockProve()

```ts
mockProve(): Transaction<S, Proof, B>;
```

Mocks proving, producing a 'proven' transaction that, while it will
*not* verify, is accurate for fee computation purposes.

Due to the variability in proof sizes, this *only* works for transactions
that do not contain unproven contract calls.

#### Returns

`Transaction`\<`S`, [`Proof`](Proof.md), `B`\>

#### Throws

If called on bound, proven, or proof-erased transactions, or if the
transaction contains unproven contract calls.

***

### prove()

```ts
prove(provider, cost_model): Promise<Transaction<S, Proof, B>>;
```

Proves the transaction, with access to a low-level proving provider.
This may *only* be called for `P = PreProof`.

#### Parameters

##### provider

[`ProvingProvider`](../type-aliases/ProvingProvider.md)

##### cost\_model

[`CostModel`](CostModel.md)

#### Returns

`Promise`\<`Transaction`\<`S`, [`Proof`](Proof.md), `B`\>\>

#### Throws

If called on bound, proven, or proof-erased transactions.

***

### serialize()

```ts
serialize(): Uint8Array;
```

#### Returns

`Uint8Array`

***

### toString()

```ts
toString(compact?): string;
```

#### Parameters

##### compact?

`boolean`

#### Returns

`string`

***

### transactionHash()

```ts
transactionHash(): string;
```

Returns the hash associated with this transaction. Due to the ability to
merge transactions, this should not be used to watch for a specific
transaction.

#### Returns

`string`

***

### wellFormed()

```ts
wellFormed(
   ref_state, 
   strictness, 
   tblock): VerifiedTransaction;
```

Tests well-formedness criteria, optionally including transaction balancing

#### Parameters

##### ref\_state

[`LedgerState`](LedgerState.md)

##### strictness

[`WellFormedStrictness`](WellFormedStrictness.md)

##### tblock

`Date`

#### Returns

[`VerifiedTransaction`](VerifiedTransaction.md)

#### Throws

If the transaction is not well-formed for any reason

***

### deserialize()

```ts
static deserialize<S, P, B>(
   markerS, 
   markerP, 
   markerB, 
raw): Transaction<S, P, B>;
```

#### Type Parameters

##### S

`S` *extends* [`Signaturish`](../type-aliases/Signaturish.md)

##### P

`P` *extends* [`Proofish`](../type-aliases/Proofish.md)

##### B

`B` *extends* [`Bindingish`](../type-aliases/Bindingish.md)

#### Parameters

##### markerS

`S`\[`"instance"`\]

##### markerP

`P`\[`"instance"`\]

##### markerB

`B`\[`"instance"`\]

##### raw

`Uint8Array`

#### Returns

`Transaction`\<`S`, `P`, `B`\>

***

### fromParts()

```ts
static fromParts(
   network_id, 
   guaranteed?, 
   fallible?, 
intent?): Transaction<SignatureEnabled, PreProof, PreBinding>;
```

Creates a transaction from its parts.

#### Parameters

##### network\_id

`string`

##### guaranteed?

[`ZswapOffer`](ZswapOffer.md)\<[`PreProof`](PreProof.md)\>

##### fallible?

[`ZswapOffer`](ZswapOffer.md)\<[`PreProof`](PreProof.md)\>

##### intent?

[`Intent`](Intent.md)\<[`SignatureEnabled`](SignatureEnabled.md), [`PreProof`](PreProof.md), [`PreBinding`](PreBinding.md)\>

#### Returns

`Transaction`\<[`SignatureEnabled`](SignatureEnabled.md), [`PreProof`](PreProof.md), [`PreBinding`](PreBinding.md)\>

***

### fromPartsRandomized()

```ts
static fromPartsRandomized(
   network_id, 
   guaranteed?, 
   fallible?, 
intent?): Transaction<SignatureEnabled, PreProof, PreBinding>;
```

Creates a transaction from its parts, randomizing the segment ID to better
allow merging.

#### Parameters

##### network\_id

`string`

##### guaranteed?

[`ZswapOffer`](ZswapOffer.md)\<[`PreProof`](PreProof.md)\>

##### fallible?

[`ZswapOffer`](ZswapOffer.md)\<[`PreProof`](PreProof.md)\>

##### intent?

[`Intent`](Intent.md)\<[`SignatureEnabled`](SignatureEnabled.md), [`PreProof`](PreProof.md), [`PreBinding`](PreBinding.md)\>

#### Returns

`Transaction`\<[`SignatureEnabled`](SignatureEnabled.md), [`PreProof`](PreProof.md), [`PreBinding`](PreBinding.md)\>

***

### fromRewards()

```ts
static fromRewards<S>(rewards): Transaction<S, PreProof, Binding>;
```

Creates a rewards claim transaction, the funds claimed must have been
legitimately rewarded previously.

#### Type Parameters

##### S

`S` *extends* [`Signaturish`](../type-aliases/Signaturish.md)

#### Parameters

##### rewards

[`ClaimRewardsTransaction`](ClaimRewardsTransaction.md)\<`S`\>

#### Returns

`Transaction`\<`S`, [`PreProof`](PreProof.md), [`Binding`](Binding.md)\>
