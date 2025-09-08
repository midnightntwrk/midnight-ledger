[**@midnight/ledger v6.1.0-alpha.1**](../README.md)

***

[@midnight/ledger](../globals.md) / ZswapSecretKeys

# Class: ZswapSecretKeys

## Properties

### coinPublicKey

```ts
readonly coinPublicKey: string;
```

***

### coinSecretKey

```ts
readonly coinSecretKey: CoinSecretKey;
```

***

### encryptionPublicKey

```ts
readonly encryptionPublicKey: string;
```

***

### encryptionSecretKey

```ts
readonly encryptionSecretKey: EncryptionSecretKey;
```

## Methods

### fromSeed()

```ts
static fromSeed(seed): ZswapSecretKeys;
```

Derives secret keys from a 32-byte seed

#### Parameters

##### seed

`Uint8Array`

#### Returns

`ZswapSecretKeys`

***

### fromSeedRng()

```ts
static fromSeedRng(seed): ZswapSecretKeys;
```

Derives secret keys from a 32-byte seed using deprecated implementation.
Use only for compatibility purposes

#### Parameters

##### seed

`Uint8Array`

#### Returns

`ZswapSecretKeys`
