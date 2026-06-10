[**@midnight/ledger v0.1.0-rc.1**](../README.md)

***

[@midnight/ledger](../globals.md) / DustGenerationTreeInsertionPath

# Class: DustGenerationTreeInsertionPath

## Constructors

### Constructor

```ts
new DustGenerationTreeInsertionPath(state, index): DustGenerationTreeInsertionPath;
```

#### Parameters

##### state

[`DustGenerationState`](DustGenerationState.md)

##### index

`bigint`

#### Returns

`DustGenerationTreeInsertionPath`

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
static deserialize(raw): DustGenerationTreeInsertionPath;
```

#### Parameters

##### raw

`Uint8Array`

#### Returns

`DustGenerationTreeInsertionPath`
