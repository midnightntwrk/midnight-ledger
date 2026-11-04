# Midnight Transaction Type Catalog

This document enumerates each transaction *type / variant* — its **purpose**,
**key fields**, **who creates it**, **when it is used**, and **validity
constraints**.  The byte-level **structure and wire serialization** are in
the companion [transaction-format.md](./transaction-format.md); refer there
for full field tables and `[vN]` tags.  Contract-call transcripts are the
[Impact opcode reference](./impact-opcodes.md).

## 1. The transaction taxonomy at a glance

Midnight has **two layers** of "transaction":

| Layer | Type | Submitted by | Pays fees? | Carries proofs? |
|---|---|---|---|---|
| **User transactions** | `Transaction::Standard` | users / wallets | yes (DUST) | yes (ZK + signatures) |
| | `Transaction::ClaimRewards` | users / wallets | yes (from the claim) | signature only |
| **System transactions** | `SystemTransaction` (9 variants) | the chain itself (privileged) | no | no |

`Transaction` is the user-facing enum (`ledger/src/structure.rs`, tag
`transaction[v12]`):

```rust
pub enum Transaction<S, P, B, D> {
    Standard(StandardTransaction<S, P, B, D>),
    ClaimRewards(ClaimRewardsTransaction<S, D>),
}
```

`SystemTransaction` is a **separate** type — it is *applied* by the node
(`LedgerState::apply_system_tx`), never submitted in a block as a user
`Transaction`, and is unauthenticated / unproven because its authority comes
from the consensus layer that produces it.

## 2. User transactions

### 2.1 `Transaction::Standard` → `StandardTransaction`

* **Tag:** `standard-transaction[v12]`.
* **Purpose:** the general-purpose transaction — shielded (Zswap) and
  unshielded (Night) value transfers, contract deploys, contract calls,
  contract maintenance, and DUST fee payment — grouped into
  atomically-applied segments.
