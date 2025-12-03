[**@midnight/ledger v6.1.0-rc.1**](../README.md)

***

[@midnight/ledger](../globals.md) / ContractAction

# Type Alias: ContractAction\<P\>

```ts
type ContractAction<P> = 
  | ContractCall<P>
  | ContractDeploy
  | MaintenanceUpdate;
```

An interactions with a contract

## Type Parameters

### P

`P` *extends* [`Proofish`](Proofish.md)
