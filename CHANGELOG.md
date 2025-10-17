All notable changes to `ledger`, `ledger-wasm` and `proof-server` are being
tracked here starting with 3.0.0-alpha.3. These packages are tracked together,
with `zswap` being tracked in [Changelog Zswap](./CHANGELOG_zswap.md).

# Change Log

## 6.1.0

- breaking: feat: Add real cost model
  - `dummy` prefixes of cost model/limits changed to `initial` prefix
  - Costs are now given in different dimensions instead of a single gas cost
  - Costs are reasonably calibrated, although measurements are not final
  - `post_block_update` now takes block fullness as an input, and adjust pricing accordingly
- fix: Fix accounting issue in Pedersen check that prevented contracts from
  minting shielded tokens.
- fix: proof server now correctly fetches Dust keys on startup
- fix: proof server no longer crashes if trying to fetch keys from within a worker thread
- fix: allow disabling time-to-dismiss check as part of fee computation for balancing
- fix: correct token type computation in contract mints
- feat: proof server now fetches missing artifacts on demand
- feat: add endpoints for estimating fees with a margin depending on allowed
  block adjustment
- fix: fix balancing bug in contracts with multiple intents

## 6.0.0

- breaking: feat: add Dust - full implications of this go beyond the scope of a
  changelog. Dust replaces shielded token 0 as the fee payment token, and is
  generated from Night tokens held over time, if these are registered.
- breaking: feat: emit events on (system)-transaction application. Events
  should now be used to maintain wallet states, and may be used for further
  purposes. This is a stablisation of the previously unstable events, and their
  structure has changed to catch more errors.
- breaking: feat: remove `inputFeeOverhead` and `outputFeeOverhead`. The use of
  these has dwindled with shielded tokens no longer being the fee payment means,
  and they were never fully accurate. As a more flexible replacement,
  `Transaction.mockProve` produces a 'proven' transaction that is accurate
  (modulo a slight overestimation in rare cases) for fee payments. This can be
  used to estimate the fees of any modification without the expensive proving
  step accurately.
- breaking: pull in breaking serialization changes
- breaking: pull in breaking proof system changes
- breaking: `LedgerState` and `Transaction` now carry a network ID, as a UTF-8
  string. These are checked to match during transaction well-formedness, to
  prevent transactions from being processed against the wrong network. This
  takes the place of the `NetworkId` attached to serializations previously.
- breaking: rename `createProvingPayload` in wasm API to `createProvingTransactionPayload`.
- breaking: add `ProvingProvider` abstraction. A proving provider provides a
  means to evaluate and prove individual proofs. This breaks rust-side proving APIs.
- breaking: rename `ProvingData` to `ProvingKeyMaterial` in wasm API, due to
  closeness with `ProofData` output from contracts. (`ProofData` is currently
  used on the rust side)
- breaking: fix: correctly take `CostModel` as an input to the prove step; this
  is required to account for the additional cost of populating `noop`
  instructions in the transcript.
- breaking: move `ProvingData` to `transient_crypto`, where it is now untyped,
  and renamed to `ProofData`.
- breaking: change the `/prove` proof server endpoint payload format to include
  an optional binding commitment override.
  received from Night with a registered Dust address,
- breaking: remove all references to 'minting', and add clear notions of the
  various Night pools under system control, with system transactions to move
  funds between pools. Night is not created or destroyed post genesis.
- feat: add a `/check` proof server endpoint to check proof preimages, and
  provide skip information.
- feat: expose `Transaction.prove` in wasm API, requiring a `ProvingProvider`
  for low-level proofs.
- feat: add testing `ProvingProvider`s for the proof server, and local proving
  using `zkir`.
- feat: re-add `createProvingPayload`, alongside `createCheckPayload` and
  `parseCheckResult` to handle new proof-server API.
- feat: expose `intentHash` on `Intent` in the WASM target, and make
  `signingData` more flexible.
- feat: add system transactions for cNight generates Dust.
- bugfix: enforce various invariants on deserialization of data.
- note: The `proving` feature now only affects tests and test utilities.
- chore: optimise wasm targets size
- bugfix: Various DUST wasm fixes
- bugfix: Corrections to Merkle Tree WASM
- note: Removal of `dust` feature, functionality now included by default
- feature: Serialization tag testing

## 5.0.0

- feature: add data support for intents
- feature: add well formedness check for intents
- feature: add simple signing capability for intents
- feature: add proving for intents
- feature: add apply for intents
- feature: add ttl replay protection for intents
- feature: rework transactions for unshielded tokens
- breaking: rexported breaking changes in `midnight-coin-structure`
- breaking: `erase_proofs` in `ledger-wasm` can no longer fail

## 4.0.0

- breaking: Integrated with the new storage model, making required objects
  `Storable` to allow storing MPT leafs as `Sp`s.
- breaking: Added segment IDs to Zswap constructors. These should be set to `1`
  for fallible offers, and `0` for guaranteed offers.
- breaking: Renamed `ZswapLocalStateNoKeys` to `ZswapLocalState`, removing the
  existing (with keys) state.
- feat: Add a data provider to fetch key material for Midnight. The source of
  this may be overriden with the `MIDNIGHT_PARAM_SOURCE` environment variable.
- breaking: feat: Switch from Pluto-Eris to BLS12-381.
- breaking: feat: Switched to using data providers instead of direct prover
  keys and parameters.
- breaking: feat: Swtich to ESM for JS/TS targets.
- bugfix: Fixed /health proof-server endpoint blocking when proof server is
  serving too many concurrent requests
- feat: Add /ready proof-server endpoint and JOB_CAPACITY config to proof server to
  support deployment behind a load balancer

