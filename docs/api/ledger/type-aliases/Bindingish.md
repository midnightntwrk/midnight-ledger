[**@midnight/ledger v6.1.0-alpha.4**](../README.md)

***

[@midnight/ledger](../globals.md) / Bindingish

# Type Alias: Bindingish

```ts
type Bindingish = 
  | Binding
  | PreBinding
  | NoBinding;
```

Whether an intent has binding cryptography applied or not. An intent's
content can no longer be modified after it is [Binding](../classes/Binding.md).
