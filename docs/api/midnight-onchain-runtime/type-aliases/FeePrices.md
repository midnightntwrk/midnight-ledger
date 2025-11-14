[**@midnight-ntwrk/onchain-runtime v1.0.0-alpha.4**](../README.md)

***

[@midnight-ntwrk/onchain-runtime](../globals.md) / FeePrices

# Type Alias: FeePrices

```ts
type FeePrices: {
  blockUsagePrice: number;
  computePrice: number;
  readPrice: number;
  writePrice: number;
};
```

The fee prices for transaction

## Type declaration

### blockUsagePrice

```ts
blockUsagePrice: number;
```

The price of block usage.

### computePrice

```ts
computePrice: number;
```

The price of time spent in single-threaded compute.

### readPrice

```ts
readPrice: number;
```

The price of time spent reading from disk.

### writePrice

```ts
writePrice: number;
```

The price of time spent writing to disk.
