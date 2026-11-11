[**@midnight/ledger v1.0.2-rc.0**](../README.md)

***

[@midnight/ledger](../globals.md) / TransactionShape

# Class: TransactionShape

Counts of a transaction's non-call parts, whose cost
[partitionTranscripts](../functions/partitionTranscripts.md) reserves from the guaranteed budget.

Pass the counts of the final transaction, so the guaranteed section is
sized for it rather than for the derived default. Raising a count moves the
calls toward the fallible section, except for Dust spends, Dust
deregistrations, Dust registrations when no NIGHT is spent, Zswap parts and
non-NIGHT outputs: their bytes can add more allowance than they cost.
Overstating those is caught by `wellFormed`.

Obtained from [TransactionShape.forCalls](#forcalls). Adjust a count
by assigning to it; a count added in a later version keeps its derived
default.

## Properties

### dustDeregistrations

```ts
dustDeregistrations: number;
```

***

### dustRegistrations

```ts
dustRegistrations: number;
```

***

### dustSpends

```ts
dustSpends: number;
```

***

### intents

```ts
intents: number;
```

***

### nightOutputs

```ts
nightOutputs: number;
```

***

### nightSpends

```ts
nightSpends: number;
```

***

### nonNightUnshieldedOutputs

```ts
nonNightUnshieldedOutputs: number;
```

***

### nonNightUnshieldedSpends

```ts
nonNightUnshieldedSpends: number;
```

***

### zswapInputs

```ts
zswapInputs: number;
```

***

### zswapOutputs

```ts
zswapOutputs: number;
```

***

### zswapTransients

```ts
zswapTransients: number;
```

## Methods

### toString()

```ts
toString(compact?): string;
```

#### Parameters

##### compact?

`boolean`

#### Returns

`string`

***

### forCalls()

```ts
static forCalls(calls, params): TransactionShape;
```

Runs each call once and derives the shape that
[partitionTranscripts](../functions/partitionTranscripts.md) assumes by default, the balanced
transaction the calls imply:

- two intents, the calls' and the fee intent, with one Dust spend;
- one UTXO output per unshielded send the calls claim to a user;
- a Zswap input or output per shielded coin the calls claim;
- per token type, one UTXO spend by the wallet when the calls receive
  more than is claimed to them, or claim more than they send or mint;
- for NIGHT, a change output when the wallet spends it or the calls
  send out more than they claim.

Change for other tokens is not counted; set `nonNightUnshieldedOutputs`
to expect it.

#### Parameters

##### calls

[`PreTranscript`](PreTranscript.md)[]

##### params

[`LedgerParameters`](LedgerParameters.md)

#### Returns

`TransactionShape`