## 3.0.7

- bugfix: Remove debug prints from proof server.

## 3.0.6

- bugfix: Fix maintenance update Schnorr proofs not surviving serialization
- introduce `coinCommitment` and `coinNullifier` functions
- introduce API separating keys from Zswap local state to Zswap packages

## 3.0.5

- bugfix: Fix a security bug in Schnorr proofs using incorrect information in
  in-memory environments.
- feature: catch breaking change in midnight-storage-0.3, where the
  serialization shape of `Map` has changed. This necessitated minor version
  bumps on all types with nested `Map`s, as until this point `Map` was
  unversioned.

## 3.0.4

- bugfix: fix transaction re-serialization being incorrect
- bugfix: remove debug logging from proof server

## 3.0.3

- security: remove various instances of triggerable panics in the ledger
- breaking: to enable the above, some data structures are no longer directly
  constructable. This is a minor release despite these breaking changes to
  force it to be picked up as a security fix, and because the API broken is not
  currently public-facing. This is an exception.
  - `Transaction` is no longer a public structured enum. It's components have
    been extracted into `StandardTransaction` and `ClaimMintTransaction`, which
    are no longer directly constructable, due to a private field having been
    added.
  - `ContractCall`, `ContractDeploy`, and `MaintenanceUpdate`, are no longer
    directly constructable, due to a private field having been added.
  - Use the `transaction-construction` feature, and methods provided by this,
    to construct your transactions instead.
- bugfix: fix WASM `checkProofData` endpoint.

## 3.0.2

- security: remove vulnerability in balance checking. This is breaking for a
  minority of transactions.
- bugfix: fix WASM API `fees` endpoint.

## 3.0.1

- bugfix: various surface-level bugs in the web-assembly target.

## 3.0.0

- Pull in various upstream performance improvements.
- Pull in breaking serialization changes for proofs, and storage primitives
- Move network ID from being a stateful thread-local variable to a parameter of
  various serialization functions
- Add a verbose mode to the proof server, logging transaction contents.

## 3.0.0-beta.3.1

- Pull in fixed exports from the onchain runtime wasm target.

## 3.0.0-beta.3

- Fix `inputFeeOverhead` and `outputFeeOverhead` to be read-only properties instead of functions
- Fix values for input and output fee overheads.

## 3.0.0-beta.2

- Pull in upstream fixes
- Add a `MaintenanceUpdate` sub-transaction for updating contracts
- Renaming `CallOrDeploy` to `ContractAction`, and include `MaintenanceUpdate`
  in them.

## 3.0.0-beta.1

- Pull in upstream proof-system improvements
- Pull in upstream addition of contract maintenance authorities to contract
  states
- Fix some wasm API endpoints

## 3.0.0-alpha.5

- Fix some wasm API endpoints

## 3.0.0-alpha.4

- Introduce `LedgerParameters`, and `TransactionCostModel`.
- Store `LedgerParameters` in `LedgerState`.
- Add priviledged `SystemTransaction` and `LedgerState::apply_system_tx`
  - Sytem transactions for:
    - Overwriting ledger parameters
    - Minting native tokens to a set of target coin public keys
    - Minting native tokens to the treasury
    - Paying to a set of targets from the treasury
- Change `Transaction::well_formed` signature to
  `Transaction::well_formed(&self, ref_state: &LedgerState, strictness:
&WellFormedStrictness) -> Result<(), MalformedTransaction>`.
  - Add `WellFormedStrictness` for more granular control of well-formedness
    checks.
  - Add `ref_state` to well-formedness, which is used to:
    - Look-up parameters to use for well-formedness checks
    - Look-up contract verifier keys for well-formedness checks
- Move contract proof verification from `LedgerState::try_apply` to
  `Transaction::well_formed`.
- Add `TransactionContext` for supplying the context in which a transaction is applied.
  - This contains a reference `LedgerState`, a `BlockContext`, and the previous `whitelist` input.
- Add `TransactionResult` and `ErasedTransactionResult`, enums for capturing
  how successful a transaction application was, along with error conditions of
  what caused a (partial) failure in the former.
- Add an extension trait `ZswapLocalStateExt` to `ZswapLocalState` that allows
  applying transactions and system transactions to it directly
  - Applying transactions requires knowledge of the `ErasedTransactionResult`
    for this transaction.
- Change `try_apply` to `apply`, with new semantics: It applies both guaranteed
  and fallible sections, returning the result state, and the
  `TransactionResult`. This takes the new `TransactionContext` as an input.
- Add `batch_apply_{independant,all_or_nothing,until_first_failure}`, batch
  application methods for `apply`.
- Add a `ClaimMint` transaction type (renaming the standard type to
  `Standard`), using Zswap's new `AuthorizedMint`. This claims minting rewards
  previously issued to the authorizing public key by a prior system
  transaction.
- Modify `TransactionIdentifier` format.
- Add to `LedgerState`:
  - A treasury balance, a map from token types to an amount of tokens
    controlled by privileged transactions
  - Unclaimed mints, a map from public keys and token types to issued mints
    (block rewards), that are claimable
  - A counter for the unminted native token supply, initialized to 24
    quadrillion atomic units. (Assumed 24 Billion denominated tokens)
- For Zswap:
  - Add an `AuthorizedMint` type, which creates a new commitment with no backing,
    however requires the recipient to approve.
    - This can be created from a `CoinInfo` and a `local::State`.
    - It can be applied to the ledger state independently of standard `Offer`s.
    - Add a `sign` operation and proof for this type
  - Unify zswap versioning with ledger versioning
- Use tagged releases
