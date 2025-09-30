[**@midnight/ledger v6.1.0-alpha.3**](../README.md)

***

[@midnight/ledger](../globals.md) / Binding

# Class: Binding

A Fiat-Shamir proof of exponent binding (or ephemerally signing) an
[Intent](Intent.md).

## Constructors

### Constructor

```ts
new Binding(data): Binding;
```

#### Parameters

##### data

`String`

#### Returns

`Binding`

## Properties

### instance

```ts
instance: "binding";
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
static deserialize(raw): Binding;
```

#### Parameters

##### raw

`Uint8Array`

#### Returns

`Binding`
