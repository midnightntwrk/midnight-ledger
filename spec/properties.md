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
> - Dust balances are *not* preserved, but rather, for a specific address:
>   - Monotonically approach a target value, proportional to the amount of
>     Night generating Dust for this address, within a fixed time window
>   - Decrease when spent to cover transaction fees, for which the total spent
>     Dust must cover at least the fees.
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
