[**@midnight/ledger v2.1.0-rc.1**](../README.md)

***

[@midnight/ledger](../README.md) / ProvingProvider

# Type Alias: ProvingProvider

```ts
type ProvingProvider = {
  check: Promise<bigint[]>;
  prove: Promise<Uint8Array<ArrayBuffer>>;
};
```

## Methods

### check()

```ts
check(serializedPreimage, keyLocation): Promise<bigint[]>;
```

#### Parameters

##### serializedPreimage

`Uint8Array`

##### keyLocation

`string`

#### Returns

`Promise`\<`bigint`[]\>

***

### prove()

```ts
prove(
   serializedPreimage, 
   keyLocation, 
overwriteBindingInput?): Promise<Uint8Array<ArrayBuffer>>;
```

#### Parameters

##### serializedPreimage

`Uint8Array`

##### keyLocation

`string`

##### overwriteBindingInput?

`bigint`

#### Returns

`Promise`\<`Uint8Array`\<`ArrayBuffer`\>\>
