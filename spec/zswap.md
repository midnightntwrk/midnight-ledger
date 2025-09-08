# Zswap Specification draft

The intention of this draft is to sketch the workings of Zswap offers, their
effects, their proofs, and their validation.

Zswap is a zerocash-like token that permits atomic swaps. It is one of
Midnight's native token types, with tokens being issuable as shielded (Zswap)
tokens, or unshielded UTXO tokens. Shielded and unshielded tokens are not
usually interchangeable.

## Preliminaries

Zswap heavily relies on hashes and commitments for security. In both cases, the
SHA-256 hash function is the primarily used one. Zswap is based on
zero-knowledge proofs, which affords signature-like behaviour by proving the
execution of a one-way function. For this reason, we use random 256-bit keys
as secret keys, and their SHA-256 hash as public keys in Zswap.

Zswap also requires a public-key encryption mechanism to send secrets from the
sender of a token to its receiver. For this, we use a non-interactive
Diffie-Hellman over the proof system's embedded curve to establish an ephemeral
secret key, which is used with Poseidon as a CTR cipher to encrypt the sent
data.

```rust
type ZswapCoinSecretKey = [u8; 32];
type ZswapCoinPublicKey = Hash<ZswapCoinSecretKey>;

type ZswapEncryptionSecretKey = Fr;
type ZswapEncryptionPublicKey = embedded::CurvePoint;
```

Zswap is UTXO-like, in that the state conceptually consists of a set of unspent
coins. It differs meaningfully from standard UTXO systems however in that the
set of unspent coins can never be determined directly. This is achieved by
maintaining two sets, called the *commitment* and *nullifier* sets, which each
contain different projections of the same coins. Conceptually, the set of
unspent coins is the set of coins in the commitment set, but not the nullifier
set, however this difference isn't computable.

```rust
struct CoinInfo {
    value: u128,
    type_: RawTokenType,
    nonce: [0u8; 32],
}
```

```rust
type CoinCommitment = Hash<(CoinInfo, ZswapCoinPublicKey)>;
type CoinNullifier = Hash<(CoinInfo, ZswapCoinSecretKey)>;
```

In the ledger's state, the set of these commitments and nullifiers are stored.
While this is the end of the story for nullifiers, commitments are stored in
different representations:
- A plain set, for preventing the creation of duplicate coins
- A Merkle tree, for proving inclusion in the set of coins
- A history of roots of the above Merkle tree, for validating old proofs.

Additionally, an index into the Merkle tree is kept for book-keeping.

```rust
struct ZswapState {
    commitment_tree: MerkleTree<CoinCommitment>,
    commitment_tree_first_free: u32,
    commitment_set: Set<CoinCommitment>,
    nullifiers: Set<CoinNullifier>,
    commitment_tree_history: TimeFilterMap<MerkleTreeRoot>,
}
```

## Zswap inputs and outputs

The primary components of Zswap transactions are *inputs* and *outputs*, as
with other UTXO schemes. Their basic structure is as follows:

```rust
struct ZswapInput<P> {
    merkle_tree_root: MerkleTreeRoot,
    nullifier: CoinNullifier,
    contract: Option<ContractAddress>,
    value_commitment: embedded::CurvePoint,
    proof: P::Proof,
}

struct ZswapOutput<P> {
    commitment: CoinCommitment,
    contract: Option<ContractAddress>,
    value_commitment: embedded::CurvePoint,
    ciphertext: Option<Ciphertext>,
    proof: P::Proof,
}
```

In each case, the proof contains the relevant `CoinInfo`, alongside some other
information, that ensure that the input or output are authorized (for
user-owned funds, the prover knows the secret keys controlling them), and
correct (the commitments, nullifiers, and value commitments are correctly
computed, and the declaration of owning contract matches that of the coin).
The `value_commitment` field of both is a homomorphic Pedersen commitment, that
can be used to ensure a collection of multiple inputs and outputs are valid by
combining them together. This commitment has a corresponding *randomness*, that
must be revealed to validate it, however can be combined before validation.
This gives a binding property to Zswap commitments, that are exploited in
Midnight's transaction structure.

---

The zero-knowledge proofs for inputs and outputs are presented here in
rust-like pseudocode, with the inputs necessary for them. These are separate
largely for modularity: It should be possible to turn a `ZswapOutput` into a
`ZswapTransient`, even if it has already been proven, to allow extending a
transaction with operations that spend it.

