**@midnight/zswap v4.0.0-rc**

***

# Zswap TypeScript API

This document outlines the usage of the Zswap TS API

## Network ID

Prior to any interaction,  setNetworkId should be used to set the [NetworkId](enumerations/NetworkId.md) to target the correct network.

## Proof stages

Most transaction components will be in one of three stages: `X`, `UnprovenX`,
or `ProofErasedX`. The `UnprovenX` stage is _always_ the first one. It is
possible to transition to the `X` stage by proving an `UnprovenTransaction`
through the proof server. For testing, and where proofs aren't necessary, the
`ProofErasedX` stage is used, which can be reached via `eraseProof[s]` from the
other two stages.

## Transaction structure

A [Transaction](classes/Transaction.md) runs in two phases: a _guaranteed_ phase, handling fee payments
and fast-to-verify operations, and a _fallible_ phase, handling operations
which may fail atomically, separately from the guaranteed phase. It therefore
contains:

* A "guaranteed" [Offer](classes/Offer.md)
* Optionally, a "fallible" [Offer](classes/Offer.md)
* Contract call information not accessible to this API

It also contains additional cryptographic glue that will be omitted in this
document.

### Zswap

A Zswap [Offer](classes/Offer.md) consists of:
* A set of [Input](classes/Input.md)s, burning coins.
* A set of [Output](classes/Output.md)s, creating coins.
* A set of [Transient](classes/Transient.md)s, indicating a coin that is created and burnt in
  the same transaction.
* A mapping from [TokenType](type-aliases/TokenType.md)s to offer balance, positive when there are more
  inputs than outputs and vice versa.

[Input](classes/Input.md)s can be created either from a [QualifiedCoinInfo](type-aliases/QualifiedCoinInfo.md) and a contract
address, if the coin is contract-owned, or from a [QualifiedCoinInfo](type-aliases/QualifiedCoinInfo.md) and a
 ZswapLocalState, if it is user-owned. Similarly, [Output](classes/Output.md)s can be created
from a [CoinInfo](type-aliases/CoinInfo.md) and a contract address for contract-owned coins, or from a
[CoinInfo](type-aliases/CoinInfo.md) and a user's public key(s), if it is user-owned. A [Transient](classes/Transient.md)
is created similarly to a [Input](classes/Input.md), but directly converts an existing
[Output](classes/Output.md).

A [QualifiedCoinInfo](type-aliases/QualifiedCoinInfo.md) is a [CoinInfo](type-aliases/CoinInfo.md) with an index into the Merkle tree of
coin commitments that can be used to find the relevant coin to spend, while a
[CoinInfo](type-aliases/CoinInfo.md) consists of a coins [TokenType](type-aliases/TokenType.md), value, and a nonce.

## State Structure

[ZswapChainState](classes/ZswapChainState.md) holds the on-chain state of Zswap, while 
ZswaplocalState contains the local, wallet state.
