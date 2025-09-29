[**@midnight/ledger v6.1.0-alpha.1**](../README.md)

***

[@midnight/ledger](../globals.md) / CoinSecretKey

# Class: CoinSecretKey

Holds the coin secret key of a user, serialized as a hex-encoded 32-byte string

## Methods

### clear()

```ts
clear(): void;
```

Clears the coin secret key, so that it is no longer usable nor held in memory

#### Returns

`void`

***

### yesIKnowTheSecurityImplicationsOfThis\_serialize()

```ts
yesIKnowTheSecurityImplicationsOfThis_serialize(): Uint8Array;
```

#### Returns

`Uint8Array`

***

### deserialize()

```ts
static deserialize(raw): CoinSecretKey;
```

#### Parameters

##### raw

`Uint8Array`

#### Returns

`CoinSecretKey`
