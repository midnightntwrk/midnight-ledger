[**@midnight/ledger v6.1.0-alpha.5**](../README.md)

***

[@midnight/ledger](../globals.md) / BlockContext

# Type Alias: BlockContext

```ts
type BlockContext = {
  parentBlockHash: string;
  secondsSinceEpoch: bigint;
  secondsSinceEpochErr: number;
};
```

Context information about the block forwarded to [CallContext](CallContext.md).

## Properties

### parentBlockHash

```ts
parentBlockHash: string;
```

The hash of the block prior to this transaction, as a hex-encoded string

***

### secondsSinceEpoch

```ts
secondsSinceEpoch: bigint;
```

The seconds since the UNIX epoch that have elapsed

***

### secondsSinceEpochErr

```ts
secondsSinceEpochErr: number;
```

The maximum error on [secondsSinceEpoch](#secondssinceepoch) that should occur, as a
positive seconds value
