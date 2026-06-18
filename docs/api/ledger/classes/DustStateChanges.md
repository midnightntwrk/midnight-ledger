[**@midnight/ledger v0.1.0-rc.1**](../README.md)

***

[@midnight/ledger](../globals.md) / DustStateChanges

# Class: DustStateChanges

## Constructors

### Constructor

```ts
new DustStateChanges(
   source, 
   receivedUtxos, 
   spentUtxos): DustStateChanges;
```

#### Parameters

##### source

`string`

##### receivedUtxos

[`QualifiedDustOutput`](../type-aliases/QualifiedDustOutput.md)[]

##### spentUtxos

[`QualifiedDustOutput`](../type-aliases/QualifiedDustOutput.md)[]

#### Returns

`DustStateChanges`

## Properties

### receivedUtxos

```ts
readonly receivedUtxos: QualifiedDustOutput[];
```

The UTXOs that were received in this state change

***

### source

```ts
readonly source: string;
```

The source of the state change, as a hex-encoded string

***

### spentUtxos

```ts
readonly spentUtxos: QualifiedDustOutput[];
```

The UTXOs that were spent in this state change

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
