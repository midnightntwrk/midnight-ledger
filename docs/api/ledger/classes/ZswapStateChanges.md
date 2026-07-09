[**@midnight/ledger v8.1.0**](../README.md)

***

[@midnight/ledger](../globals.md) / ZswapStateChanges

# Class: ZswapStateChanges

## Constructors

### Constructor

```ts
new ZswapStateChanges(
   source, 
   receivedCoins, 
   spentCoins): ZswapStateChanges;
```

#### Parameters

##### source

`string`

##### receivedCoins

[`QualifiedShieldedCoinInfo`](../type-aliases/QualifiedShieldedCoinInfo.md)[]

##### spentCoins

[`QualifiedShieldedCoinInfo`](../type-aliases/QualifiedShieldedCoinInfo.md)[]

#### Returns

`ZswapStateChanges`

## Properties

### receivedCoins

```ts
readonly receivedCoins: QualifiedShieldedCoinInfo[];
```

The coins that were received in this state change

***

### source

```ts
readonly source: string;
```

The source of the state change, as a hex-encoded string

***

### spentCoins

```ts
readonly spentCoins: QualifiedShieldedCoinInfo[];
```

The coins that were spent in this state change

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
