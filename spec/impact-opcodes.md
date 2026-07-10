# Impact VM Opcode Reference

This document is the per-opcode reference for the Impact VM — the on-chain
stack machine that applies a contract's state transition.  The conceptual
model (programs, `StateValue` types, the stack, kernel operations, context,
effects) is specified in [onchain-runtime.md](./onchain-runtime.md); this
document specifies, for each opcode: its **binary encoding**, **execution
semantics**, **error conditions**, and **gas/cost model**.

The implementation of the VM lives in the `onchain-vm` crate
(`onchain-vm/src/{ops.rs,vm.rs,cost_model.rs}`); on the stack each item is a
`VmValue` (`onchain-vm/src/vm_value.rs`), which wraps a `StateValue`
(`onchain-state/src/state.rs`) with a *strength* tag (§1). The `RunningCost`
accumulator is in `base-crypto/src/cost_model.rs`.

The Impact VM is a different layer from [ZKIR](./zkir.md) and the two should
not be confused:

| | **Impact VM** (this doc) | **ZKIR** |
|---|---|---|
| Crate | `onchain-vm` | `zkir` / `zkir-v3` |
| Runs | publicly, on every node | privately, in the prover |
| Operates on | ledger `StateValue`s (a stack) | native field elements (a memory tape) |
| Purpose | the public state transition | the zero-knowledge proof |
| Metered as | DUST gas (this cost model) | proof generation cost |

