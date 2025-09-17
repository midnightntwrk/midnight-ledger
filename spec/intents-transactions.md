# Transaction and Intents

A transaction consists of a set of intents, a guaranteed Zswap offer, a
fallible Zswap offer, and binding randomness.

Intents and fallible offers carry a `segment_id: u16`. This must not be 0
(which is reserved for the guaranteed section), and groups parts that apply
atomically together. These are important for [sequencing](#sequencing) the
order in which parts of the transaction are applied.

```rust
struct Transaction<S, P, B> {
    intents: Map<u16, Intent<S, P, B>>,
    guaranteed_offer: Option<ZswapOffer<P>>,
    fallible_offer: Map<u16, ZswapOffer<P>>,
    binding_randomness: Fr,
}
```

An intent consists of guaranteed and fallible *unshielded* offers, a sequence
of contract actions, a set of dust payments, a TTL timestamp, and a binding commitment
with a proof of knowledge of exponent of `g`, to prevent interfering with the
Zswap value commitments.

A canonical ordering is imposed on the set of dust payments, with only this
order being considered valid. One offer, call, or dust payment must be present
for the intent to be valid.

The transaction is only valid if the TTL is a) not in the past, and b) not too
far in the future (by the ledger parameter `global_ttl`).

```rust
struct Intent<S, P, B> {
    guaranteed_unshielded_offer: Option<UnshieldedOffer<S>>,
    fallible_unshielded_offer: Option<UnshieldedOffer<S>>,
    actions: Vec<ContractAction<P>>,
    dust_actions: Option<DustActions<P>>,
    ttl: Timestamp,
    binding_commitment: B,
}

type ErasedIntent = Intent<(), (), Pedersen>;
type IntentHash = Hash<(u16, ErasedIntent)>;
```

## Lifecycle of Intents and Transactions

The number of type parameters of `Transaction` and `Intent` deserves some
discussion, as it's directly linked to the lifecycle of intents and
transactions. Broadly speaking, a transaction goes through the following
phases, which are represented by parameterising different parts of the
cryptography differently:

- **Transaction construction**: This is gathering the content that should be in a
  transaction - at this point there are no privacy guarantees, the transaction
  can be freely modified, and authenticating signatures are missing.
  Parameterised as `Transaction<Signature, PreProof, PedersenRandomness>`,
  `well_formed` is not expected to succeed signature or balancing checks.
- **Transaction balancing**: At this point, the transaction gets handed to the
  wallet, which should not handle private information relating to contract calls.
  For this purpose zero-knowledge proofs are done before the handover.
  Parameterised as `Transaction<Signature, Proof, PedersenRandomness>`. This
  still allows `Intent`s to be modified, however existing contract calls cannot
  be moved around. `well_formed` is not expected to succeed signature or
  balancing checks.
- **Transaction signing**: At this point, all UTXOs have been added, and
  transaction binding has been enforced. The signatures still need to be added,
  which may be handled by an independent hardware security module handling
  solely this step. Parameterised as `Transaction<Signature, Proof,
  FiatShamirPedersen>`. `well_formed` is not expected to succeed signature
  checks.
- **Transaction submission**: At this point, transactions are submitted and
  catalogued by the node. Parameterised as `Transaction<Signature, Proof,
  FiatShamirPedersen>`. `well_formed` is expected to succeed.

At any stage, parts of the system may want to access a consistent view of the
transaction access all stages, without authenticating information. This view is
captured by parametrising the transaction as `Transaction<(), (), Pedersen>`.
This is also used to define a hash consistent across the latter two stages.

## Sequencing

To execute a transaction, an ordering for the component `Intent`s must first be
established. The guaranteed section always executes first, and the rest of the
transaction executes by segment ID. This has the added benefit that it prevents
malicious 'frontrunning', as a user can simply use segment ID 1 to avoid being
frontrun. This does make co-incidental merges less likely as many transactions
are likely to use the same segment IDs.

There is the additional question of how to sequence calls to the same contract
from different segments. If two segments, with IDs `a < b` are executed, and
each call the same contract `c`, how are the transcripts sequenced?

