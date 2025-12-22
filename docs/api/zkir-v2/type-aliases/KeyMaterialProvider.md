[**@midnight/ledger v2.0.0-rc.2**](../README.md)

***

[@midnight/ledger](../README.md) / KeyMaterialProvider

# Type Alias: KeyMaterialProvider

```ts
type KeyMaterialProvider = {
  getParams: Promise<Uint8Array<ArrayBuffer>>;
  lookupKey: Promise<ProvingKeyMaterial>;
};
```

## Methods

### getParams()

```ts
getParams(k): Promise<Uint8Array<ArrayBuffer>>;
```

#### Parameters

##### k

`number`

#### Returns

`Promise`\<`Uint8Array`\<`ArrayBuffer`\>\>

***

### lookupKey()

```ts
lookupKey(keyLocation): Promise<ProvingKeyMaterial>;
```

#### Parameters

##### keyLocation

`string`

#### Returns

`Promise`\<[`ProvingKeyMaterial`](ProvingKeyMaterial.md)\>
