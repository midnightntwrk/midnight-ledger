[**@midnight-ntwrk/onchain-runtime v1.0.0-alpha.3**](../README.md)

***

[@midnight-ntwrk/onchain-runtime](../globals.md) / PublicAddress

# Type Alias: PublicAddress

```ts
type PublicAddress: {
  address: UserAddress;
  tag: "user";
 } | {
  address: ContractAddress;
  tag: "contract";
};
```

A public address that an entity can be identified by
