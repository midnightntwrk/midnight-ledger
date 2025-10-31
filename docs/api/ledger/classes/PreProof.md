[**@midnight/ledger v6.1.0-alpha.5**](../README.md)

***

[@midnight/ledger](../globals.md) / PreProof

# Class: PreProof

The preimage, or data required to produce, a [Proof](Proof.md).

## Constructors

### Constructor

```ts
new PreProof(data): PreProof;
```

#### Parameters

##### data

`String`

#### Returns

`PreProof`

## Properties

### instance

```ts
instance: "pre-proof";
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
static deserialize(raw): PreProof;
```

#### Parameters

##### raw

`Uint8Array`

#### Returns

`PreProof`
