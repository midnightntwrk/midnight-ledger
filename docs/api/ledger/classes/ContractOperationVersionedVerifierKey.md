[**@midnight/ledger v6.2.0-rc.2**](../README.md)

***

[@midnight/ledger](../globals.md) / ContractOperationVersionedVerifierKey

# Class: ContractOperationVersionedVerifierKey

A versioned verifier key to be associated with a [ContractOperation](ContractOperation.md).

## Constructors

### Constructor

```ts
new ContractOperationVersionedVerifierKey(version, rawVk): ContractOperationVersionedVerifierKey;
```

#### Parameters

##### version

`"v2"`

##### rawVk

`Uint8Array`

#### Returns

`ContractOperationVersionedVerifierKey`

## Properties

### rawVk

```ts
readonly rawVk: Uint8Array;
```

***

### version

```ts
readonly version: "v2";
```

## Methods

### toString()

```ts
toString(compact?): string;
```

#### Parameters

##### compact?

`boolean`

#### Returns

`string`
