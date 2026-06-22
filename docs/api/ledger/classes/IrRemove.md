[**@midnight/ledger v1.0.0-rc.3**](../README.md)

***

[@midnight/ledger](../globals.md) / IrRemove

# Class: IrRemove

An update instruction to remove IR metadata of a specific operation.

## Constructors

### Constructor

```ts
new IrRemove(operation): IrRemove;
```

#### Parameters

##### operation

`string` | `Uint8Array`\<`ArrayBufferLike`\>

#### Returns

`IrRemove`

## Properties

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
