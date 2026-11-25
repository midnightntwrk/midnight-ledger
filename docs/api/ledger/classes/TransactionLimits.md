[**@midnight/ledger v1.0.2-rc.0**](../README.md)

***

[@midnight/ledger](../globals.md) / TransactionLimits

# Class: TransactionLimits

The limits transactions and blocks are held to, part of
[LedgerParameters](LedgerParameters.md)

## Properties

### blockLimits

```ts
readonly blockLimits: SyntheticCost;
```

The resource limits of a block, against which costs are normalized as in
[LedgerParameters.normalizeFullness](LedgerParameters.md#normalizefullness)

***

### blockWithdrawalMinimumMultiple

```ts
readonly blockWithdrawalMinimumMultiple: number;
```

The minimum amount of Night withdrawable from block rewards, as a multiple
of the amount which would be able to pay the theoretical fees for the
withdrawal when Dust reaches its cap

***

### maxContractMetadataSize

```ts
readonly maxContractMetadataSize: bigint;
```

The largest a contract's maintenance authority, or any one of its
operations with its entry point name, may be, in bytes

***

### minTimeToDismiss

```ts
readonly minTimeToDismiss: bigint;
```

The time to dismiss any transaction is allowed regardless of its size, in
picoseconds

***

### timeToDismissPerByte

```ts
readonly timeToDismissPerByte: bigint;
```

The time to dismiss a transaction is allowed per byte of its size, in
picoseconds

***

### transactionByteLimit

```ts
readonly transactionByteLimit: bigint;
```

The largest a transaction may be, in bytes, held against its serialized
size

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
