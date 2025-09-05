# Night and other unshielded tokens

We construct an unshielded UTXO token set, for Night, extensible to other token
types. UTXOs, or unspent transaction outputs, are data recording individual
transaction outputs, each having a value and an owner. As we are extending
these with token types, they also have an associated token type in our model.

## Building UTXOs

We define the basic structure of an individual UTXO, define the data for
creating a new UTXO output, which is just the UTXO itself, and for spending a
UTXO. The latter acts not just as a standalone transaction part, but
encompasses other data – this is because a spend comes with conditions on what
it is used for. Defining the composition of these is beyond this document.
Finally, the state maintained for UTXOs at any time is simply a set of all
UTXOs.

We use the term `value` here to mean 'amount of indivisible units of the given
token type'.

```rust
type NightAddress = Hash<VerifyingKey>;

struct Utxo {
    value: u128,
    owner: NightAddress,
    type_: RawTokenType,
    intent_hash: IntentHash,
    output_no: u32,
}

struct UtxoOutput {
    value: u128,
    owner: NightAddress,
    type_: RawTokenType,
}

struct UtxoSpend {
    value: u128,
    owner: VerifyingKey,
    type_: RawTokenType,
    intent_hash: IntentHash,
    output_no: u32,
}

impl From<UtxoSpend> for Utxo {
    fn from(UtxoSpend { value, owner, type_, intent_hash, output_no }: UtxoSpend) -> Utxo {
        Utxo {
            value,
            owner: hash(owner),
            type_,
            intent_hash,
            output_no,
        }
    }
}
```

The state associated with the UTXO subsystem is a set of UTXOs, associated with
metadata. This is represented as a map from the UTXO, to its metadata.
Presently, the metadata solely consists of a creating time timestamp of the
UTXO.

```rust
struct UtxoMeta {
    ctime: Timestamp,
}

struct UtxoState {
    utxos: Map<Utxo, UtxoMeta>,
}
```

Building this into a transaction will be throughout [intent
system](./intents-transactions.md), where the component here is an *unbalanced
and unshielded offer*. The word offer here implies a collection of UTXO inputs
and outputs, that is *not* necessarily balanced by itself. This collection must
have a set of signatures, which each sign the containing
[`Intent`](./intents-transactions.md) object. Taken at face value, this would
be self-referential, as the `Intent` contains the signatures. To avoid this, we
sign a *variant* of the `Intent` *without* signatures. In practice, this is
achieved with a type parameter `S`, which may be instantiated either
with the unit type `()`, or the type `Signature`. The full type for `Intent` is
`Intent<Signature, Proof, FiatShamirPedersen>`, and it signs `ErasedIntent =
Intent<(), (), Pedersen>`. This also erases zero-knowledge proofs, and some of
the Pedersen commitment. This process is called *signature-*, *proof-* or
*binding-erasure*.

Due to the technicalities of [dust generation](./dust.md), a variant of
`UtxoOutput`, `GeneratingUtxoOutput` also exists, which can appear in the place
of outputs here, once Dust is enabled.

```rust
struct UnshieldedOffer<S> {
    inputs: Vec<UtxoSpend>,
    // This will soon become the following with the introduction of Dust
    // tokenomics:
    // outputs: Vec<Either<UtxoOutput, GeneratingUtxoOutput>>,
    outputs: Vec<UtxoOutput>,
    // Note that for S = (), this has a fixed point of ().
    // This signs the intent, and the segment ID
    signatures: Vec<S::Signature<(u16, ErasedIntent>>,
}
```

A canonical ordering is imposed on the inputs, and outputs sets. The signatures
must be the same length as inputs, with each signature authorizing the
corresponding input. It signs the parent intent data, excluding signatures or
ZK-proofs, and must be valid wrt. the respective input's verifying key.

```rust
impl<S> UnshieldedOffer<S> {
    fn well_formed(self, segment_id: u16, parent: ErasedIntent) -> Result<()> {
        assert!(self.inputs.is_sorted());
        assert!(self.outputs.is_sorted());
        assert!(self.inputs.len() == self.signatures.len());
        assert!(self.inputs.no_duplicates());
        for (inp, sig) in self.inputs.iter().zip(self.signatures.iter()) {
            signature_verify((segment_id, parent), inp.owner, sig)?;
        }
    }

    fn balance(self) -> Result<Map<RawTokenType, i128>> {
        let mut map = Map::empty();
        for inp in self.inputs {
            let entry = map.get_mut_or_default(inp.type_);
            *entry = *entry.checked_add(inp.value)?;
        }
        for out in self.outputs {
            let entry = map.get_mut_or_default(out.type_);
            *entry = *entry.checked_sub(out.value)?;
        }
        Ok(map)
    }
}
```

The effect of an offer is the removal of each of the spends from the
`UtxoState` (which must be unique and present), and the insertion of the
outputs into the `UtxoState` (which must be unique and *not* present). Note
that transactions are fully defined in [the relevant
section](./intents-transactions.md), and that offers must be balanced to be
applied.

```rust
impl UtxoState {
    fn apply_offer<S>(
        mut self,
        offer: UnshieldedOffer<S>,
        segment: u16,
        parent: ErasedIntent,
        tnow: Timestamp,
    ) -> Result<UtxoState> {
        let inputs = offer.inputs.iter().map(Utxo::from).collect();
        assert!(self.utxos.hasSubset(inputs));
        for input in inputs {
            self.utxos = self.utxos.remove(input);
        }
        let intent_hash = hash((segment, parent));
        let outputs = offer.outputs.iter().enumerate().map(|(output_no, output)| Utxo {
            value: output.value,
            owner: output.owner,
            type_: output.type_,
            intent_hash,
            output_no,
        }).collect();
        // The below is *not* needed, due to the uniqueness of outputs.
        // assert!(self.utxos.intersection(outputs).is_empty());
        for output in outputs {
            self.utxos.insert(output, UtxoMeta {
                ctime: tnow,
            });
        }
        Ok(self)
    }
}
```

See [the replay protection
section](./intents-transactions.md#replay-protection) for a justification on
the uniqueness of outputs, as the semantics presented here are not sufficient
to guarantee this.
