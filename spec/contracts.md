# Contracts states and interactions

This specification's goal is to be fairly detached from the functioning of
smart contracts, however it does have to model them to some level of
abstraction. This is because the sequencing of transcripts, and the *effects*
of transcripts affects what are valid transactions, as contracts can directly
interact with other parts of the ledger.

At it's core, a contract has an *internal state type*, which we refer to as
`StateValue`. This type is left unspecified in this spec, however in practice
it is an [onchain runtime state
value](../apis-and-common-types/onchain-runtime/README.md#values).

```rust
type StateValue;
```

A contract's state consists of:
- It's current `StateValue`
- Maintenance information, in the form of a contract maintenance authority
- A mapping from entry point names to verifier keys
- A token balance mapping for this contract

```rust
struct ContractState {
    data: StateValue,
    operations: Map<Bytes, ZkVerifierKey>,
    maintenance_authority: ContractMaintenanceAuthority,
    balance: Map<TokenType, u128>,
}
```

We will not detail contract maintenance authorities, or maintenance
transactions here at this point, but will assume they exist.

```rust
type ContractMaintenanceAuthority;
type MaintenanceUpdate;

impl LedgerContractState {
    fn apply_maintenance_update(self, upd: MaintenanceUpdate) -> Result<Self>;
}
```

## Deploying a contract

A contract is deployed simply by providing its initial state and a nonce.
The latter is used to ensure that the same contract can be deployed multiple
times, as different instances of the same contract. For the deployment to be
valid, this initial state must have zero balance.

```rust
struct ContractDeploy {
    initial_state: ContractState,
    nonce: [u8; 32],
}

type ContractAddress = Hash<ContractDeploy>;

impl ContractDeploy {
    fn well_formed(self) -> Result<()> {
        assert!(self.initial_state.balance.is_empty());
    }
}

struct LedgerContractState {
    contract: Map<ContractAddress, ContractState>,
}

impl LedgerContractState {
    fn apply_deploy(mut self, deploy: ContractDeploy) -> Result<Self> {
        let contract_address = hash(deploy);
        assert!(!self.contract.contains(contract_address));
        self.contract = self.contract.insert(contract_address, deploy);
        Ok(self)
    }
}
```

## Calling a contract

The most important interaction with a contract is the *call*, which executes
arbitrary code against the contract with a proof of correctness. It's core form
consists of:

- The address and entry point of the contract being called, used to retrieve
  the relevant circuit
- A guaranteed and fallible transcript (to be described later, for now, these
  can be thought of as a `fn(StateValue) -> Option<StateValue>`)
- A communication commitment, which commits to the inputs and outputs of the
  circuit being called
- A ZK proof

```rust
struct ContractCall<P> {
    address: ContractAddress,
    entry_point: Bytes,
    guaranteed_transcript: Option<Transcript>,
    fallible_transcript: Option<Transcript>,
    communication_commitment: Fr,
    proof: P::Proof,
}
```

Conceptually, first the guaranteed transcript is applied, then the fallible
transcript, with any failure during the fallible transcript application
reverting to the state after the guaranteed transcript was applied. This has
the benefit of allowing execution of the guaranteed transcript _prior_ to
taking fee payments for processing the transaction, at the cost of severely
limiting the execution budget provided for it. The fallible transcript by
contrast can have a higher budget, but fees paying for it must always be taken,
even if the transcript does not succeed. This process is transparent for users,
with transaction construction machinery simply trying to place as much of the
call as possible into the guaranteed transcript.

In practice, the `Transcript` is *not* just a partial function over
`StateValue`, but crucially also contains an `Effects` object, which describes
how this contract call interacts with the rest of the system. It also contains
a `gas` bound on the execution costs of the transcript program, but the details
for this are not covered in this spec.

`Effects` are _contained in_ `Transcript`, even though they are computable by
running `program`, as it is assumed that the latter is an expensive operation,
while checking that `Effects` is consistent with the rest of the transaction is
assumed to be a fast check. Essentially, `Effects` declares up front what a
contract _will_ do, and then the longer check that this is correct is deferred.

Similarly, the input to the partial function is not just a `StateValue`, but it
also includes a `CallContext`, which can provide additional information to the
transaction about the conditions it is executed in.

```rust
struct Transcript {
    gas: u64,
    effects: Effects,
    // Model type, not actual
    program: fn(StateValue, CallContext) -> Option<(Effects, StateValue)>,
}
```

Note that the real program is a program as described in the [onchain runtime
specification](../apis-and-common-types/onchain-runtime/README.md).

### Effects

The effects of a contract include:
- Which Zswap coins the contract authorized to spend, by their nullifier
- Which Zswap coins the contract expected to receive, by their commitment
- Which Zswap coins the contract requires to be uniquely present as an output,
  by their commitment
- Which contract calls the contract requires to be uniquely present, by their:
  - Sequence number, for ordering
  - Contract address
  - Hash of the entry point used
  - Communication commitment expected
- Which Zswap coins the contract minted
- Which unshielded coins the contract minted
- Which unshielded coins are expected to have been received
- Which unshielded coins the contract authorized to spend
- Which unshielded outputs the contract requires to be uniquely present as a
  UTXO, or input to another contract
  - Note that the type allows encoding outputting Zswap/Dust tokens to a UTXO
    verifying key. Effects containing this are not considered well formed.

```rust
struct Effects {
    claimed_nullifiers: Set<CoinNullifier>,
    claimed_shielded_receives: Set<CoinCommitment>,
    claimed_shielded_spends: Set<CoinCommitment>,
    claimed_contract_calls: Set<(u64, ContractAddress, Hash<Bytes>, Fr)>,
    shielded_mints: Map<[u8; 32], u64>,
    unshielded_mints: Map<[u8; 32], u64>,
    unshielded_inputs: Map<TokenType, u128>,
    unshielded_outputs: Map<TokenType, u128>,
    claimed_unshielded_spends: Map<(TokenType, PublicAddress), u128>,
}

enum PublicAddress {
    User(Hash<VerifyingKey>),
    Contract(ContractAddress),
}
```

### Context

The call context currently consists of time information, the block hash of
the parent block, and an optional caller, as a contract address or verifying
key hash.

```rust
struct CallContext {
    own_address: ContractAddress,
    com_indicies: Map<CoinCommitment, u64>,
    seconds_since_epoch: Timestamp,
    seconds_since_epoch_err: Duration,
    parent_block_hash: Hash<Block>,
    caller: Option<PublicAddress>,
    balance: Map<TokenType, u128>,
}
```

The call context is in part derived from a block context, given at application
time, and in part from the containing `Intent` and its application. In
particular, the `caller` value comes from the intent and is determined as (in
order):

- The calling contract's address, if applicable
- If there is at least one UTXO input, and all UTXO inputs share their `owner`
  field, then the value of this field
- Otherwise, the call is treated as having no caller

The `com_indicies` value represents the indicies of Zswap `CoinCommitment`s,
which are generated during the application of Zswap outputs, and threaded in
from there.

```rust
struct BlockContext {
    seconds_since_epoch: Timestamp,
    seconds_since_epoch_err: Duration,
    parent_block_hash: Hash<Block>,
}

impl ContractCall {
    fn context(
        self,
        block: BlockContext,
        intent: ErasedIntent,
        state: ContractState,
        com_indicies: Map<CoinCommitment, u64>,
    ) -> CallContext {
        let caller = intent.actions.iter()
            .find_map(|action| match action {
                ContractAction::Call(caller) if caller.calls(self) =>
                    Some(PublicAddress::Contract(caller.address)),
                _ => None,
            })
            .or_else(|| {
                let owners = intent.guaranteed_offer.iter()
                    .chain(intent.fallible_offer.iter())
                    .flat_map(|o| o.inputs.iter())
                    .map(|i| hash(i.owner));
                let owner = owners.next()?;
                if owners.all(|owner2| owner == owner2) {
                    Some(PublicAddress::User(owner))
                } else {
                    None
                }
            });
        CallContext {
            own_address: self.address,
            com_indicies,
            seconds_since_epoch: block.seconds_since_epoch,
            seconds_since_epoch_err: block.seconds_since_epoch_err,
            parent_block_hash: block.parent_block_hash,
            caller,
            balance: state.balance,
        }
    }

    fn calls(self, callee: ContractCall) -> bool {
        self.calls_with_seq(callee).is_some()
    }

    fn calls_with_seq(self, callee: ContractCall) -> Option<(bool, u64)> {
        let calls = self.guaranteed_transcript.iter().map(|t| (t, true))
            .chain(self.fallible_transcript.iter().map(|t| (t, false))
            .flat_map(|(t, guaranteed)| t.effects.claimed_contract_calls.iter().map(|cc| (cc, guaranteed)));
        calls.find(|((seq, addr, ep, cc), guaranteed)|
            if addr == callee.address &&
               ep == hash(callee.entry_point) &&
               cc == callee.communication_commitment
            {
                Some((guaranteed, seq))
            } else {
                None
            })
    }
}
```

### Call well-formedness

A contract call is considered 'well-formed' with respect to a reference state
if the proof verifies against the key recorded at the location in the reference
state. The binding input for the proof is the parent intent's binding
commitment. Using this over the intent hash allows the intent to be modified
after the zero-knowledge proof is carried out, but not after the Pedersen
commitment is finalized.

```rust
impl ContractCall {
    fn well_formed(
        self,
        ref_state: LedgerContractState,
        segment_id: u16,
        parent: ErasedIntent,
    ) -> Result<()> {
        let circuit = ref_state.contract.get(self.address)?.operations.get(self.entry_point)?;
        zk_verify(
            circuit,
            (
                self.guaranteed_transcript.map(|t| t.program),
                self.fallible_transcript.map(|t| t.program),
            ),
            (segment_id, parent.binding_input),
            self.proof,
        )?;
    }
}
```

### Call application

Calls are applied in two steps, first the guaranteed step, then the fallible
step. These are applied specifically during intent application.

```rust
impl LedgerContractState {
    fn apply_call(
        mut self,
        call: ContractCall,
        guaranteed: bool,
        block_context: BlockContext,
        parent_intent: ErasedIntent,
        com_indicies: Map<CoinCommitment, u64>,
    ) -> Result<Self> {
        let transcript = if guaranteed {
            call.guaranteed_transcript
        } else {
            call.fallible_transcript
        };
        let Some(transcript) = transcript else return Ok(self);
        let mut state = self.contract.get(call.address)?;
        let context = call.context(block_context, parent_intent, com_indicies);
        let (effects, data) = transcript.program(state.data, context)?;
        assert!(effects == transcript.effects);
        state.data = data;
        for (tt, val) in transcript.effects.unshielded_inputs {
            let bal = state.balance.get_mut_or_default(tt);
            *bal = (*bal).checked_add(val)?;
        }
        for (tt, val) in transcript.effects.unshielded_outputs {
            let mut bal = state.balance.get_mut_or_default(tt);
            assert!(*bal >= val);
            *bal = (*bal).checked_sub(val)?;
        }
        self.contract = self.contract.insert(call.address, state);
        Ok(self)
    }
}
```

## Contract Actions

A contract action is simply a disjoint union of the above types:

```rust
enum ContractAction {
    Deploy(ContractDeploy),
    Call(ContractCall),
    Maintain(MaintenanceUpdate),
}
```

As calls are the most complex sub-type to apply and check well-formedness,
applying an action inherits from this. Deploys and maintenance updates are
applied as fallible parts of the transaction, as they can involve costly writes
to state.

```rust
impl ContractAction {
    fn well_formed(self, ref_state: LedgerContractState, parent_hash: IntentHash) -> Result<()> {
        match self {
            ContractAction::Deploy(deploy) => deploy.well_formed(),
            ContractAction::Maintain(upd) => upd.well_formed(),
            ContractAction::Call(call) => call.well_formed(ref_state, parent_hash),
        }
    }

    fn as_call(self) -> Option<ContractCall> {
    }
}

impl LedgerContractState {
    fn apply_action(
        mut self,
        action: ContractAction,
        guaranteed: bool,
        block_context: BlockContext,
        parent_intent: ErasedIntent,
        com_indicies: Map<CoinCommitment, u64>,
    ) -> Result<Self> {
        match action {
            ContractAction::Deploy(deploy) if !guaranteed =>
                self.apply_deploy(deploy),
            ContractAction::Maintain(upd) if !guarnateed =>
                self.apply_maintenance_update(upd),
            ContractAction::Call(call) =>
                self.apply_call(
                    call,
                    guaranteed,
                    block_context,
                    parent_intent,
                    com_indicies,
                ),
            _ => Ok(()),
        }
    }
}
```