This is an issue because the contract call *may* contain both guaranteed and
fallible transcripts, but the guaranteed part of `b` must run *before* the
fallible part of `a`. This would violate an assumption that the fallible part
of `a` applies *immediately after* the guaranteed part.

To resolve this, a constraint is placed on merged transactions: If two segments
`a < b` call the same contract, then one of the following must be true:
- `a` does not have a fallible transcript for this call
- `b` does not have a guaranteed transcript for this call
We will refer to this relation as `a` *causally precedes* `b`.

For a longer sequence, this means there must be at most one segment with both a
guaranteed and fallible transcript, and any segment prior to this must have
only guaranteed transcript, and any segment after must have only fallible
transcripts.

Causal precedence is also extended to contract-to-contract calls within a
single intent: If `a` calls `b`, then `a` must causally precede `b`. Finally,
causal precedence is extended to be transitive: If `a` causally precedes `b`,
and `b` causally precedes `c`, then `a` must causally precede `c`. Note that
this *isn't* direct, and must be enforced.

## Replay Protection

To prevent the replay of transactions, all of the intent hashes are kept
in a history of seen intents. If an intent hash is encountered again, it is
rejected.

```rust
struct ReplayProtectionState {
    intent_history: TimeFilterMap<Set<Hash<ErasedIntent>>>,
}

impl ReplayProtectionState {
    fn apply_intent<S, P, B>(mut self, intent: Intent<S, P, B>, tblock: Timestamp) -> Result<Self> {
        let hash = hash(intent.erase_proofs());
        assert!(!self.intent_history.contains(hash));
        assert!(intent.ttl >= tblock && intent.ttl <= tblock + global_ttl);
        self.intent_history = self.intent_history.insert(intent.ttl, hash);
        Ok(self)
    }

    fn apply_tx<S, P, B>(mut self, tx: Transaction<S, P, B>, tblock: Timestamp) -> Result<Self> {
        tx.intents.values().fold(|st, intent| (st?).apply_intent(intent, tblock), Ok(self))
    }

    fn post_block_update(mut self, tblock: Timestamp) -> Self {
        self.intent_history = self.intent_history.filter(tblock);
        self
    }
}
```

Note that no additional replay protection is added for Zswap, as Zswap provides
its own replay protection. This comes at the cost of linear growth, which is a
known bound of the Zswap solution.

## Well-Formedness (and Balancing)

Partly, a transactions well-formedness is just the sum of its parts, however
there are additional checks to perform to ensure a holistic correctness. Those
are:

