# Ledger TypeScript API

This document outlines the flow of transaction assembly and usage with the
ledger TS API.

## Proof stages

Most transaction components will be in one of three stages: `X`, `UnprovenX`,
or `ProofErasedX`. The `UnprovenX` stage is _always_ the first one. It is
possible to transition to the `X` stage by proving an `UnprovenTransaction`
through the proof server. For testing, and where proofs aren't necessary, the
`ProofErasedX` stage is used, which can be reached via `eraseProof[s]` from the
other two stages.

## Transaction structure

A {@link Transaction} runs in two phases: a _guaranteed_ segment, handling fee payments
and fast-to-verify operations, and a series of _fallible segments_. Each segment
may fail atomically, separately from the guaranteed segment. It therefore
contains:

* A "guaranteed" {@link ZswapOffer}
* A map of segment IDs to "fallible" {@link ZswapOffer}s.
* A map of segment IDs to {@link Intent}s, which include {@link UnshieldedOffer}s and {@link ContractAction}s.

It also contains additional cryptographic glue that will be omitted in this
document.

### Zswap

A {@link ZswapOffer} consists of:
* A set of {@link ZswapInput}s, burning coins.
* A set of {@link ZswapOutput}s, creating coins.
* A set of {@link ZswapTransient}s, indicating a coin that is created and burnt in
  the same transaction.
* A mapping from {@link RawTokenType}s to offer balance, positive when there are more
  inputs than outputs and vice versa.

{@link ZswapInput}s can be created either from a {@link QualifiedShieldedCoinInfo} and a contract
address, if the coin is contract-owned, or from a {@link QualifiedShieldedCoinInfo} and a
{@link ZswapLocalState}, if it is user-owned. Similarly, {@link ZswapOutput}s can be created
from a {@link ShieldedCoinInfo} and a contract address for contract-owned coins, or from a
{@link ShieldedCoinInfo} and a user's public key(s), if it is user-owned. A {@link ZswapTransient}
is created similarly to a {@link ZswapInput}, but directly converts an existing
{@link ZswapOutput}.

A {@link QualifiedShieldedCoinInfo} is a {@link ShieldedCoinInfo} with an index into the Merkle tree of
coin commitments that can be used to find the relevant coin to spend, while a
{@link ShieldedCoinInfo} consists of a coin's {@link RawTokenType}, value, and a nonce.

### Calls

A {@link ContractDeploy} consists of an initial contract state, and a nonce.

A {@link ContractCall} consists of a contract's address, the entry point used on this
contract, a guaranteed and a fallible public oracle transcript, a communication
commitment, and a proof. {@link ContractCall}s are constructed via
{@link ContractCallPrototype}s, which consist of the following raw pieces of data:
* The contract address
* The contract's entry point
* The contract operation expected (that is, the verifier key and transcript
  shape expected to be at this contract address and entry point)
* The guaranteed transcript (as produced by the generated JS code)
* The fallible transcript (as produced by the generated JS code)
* The outputs of the private oracle calls (As a FAB {@link AlignedValue}s)
* The input(s) to the call, concatenated together (As a FAB {@link AlignedValue})
* The output(s) to the call, concatenated together (As a FAB {@link AlignedValue})
* The communications commitment randomness (As a hex-encoded field element string)
* A unique identifier for the ZK circuit used (used by the proof server to index for the prover key)

NOTE: currently the JS code only generates a single transcript. We probably
just want a canonical way to split this into guaranteed/fallible?

A {@link Intent} object is assembed, and {@link ContractCallPrototype}s /
{@link ContractDeploy}s are added to this directly. This can then be inserted into an
{@link Transaction}.

## State Structure

The {@link LedgerState} is the primary entry point for Midnight's ledger state,
and it consists of a {@link ZswapChainState}, as well as a mapping from {@link
ContractAddress}es to {@link ContractState}s. States are immutable, and
applying transactions always produce new outputs states.
