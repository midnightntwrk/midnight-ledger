[**@midnight/ledger v6.1.0-alpha.1**](../README.md)

***

[@midnight/ledger](../globals.md) / LedgerParameters

# Class: LedgerParameters

Parameters used by the Midnight ledger, including transaction fees and
bounds

## Properties

### dust

```ts
readonly dust: DustParameters;
```

The parameters associated with DUST.

***

### transactionCostModel

```ts
readonly transactionCostModel: TransactionCostModel;
```

The cost model used for transaction fees contained in these parameters

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
static deserialize(raw): LedgerParameters;
```

#### Parameters

##### raw

`Uint8Array`

#### Returns

`LedgerParameters`

***

### initialParameters()

```ts
static initialParameters(): LedgerParameters;
```

The initial parameters of Midnight

#### Returns

`LedgerParameters`
