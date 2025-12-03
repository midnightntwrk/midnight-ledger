# Field-Aligned Binary Data

This document describes the Field-Aligned Binary data format, as JS values,
binary, and field representations.

## Assumptions

This document assumes the existence of a primary modular field of interest. This
shall be taken to be the current modular prime field of the base elliptic curve
used in Midnight. Should Midnight abandon the use of elliptic curves for a
binary cryptosystem, the modular field shall be taken to be $\mod 2^{128}$, and
it shall be guaranteed to be at least this large.

## Data types

This document defines the following data types, and defines both a JS value, and
binary format for each. Where not obvious from context, these may be prefixed
with `Fab` to signify their nature as Field-Aligned Binary.

* `Value`
* `ValueAtom`
* `Alignment`
* `AlignmentSegment`
* `AlignmentAtom`

In addition to the JSON and binary formats, a `Value` annotated with its
`Alignment`, or a `ValueAtom` annotated with its `AlignmentAtom` has a *field
representation*, which will be represented below.

## Binary lengths and supplementary bits

In binary formats, this document will refer to *integers with flags xy*. This
refers to a (sequence of) bytes adhering the the following formats. Formats are
written MSB first, and we write `[a]` for the integer value represented by a
sequence of bits `a`.

```
xy0a aaaa                     ~ [a]
xy1a aaaa 0bbb bbbb           ~ [a] + [b] << 5
xy1a aaaa 1bbb bbbb 0ccc cccc ~ [a] + [b] << 5 + [c] << 12
--1- ---- 1--- ---- 1--- ---- reserved
```

For instance, `0101 1011` represents 51 with flags `01`.

In the second and third variants, `[b]` and `[c]` respectively *must not* be 0.

Note that the limit to three bytes limits us to integers < `2^19`, or
524,288. As this is primarily used for lengths for on-chain data, this seems
reasonable, but it can be extended with only a minor version increment.

## `Value`, `ValueAtom` representation

Conceptually, a `Value` is an array of `ValueAtom`s, which are each an array of
unsigned 8-bit integers.

### In TypeScript

```typescript
type Value = Array<ValueAtom>;
type ValueAtom = Uint8Array;
```

### In Binary

A `Value` begins with an integer with flags `xy`. Depending on `xy`:

* `xy = 0-`, is handled as if parsing a `ValueAtom` with flags `xy`, and
  wrapping the result in a singleton list.
* `xy = 10`, the integer encodes the number of `ValueAtom`s that follow. The
  integer *must not* be 1.
* `xy = 11` is reserved.

A `ValueAtom` begins with an integer with flags `xy`. Depending on `xy`:

* `xy = 00`, the integer encodes a single byte, which *must* be > 0 and < 64.
  This byte is a singleton in this `ValueAtom`.
* `xy = 01`, the integer encodes the number of bytes that follow. *If* the
  integer is 1, the following bytes *must* be >= 64. The final byte in the
  sequence *must not* be 0.
* `xy = 1-` is reserved.

## `Alignment`, `AlignmentSegment`, and `AlignmentAtom` representation

Conceptually, an `Alignment` is a sequence of `AlignmentSegment`s, each of
which is either a list of `Alignment`s (encoding sum types), or an
`AlignmentAtom`. Each alignment atom is one of: `compress`, indicating
arbitrary data hashed into a field element, `field`, indicating a raw field
element, or `bytes<x>`, indicating a sequence of `x` bytes.

An `AlignedValue` is a list of `ValueAtom` / `AlignmentAtom` pairs.

### In TypeScript

```typescript
type Alignment = Array<AlignmentSegment>;
type AlignmentSegment = { tag: "option", options: Array<Alignment> } | { tag: "atom", atom: AlignmentAtom };
type AlignmentAtom = { tag: "compress"} | { tag: "field" } | { tag: "bytes", length: number };
type AlignedValue = Array<[ValueAtom, AlignmentAtom]>;
```

### In Binary

An `Alignment` is one of:

* `xy = 11`, the integer encodes the number of alignment segments that follow
* all other `xy` should be parsed as-is as a singleton `AlignmentSegment`.

An `AlignmentSegment` *is* an integer with flags `xy`:

* `xy = 0-`, the integer encodes an AlignmentAtom with flags `0y`.
* `xy = 10`, the integer encodes the number of `Alignment`s that follow.
* `xy = 11` reserved.

An `AlignmentAtom` *is* an integer `i` with flags `xy`:
* `xy = 00`, the integer encodes the length of a `bytes` `AlignmentAtom`.
* `xy = 01 and i = 0`, encodes `compress`.
* `xy = 01 and i = 1`, encodes `field`.
* `x = 1 or xy = 01 and i > 1` reserved.