- Check that the different offers' inputs (and for Zswap, outputs) are disjoint
- Check the [sequencing restrictions](#sequencing) laid out earlier.
- Cross-check token-contract constraints, and contract call constraints
  - For each contract claim in `Effects`, there is one matching call in the
    same segment, and the mapping is bidirectional
  - For each claimed nullifier in `Effects`, there is one matching nullifier in
    the same segment, and the mapping is bidirectional
  - For each claimed shielded spend in `Effects`, there is one matching coin
    commitment in the same segment, and the mapping is bidirectional
  - For each claimed shielded receive in `Effects`, there is one matching
    commitment in the same segment, and the mapping is bidirectional (but may
    overlap with the spend mapping)
  - For each unshielded spend in `Effects`, there is one matching unshielded
    UTXO output or contract input (in `Effects::unshielded_inputs`) in the same
    segment, and the mapping is bidirectional.
- Ensure that the transaction is balanced

Balancing is done on a per-segment-id basis, where segment ID `0` encompasses
the guaranteed section. Balancing also includes fee payments, which are
denominated in `DUST`. Fees and Dust actions across all segments are
accumulated when applying segment 0.

It's also during this time that contract interactions, both with tokens and
with other contracts are enforced. These are enforced as static 1-to-1
existence constraints, where specific interactions also mandate the existence
of another part in a contract.

```rust
impl<S, P, B> Intent<S, P, B> {
    fn well_formed(
        self,
        tblock: Timestamp,
        segment_id: u16,
        ref_state: LedgerState,
    ) -> Result<()> {
        let erased = self.erase_proofs();
        self.guaranteed_offer.map(|offer| offer.well_formed(erased)).transpose()?;
        self.fallible_offer.iter()
            .all(|offer| offer.well_formed(erased))
            .collect()?;
        self.actions.iter()
            .all(|action|
                action.well_formed(
                    ref_state.contract,
                    hash((segment_id, erased)),
                ))
            .collect()?;
        self.dust_actions.iter()
            .all(|dust_actions|
                dust_actions.well_formed(
                    ref_state.dust,
                    ref_state.utxo,
                    segment_id,
                    erased,
                    tblock,
                    ref_state.params.dust,
                ))
            .collect()?;
        B::valid(self.binding_commitment, erased)?;
    }
}

const SEGMENT_GUARANTEED: u16 = 0;

impl<S, P, B> Transaction<S, P, B> {
    fn well_formed(self, tblock: Timestamp, ref_state: LedgerState) -> Result<()> {
        self.guaranteed_offer.map(|offer| offer.well_formed(tblock, 0, ref_state)).transpose()?;
        for (segment, offer) in self.fallible_offer.sorted_iter() {
            assert!(segment != SEGMENT_GUARANTEED);
            offer.well_formed(segment)?;
        }
        for (segment, intent) in self.intents.sorted_iter() {
            assert!(segment != SEGMENT_GUARANTEED);
            intent.well_formed(tblock, segment, ref_state)?;
        }
        self.disjoint_check()?;
        self.sequencing_check()?;
        self.balancing_check()?;
        self.pedersen_check()?;
        self.effects_check()?;
        self.ttl_check_weak(tblock)?;
    }
}
```

The weak TTL check simply checks if the transaction is in the expected time window:

```rust
impl<S, P, B> Transaction<S, P, B> {
    fn ttl_check_weak(self, tblock: Timestamp) -> Result<()> {
        for (_, intent) in self.intents {
            assert!(intent.ttl >= tblock && intent.ttl <= tblock + global_ttl);
        }
    }
}
```

The disjoint check ensures that no inputs or outputs in the different parts
overlap:

```rust
impl<S, P, B> Transaction<S, P, B> {
    fn disjoint_check(self) -> Result<()> {
        let mut shielded_inputs = Set::new();
        let mut shielded_outputs = Set::new();
        let mut unshielded_inputs = Set::new();
        let shielded_offers = self.guaranteed_offer.iter().chain(self.fallible_offer.sorted_values());
        for offer in shielded_offers {
            let inputs = offer.inputs.iter()
                .chain(offer.transients.iter().map(ZswapTransient::as_input))
                .collect();
            let outputs = offer.outputs.iter()
                .chain(offer.transients.iter().map(ZswapTransient::as_output))
                .collect();
            assert!(shielded_inputs.disjoint(inputs));
            assert!(shielded_outputs.disjoint(outputs));
            shielded_inputs += inputs;
            shielded_outputs += outputs;
        }
        let unshielded_offers = self.intents.values()
            .flat_map(|intent| [
                intent.guaranteed_unshielded_offer,
                intent.fallible_unshielded_offer,
            ].into_iter());
        for offer in unshielded_offers {
            assert!(unshielded_inputs.disjoint(offer.inputs));
            unshielded_inputs += offer.inputs;
        }
    }
}
```

The sequencing check enforces the 'causal precedence' partial order above:

```rust
impl<S, P, B> Transaction<S, P, B> {
    fn sequencing_check(self) -> Result<()> {
        // NOTE: this is implemented highly inefficiently, and should be
        // optimised for the actual implementation to run sub-quadratically.
        let mut causal_precs = Set::new();
        // Assuming in-order iteration
        for ((sid1, intent1), (sid2, intent2)) in self.intents.iter().product(self.intents.iter()) {
            if sid1 > sid2 {
                continue;
            }
            // If a calls b, a causally precedes b.
            // Also, if a contract is in two intents, the prior precedes the latter
            for ((cid1, call1), (cid2, call2)) in intent1.actions.iter()
                .enumerate()
                .filter_map(ContractAction::as_call)
                .product(intent2.actions.iter()
                    .enumerate()
                    .filter_map(ContractAction::as_call))
            {
                if sid1 == sid2 && cid1 == cid2 {
                    continue;
                }
                if (sid1 == sid2 && call1.calls(call2)) || (sid1 != sid2 && call1.address == call2.address) {
                    causal_precs = causal_precs.insert(((sid1, cid1, call1), (sid2, cid2, call2)));
                }
            }
        }
        // If a calls b and c, and the sequence ID of b precedes
        // that of c, then b must precede c in the intent.
        for (_, intent) in self.intents.iter() {
            for ((cid1, call1), (cid2, call2), (cid3, call3)) in intent.actions.iter()
                .enumerate()
                .filter_map(ContractAction::as_call)
                .product(intent.actions.iter()
                    .enumerate()
                    .filter_map(ContractAction::as_call))
                .product(intent.actions.iter()
                    .enumerate()
                    .filter_map(ContractAction::as_call))
            {
                if let (Some((_, s1)), Some((_, s2))) = (call1.calls_with_seq(call2), call1.calls_with_seq(call3)) {
                    assert!(cid1 < cid2);
                    assert!(cid1 < cid3);
                    assert!(s1 < s2 == cid2 < cid3);
                }
            }
        }
        // If a calls `b`, `b` must be contained within the 'lifetime' of the
        // call instruction in `a`.
        // Concretely, this means that:
        // - If the call to `b` in in `a`'s guaranteed section, it *must*
        //   contain only a guaranteed section.
        // - If the call to `b` in in `a`'s fallible section, it *must*
        //   contain only a fallible section.
        for (_, intent) in self.intents.iter() {
            for ((cid1, call1), (cid2, call2)) in intent.actions.iter()
                .filter_map(ContractAction::as_call)
                .product(intent.actions.iter().filter_map(ContractAction::as_call))
            {
                if let Some((guaranteed, _)) = call1.calls_with_seq(call2) {
                    if guaranteed {
                        assert!(call2.fallible_transcript.is_none());
                    } else {
                        assert!(call2.guaranteed_transcript.is_none());
                    }
                }
            }
        }
        // Build transitive closure
        let mut prev = Vec::new();
        while causal_precs != prev {
            prev = causal_precs;
            for ((a, b), (c, d)) in prev.iter().product(prev.iter()) {
                if b == c && !prev.contains((a, d)) {
                    causal_precs = causal_precs.insert((a, d));
                }
            }
        }
        // Enforce causality requirements
        for ((_, _, a), (_, _, b)) in causal_precs.iter() {
            assert!(a.fallible_transcript.is_none() || b.guaranteed_transcript.is_none());
        }
    }
}
```

The balance check depends on fee calculations (out of scope), and the overall
balance of the transaction, which is per token type, per segment ID:

```rust
const FEE_TOKEN: TokenType = DUST;

impl<S, P, B> Transaction<S, P, B> {
    fn fees(self) -> Result<u128> {
        // Out of scope of this spec
    }

    fn balance(self, deltas_only: bool) -> Result<Map<(TokenType, u16), i128>> {
        let mut res = Map::new();
        let mut dust_bal = - (self.fees() as i128);
        for (segment, intent) in self.intents.sorted_iter() {
            for dust_spend in self.dust_actions.iter().flat_map(|da| da.spends) {
                dust_bal += min(spends.v_fee, i128::MAX) as i128;
            }
            for dust_reg in self.dust_actions.iter().flat_map(|da| da.registrations) {
                dust_bal += min(dust_reg.allow_fee_payment, i128::MAX) as i128;
            }

            for (segment, offer) in [
                (0, intent.guaranteed_unshielded_offer),
                (segment, intent. fallible_unshielded_offer),
            ] {
                for inp in offer.inputs {
                    let bal = res.get_mut_or_default((TokenType::Unshielded(inp.type_), segment));
                    *bal = (*bal).checked_add(inp.value)?;
                }
                for out in offer.outputs {
                    let bal = res.get_mut_or_default((TokenType::Unshielded(out.type_), segment));
                    *bal = (*bal).checked_sub(out.value)?;
                }
            }

            if deltas_only {
                continue;
            }
            for call in self.actions.iter().filter_map(|action| match action {
                ContractAction::Call(call) => Some(call),
                _ => None,
            }) {
                let transcripts = call.guaranteed_transcript.iter()
                    .map(|t| (0, t))
                    .chain(call.fallible_transcript.iter()
                        .map(|t| (segment, t)));
                for (segment, transcript) in transcripts {
                    for (pre_token, val) in transcript.effects.shielded_mints {
                        let tt = TokenType::Shielded(hash((call.address, pre_token)));
                        let bal = res.get_mut_or_default((tt, segment));
                        *bal = (*bal).checked_add(val)?;
                    }
                    for (pre_token, val) in transcript.effects.unshielded_mints {
                        let tt = TokenType::Unshielded(hash((call.address, pre_token)));
                        let bal = res.get_mut_or_default((tt, segment));
                        *bal = (*bal).checked_add(val)?;
                    }
                    for (tt, val) in transcript.effects.unshielded_inputs {
                        // NOTE: This is an input *to* the contract, so an
                        // output of the transaction.
                        let bal = res.get_mut_or_default((tt, segment));
                        *bal = (*bal).checked_sub(val)?;
                    }
                    for (tt, val) in transcript.effects.unshielded_outputs {
                        // NOTE: This is an output *from* the contract, so an
                        // input to the transaction.
                        let bal = res.get_mut_or_default((tt, segment));
                        *bal = (*bal).checked_add(val)?;
                    }
                }
            }
        }
        for (segment, offer) in self.fallible_offer.sorted_iter()
            .chain(self.guaranteed_offer.iter().map(|o| (0, o)))
        {
            for (tt, val) in offer.deltas {
                let bal = res.get_mut_or_default((TokenType::Shielded(tt), segment));
                *bal = (*bal).checked_add(val)?;
            }
        }
        res.insert((DUST, 0), dust_bal);
        Ok(res)
    }

    fn balancing_check(self) -> Result<()> {
        for bal in self.balance(false)?.map(|(_, bal)| bal) {
            assert!(bal >= 0);
        }
    }
}
```

The Pedersen check ensures that the Pedersen commitments are openable to the
declared balances:

```rust
impl<S, P, B> Transaction<S, P, B> {
    fn pedersen_check(self) -> Result<()> {
        let comm_parts =
            self.intents.sorted_values()
                .map(|intent| {
                    let hash = hash(intent.erase_proofs());
                    intent.binding_commitment.well_formed(hash)?;
                    Ok(Pedersen::from(intent.binding_commitment))
                })?
                .chain(
                    self.guaranteed_offer.iter()
                        .chain(self.fallible_offer.sorted_values())
                        .flat_map(|offer|
                            offer.inputs.iter()
                                .map(|inp| inp.value_commitment)
                                .chain(offer.outputs.iter()
                                    .map(|out| -out.value_commitment))
                                .chain(offer.transients.iter()
                                    .map(|trans| trans.value_commitment_input))
                                .chain(offer.transients.iter()
                                    .map(|trans| -trans.value_commitment_output))));
        let comm = comm_parts.fold(|a, b| a + b, embedded::CurvePoint::identity);
        let expected = self.balance(true)?.filter_map(|((tt, segment), value)| match tt {
            TokenType::Shielded(tt) => Some(hash_to_curve(tt, segment) * value),
            _ => None,
        }).fold(
            |a, b| a + b,
            embedded::CurvePoint::GENERATOR * self.binding_randomness,
        );
        assert!(comm == expected);
        Ok(())
    }
}
```

The effects check ensures that the requirements of each `ContractCall`s
`Effects` section are fulfilled.

```rust
impl<S, P, B> Transaction<S, P, B> {
    fn effects_check(self) -> Result<()> {
        // We have multisets for the following:
        // - Claimed nullifiers (per segment ID)
        // - Claimed contract calls (per segment ID)
        // - Claimed shielded spends (per segment ID)
        // - Claimed shielded receives (per segment ID)
        // - Claimed unshielded spends (per segment ID)

        // transcripts associate with both the their intent segment, and their
        // logical segment (0 for guarnateed transcripts), as the matching uses
        // the former for calls, and the latter for zswap.
        let calls = self.intents.sorted_iter()
            .flat_map(|(segment, intent)|
                intent.actions.iter().filter_map(|action| match action {
                    ContractAction::Call(call) => Some((segment, call)),
                    _ => None,
                }))
            .collect();
        let transcripts = calls.iter()
            .flat_map(|(segment, call)|
                call.guaranteed_transcript.iter()
                    .map(|t| (segment, 0, t))
                    .chain(call.fallible_transcript.iter()
                        .map(|t| (segment, segment, t, call.address)))))
            .collect();
        let offers = self.guaranteed_offer.iter()
            .map(|o| (0, o))
            .chain(self.fallible_offer.sorted_iter())
            .collect();
        let commitments: MultiSet<(u16, CoinCommitment, ContractAddress)> =
            offers.iter().flat_map(|(segment, offer)|
                offer.outputs.iter()
                    .filter_map(|o| o.contract.map(|addr| (o.commitment, addr)))
                    .chain(offer.transients.iter()
                        .filter_map(|t| t.contract.map(|addr| (t.commitment, addr))))
                    .map(|(com, addr)| (segment, com, addr)))
                .collect();
        let nullifiers: MultiSet<(u16, CoinNullifier, ContractAddress)> =
            offers.iter().flat_map(|(segment, offer)|
                offer.inputs.iter()
                    .flat_map(|i| i.contract.map(|addr| (i.nullifier, addr)))
                    .chain(offer.transients.iter()
                        .flat_map(|t| t.contract.map(|addr| (t.nullifier, addr))))
                    .map(|n| (segment, n)))
                .collect();
        let claimed_nullifiers: MultiSet<(u16, CoinNullifier, ContractAddress)> =
            transcripts.iter()
                .flat_map(|(_, segment, t, addr)|
                    t.effects.claimed_nullifiers.iter()
                        .map(|n| (segment, n, addr)))
                .collect();
        // All contract-associated nullifiers must be claimed by exactly one
        // instance of the same contract in the same segment.
        assert!(nullifiers == claimed_nullifiers);
        let claimed_shielded_receives: MultiSet<(u16, CoinCommitment, ContractAddress)> =
            transcripts.iter()
                .flat_map(|(_, segment, t, addr)|
                    t.effects.claimed_shielded_receives.iter()
                        .map(|c| (segment, c, addr)))
                .collect();
        // All contract-associated commitments must be claimed by exactly one
        // instance of the same contract in the same segment.
        assert!(commitments == claimed_shielded_receives);
        let claimed_shielded_spends: MultiSet<(u16, CoinCommitment)> =
            transcripts.iter()
                .flat_map(|(_, segment, t)|
                    t.effects.claimed_shielded_spends.iter()
                        .map(|c| (segment, c)))
                .collect();
        assert!(claimed_shielded_spends.iter_count().all(|(_, count)| count <= 1));
        let all_commitments: MultiSet<(u16, CoinCommitment)> =
            offers.iter().flat_map(|(segment, offer)|
                offer.outputs.iter().map(|o| o.commitment)
                    .chain(offer.transients.iter().map(|t| t.commitment))
                    .map(|c| (segment, c)))
                .collect();
        // Any claimed shielded outputs must exist, and may not be claimed by
        // another contract.
        assert!(all_commitments.has_subset(claimed_shielded_spends));
        let claimed_calls: MultiSet<(u16, (ContractAddress, Hash<Bytes>, Fr))> =
            transcripts.iter()
                .flat_map(|(segment, _, t)|
                    t.effects.claimed_contract_calls.iter()
                        .map(|c| (segment, c)))
                .collect();
        assert!(claimed_contract_calls.iter_count().all(|(_, count)| count <= 1));
        let real_calls: MultiSet<(u16, (ContractAddress, Hash<Bytes>, Fr))> =
            calls.iter().map(|(segment, call)| (
                segment,
                (
                    call.address,
                    hash(call.entry_point),
                    call.communication_commitment,
                )
            )).collect();
        // Any claimed call must also exist within the same segment
        assert!(real_calls.has_subset(claimed_contract_calls));
        let claimed_unshielded_spends: MultiSet<(
            (u16, bool),
            ((TokenType, Either<Hash<VerifyingKey>, ContractAddress>), u128)
        )> = transcripts.iter()
                .flat_map(|(intent_seg, logical_seg, t, _)|
                    t.effects.claimed_unshielded_spends.iter()
                        .map(|spend| ((intent_seg, logical_seg == 0), spend))
                .collect();
        let real_unshielded_spends: MultiSet<(
            (u16, bool),
            ((TokenType, Either<Hash<VerifyingKey>, ContractAddress>), u128)
        )> = transcripts.iter()
                .flat_map(|(intent_seg, logical_seg, t, addr)|
                    t.effects.unshielded_inputs.map(|(tt, val)|
                        (
                            (intent_seg, logical_seg == 0),
                            ((tt, Right(addr)), val),
                        )))
                .chain(self.intents.sorted_iter()
                    .flat_map(|(segment, intent)|
                        intent.guaranteed_unshielded_offer.outputs.iter()
                            .map(|o| (true, o))
                            .chain(intent.fallible_unshielded_offer.outputs.iter()
                                .map(|o| (false, o)))
                            .map(|(guaranteed, output)| (
                                (segment, guarnateed),
                                ((TokenType::Unshielded(output.type_), Left(output.owner)), output.value),
                            ))))
                .collect();
        assert!(real_unshielded_spend.has_subset(claimed_unshielded_spends));
    }
}
```

## Transaction application

Transaction application roughly follows the following procedure:
1. Apply the guaranteed section of all intents, and the guaranteed offer.
  1. When applying fee payments, first all `DustSpend`s are processed
  2. Then all `DustRegistration`s are processed sequentially.
  3. Contract calls and both shielded/unshielded offers are independent of
     this, although contract calls are processed sequentially themselves.
2. Check if each fallible Zswap offer is applicable in isolation. That is:
(that is: are the Merkle trees valid and the nullifiers unspent?).
3. In order of sequence IDs, apply the fallible sections of contracts, and the
   fallible offers (both Zswap and unshielded).


If any one sequence in 3. fails, this sequence, and this sequence only, is
rolled back. If any part of 1. or 2. fails, the transaction fails in its
entirety. To represent this, the transaction returns a success state which is
one of:
- `SucceedEntirely` (all passed with no failures)
- `FailEntirely` (failure in 1. or 2.)
- `SucceedPartially`, annotated with which segment IDs succeeded, and which
  failed.

```rust
enum TransactionResult {
    SucceedEntirely,
    FailEntirely,
    SucceedPartially {
        // Must include (0, true).
        segment_success: Map<u16, bool>,
    }
}

impl<S, P, B> Transaction<S, P, B> {
    fn segments(self) -> Vec<u16> {
        let mut segments = once(0)
            .chain(self.intents.sorted_iter().map(|(s, _)| s))
            .chain(self.fallible_offer.sorted_iter().map(|(s, _)| s))
            .collect::<Vec<_>>();
        segments.sort();
        segments.dedup();
        segments
    }
}

struct LedgerParameters {
    // ...
    dust: DustParameters,
}

struct LedgerState {
    utxo: UtxoState,
    zswap: ZswapState,
    contract: LedgerContractState,
    replay_protection: ReplayProtectionState,
    dust: DustState,
    params: LedgerParameters,
}

impl LedgerState {
    fn apply<S, P, B>(
        mut self,
        tx: Transaction<S, P, B>,
        block_context: BlockContext,
    ) -> (Self, TransactionResult) {
        let segments = tx.segments();
        let mut segment_success = Map::new();
        let mut total_success = true;
        for segment in segments.iter() {
            match self.apply_segment(tx, segment, block_context) {
                Ok(state) => {
                    self = state;
                    segment_success = segment_success.insert(segment, true);
                }
                Err(e) => if segment == 0 {
                    return (self, TransactionResult::FailEntirely);
                } else {
                    segment_success = segment_success.insert(segment, false);
                    total_success = false;
                },
            }
        }
        (self, if total_success {
            TransactionResult::SucceedEntirely
        } else {
            TransactionResult::SucceedPartially {
                segment_success,
            }
        })
    }

    fn apply_segment<S, P, B>(
        mut self,
        tx: Transaction<S, P, B>,
        segment: u16,
        block_context: BlockContext,
    ) -> Result<Self> {
        if segment == 0 {
            // Apply replay protection
            self.replay_protection = self.replay_protection.apply_tx(
                tx,
                block_context.tblock,
            )?;
            let com_indicies = if let Some(offer) = tx.guaranteed_offer {
                (self.zswap, indicies) = self.zswap.apply(offer)?;
                indicies
            } else {
                Map::new()
            };
            // Make sure all fallible offers *can* be applied
            assert!(tx.fallible_offer.sorted_values()
                .fold(Ok(self.zswap), |st, offer| st?.apply(offer))
                .is_ok());
            // Apply all guaranteed intent parts
            for intent in tx.intents.sorted_values() {
                let erased = intent.erase_proofs();
                if let Some(offer) = intent.guaranteed_unshielded_offer {
                    self.utxo = self.utxo.apply_offer(
                        offer,
                        0,
                        erased,
                        block_context.tblock,
                    )?;
                    self.dust = self.dust.apply_offer(
                        offer,
                        0,
                        erased,
                        block_context.tblock,
                    )?;
                }
                for action in intent.actions.iter() {
                    self.contract = self.contract.apply_action(
                        action,
                        true,
                        block_context,
                        erased,
                        com_indicies,
                    )?;
                }
            }
            // process fees and dust actions first. This is not in `apply_segment`,
            // as they are not processed segment-by-segment.
            let mut fees_remaining = tx.fees();
            // apply spends first, to make sure registration outputs get the
            // maximum dust they can.
            for (time, dust_spend) in tx.intents.sorted_values()
                .flat_map(|i| i.dust_actions.iter().flat_map(|a| a.spends.iter().map(|spend| (a.ctime, spend))))
            {
                self.dust = self.dust.apply_spend(dust_spend)?;
                fees_remaining = fees_remaining.saturating_sub(dust_spend.v_fee);
            }
            // Then apply registrations
            for intent in tx.intents.sorted_values() {
                for (time, reg) in intent.dust_actions
                    .iter()
                    .flat_map(|a| a.registrations.iter().map(|reg| (a.ctime, reg)))
                {
                    (self.dust, fees_remaining) = self.dust.apply_registration(
                        self.utxo,
                        fees_remaining,
                        intent.erase_proofs(),
                        reg,
                        self.params.dust,
                        ctime,
                        block_context,
                    )?;
                }
            }
            assert!(fees_remaining == 0);
        } else {
            let com_indicies = if let Some(offer) = tx.fallible_offer.get(segment) {
                (self.zswap, indicies) = self.zswap.apply(offer)?;
                indicies
            } else {
                Map::new()
            };
            if let Some(intent) = tx.intents.get(segment) {
                let erased = intent.erase_proofs();
                if let Some(offer) = intent.fallible_unshielded_offer {
                    self.utxo = self.utxo.apply_offer(
                        offer,
                        segment,
                        erased,
                        block_context.tblock,
                    )?;
                    self.dust = self.dust.apply_offer(
                        offer,
                        segment,
                        erased,
                        block_context.tblock,
                    )?;
                }
                for action in intent.actions.iter() {
                    self.contract = self.contract.apply_action(
                        action,
                        false,
                        block_context,
                        erased,
                        com_indicies,
                    )?;
                }
            }
        }
        Ok(self)
    }
}
```
