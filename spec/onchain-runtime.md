# On-chain Runtime

This document describes the onchain program format, as represented in JavaScript,
on-the-wire binary, and in prime fields for proof verification. It further
describes the data structures stored in onchain, and how they may be represented,
and argues the primary theorem, stated in the title of the document.

## Data types

This document will make use of the [Field-Aligned
Binary](field-aligned-binary.md) format, and data types represented in it.  This
document defines the `Program` and `StateValue` data formats, and defines
execution of `Program`s on `StateValue`s.

### Values

The `StateValue` data type is defined as a disjoint union of the following types:
* `Null`: An empty value.
* `Cell`: memory cell containing a single FAB `AlignedValue`
* `Map`: key-value map from FAB `AlignedValue`s to state values.
* `Array(n)` for `n <= 16`: fixed-length array of state values
* `BoundedMerkleTree(n)` for `0 < n <= 32`: depth-`n` Merkle tree of leaf hash values.

Note: we will want to add in a future version:
* `SortedMerkleTree`: an ordered Merkle tree of arbitrary depth of FAB values.

Note that state values appear only in positions where they are *readable*, and where they are not used for indexing.

#### Merklization

A state value may be Merklized (as a separate, base-16 Merkle-Patricia trie, *not* as a binary Merkle tree) as a node whose first child is a tag identifying the type, and whose remaining are:
* `Null`: blank
* `Cell`: A single leaf.
* `Map`: Trees of key-value pairs `(k, v)`, where the path is `0x[H*(k)]`, and the value is stored in its Merklized form at the node, for `H*` being `persistent_hash`, but with the following modification: If the first nibble of the result is zero, it will be replaced with the first non-zero nibble occurring in the result (e.g. `0x00050a...` becomes `0x50050a...`).
* `Array(n)`: As the entries of the array.
* `BoundedMerkleTree(n)` as itself

#### On-the-wire representation

The on-the-wire representations make use of [FAB](field-aligned-binary.md)
representations. We represent both *state value*, and *programs*.

##### State value representation

###### As field elements

The first field element `f` distinguishes the type of the state value, with the
remainder being specific on the type.

* `f = 0` encodes a `Null`, with no additional data.
* `f = 1` encodes a `Cell`, with the following field elements encoding a FAB
  `AlignedValue` stored within it (including the alignment encoding!).
* `f = 2 | (n << 4)`, for `n: u64` encodes a `Map` of length `n`. It is followed
  by, in stored order by encoded key-value pairs, consisting of FAB `AlignedValue` keys, and
  `StateValue` values.
* `f = 3 | (n << 4)`, for integers `n < 16` encodes a `Array(n)`. It is followed by `n` `StateValue` encodings.
* `f = 4 | (n << 4) | (m << 12)`, for integers `0 <= n < 256` encodes a
  `BoundedMerkleTree(n)`. It is followed by `m` key-value pairs, with keys
  encoded directly as field elements, and values encoded as `bytes(32)`-aligned
  hashes.

##### Program representations

A program is encoded by encoding its sequence of instructions in order, with
each instruction starting with an opcode, optionally followed by some arguments
to this instruction.

To define program representations, we first define a common argument type:
`path(n)`, an array with `n` path entries, each being either a FAB `AlignedValue`, or
the symbol `stack`.

###### As Field Elements

A program is encoded similarly to its binary form as fields. Opcodes are encoded
as a single field element, integers as single field elements, and `Adt`s as above.

An exception is `noop n`, which is encoded as `n` field elements.

A `path(n)` is as `-1` if it is a `stack` symbol, otherwise by encoding the `AlignedValue` directly.
Note that as an `AlignedValue` starts with its length, these are guaranteed not to collide.

### Programs

A `Program` is a sequence of `Op`s. Each `Op`
consists of an opcode, potentially followed by a number of arguments depending
on the specific opcode. For read operations, the operation may return a result
of some length. `Program`s can be run in two modes: evaluating and
verifying. In verifying mode, `popeq[c]` arguments are enforced for equality,
while in evaluating move, the results here are gathered instead.

`Programs` run on a stack machine with a stack of
`StateValue`s, guaranteed to have exactly one item on the stack to start. Each
`Op` has a fixed effect on the stack, which will be written as `-{a, b} +{c,
d}`: consuming items `a` and `b` being at the top of the stack (with `a` above
`b`), and replacing them with `c` and `d` (with `d` above `c`). The number of
values here is just an example. State values are _immutable_ from the perspective
of programs: A value on the stack cannot be changed, but it can be
replaced with a modified version of the same value. We write `[a]` to refer to
the FAB value stored in the `Cell` `a`. Due to the ubiquity of it, we write
"sets `[a] := ...`" for "create `a` as a new `Cell` containing `...`". We
prefix an output value with a `'` to indicate this is a *weak* value, kept
solely in-memory, and not written to disk, and an input value with `'` to
indicate it *may* be a weak value. We use `"` and `†` to indicate that an input
*may* be a weak value, and *iff* it is, the correspondingly marked output will
be a weak value.

Cells are not guaranteed to be fully loaded, if they exceed one database entry.
The first entry is always loaded, which contains the cell's length, and the
rest *can* only be necessary on a `popeq` or `concat` instruction, which
require specifying if the data is expected to reside in-cache or not.

The full opcode reference — including the **binary encoding**, **per-opcode
semantics and stack effects**, **error conditions**, **gas / cost model**, and
**short-hand notation** (`a op b`, `typeof`, `size`, `has_key`, `new`, `get`,
`rem`, `ins`, `root`) used by it — is in
[**impact-opcodes.md**](./impact-opcodes.md).

## Use in Midnight

### Kernel Operation, Context and Effects

Kernel operations affect things, and retrieve knowledge from outside of the
contract's state. We model this by running a program not just on the
contract's current state, but on an initial stack of `{state, effects,
context}`. When the program finishes executing, it should leave a stack of
`{state', effects', _}`. `state'` is used to replace the contract's state, and
`effects'` must adhere to the structure given here, specifying the effects of
the operation.

The `context` is an `Array(_)`, encoding the [`CallContext`](./contracts.md#context).
This may be extended in the future in a minor version increment.

The `effects` is an `Array(_)`, encoding the [contract `Effects`](./contracts.md#effects).
This may be extended in the future in a minor version increment.

All of `context` and `effects` may be considered cached. To prevent cheaply
copying data into contract state with as little as two opcodes, both are
flagged as *tainted*, and any operations performed with them, that are not
size-bounded (such as `add`) will return a tainted value. If the final `state'`
is tainted, the transaction fails.
