[**@midnight/ledger v6.1.0-alpha.5**](../README.md)

***

[@midnight/ledger](../globals.md) / CallContext

# Type Alias: CallContext

```ts
type CallContext = {
  balance: Map<TokenType, bigint>;
  caller?: PublicAddress;
  comIndices: Map<CoinCommitment, number>;
  ownAddress: ContractAddress;
  parentBlockHash: string;
  secondsSinceEpoch: bigint;
  secondsSinceEpochErr: number;
};
```

The context information of a call provided to the VM.

## Properties

### balance

```ts
balance: Map<TokenType, bigint>;
```

The balances held by the called contract at the time it was called.

***

### caller?

```ts
optional caller: PublicAddress;
```

A public address identifying an entity.

***

### comIndices

```ts
comIndices: Map<CoinCommitment, number>;
```

The commitment indices map accessible to the contract.

***

### ownAddress

```ts
ownAddress: ContractAddress;
```

***

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
