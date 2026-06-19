[**@midnight/ledger v0.1.0-rc.1**](../README.md)

***

[@midnight/ledger](../globals.md) / LogEventType

# Type Alias: LogEventType

```ts
type LogEventType = 
  | "shielded-spend"
  | "shielded-receive"
  | "shielded-mint"
  | "shielded-burn"
  | "unshielded-spend"
  | "unshielded-receive"
  | "unshielded-mint"
  | "unshielded-burn"
  | "paused"
  | "unpaused"
  | "misc";
```

The type of a log event embedded in [GatherResult](GatherResult.md).
