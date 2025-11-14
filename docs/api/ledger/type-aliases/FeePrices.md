[**@midnight/ledger v6.1.0-alpha.4**](../README.md)

***

[@midnight/ledger](../globals.md) / FeePrices

# Type Alias: FeePrices

```ts
type FeePrices = {
  blockUsagePrice: number;
  computePrice: number;
  readPrice: number;
  writePrice: number;
};
```

The fee prices for transaction

## Properties

### blockUsagePrice

```ts
blockUsagePrice: number;
```

The price of block usage.

***

### computePrice

```ts
computePrice: number;
```

The price of time spent in single-threaded compute.

***

### readPrice

```ts
readPrice: number;
```

The price of time spent reading from disk.

***

### writePrice

```ts
writePrice: number;
```

The price of time spent writing to disk.
