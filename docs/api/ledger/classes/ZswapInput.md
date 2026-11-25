[**@midnight/ledger v1.0.2-rc.0**](../README.md)

***

[@midnight/ledger](../globals.md) / ZswapInput

# Class: ZswapInput\<P\>

A shielded transaction input

## Type Parameters

### P

`P` *extends* [`Proofish`](../type-aliases/Proofish.md)

## Properties

### contractAddress

```ts
readonly contractAddress: string | undefined;
```

The contract address receiving the input, if the sender is a contract

***

### nullifier

```ts
readonly nullifier: string;
```

The nullifier of the input

***

### proof

```ts
readonly proof: P;
```

The proof of this input

## Methods

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

### deserialize()

```ts
static deserialize<P>(markerP, raw): ZswapInput<P>;
```

#### Type Parameters

##### P

`P` *extends* [`Proofish`](../type-aliases/Proofish.md)

#### Parameters

##### markerP

`P`\[`"instance"`\]

##### raw

`Uint8Array`

#### Returns

`ZswapInput`\<`P`\>

***

### newContractOwned()

```ts
static newContractOwned(
   coin, 
   segment, 
   contract, 
   state, 
   mtIndex?): UnprovenInput;
```

Creates a new input, spending a coin owned by a smart contract

The coin's leaf in `state`'s commitment tree is located by the coin's
commitment. `mtIndex` is an optional hint at where that leaf is, e.g. as
reported by [ZswapChainState.tryApply](ZswapChainState.md#tryapply). It is checked against the
leaf, so a wrong hint costs a search of the tree, not an invalid input

#### Parameters

##### coin

[`ShieldedCoinInfo`](../type-aliases/ShieldedCoinInfo.md)

##### segment

`number` | `undefined`

##### contract

`string`

##### state

[`ZswapChainState`](ZswapChainState.md)

##### mtIndex?

`bigint`

#### Returns

[`UnprovenInput`](../type-aliases/UnprovenInput.md)
