[**@midnight/ledger v6.1.0-alpha.1**](../README.md)

***

[@midnight/ledger](../globals.md) / EncryptionSecretKey

# Class: EncryptionSecretKey

Holds the encryption secret key of a user, which may be used to determine if
a given offer contains outputs addressed to this user

## Methods

### test()

```ts
test<P>(offer): boolean;
```

#### Type Parameters

##### P

`P` *extends* [`Proofish`](../type-aliases/Proofish.md)

#### Parameters

##### offer

[`ZswapOffer`](ZswapOffer.md)\<`P`\>

#### Returns

`boolean`

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
static deserialize(raw): EncryptionSecretKey;
```

#### Parameters

##### raw

`Uint8Array`

#### Returns

`EncryptionSecretKey`
