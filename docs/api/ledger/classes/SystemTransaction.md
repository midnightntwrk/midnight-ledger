[**@midnight/ledger v6.1.0-alpha.5**](../README.md)

***

[@midnight/ledger](../globals.md) / SystemTransaction

# Class: SystemTransaction

A privileged transaction issued by the system.

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
static deserialize(raw): SystemTransaction;
```

#### Parameters

##### raw

`Uint8Array`

#### Returns

`SystemTransaction`
