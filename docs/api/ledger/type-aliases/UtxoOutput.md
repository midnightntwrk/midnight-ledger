[**@midnight/ledger v6.1.0-alpha.3**](../README.md)

***

[@midnight/ledger](../globals.md) / UtxoOutput

# Type Alias: UtxoOutput

```ts
type UtxoOutput = {
  owner: UserAddress;
  type: RawTokenType;
  value: bigint;
};
```

An output appearing in an [Intent](../classes/Intent.md).

## Properties

### owner

```ts
owner: UserAddress;
```

The address owning these tokens.

***

### type

```ts
type: RawTokenType;
```

The token type of this UTXO

***

### value

```ts
value: bigint;
```

The amount of tokens this UTXO represents
