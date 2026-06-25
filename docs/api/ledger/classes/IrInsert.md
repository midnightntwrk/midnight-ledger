[**@midnight/ledger v1.0.0-rc.3**](../README.md)

***

[@midnight/ledger](../globals.md) / IrInsert

# Class: IrInsert

An update instruction to insert IR metadata at a specific operation.

## Constructors

### Constructor

```ts
new IrInsert(operation, ir): IrInsert;
```

#### Parameters

##### operation

`string` | `Uint8Array`\<`ArrayBufferLike`\>

##### ir

`Uint8Array`

#### Returns

`IrInsert`

## Properties

### ir

```ts
readonly ir: Uint8Array;
```

***

### operation

```ts
readonly operation: string | Uint8Array<ArrayBufferLike>;
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
