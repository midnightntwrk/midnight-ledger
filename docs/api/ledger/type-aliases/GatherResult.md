[**@midnight/ledger v6.1.0-alpha.3**](../README.md)

***

[@midnight/ledger](../globals.md) / GatherResult

# Type Alias: GatherResult

```ts
type GatherResult = 
  | {
  content: AlignedValue;
  tag: "read";
}
  | {
  content: EncodedStateValue;
  tag: "log";
};
```

An individual result of observing the results of a non-verifying VM program
execution
