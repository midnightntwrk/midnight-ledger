[**@midnight/ledger v0.1.0-rc.1**](../README.md)

***

[@midnight/ledger](../globals.md) / SignatureEnabled

# Class: SignatureEnabled

## Constructors

### Constructor

```ts
new SignatureEnabled(data): SignatureEnabled;
```

#### Parameters

##### data

[`Signature`](../type-aliases/Signature.md)

#### Returns

`SignatureEnabled`

## Properties

### instance

```ts
readonly instance: "signature";
```

***

### value

```ts
readonly value: Signature;
```

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
static deserialize(raw): SignatureEnabled;
```

#### Parameters

##### raw

`Uint8Array`

#### Returns

`SignatureEnabled`
