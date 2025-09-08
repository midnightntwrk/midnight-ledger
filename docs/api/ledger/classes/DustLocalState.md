[**@midnight/ledger v6.1.0-alpha.1**](../README.md)

***

[@midnight/ledger](../globals.md) / DustLocalState

# Class: DustLocalState

## Constructors

### Constructor

```ts
new DustLocalState(params): DustLocalState;
```

#### Parameters

##### params

[`DustParameters`](DustParameters.md)

#### Returns

`DustLocalState`

## Properties

### utxos

```ts
readonly utxos: QualifiedDustOutput[];
```

## Methods

### generationInfo()

```ts
generationInfo(qdo): 
  | undefined
  | DustGenerationInfo;
```

#### Parameters

##### qdo

[`QualifiedDustOutput`](../type-aliases/QualifiedDustOutput.md)

#### Returns

  \| `undefined`
  \| [`DustGenerationInfo`](../type-aliases/DustGenerationInfo.md)

***

### processTtls()

```ts
processTtls(time): DustLocalState;
```

#### Parameters

##### time

`Date`

#### Returns

`DustLocalState`

***

### replayEvents()

```ts
replayEvents(sk, events): DustLocalState;
```

#### Parameters

##### sk

`bigint`

##### events

[`Event`](Event.md)[]

#### Returns

`DustLocalState`

***

### serialize()

```ts
serialize(): Uint8Array;
```

#### Returns

`Uint8Array`

***

### spend()

```ts
spend(
   sk, 
   utxo, 
   vFee, 
   ctime): [DustLocalState, DustSpend<PreProof>];
```

#### Parameters

##### sk

`bigint`

##### utxo

[`QualifiedDustOutput`](../type-aliases/QualifiedDustOutput.md)

##### vFee

`bigint`

##### ctime

`Date`

#### Returns

\[`DustLocalState`, [`DustSpend`](DustSpend.md)\<[`PreProof`](PreProof.md)\>\]

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

### walletBalance()

```ts
walletBalance(time): bigint;
```

#### Parameters

##### time

`Date`

#### Returns

`bigint`

***

### deserialize()

```ts
static deserialize(raw): DustLocalState;
```

#### Parameters

##### raw

`Uint8Array`

#### Returns

`DustLocalState`
