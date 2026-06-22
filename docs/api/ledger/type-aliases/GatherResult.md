[**@midnight/ledger v1.0.0-rc.3**](../README.md)

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
  content: {
     data: EncodedStateValue;
     eventType: LogEventType;
     version: number;
  };
  tag: "log";
};
```

An individual result of observing the results of a non-verifying VM program
execution
