[**@midnight/ledger v6.1.0-alpha.5**](../README.md)

***

[@midnight/ledger](../globals.md) / ReplaceAuthority

# Class: ReplaceAuthority

An update instruction to replace the current contract maintenance authority
with a new one.

## Constructors

### Constructor

```ts
new ReplaceAuthority(authority): ReplaceAuthority;
```

#### Parameters

##### authority

[`ContractMaintenanceAuthority`](ContractMaintenanceAuthority.md)

#### Returns

`ReplaceAuthority`

## Properties

### authority

```ts
readonly authority: ContractMaintenanceAuthority;
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
