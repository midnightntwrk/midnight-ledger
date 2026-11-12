# Properties of Midnight Transactions

At this time, we do not have formal security proofs for Midnight's behaviour.
However, this document strives to *state* theorems we expect to hold about
Midnight transactions, and to provide an argument for why we believe these to
be true.

## Balance Preservation

> **Theorem 1 (Balance Preservation).** A transaction does not modify the total
> amount of funds in the system, with the following exceptions:
> - Contract *mint* operations (as witnessed by contained `Effects` in
>   transcripts in each executed segment) create the recorded amount of
>   funds in a token type specific to the issuing contract.
> - Dust balances are *not* preserved. Instead, the following hold:
>   - **(Supply cap.)** At any time, the sum of the updated values of all unspent Dust UTXOs is at most `night_dust_ratio` times the total Night supply.
>   - **(Generation rate.)** Over any interval, the total Dust credited against a given source Night is at most `generation_decay_rate` times its value times the elapsed time, independent of how many transactions interact with it.
>   - For a given Dust address, values:
>     - Monotonically approach a target value, proportional to the amount of
>       Night generating Dust for this address, within a fixed time window
>     - Decrease when spent to cover transaction fees, for which the total spent
>       Dust must cover at least the fees.
> - Transactions with a net positive balance will be paid into the treasury.

Importantly, the total in balance preservation is between the utxo sets, and
the unshielded token balances held by contracts. This holds for both unshielded
and shielded tokens, but is not observable for the shielded ones.

*Correctness Argument.* The transaction balancing check tests that transactions
are positive value, which excess value going to the treasury. Therefore, it is
sufficient to show that the value of a transaction is accurate, and not counted
incorrectly at any point. This is provided by the following enforced checks:

- The unshielded balance computation ensuring that the sum of inputs minus the
  sum of outputs is positive
  - This matching exactly to inputs that are removed from the state, and
    outputs that are added to the state ensuring this correctly reflects movement
    in the state
  - Contract inputs being subtracted from the balance, and contract outputs
    being added to the balance, exactly matching the delta to the contract
    balances
- As each check is enforced on a per-intent basis, regardless of the
  combination of intents that succeeds, the applied changes must be balanced.
- For the shielded portion, for (visible) contract balances, as for unshielded
  contract balances.
- For the shielded portion, a similar argument to applying inputs and outputs
  applies.
  - In this case, the check is replaced with the opening of the Pedersen
    commitment to the expected balance.
  - The integrity and correctness of this commitment is provided by the
    zero-knowledge proof.
  - The impossibility to interfere between different token types, is given by
    the hardness of discrete logarithm.
  - The impossibility to interfere with the `Intent` Pedersen commitment is
    given by the Fiat-Shamir transform, guaranteeing the use of the generator,
    and the hardness of discrete logarithm.

*Correctness Argument (Dust).* The argument above covers the conserved token
types. Dust is not conserved, and the two bounds on it rest on separate
mechanisms.

For the **supply cap**, it suffices that each unspent Dust UTXO is bounded by the
Night backing it, that backings are not shared, and that the backings of one
Night principal do not accumulate:

- `updated_value` clamps the generating phases to `vfull = gen.value *
  night_dust_ratio`, and the decaying phase only subtracts from that. An
  individual Dust UTXO therefore never exceeds `night_dust_ratio` times the value
  recorded in its generation info, whatever its initial value.
- Generation infos are unique: `fresh_dust_output` refuses to insert a
  `DustGenerationUniquenessInfo` already present in `generating_set`, and each is
  keyed by an `InitialNonce` derived from the intent hash and output number of the
  Night output that created it. Distinct generation infos thus correspond to
  distinct Night outputs, and the value recorded in each is that output's value.
- Each generation info backs at most one unspent Dust UTXO at a time: exactly one
  is created alongside it, and Dust spends are 1-to-1, consuming a nullifier and
  producing a single commitment for the same owner and backing Night.
- When Night moves, retiring and arriving generation infos offset exactly. Both
  the cap and the rate are linear in `gen.value`; the input's `dtime` and the
  output's `ctime` are both set to the block time; and Night itself is conserved
  by the argument above. The decay of the retiring record therefore proceeds at
  the same aggregate rate as the generation of the arriving ones, and their sum
  does not increase.

Summing over generation infos gives the bound, on the reading that "total Night
supply" counts Night held in the treasury and Night bridged to Cardano. That
reading is load-bearing: Dust outlives the movement of its backing Night by up to
`night_dust_ratio / generation_decay_rate`, so an accounting in which Night can
leave the total while its Dust is still decaying would violate the bound for that
period.

The **generation rate** bound follows from the crediting intervals of a single
Night principal being disjoint:

- A generation info credits Dust only over the `[ctime, dtime]` window of the
  Dust UTXO chain it backs, at a slope of `gen.value * generation_decay_rate`.
- Where one generation info retires and another is created for the same
  principal, the first's `dtime` and the second's `ctime` are both the block time
  of the transaction spending the Night. The two windows abut without overlapping;
  in particular neither endpoint is the author-declared `DustActions.ctime`.
- The only route to a Dust UTXO with a nonzero initial value — the registration
  entitlement computed by `generationless_fee_availability` — is restricted to
  Night inputs absent from `night_indices`, that is, to principals with no
  generation info crediting them over the same period. The amount is bounded by
  the time elapsed since the Night UTXO's own creation time as recorded in the
  UTXO state, and clamped to `vfull`.