```rust
fn input_valid<()>(
    input: Public<ZswapInput<()>>,
    segment: Public<u16>,
    sk: Private<Either<ZswapCoinSecretKey, ContractAddress>>,
    merkle_tree: Private<MerkleTree<CoinCommitment>>,
    coin: Private<CoinInfo>,
    rc: Private<embedded::Scalar>,
) -> bool {
    assert!(input.merkle_tree_root == merkle_tree.root());
    assert!(merkle_tree.contains(coin.commitment(sk.public_key())));
    assert!(input.nullifier == coin.nullifier(sk));
    assert!(input.contract == match sk {
        Left(_) => None,
        Right(contract) => Some(contract),
    });
    let value_commitment = hash_to_curve(coin.type_, segment) * coin.value + curve::embedded::GENERATOR * rc;
    assert!(input.value_commitment == value_commitment);
}

fn output_valid(
    output: Public<ZswapOutput<()>>,
    segment: Public<u16>,
    pk: Private<Either<ZswapCoinPublicKey, ContractAddress>>,
    coin: Private<CoinInfo>,
    rc: Private<embedded::Scalar>,
) -> bool {
    assert!(output.commitment = coin.commitment(pk));
    assert!(output.contract == match pk {
        Left(_) => None,
        Right(contract) => Some(contract),
    });
    let value_commitment = hash_to_curve(coin.type_, segment) * coin.value + curve::embedded::GENERATOR * rc;
    assert!(output.value_commitment == value_commitment);
}
```

For the most part, these are consistency checks, ensuring that the declared
commitments, nullifiers, and contracts match ones computed from the actual coin
and keys, and that that the homomorphic Pedersen commitment is correctly
computed. The value `rc` is the Pedersen commitment's randomness, and is used
later in the aggregate `Offer` structure to combine multiple inputs and
outputs.

Crucially, `segment`, as well as `coin.type_`, are preimages to the multi-base
part of this commitment, ensuring that a change to either is not homomorphic,
and effectively unmixable with each other (that is, a coin in Segment 1 will
not mix with one in Segment 2, nor with one of a different type). This not only
ensures that coins of
different types cannot be exchanged with each other, but that a transaction can
be divided into independent segments, that are each balanced independently.

Not explicitly included here is that each circuit also accepts an arbitrary
input that is *bound* to, that is, that the proof will verify only if this
input matches exactly. This is used to bind to the ciphertext for
`ZswapOutput`, but is currently unused for `ZswapInput`.

Explicitly, proof verification is performed as:

```rust
impl<P> ZswapInput<P> {
    fn well_formed(self, segment: u16) -> Result<()> {
        assert!(zk_verify(input_valid, (self, segment), None, self.proof));
    }
}

impl<P> ZswapOutput<P> {
    fn well_formed(self, segment: u16) -> Result<()> {
        assert!(zk_verify(output_valid, (self, segment), Some(ciphertext), self.proof));
        // Can't have ciphertexts for contracts.
        assert!(self.contract.is_none() || self.ciphertext.is_none());
    }
}
```

## Effects of inputs and outputs

Inputs and outputs can be applied against the current state. This application
can either succeed or be rejected as invalid. Conceptually, for inputs the
application checks that the Merkle tree root is valid, and that the nullifier
is not already present, and adds the nullifier to the set. For outputs, it
checks that the commitment is not already present, and adds the commitment to
the set.

Formally:

```rust
impl ZswapState {
    fn apply_input<P>(mut self, inp: ZswapInput<P>) -> Result<Self> {
        assert!(self.commitment_tree_history.contains(inp.merkle_tree_root));
        assert!(!self.nullifiers.contains(inp.nullifier));
        self.nullifiers = self.nullifiers.insert(inp.nullifier);
        self
    }

    fn apply_output<P>(mut self, out: ZswapOutput<P>) -> Result<(Self, CoinCommitment, u64)> {
        assert!(!self.commitment_set.contains(out.commitment));
        self.commitment_set = self.commitment_set.insert(out.commitment);
        self.commitment_tree = self.commitment_tree.insert(self.commitment_tree_first_free, out.commitment);
        self.commitment_tree_first_free = self.commitment_tree_first_free + 1;
        (self, out.commitment, self.commitment_tree_first_free - 1)
    }
}

```

Note that the `commitment_tree_history` does not get updated here. This should
be updated separately during block processing, inserting a new entry at the end
of processing a block, and cleaning up entries outside of a TTL parameter.

```rust
impl ZswapState {
    fn post_block_update(mut self, tblock: Timestamp) -> Self {
        self.commitment_tree_history = self.commitment_tree_history.insert(tblock, self.commitment_tree.root()).filter(tblock - global_ttl);
        self
    }
}
```

## Transients

In some cases, it is desirable for a `ZswapOutput` to be spent within the same
transaction. As the commitment tree is only updated after the block is
processed, and the insertion point is dependent on interleaved transactions,
this isn't directly possible. Instead, a `ZswapTransient` part is introduced
that explicitly matches an input and output in the same transaction, where the
input spends the output. Conceptually, this is simply a concatenation of the
two, where the `ZswapInput` spends from an ephemeral Merkle tree containing
just the commitment of the `ZswapOutput`. In practice, a few of the fields are
duplicates, although some apparent duplicates (such as `value_commitment` and
`proof`) are not duplicate in practice, as they still have a necessary
function.

