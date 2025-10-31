[**@midnight/ledger v6.1.0-alpha.5**](../README.md)

***

[@midnight/ledger](../globals.md) / VerifierKeyRemove

# Class: VerifierKeyRemove

An update instruction to remove a verifier key of a specific operation and
version.

## Constructors

### Constructor

```ts
new VerifierKeyRemove(operation, version): VerifierKeyRemove;
```

#### Parameters

##### operation

`string` | `Uint8Array`\<`ArrayBufferLike`\>

##### version

[`ContractOperationVersion`](ContractOperationVersion.md)

#### Returns

`VerifierKeyRemove`

## Properties

### operation

```ts
readonly operation: string | Uint8Array<ArrayBufferLike>;
```

***

### version

```ts
readonly version: ContractOperationVersion;
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
