[**@midnight/ledger v6.1.0-alpha.1**](../README.md)

***

[@midnight/ledger](../globals.md) / RunningCost

# Type Alias: RunningCost

```ts
type RunningCost = {
  bytesDeleted: bigint;
  bytesWritten: bigint;
  computeTime: bigint;
  readTime: bigint;
};
```

A running tally of synthetic resource costs.

## Properties

### bytesDeleted

```ts
bytesDeleted: bigint;
```

The number of (modelled) bytes deleted.

***

### bytesWritten

```ts
bytesWritten: bigint;
```

The number of (modelled) bytes written.

***

### computeTime

```ts
computeTime: bigint;
```

The amount of (modelled) time spent in single-threaded compute, measured in picoseconds.

***

### readTime

```ts
readTime: bigint;
```

The amount of (modelled) time spent reading from disk, measured in picoseconds.
