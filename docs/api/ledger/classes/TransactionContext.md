[**@midnight/ledger v6.1.0-alpha.1**](../README.md)

***

[@midnight/ledger](../globals.md) / TransactionContext

# Class: TransactionContext

The context against which a transaction is run.

## Constructors

### Constructor

```ts
new TransactionContext(
   ref_state, 
   block_context, 
   whitelist?): TransactionContext;
```

#### Parameters

##### ref\_state

[`LedgerState`](LedgerState.md)

A past ledger state that is used as a reference point
for 'static' data.

##### block\_context

[`BlockContext`](../type-aliases/BlockContext.md)

Information about the block this transaction is, or
will be, contained in.

##### whitelist?

`Set`\<`string`\>

A list of contracts that are being tracked, or
`undefined` to track all contracts.

#### Returns

`TransactionContext`

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
