[**@midnight/ledger v2.1.0**](../README.md)

***

[@midnight/ledger](../README.md) / KeyMaterialProvider

# Type Alias: KeyMaterialProvider

```ts
type KeyMaterialProvider = {
  getParams: Promise<Uint8Array<ArrayBufferLike>>;
  lookupKey: Promise<ProvingKeyMaterial | undefined>;
};
```

## Methods

### getParams()

```ts
getParams(k): Promise<Uint8Array<ArrayBufferLike>>;
```

#### Parameters

##### k

`number`

#### Returns

`Promise`\<`Uint8Array`\<`ArrayBufferLike`\>\>

***

### lookupKey()

```ts
lookupKey(keyLocation): Promise<ProvingKeyMaterial | undefined>;
```

#### Parameters

##### keyLocation

`string`

#### Returns

`Promise`\<[`ProvingKeyMaterial`](ProvingKeyMaterial.md) \| `undefined`\>