**Wire-format tags** for the serialised forms (see
[transaction-format.md §7.1](./transaction-format.md#71-container-format-the-serialize-crate)
for the tagging convention):

| Tag | Type | Source |
|---|---|---|
| `impact-op[v1]` | the `Op` enum | `onchain-vm/src/ops.rs` |
| `impact-cost-model[v4]` | `CostModel` | `onchain-vm/src/cost_model.rs` |
| `impact-idx-key` | the `idx` / `ins` path-key element | `onchain-vm/src/ops.rs` |
| `impact-log-event-type[v1]` | the `log` opcode event-type tag | `onchain-vm/src/ops.rs` |
| `impact-versioned-log-item[v1]` | a `log` payload item | `onchain-vm/src/ops.rs` |

## 1. Stack-effect notation

All entries below use the notation defined in
[onchain-runtime.md §Programs](./onchain-runtime.md#programs):

* `-{a, b} +{c, d}` — consumes `a` (top) and `b` (below it) and pushes `c` then
  `d` (`d` ends up on top). **`a` is the top of the stack.**
* `[a]` — the FAB `AlignedValue` inside `Cell` `a`. "Sets `[c] := …`" means
  "push a new `Cell` `c` containing …".
* `'a` (prefix on an output) — a **weak** value; `'a` on an input — *may* be
  weak. `"a` … `†b` — paired markers: the marked output is weak **iff** the
  marked input was weak.
* `x*` — `n` arbitrary stack items spanned by the opcode's immediate.

A program may run in one of two **result modes**
(`onchain-vm/src/result_mode.rs`):

* **Verifying** (`ResultModeVerify`) — `popeq`/`popeqc` carry an expected
  `result: AlignedValue` that is *enforced* for equality (a mismatch fails the
  program with `ReadMismatch`). This is the mode used to validate a submitted
  transaction.
* **Gathering** (`ResultModeGather`) — `popeq` results are *collected* rather
  than checked. Used to build a transcript.

The `result` field is always structurally present in the `Op::Popeq` AST; its
type is `M::ReadResult` — `AlignedValue` in verifying mode, `()` in gathering
mode.

The gathered/checked `popeq` values form the call's **public transcript** —
the bridge to the contract's ZK proof (see §8).

## 2. Caching and the `*c` variants

Reads from `Map`/`BoundedMerkleTree`/`Array` normally incur an I/O read cost.
The **cached** opcode variants (`idxc`, `idxpc`, `remc`, `insc`) assert the
relevant node is already in the in-memory cache: they **skip the read cost**,
but raise `CacheMiss` if the data is not present (except `insc` overwriting an
`Array` cell, where the miss is permissible).  `popeqc` and `concatc` are
accepted for compatibility but the `cached` flag has **no effect** on them in
the current implementation.

## 3. Limits

| Limit | Value | Source |
|---|---|---|
| Max stack height | `2^16` (`MAX_STACK_HEIGHT`) | `vm.rs` → `StackOverflow` |
| Max `log` payload | `2^19` bytes (`MAX_LOG_SIZE`) | `vm.rs` → `LogBoundExceeded` |
| Array length | `≤ 16` | `new`/`StateValue` |
| Merkle-tree depth | `≤ 32` | `StateValue` |
| `concat` result | `≤ CELL_BOUND` | `vm.rs` → `CellBoundExceeded` |
| Cell `eq` operand | ≤ 64 bytes on at least one side | `eq` |

A **gas limit** (`RunningCost`) may be supplied; if a step pushes cumulative
`compute_time` or `read_time` over the limit, the program fails with
`OutOfGas`.

## 4. Binary encoding

Each opcode is a single byte (encoded as one field element in the
field-element form used for proofs; see
[onchain-runtime.md §Program representations](./onchain-runtime.md#program-representations)).
Four opcode families pack a small integer into the low nibble:

| Encoding | Family | Low nibble |
|---|---|---|
| `0x3n` | `dup n` | `n` = stack depth of the item to copy (`0`=top), `0..15` |
| `0x4n` | `swap n` | `n` = items between the two swapped, `0..15` |
| `0x5n` / `0x6n` / `0x7n` / `0x8n` | `idx` / `idxc` / `idxp` / `idxpc` | `n` = **path length − 1** (`0..15` ⇒ 1–16 keys) |
| `0x9n` / `0xan` | `ins` / `insc` | `n` = number of insert levels, `1..15` |

`noop n` is special: it encodes as `n` repeated `0x00` field elements.
`addi` / `subi` / `push` / `pushs` / `popeq` / `concat` / `idx*` are followed
by their argument(s).

Full single-byte map (from `Op::field_repr`, `ops.rs`):

| Byte | Op | Byte | Op | Byte | Op |
|---|---|---|---|---|---|
| `00` | `noop` | `0c` | `popeq` | `16` | `concat` |
| `01` | `lt` | `0d` | `popeqc` | `17` | `concatc` |
| `02` | `eq` | `0e` | `addi` | `18` | `member` |
| `03` | `type` | `0f` | `subi` | `19` | `rem` |
| `04` | `size` | `10` | `push` | `1a` | `remc` |
| `05` | `new` | `11` | `pushs` | `3n` | `dup` |
| `06` | `and` | `12` | `branch` | `4n` | `swap` |
| `07` | `or` | `13` | `jmp` | `5n` / `6n` / `7n` / `8n` | `idx` / `idxc` / `idxp` / `idxpc` |
| `08` | `neg` | `14` | `add` | `9n` / `an` | `ins` / `insc` |
| `09` | `log` | `15` | `sub` | `ff` | `ckpt` |
| `0a` | `root` | | | | |
| `0b` | `pop` | | | | |

## 5. Opcode reference

Operand / stack columns use the §1 notation.  "Gas" names the relevant
[`CostModel`](#6-gas-and-cost-model) fields; reads marked *(+read)* additionally
charge an I/O read cost on the uncached variant.

### Stack manipulation and constants

#### `push` (`10`) / `pushs` (`11`)
* **Stack:** `-{} +{'a}` (`push`) / `-{} +{a}` (`pushs`). **Arg:** `a: StateValue`.
* **Semantics:** push the literal value `a`. `push` makes it **weak**, `pushs`
  **strong** (persisted). **Gas:** `push_<type>` / `pushs_<type>`.

#### `pop` (`0b`)
* **Stack:** `-{'a} +{}`. **Semantics:** discard the top value. **Gas:** `pop`.

#### `dup n` (`3n`)
* **Stack:** `-{x*, "a} +{"a, x*, "a}`. **Semantics:** copy the item at depth
  `n` (top = 0) and push it. **Gas:** `dup_constant + dup_coeff_arg·n`.

#### `swap n` (`4n`)
* **Stack:** `-{"a, x*, †b} +{†b, x*, "a}`. **Semantics:** swap the top with
  the item `n+1` below it. **Gas:** `swap_constant + swap_coeff_arg·n`.

### Integer arithmetic

64-bit unsigned, alignment `b8`; overflow / underflow → `ArithmeticOverflow`.

#### `add` (`14`) / `sub` (`15`)
* **Stack:** `-{'a, 'b} +{c}`. **Semantics:** `add` ⇒ `[c] := [a] + [b]`;
  `sub` ⇒ `[c] := [b] − [a]` (i.e. *second-from-top minus top*).
  **Gas:** `add` / `sub`.

#### `addi c` (`0e`) / `subi c` (`0f`)
* **Stack:** `-{'a} +{b}`. **Arg:** `c: u32` immediate.
* **Semantics:** `[b] := [a] + c` / `[a] − c`. **Gas:** `addi` / `subi`.

### Comparison and boolean logic

#### `lt` (`01`)
* **Stack:** `-{'a, 'b} +{c}`.
* **Semantics:** `[c] := [b] < [a]` — the **second-from-top** value compared
  against the **top**.  Both operands are 64-bit unsigned `Cell`s, alignment
  `b8`.  **Gas:** `lt`.

#### `eq` (`02`)
* **Stack:** `-{'a, 'b} +{c}`. **Semantics:** `[c] := [a] == [b]`. At least one
  side must contain ≤ 64 bytes (else `TooLongForEqual`). **Gas:** `eq`.

#### `and` (`06`) / `or` (`07`) / `neg` (`08`)
* **Stack:** `-{'a, 'b} +{c}` (binary) / `-{'a} +{b}` (`neg`).
* **Semantics:** boolean `&` / `|` / `!` over operands that must each be `0`
  or `1`. **Gas:** `and` / `or` / `neg`.

### Type, size, and construction

#### `type` (`03`)
* **Stack:** `-{'a} +{b}`. **Semantics:** `[b] := typeof(a)` — `Cell`=0,
  `Null`=1, `Map`=2, `Array(n)`=`3+n·8`, `BoundedMerkleTree(n)`=`4+(n−1)·8`.
  **Gas:** `type_<kind>`.

#### `size` (`04`)
* **Stack:** `-{'a} +{b}`. **Semantics:** `[b] := size(a)` — non-null entries
  of a `Map`, `n` for `Array(n)`, or height for a `BoundedMerkleTree`.
  `TypeError` on other types. **Gas:** `size_map` / `size_bmt` / `size_array`.

#### `new` (`05`)
* **Stack:** `-{'a} +{b}`. **Semantics:** create an empty value of the type
  tag `[a]` (lower 3 bits select the type; for `Array` / `BMT` the upper bits
  give size / height).  Array length > 16 or unknown tag → `InvalidArgs`.
  **Gas:** `new_<kind>`.

### Map / tree / array access

#### `member` (`18`)
* **Stack:** `-{'a, 'b} +{c}`. **Semantics:** `[c] := has_key(b, a)` — whether
  key `a` maps to a non-null value in `Map` `b`. `Map`-only.
* **Gas:**
  `member_constant + member_coeff_key_size·|key| + member_coeff_container_log_size·log₂(size)`
  *(+read)*.

#### `idx` / `idxc` / `idxp` / `idxpc` (`5n` / `6n` / `7n` / `8n`)
* **Stack:** `-{k*, "a} +{"b}` (`idx` / `idxc`) or
  `-{k*, "a} +{"b, pth*}` (`idxp` / `idxpc`).
* **Arg:** `path(n)` — a list of keys, each either a literal `AlignedValue` or
  the `stack` symbol (consumed from the stack as `k*`).
* **Semantics:** walk `a` following the path, indexing into `Map` / `Array` /
  `BMT` at each step, and push the final sub-value `b`. The `p` (push-path)
  variants additionally push each intermediate `(container, key)` pair — used
  to later `ins` an updated value back along the same path.  The `c` (cached)
  variants require each node already cached (`CacheMiss` otherwise) and skip
  read costs.
* **Gas:** `idx[p][c]_<kind>_{constant, coeff_key_size, coeff_container_log_size}`
  per step *(+read on uncached)*.

#### `ins` / `insc n` (`9n` / `an`)
* **Stack:** `-{"a, pth*} +{†b}` where `pth*` is the `{key, container}` pairs
  produced by a matching `idxp` / `idxpc`.
* **Semantics:** insert value `a` back down the `n`-level path, producing
  updated containers bottom-up; `b` is the new root container. Strength `†` =
  weakest of the inputs. `Map` insert, `Array` index-checked insert, `BMT`
  leaf update (index `< 2^height`). `insc` requires cached nodes.
* **Gas:** `ins[c]_<kind>_…` per level *(+read on uncached)*.

#### `rem` / `remc` (`19` / `1a`)
* **Stack:** `-{a, "b} +{"c}`. **Semantics:** `[c] := rem(b, a)` — remove key
  `a` from `Map` / `BMT` `b`.  `remc` requires the node cached.
* **Gas:** `rem[c]_<kind>_…` *(+read on uncached)*.

### Concatenation

#### `concat` / `concatc n` (`16` / `17`)
* **Stack:** `-{'a, 'b} +{c}`. **Arg:** `n: u32` bound.
* **Semantics:** `[c] := [b] ++ [a]` (second-from-top followed by top), provided
  `|[a]| + |[b]| ≤ n`; exceeding the cell bound → `CellBoundExceeded`.
  (`concatc`'s `cached` flag has no effect.)
* **Gas:** `concat_constant + concat_coeff_total_size·(|a|+|b|)`.

### Merkle trees

#### `root` (`0a`)
* **Stack:** `-{'a} +{b}`. **Semantics:** `[b] := root(a)` — the Merkle root
  of a `BoundedMerkleTree` (must be rehashed); `TypeError` otherwise.
  **Gas:** `root`.

### Output, logging, and reads

#### `popeq` / `popeqc` (`0c` / `0d`)
* **Stack:** `-{'a} +{}`. **Arg:** `result: M::ReadResult` — the expected
  value in verifying mode (`AlignedValue`); `()` in gathering mode.
* **Semantics:** pop a `Cell` and *return* its value as a transcript read. In
  verifying mode it must equal `result` (else `ReadMismatch`); in gathering
  mode the value is collected.  (`popeqc`'s `cached` flag has no effect.)
* **Gas:** `popeq[c]_constant + popeq[c]_coeff_value_size·|value|`.

#### `log` (`09`)
* **Stack:** `-{'a} +{}`. **Semantics:** emit `a` as an event (subject to
  `MAX_LOG_SIZE`; `LogBoundExceeded` otherwise). Charged as both
  `bytes_written` and `bytes_deleted` (full churn).
* **Gas:** `log_<kind>_constant + log_<kind>_coeff_value_size·size`.

### Control flow

#### `noop n` (`00`)
* **Stack:** `-{} +{}`. **Arg:** `n: u21`.
* **Gas:** `noop_constant + noop_coeff_arg·n`.

#### `branch n` (`12`) / `jmp n` (`13`)
* **Stack:** `-{'a} +{}` (`branch`) / `-{} +{}` (`jmp`).
* **Semantics:** `jmp n` unconditionally skips the next `n` opcodes;
  `branch n` pops the top value, and if it is **non-empty** (truthy) skips
  `n` opcodes — otherwise falls through.  Skipping past the end of the program
  raises `RanPastProgramEnd`.
* **Gas:** `branch_constant + branch_coeff_arg·n` /
  `jmp_constant + jmp_coeff_arg·n`.

#### `ckpt` (`ff`)
* **Stack:** `-{} +{}`. **Semantics:** no-op marker delimiting internally
  atomic program segments — **jumps must not cross it**.  This is how the
  guaranteed / fallible phase split (Compact's `kernel.checkpoint()`) is
  expressed in a transcript.
* **Gas:** `ckpt`.

A complete one-line index is in [Appendix A](#appendix-a--opcode-index).

## 6. Gas and cost model

### 6.1 Multi-dimensional cost

The Impact VM uses a **multi-dimensional** cost model
([cost-model.md](./cost-model.md)).  The VM accumulates a `RunningCost`
(`base-crypto/src/cost_model.rs`) with four tracked dimensions:

| Dimension | Unit | Meaning |
|---|---|---|
| `compute_time` | picoseconds (`CostDuration`) | single-threaded CPU time |
| `read_time` | picoseconds | I/O time to read required data |
| `bytes_written` | bytes | persistent storage added |
| `bytes_deleted` | bytes | "churn" / temporary storage freed |

(The broader model in `cost-model.md` also counts **consensus throughput** —
transaction size — which is accounted at the transaction level, not
per-opcode.)  These dimensions are what ultimately price a transaction in
**DUST**.

### 6.2 Pricing functions

`CostModel` (`onchain-vm/src/cost_model.rs`) holds the coefficients of
**affine-linear** per-op pricing functions.  An op's compute cost is:

```text
cost = <op>_constant
     + <op>_coeff_key_size           · serialized_size(key)
     + <op>_coeff_container_log_size · log2(container_size)
```

with constant-priced ops (`lt`, `eq`, `add`, …) using just a single `<op>`
field, and type-specialised ops carrying separate coefficients per container
/ value kind (`_map`, `_bmt`, `_array`, `_cell`, `_null`). Example, `rem` on a
`Map` key `k`:

```text
rem_map_constant + rem_map_coeff_key_size·|k| + rem_map_coeff_container_log_size·log2(size)
```

The concrete coefficient values are **learned by linear regression** against
VM micro-benchmarks (`onchain-runtime/benches/benchmarking.rs`); the pricing
*shape* is fixed by `run_program_internal`.

### 6.3 I/O read costs

Uncached reads add a `read_time` term computed from the data layout
(`read_cell`, `read_map`, `read_bmt`, `read_array`), in 4 KiB blocks,
distinguishing:

* **synchronous** random reads — `read_time_synchronous_4k` (≈ 85 µs / 4 KiB),
  and
* **batched / sequential** reads — `read_time_batched_4k` (≈ 2 µs / 4 KiB).

The `*c` (cached) opcode variants skip these read terms (see §2).

### 6.4 Enforcement

A caller may pass a `gas_limit: Option<RunningCost>`. After each op, if
cumulative `compute_time` **or** `read_time` exceeds the limit, the program
halts with `OutOfGas`. `bytes_written` / `bytes_deleted` are reported in the
result for transaction-level pricing.

## 7. Short-hand notation used in the opcode reference

Where not specified, result values are placed in a `Cell` and encoded as FAB
values.

* `a + b`, `a - b`, or `a < b` (collectively `a op b`), for applying `op` on
  the contents of `Cell`s `a` and `b`, interpreted as 64-bit unsigned integers,
  with alignment `b8`.
* `a ++ b` is the FAB `AlignedValue` of the concatenation of `a` and `b`.
* `a == b` for checking two `Cell`s for equality, at least one of which must
  contain at most 64 bytes of data (sum of all FAB atoms).
* `a & b`, `a | b`, `!a` are processed as boolean and, or, and not over the
  contents of `Cell`s `a` and maybe `b`. These must encode 1 or 0.
* `typeof(a)` returns a tag representing the type of a state value:
  * `Cell`: 0
  * `Null`: 1
  * `Map`: 2
  * `Array(n)`: 3 + n · 8
  * `BoundedMerkleTree(n+1)`: 4 + n · 8
* `size(a)` returns the number of non-null entries in a `Map`, `n` for
  an `Array(n)` or `BoundedMerkleTree(n)`.
* `has_key(a, b)` returns `true` if `b` is a key to a non-null value in the
  `Map` `a`.
* `new ty` creates a new instance of a state value according to the tag `ty`
  (as returned by `typeof`):
  * `Cell`: containing the empty value
  * `Null`: `null`
  * `Map`: the empty map
  * `Array(n)`: an array of `n` `Null`s
  * `BoundedMerkleTree(n)`: a blank Merkle tree
* `a.get(b, cached)` retrieves the sub-item indexed with `b`. If the
  sub-item is *not* loaded in memory, *and* `cached` is `true`, this command
  fails. For different `a`:
  * `a: Map`, the value stored at the key `b`
  * `a: Array(n)`, the value at the index `b` < n
* `rem(a, b, cached)` removes the sub-item indexed (as in `get`) with `b`
  from `a`. If the sub-item is *not* loaded in memory, *and* `cached` is
  `true`, this command fails.
* `ins(a, b, cached, c)` inserts `c` as a sub-item into `a` at index `b`. If
  the path for this index is *not* loaded in memory, *and* `cached` is
  `true`, this command fails.
* `root(a)` outputs the Merkle-tree root of the `BoundedMerkleTree(n)` or
  `SortedMerkleTree` `a`.

## 8. Relationship to ZKIR

A Compact contract circuit compiles to **two coordinated artifacts**:

1. an **Impact program** (these opcodes) — the public reads / writes to ledger
   state, executed by every node; and
2. a **ZKIR circuit** — the zero-knowledge proof of the private computation
   (see [zkir.md](./zkir.md)).

They are **not** a 1:1 opcode↔instruction mapping — they are different layers
— but they meet at two points:

* **The public transcript.** The values a contract call returns via
  `popeq` / `popeqc` form the call's public transcript. That same transcript
  is what the ZK proof is bound to: in ZKIR, `PublicInput` / `DeclarePubInput`
  consume / produce the public-transcript stream. So `popeq` (Impact) and
  `PublicInput` / `DeclarePubInput` (ZKIR) are the two ends of one channel.
* **Shared cryptographic primitives.** `CostModel` also prices proof / curve
  primitives — `proof_verify_*`, `verifier_key_load`, `pedersen_valid`,
  `ec_add`, `ec_mul`, `hash_to_curve`, `transient_hash` — that correspond
  directly to ZKIR instructions (`EcAdd`, `EcMul`, `HashToCurve`,
  `TransientHash`).  These are charged when a transaction's proofs are
  verified, alongside the per-opcode Impact costs.

At the transaction level, Impact programs run inside contract **calls** within
**intents**, and `ckpt` marks the boundary between the **guaranteed** and
**fallible** execution segments (see
[transaction-format.md](./transaction-format.md) and
[intents-transactions.md](./intents-transactions.md)).

## Appendix A — opcode index

The **Cost (unscaled)** column gives the legacy single-number cost
shorthand: `1` for constant-time ops; `|a|` for ops whose cost scales with
the size of an operand; `size(b)` for container ops; sums for path-walking
ops. See §6 for the full multi-dimensional cost model that supersedes this.

| Name | Opcode | Stack | Arguments | Cost (unscaled) | Description |
| :--- | -----: | :---- | --------: | --------------: | ----------- |
| `noop` | `00` | `-{} +{}` | `n: u21` | `n` | nothing |
| `lt` | `01` | `-{'a, 'b} +{c}` | — | `1` | sets `[c] := [b] < [a]` (second-from-top `<` top) |
| `eq` | `02` | `-{'a, 'b} +{c}` | — | `1` | sets `[c] := [a] == [b]` |
| `type` | `03` | `-{'a} +{b}` | — | `1` | sets `[b] := typeof(a)` |
| `size` | `04` | `-{'a} +{b}` | — | `1` | sets `[b] := size(a)` |
| `new` | `05` | `-{'a} +{b}` | — | `1` | sets `[b] := new [a]` |
| `and` | `06` | `-{'a, 'b} +{c}` | — | `1` | sets `[c] := [a] & [b]` |
| `or` | `07` | `-{'a, 'b} +{c}` | — | `1` | sets `[c] := [a] \| [b]` |
| `neg` | `08` | `-{'a} +{b}` | — | `1` | sets `[b] := ![a]` |
| `log` | `09` | `-{'a} +{}` | — | `1` | outputs `a` as an event |
| `root` | `0a` | `-{'a} +{b}` | — | `1` | sets `[b] := root(a)` |
| `pop` | `0b` | `-{'a} +{}` | — | `1` | removes `a` |
| `popeq` | `0c` | `-{'a} +{}` | `result: M::ReadResult` | `\|a\|` | returns `a` (transcript read); verifying mode checks `a == result` |
| `popeqc` | `0d` | `-{'a} +{}` | `result: M::ReadResult` | `\|a\|` | as `popeq`; the `cached` flag has no effect in the current implementation |
| `addi` | `0e` | `-{'a} +{b}` | `c: u32` | `1` | sets `[b] := [a] + c` |
| `subi` | `0f` | `-{'a} +{b}` | `c: u32` | `1` | sets `[b] := [a] − c` |
| `push` | `10` | `-{} +{'a}` | `a: StateValue` | `\|a\|` | push `a` as a weak value |
| `pushs` | `11` | `-{} +{a}` | `a: StateValue` | `\|a\|` | push `a` as a strong (persisted) value |
| `branch` | `12` | `-{'a} +{}` | `n: u21` | `1` | if `a` is non-empty (truthy), skip `n` operations |
| `jmp` | `13` | `-{} +{}` | `n: u21` | `1` | skip `n` operations |
| `add` | `14` | `-{'a, 'b} +{c}` | — | `1` | sets `[c] := [a] + [b]` |
| `sub` | `15` | `-{'a, 'b} +{c}` | — | `1` | sets `[c] := [b] − [a]` |
| `concat` | `16` | `-{'a, 'b} +{c}` | `n: u21` | `1` | sets `[c] := [b] ++ [a]`, if `\|[a]\| + \|[b]\| ≤ n` |
| `concatc` | `17` | `-{'a, 'b} +{c}` | `n: u21` | `1` | as `concat`; the `cached` flag has no effect in the current implementation |
| `member` | `18` | `-{'a, 'b} +{c}` | — | `size(b)` | sets `[c] := has_key(b, a)` |
| `rem` | `19` | `-{a, "b} +{"c}` | — | `size(b)` | sets `c := rem(b, a, false)` |
| `remc` | `1a` | `-{a, "b} +{"c}` | — | `size(b)` | sets `c := rem(b, a, true)` |
| `dup` | `3n` | `-{x*, "a} +{"a, x*, "a}` | (`n` in opcode) | `1` | duplicates `a`, where `x*` are `n` stack items above it |
| `swap` | `4n` | `-{"a, x*, †b} +{†b, x*, "a}` | (`n` in opcode) | `1` | swap two stack items with `n` items `x*` between them |
| `idx` | `5n` | `-{k*, "a} +{"b}` | `c: path(n+1)` | `\|c\| + Σ size(x_i)` | walks `a` along path `c`. `k*` are `m` stack items `k_1 .. k_{m+1}` matching the `stack` symbols in `c`. Sets `"x_1 = "a`, `key_j = if c_j == stack then k_{i++} else c_j`, `"x_{j+1} = "x_j.get(key_j, cached)`, `"b = "x_{n+2}`, with `i` initialised to `1` and `cached = false` |
| `idxc` | `6n` | `-{k*, "a} +{"b}` | `c: path(n+1)` | `\|c\| + Σ size(x_i)` | as `idx`, with `cached = true` |
| `idxp` | `7n` | `-{k*, "a} +{"b, pth*}` | `c: path(n+1)` | `\|c\| + Σ size(x_i)` | as `idx`, additionally pushing `pth*` = `{key_{n+1}, "x_{n+1}, …, key_1, "x_1}` |
| `idxpc` | `8n` | `-{k*, "a} +{"b, pth*}` | `c: path(n+1)` | `\|c\| + Σ size(x_i)` | as `idxp`, with `cached = true` |
| `ins` | `9n` | `-{"a, pth*} +{†b}` | (`n` levels in opcode) | `Σ size(x_i)` | where `pth*` is `{key_{n+1}, x_{n+1}, …, key_1, x_1}` (produced by a matching `idxp`), sets `x'_{n+2} = a`, `x'_j = ins(x_j, key_j, cached, x'_{j+1})`, `b = x'_1`. `†` is the weakest modifier of `a` and the `x_j`s; `cached = false` |
| `insc` | `an` | `-{"a, pth*} +{†b}` | (`n` levels in opcode) | `Σ size(x_i)` | as `ins`, with `cached = true` |
| `ckpt` | `ff` | `-{} +{}` | — | `1` | boundary between internally atomic program segments — must not be crossed by jumps |

## Appendix B — error conditions (`OnchainProgramError`)

| Error | Cause |
|---|---|
| `RanOffStack` | an op required more stack items than present |
| `RanPastProgramEnd` | a `jmp` / `branch` skipped beyond the program |
| `StackOverflow` | stack exceeded `2^16` items |
| `OutOfGas` | cumulative `compute_time` / `read_time` exceeded the limit |
| `ExpectedCell` | an op needed a `Cell` but got another `StateValue` |
| `TypeError` | wrong `StateValue` type for the op (e.g. `size` of a `Cell`) |
| `ArithmeticOverflow` | `add` / `sub` / `addi` / `subi` over- or underflow (u64) |
| `TooLongForEqual` | `eq` with both operands > 64 bytes |
| `CellBoundExceeded` | `concat` result exceeds the cell bound |
| `LogBoundExceeded` | `log` payload exceeds `MAX_LOG_SIZE` (`2^19`) |
| `CacheMiss` | a `*c` (cached) op found the node not in cache |
| `ReadMismatch` | verifying-mode `popeq` value ≠ expected `result` |
| `BoundsExceeded` / `InvalidArgs` | argument out of range (e.g. `new` array > 16) |
| `MissingKey` / `AttemptedArrayDelete` | invalid container access |
| `MerkleTreeError` | invalid Merkle-tree update |
| `Decode` | malformed FAB value |