* **Key fields:** `network_id`, `intents: HashMap<Segment, Intent>`,
  `guaranteed_coins: Option<ZswapOffer>`,
  `fallible_coins: HashMap<Segment, ZswapOffer>`, `binding_randomness`. (Full
  breakdown: [transaction-format.md §2.1](./transaction-format.md#21-standardtransaction).)
* **Who creates it:** users, via wallets / the SDK. It moves through the
  lifecycle stages (construction → balancing → signing → submission) encoded
  by the `S` / `P` / `B` type parameters.
* **When it is used:** essentially all normal on-chain activity.
* **Validity constraints** (`Transaction::well_formed`, see
  [intents-transactions.md](./intents-transactions.md)): inputs / outputs
  across offers are disjoint; the guaranteed / fallible **sequencing**
  (causal-precedence) rules hold; the transaction **balances** per
  `(token, segment)` including DUST fees; **Pedersen** commitments open to
  the declared balances; each contract call's `Effects` are matched 1-to-1;
  signatures and ZK proofs verify; and every intent's `ttl` lies in
  `[tblock, tblock + global_ttl]`.
* **Outcome** (`TransactionResult`, `ledger/src/semantics.rs`):
  `Success` / `Failure` / `PartialSuccess` carrying a per-segment
  `BTreeMap<u16, Result<(), TransactionInvalid>>` (guaranteed section is
  all-or-nothing; fallible segments roll back independently).

### 2.2 `Transaction::ClaimRewards` → `ClaimRewardsTransaction`

* **Tag:** `claim-rewards-transaction[v2]`.
* **Purpose:** withdraw Night that has accrued to an address — either
  **block rewards** or **Cardano-bridge** receipts — into a spendable UTXO.
  It is a deliberately minimal, proof-free transaction.
* **Key fields:** `network_id`, `value: u128`, `owner: SignatureVerifyingKey`,
  `nonce: Nonce`, `signature`, `kind: ClaimKind` (`Reward` | `CardanoBridge`).
* **Who creates it:** the owner of the unclaimed rewards. The signature is over
  the tagged `ClaimRewardsTransactionSigningEnvelope` — i.e. the transaction
  fields `network_id`, `value`, `owner`, `nonce`, and `kind` (the `signature`
  field erased) — under the domain separator
  `midnight:claim-rewards-transaction-signing-envelope[v2]:`. See
  [transaction-format.md §2.2](./transaction-format.md) for the byte layout.
* **When it is used:** to claim the balance previously credited to
  `unclaimed_block_rewards` / `bridge_receiving` by a `DistributeNight`
  system transaction (§3).
* **Validity constraints:** the signature must verify; `network_id` must
  match; the amount must be claimable (the ledger enforces a minimum,
  `LedgerParameters::min_claimable_rewards`, derived so the claim can cover
  its own theoretical DUST fee at the Dust cap — otherwise `RewardTooSmall`);
  and the claimed value must not exceed the address's claimable balance.

> `VerifiedTransaction` (`structure.rs`) is not a separate variant — it is a
> wrapper produced after verification, holding the proof-erased
> `Transaction<(),(),Pedersen>` plus its `TransactionHash` (tag
> `transaction-hash`).

## 3. System transactions (`SystemTransaction`)

* **Tag:** `system-transaction[v9]` (`#[non_exhaustive]`).
* **Who creates them:** the **chain / consensus layer** (block production,
  the partner-chain / Cardano bridge, and governance). They are *privileged*
  — applied directly to `LedgerState` via `apply_system_tx`, carry no fees,
  signatures, or proofs, and are logged as `[privileged]`.
* **Global validity invariant:** Night is conserved across the four pools —
  `treasury`, `reserve_pool`, `block_reward_pool`, and `locked_pool` — so
  payouts are rejected if the source pool is insufficient.

| Variant | Purpose | Key fields | Validity / notes |
|---|---|---|---|
| `OverwriteParameters` | replace the active `LedgerParameters` (cost model, limits, dust, fees, TTL, bridge params) | `LedgerParameters` | `cardano_to_midnight_bridge_fee_basis_points ≤ 10_000` (else `InvalidBasisPoints`); emits a `ParamChange` event |
| `DistributeNight` | credit Night to addresses from a pool, by `ClaimKind` | `ClaimKind`, `Vec<OutputInstructionUnshielded>` | total `≤ block_reward_pool` (`Reward`) or `≤ locked_pool` (`CardanoBridge`), else `IllegalPayout`. `Reward` credits `unclaimed_block_rewards`; `CardanoBridge` deducts the bridge fee (basis points; whole amount if `< c_to_m_bridge_min_amount`) to `treasury` and credits `bridge_receiving`. Replay-protected per output |
| `PayBlockRewardsToTreasury` | move block-reward Night into the treasury | `amount: u128` | `amount ≤ block_reward_pool`, else `IllegalPayout` |
| `DistributeReserve` | release reserve Night into the block-reward pool (emission) | `{ amount: u128 }` | `amount ≤ reserve_pool`, else `IllegalReserveDistribution`; moves `reserve_pool → block_reward_pool` |
| `CNightGeneratesDustUpdate` | register / deregister cardano-Night → DUST generation | `events: Vec<CNightGeneratesDustEvent>` | each event is `Create` or `Destroy` of a dust-generation entry (value, owner, time, nonce) |
| `UnlockToTreasury` | release Night from the locked (Cardano-bridge) pool into the treasury | `{ amount: u128 }` | `amount ≤ locked_pool`, else `IllegalPayout`; moves `locked_pool → treasury` |
| `UnlockToReserve` | release Night from the locked pool into the reserve pool | `{ amount: u128 }` | `amount ≤ locked_pool`, else `IllegalPayout`; moves `locked_pool → reserve_pool` |
| `PayFromTreasuryShielded` | (intended) pay shielded outputs from the treasury | `outputs`, `nonce`, `token_type` | **currently DISABLED** — returns `TreasuryDisabled` pending treasury governance |
| `PayFromTreasuryUnshielded` | (intended) pay unshielded outputs from the treasury | `outputs`, `token_type` | **currently DISABLED** — returns `TreasuryDisabled` |

Supporting payload types (`structure.rs`): `OutputInstructionUnshielded`
(`amount`, `target_address`, `nonce`; tag
`output-instruction-unshielded[v1]`), `OutputInstructionShielded` (`amount`,
`target_key`; `output-instruction-shielded[v2]`), `ClaimKind`
(`Reward` | `CardanoBridge`; `claim-kind[v1]`), `CNightGeneratesDustEvent`
(`value`, `owner`, `time`, `action: Create|Destroy`, `nonce`;
`cnight-generates-dust-event[v1]`).

`SystemTransactionError` (`error.rs`) enumerates the rejection reasons:
`IllegalPayout`, `IllegalReserveDistribution`, `InsufficientTreasuryFunds`,
`TreasuryDisabled`, `InvalidBasisPoints`, `ReplayProtectionFailure`,
`CommitmentAlreadyPresent`, `GenerationInfoAlreadyPresent`,
`InvariantViolation`, `MerkleTreeError`.

## 4. Transaction identifiers (`TransactionIdentifier`)

* **Tag:** `transcation-id[v1]` *(sic — the on-wire tag is misspelled; see
  [transaction-format.md §8](./transaction-format.md#8-discrepancies-and-gaps-vs-the-narrative-spec))*.

```rust
pub enum TransactionIdentifier { Merged(Pedersen), Unique(HashOutput) }
```

A transaction yields one **or more** identifiers
(`Transaction::identifiers()`), used to recognise it (or its parts) even
after transactions have been **merged**:

* **`Merged(Pedersen)`** — produced for a `Standard` transaction, **one per**
  Zswap input value commitment, output value commitment, transient
  input / output commitment, and per **intent binding commitment**
  (downgraded to a Pedersen point). Because merging concatenates these
  components, a merged transaction still carries each part's identifier, so
  membership can be tested with `has_identifier`.
* **`Unique(HashOutput)`** — produced for a `ClaimRewards` transaction: a
  single content hash, computed as the `IntentHash` of the equivalent
  `OutputInstructionUnshielded` (`{value, owner→address, nonce}`) over
  `NIGHT`.

`TransactionIdentifier` serialises to / from a byte form via the `serialize`
crate and is also `serde`-encodable.

## 5. Cost and limits (cross-reference)

Every user transaction is priced and bounded by the active `LedgerParameters`
(`ledger-parameters[v8]`), which embeds:

### `TransactionCostModel` (`transaction-cost-model[v5]`)

| Field | Meaning |
|---|---|
| `runtime_cost_model` | the **Impact VM** `CostModel` (per-opcode gas; see the [opcode reference §6](./impact-opcodes.md#6-gas-and-cost-model)) |
| `baseline_cost` | fixed `RunningCost` floor per transaction (initial: `100 µs` compute) |
| `validation_factor` | `FixedPoint` multiplier applied to well-formedness/validation compute (initial: `1/4` — the carry-over of the former `parallelism_factor: 4`) |
| `guaranteed_factor` | `FixedPoint` multiplier on guaranteed-section compute (initial: `1`) |
| `fallible_factor` | `FixedPoint` multiplier on fallible-section compute (initial: `1`) |

The cost model turns a transaction's reads / compute / writes into a
multi-dimensional `SyntheticCost`, which `FeePrices` then converts into a
**DUST** fee. It also prices proof verification (`proof_verify`), state
reads / writes, map / Merkle-tree updates, etc.

### `TransactionLimits` (`transaction-limits[v3]`)

| Field | Initial value | Meaning |
|---|---|---|
| `transaction_byte_limit` | `1 MiB` | max serialised transaction size (`TransactionTooLarge` otherwise) |
| `time_to_dismiss_per_byte` | `2 µs` | per-byte component of the dismissal window |
| `min_time_to_dismiss` | `15 ms` | floor on the dismissal window |
| `block_limits` | 1 s read / 1 s compute / 200k block-usage / 50k written / 1M churned | per-block resource ceiling (`BlockLimitExceeded`) |
| `block_withdrawal_minimum_multiple` | `1/2` | multiple driving `min_claimable_rewards` (§2.2) |
| `max_contract_metadata_size` | `50_000` | hard limit on the size of associated contract metadata |

`LedgerParameters` also carries `dust: DustParameters`,
`fee_prices: FeePrices`, `global_ttl` (initial `3600 s` — bounds intent TTLs
and replay history), `cost_dimension_min_ratio`,
`price_adjustment_a_parameter`, the Cardano-bridge fee parameters, and
`min_block_price: FixedPoint` — the minimum value for
`fee_prices.overall_price`. The active values are `INITIAL_PARAMETERS` until
changed by an `OverwriteParameters` system transaction (§3).

## Appendix A — type / tag index

| Type | Tag | Catalogued in |
|---|---|---|
| `Transaction` | `transaction[v12]` | §1 |
| `StandardTransaction` | `standard-transaction[v12]` | §2.1 |
| `ClaimRewardsTransaction` | `claim-rewards-transaction[v2]` | §2.2 |
| `SystemTransaction` | `system-transaction[v9]` | §3 |
| `ClaimKind` | `claim-kind[v1]` | §3 |
| `OutputInstructionUnshielded` | `output-instruction-unshielded[v1]` | §3 |
| `OutputInstructionShielded` | `output-instruction-shielded[v2]` | §3 |
| `CNightGeneratesDustEvent` | `cnight-generates-dust-event[v1]` | §3 |
| `TransactionIdentifier` | `transcation-id[v1]` *(sic)* | §4 |
| `TransactionHash` | `transaction-hash` | §2.2 |
| `TransactionCostModel` | `transaction-cost-model[v5]` | §5 |
| `TransactionLimits` | `transaction-limits[v3]` | §5 |
| `LedgerParameters` | `ledger-parameters[v8]` | §5 |