An `AlignedValue` is encoded by first encoding the `ValueAtom`s as a `Value`,
then by encoding each of the `AlignmentAtom`s in turn.

## Validity

A `value: ValueAtom` is *valid* for an `align: AlignmentAtom` if all of the
following hold:

* `value` does not end with a zero byte. 
* If `align = field` then the length of `value` does not exceed the number of
  bytes needed to store a field element at any point in Midnight's history
  (currently: 32 bytes).
* If `align = bytes<n>`, then the length of `value` is at most `n`.

A `value: Value` is *valid* for an `align: Alignment` if *consuming* the
alignment succeeds, and leaves no `ValueAtom`s. A value is consumed by removing
`ValueAtom`s from the front of the `Value`, while removing `AlignmentSegment`s
from the front of the alignment:

* A `AlignmentAtom` consumes exactly one `ValueAtom`, if and only if the latter
  is valid for the former.
* A "option"-type `AlignmentSegment` first consumes exactly one `ValueAtom`,
  which must be valid for `bytes<4>`. This is interpreted as a little-endian,
  unsigned, 32-bit integer `i`. `i` must be less than the number of
  `Alignment` options. It then consumes the `Alignment` at index `i` in the
  segment.

## Field representation

An alignment-annotated `value: ValueAtom`, valid for alignment `align:
AlignmentAtom` has a representation as a sequence of field elements:

* If `align = compress`, it is represented as a single field element by
  applying cryptographically hashing `value`. (length: 1)
* If `align = field`, it is represented as a single field element by
  interpreting `value` as a little-endian bigint, modolo the field order. (length: 1)
* If `align = bytes<n>`, `value` is split into chunks according to the largest
  number of bytes representable in a single field `k` (currently: 31), each of
  which are encoded independently as a single field element is in `align =
  field`. Chunks are filled *from the end* of the byte array: A `value` of
  length 40, with 31 bytes representable, is represented as two field elements,
  the first encoding the first 9 bytes, the second the remaining 31. (length: `ceil(n / k)`)

An alignment-annotated `value: Value`, valid for alignment `align: Alignment`
has a representation as a sequence of field elements, arrived at by consuming
the alignment, and appending the generated parts in sequence:

* Consuming an `AlignmentAtom` produces the representation above 
* Consuming an "option"-type `AlignmentSegment` proceeds as consuming one in
  [Validty](#validity), consuming first the `bytes<4>` representation, then
  the chosen `Alignment`. The length of each `Alignment` option's field
  representation is calculated, and if the chosen one is shorter than this, it
  is padded to this length with zero elements.

A `AlignedValue` can be completely encoded into field elements as follows:
* Encoding the size length of the `AlignedValue` as a single field element.
* Encoding each `AlignmentAtom` as follows:
  * `bytes<n>` encoded as `<n>`
  * `compress` encoded as `-1`
  * `field` encoded as `-2`
* Encoding each `ValueAtom`s with respect to their `AlignmentAtom`.

## High-level vs low-level JS values

Field-Aligned Binary does not contain sufficient structural information to
output high-level JS values, such as objects, but with type information the
Compact (soon to be Minokawa) compiler should be able to perform automatic mappings between some
typescript types and the low-level JS values listed above. 

The high(er)-level typescript types understood are:

* Plain objects, e.g. `{foo: "foo", bar: "bar"}`
* Arrays, e.g. `["foo", "bar"]`
* Unsigned integer `number`s
* `true` and `false`
* Tagged union types, e.g. `{ tag: "foo", bar: number, baz: string } | { tag: "bar" }`
* `string` and `Uint8Array`
* Arbitrary types with `<type>.decode(from: Value)` and `<value>.encode(): Value` methods.

Encoding is straight-forward:

* Plain objects are encoded by encoding their fields in sequence (in the order
  specified in the Compact type declaration)
* Arrays are encoded by encoding their elements in sequence.
* Unsigned integers are encoded as a singleton array of a little-endian byte array.
* `true` is encoded as `[[1]]`, and `false` as `[[]]`.
* Tagged unions are encoded as the encoding of their tag's index in the type,
  followed by encoding of the remaining fields in sequence. Note that Compact
  currently does not offer disjoint/tagged union support, but if it were added
  in the future, the tag index would be derived from the order in the Compact
  type declaration.
* `string`s are encoded in utf-8 as a singleton byte array.
* `Uint8Array`s are encoded as a singleton of themselves.
* Arbitrary types are encoded with `.encode`.

Decoding proceeds similarly, but more often requires type information to
reconstruct discarded information.