```rust
struct ZswapTransient<P> {
    nullifier: CoinNullifier,
    commitment: CoinCommitment,
    contract: Option<ContractAddress>,
    value_commitment_input: embedded::CurvePoint,
    value_commitment_output: embedded::CurvePoint,
    proof_input: P::Proof,
    proof_output: P::Proof,
}

impl<P> ZswapTransient<P> {
    fn as_input(self) -> ZswapInput<P> {
        ZswapInput {
            merkle_tree_root: MerkleTree::new().insert(0, self.commitment).root(),
            nullifier: self.nullifier,
            contract: self.contract,
            value_commitment: self.value_commitment_input,
            proof: self.proof_input
        }
    }
    fn as_output(self) -> ZswapOutput<P> {
        ZswapOutput {
            commitment: self.commitment,
            contract: self.contract,
            value_commitment: self.value_commitment_output,
            ciphertext: None,
            proof: self.proof_output
        }
    }
    fn well_formed(self, segment: u16) -> Result<()> {
        self.as_input().verify()?;
        self.as_output().verify()?;
    }
}

impl ZswapState {
    fn apply_transient<P>(mut self, trans: ZswapTransient<P>) -> Result<(Self, CoinCommitment, u64)> {
        assert!(!self.commitment_set.contains(trans.commitment));
        assert!(!self.nullifiers.contains(trans.nullifier));
        self.commitment_set = self.commitment_set.insert(trans.commitment);
        self.commitment_tree = self.commitment_tree.insert(self.commitment_tree_first_free, trans.commitment);
        self.commitment_tree_first_free = self.commitment_tree_first_free + 1;
        self.nullifiers = self.nullifiers.insert(trans.nullifier);
        (self, trans.commitment, self.commitment_tree_first_free - 1)
    }
}
```

## Offers

The top-level structure of Zswap is the *offer*. Offers consist of a set of
inputs, a set of outputs, a set of transients, and a declaration of the
imbalance of the offer, in the shape of a mapping from token types to *signed*
integers.

```rust
struct ZswapOffer<P> {
    inputs: Set<ZswapInput<P>>,
    outputs: Set<ZswapOutput<P>>,
    transients: Set<ZswapTransient<P>>,
    deltas: Map<RawTokenType, i128>,
}

impl<P> ZswapOffer<P> {
    fn well_formed(self, segment: u16) -> Result<()> {
        inputs.all(|inp| inp.well_formed(segment))?;
        outputs.all(|out| out.well_formed(segment))?;
        transients.all(|trans| trans.well_formed(segment))?;
    }
}

impl ZswapState {
    fn apply<P>(mut self, offer: ZswapOffer<P>) -> Result<(Self, Map<CoinCommitment, u64>)> {
        let mut com_indicies = Map::new();
        self = offer.inputs.fold(self, ZswapState::apply_input)?;
        (self, com_indicies) = offer.outputs.fold((self, com_indicies),
            |(state, indicies), output| {
                (state, com, index) = state.apply_output(output)?;
                (state, indicies.insert(com, index))
            })?;
        (self, com_indicies) = offer.transients.fold((self, com_indicies),
            |(state, indicies), trans| {
                (state, com, index) = state.apply_transient(trans)?;
                (state, indicies.insert(com, index))
            })?;
        (self, com_indicies)
    }
}
```

The effect of an offer is trivially the effect of its constituent parts.
Meanwhile, it is validated against a `segment: u16`, by validating all of the
constituent proofs. This is not the entire story however, as in a transaction
the offer must also be *balanced*. This means that both the values in `deltas`
are all non-negative, and that the constituent homomorphic commitments must be
openable to the zero-commitment.

In practice, each offer has a homomorphically computable *overall commitment*,
computed as:

```rust
impl ZswapOffer {
    fn value_commitment(self, segment: u16) -> curve::embedded::Affine {
        self.inputs.map(|i| i.value_commitment).sum() +
            self.transients.map(|t| t.value_commitment_input).sum() -
            self.outputs.map(|o| o.value_commitment).sum() -
            self.transients.map(|t| t.value_commitment_output).sum() -
            deltas.map(|(ty, val)| hash_to_curve(ty, segment) * val).sum()

    }
}
```

This value commitment must then be demonstrated to be equal to
`curve::embedded::GENERATOR * rc_all`, where `rc_all` is the sum of all
`ZswapInput` `rc` values, minus the sum of all `ZswapOutput` `rc` values.

---

`ZswapOffer`s may be *merged* if they are disjoint, by taking the set unions,
and computing the sum of the deltas. Note that any entry with value `0` in
deltas should be omitted, as this does not affect the binding calculations.

```rust
impl<P> ZswapOffer<P> {
    fn merge(self, other: ZswapOffer<P>) -> Result<Self> {
        assert!(self.inputs.disjoint(other.inputs));
        assert!(self.outputs.disjoint(other.outputs));
        assert!(self.transients.disjoint(other.transients));
        ZswapOffer {
            inputs: self.inputs + other.inputs,
            outputs: self.outputs + other.outputs,
            transients: self.transients + other.transients,
            deltas: self.deltas + other.deltas,
        }
    }
}
```