- None of these endpoints depends on the number of intents, registrations, or
  transactions involved, so the bound is independent of transaction frequency.

For Night bridged from Cardano both endpoints are instead the times supplied by
the system transaction, and the bounds hold only insofar as those times are
consistent with the Cardano-side lifetime of the backing cNight.

## Binding

> **Theorem 2 (Transaction Binding).** A transaction, once assembled, can only
> be disassembled by the user that first assembled it. No part of the
> transaction can be meaningfully used in another transaction, without
> including all other parts with it.

*Security Argument.* Transaction binding is primarily provided by the
binding randomness and Pedersen commitments. This works in combination with the
binding properties of signatures and of zero-knowledge proofs to ensure that
the transaction as a whole is binding.

- Each intent, and Zswap input/output, has an associated Pedersen commitment
- The transaction overall reveals the sum of all their binding randomnesses.
- Due to the hardness of the discrete logarithm problem, if each Pedersen
  randomness is uniformly randomly distributed, there is no feasible way to
  recover the randomness from any intent or input/output given this
  transaction.
- Therefore, the transaction is binding on a macro-level: For any given part,
  given by a Pedersen commitment, this part cannot be isolated
- For intents, the Pedersen commitment is binding over the intent, due to the
  Fiat-Shamir transform taking the intent hash as part of the challenge string,
  and therefore the proof of knowledge of exponent not being able to be applied
  to a different intent.
  - It also cannot be recomputed without the knowledge of the individual
    randomness, which was ruled out above.
- For zswap inputs and outputs, the zero-knowledge proof is binding over the
  input and output (including the Pedersen commitment), and without knowledge of
  the Pedersen randomness, cannot be recreated for a different input or output.
- For transients, there is a direct malleability that the transient can be
  decomposed into its input and output constituents.
  - While this is possible, the decomposed input will not be valid, as it is
    proven against a Merkle tree containing only itself, which will not be a
    valid Merkle tree.
    - A corner case is if Midnight ever had a single shielded UTXO in its
      state. This case will be mitigated by initializing the shielded set with
      a single unspendable UTXO at genesis.
- For the deltas provided in the Zswap offers, note that these are fully
  determined by the Pedersen commitments, as only one valid assignment can be
  used to open the summed commitment.

## Infragility

> **Theorem 3 (Infragility).** For a *defensively created* transaction `t`, a
> malicious user cannot cause `t` to fail by merging a malicious transaction
> `m` with `t`, except for the following ways:
> - If the malicious user could replicate the failure by first getting a
>   malicious transaction `m'` accepted, and then applying `t`.
> - If, while `merge(m, t)` fails, `t` itself may still subsequently succeed.

By *defensively created*, this theorem explicitly means that the segment IDs of
the intents in `t` count from `1`; that is, for some natural `n`, all intents
`0 < m < n` are present in `t`. This prevents a malicious user from
'frontrunning' any of the intents in `t`, preventing a class of relatively
acceptable failures.

*Security Argument.* The general idea here is that a transaction falls into one
of three cases:
1. The merged transaction fails during the execution of the guaranteed phase,
   in which case `t` either would fail during the guaranteed phase as well, or
   the transaction has no effect, so `t` itself is still valid.
2. The merged transaction fails during one of the segments originally in `t`.
   If it fails due to new additions in the merge, these could be:
   - Additions to the zswap offer, which get checked during the guaranteed
     phase (actually falling into case 1.).
   - Additions fund transfers added to the guaranteed phase. These cannot
     conflict with spends by the honest user, as the adversary cannot spend their
     funds (Theorem 5), and contract transfers fall under the more general case
     of a contract call (below).
   - Additional contract calls. If these affect the execution of the intents in
     `t`, they must be to the same contract as the conflict in `t`.
     - The adversarial call `A` has a guaranteed section by assumption.
     - The original call `C` has a fallible section by assumption that it can
       conflict in the fallible segment.
     - Therefore, for these to satisfy the causal precedence check, `C`
       *cannot* have a guaranteed section.
     - However, if this is the case, `A` can be extracted from the transaction
       into an earlier transaction.
3. The merged transaction fails during a segment not originally in `t`. In this
   case, `t` itself has effectively succeeded, as all effects it wanted to
   execute against the state have been executed.

## Causality

> **Theorem 4 (Causality).** If a transaction includes a contract call from `A`
> to `B`, then `A` succeeding must imply `B` succeeding.

*Security Argument.* Note that for a call from `A` to `B` to be considered
valid (under the effects check for contract calls), `A` and `B` must be in the
same `Intent`, and adds a 'lifetime' check that ensures that `B` is confined to
the section that enforces the call to it in `A`. As a result, the contract `A`
sees `B` as having been called if and only if it has.

## Self-determination

> **Theorem 5 (Self-determination).**
> 1. A user cannot spend the funds of another user. No contract can spend funds
>    of a user.
> 2. A contract can only be modified according to the rules of the contract.

*Security Argument.* 1. is given by spending user funds requiring a signature
with this users secret key. 2. is given by limits on semantics:
- Only `ContractAction`s affect contract states
- `ContractDeploy`s do not affect already existing contracts
- `MaintenanceUpdate`s are signed with keys explicitly authorized by the contract
- `ContractCall`s must satisfy one of the verifier keys explicitly set by the
  contract, and change the state according to this verifier key's restrictions.
