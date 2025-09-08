# Midnight Onchain Runtime TypeScript API

This API provides a TypeScript interface to Midnight's onchain runtime,
including the execution of VM instructions, and the primitives required to
successfully use them.

Key parts of this API are:

- {@link ContractState}, encapsulating the entirety of a smart contract's
  on-chain state
- {@link StateValue}, encoding data a contract maintains on-chain
- {@link QueryContext}, providing an annotated view into the contract state,
  against which on-chain VM programs can be run
- {@link Op}, providing the TypeScript encoding of on-chain VM programs
- {@link AlignedValue}, the "base" value type that encodes all user data stored
  on-chain
