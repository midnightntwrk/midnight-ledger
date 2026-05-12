// This file is part of midnight-ledger.
// Copyright (C) Midnight Foundation
// SPDX-License-Identifier: Apache-2.0
// Licensed under the Apache License, Version 2.0 (the "License");
// You may not use this file except in compliance with the License.
// You may obtain a copy of the License at
// http://www.apache.org/licenses/LICENSE-2.0
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Decode the byte-flat `Impact` instruction's `inputs: Vec<Operand>` into a
//! gather-mode `Vec<Op<ResultModeGather, D>>` ready to feed to
//! `query_context.query` at ZKIR v3 execution time.
//!
//! ## Wire format
//!
//! Each entry in `inputs` is an `Operand`:
//!   * `Operand::Immediate(fr)` — opcode bytes, structural metadata
//!     (alignment lengths and sentinels, StateValue tag bytes,
//!     branch/jmp skips, addi/subi immediates, concat n, BMT entry
//!     indices, etc.) and any compile-time constant value field.
//!   * `Operand::Variable(id)` — operand-bearing value field whose
//!     resolution is fetched from `memory`.
//!
//! For non-Compress atoms (`Field`, `Bytes(N)`, `Option`) the wire
//! format and behaviour exactly mirror the runtime VM's
//! `<Op<ResultModeVerify, D> as FieldRepr>::field_repr` from
//! `onchain-vm/src/ops.rs:399-464`. Operand resolution for these atoms
//! produces an `Fr` (`IrValue::Native` extracts the inner field
//! element; `IrValue::Opaque` extracts the precomputed commit;
//! immediates are passed through), and the decoder reassembles
//! `ValueAtom`s by parsing the Fr stream against the alignment.
//!
//! For `Compress` atoms — the load-bearing case — the decoder
//! dispatches on the operand variant:
//!   * `Operand::Variable(id)` resolving to `IrValue::Opaque { bytes,
//!     commit }`: use `bytes` as the `ValueAtom`. The runtime
//!     `AlignedValue` thus carries the full preimage, byte-for-byte
//!     equal to what the JS bridge's
//!     `_descriptor_6.toValue(s_0).concat(...)` produces. The commit
//!     played its role earlier in the SNARK PI vector via
//!     `Fr::try_from(IrValue::Opaque)`; the runtime VM doesn't need it.
//!   * `Operand::Immediate(byte_len_fr)` followed by
//!     `ceil(byte_len/FR_BYTES_STORED)` immediate Frs: decode bytes via
//!     `bytes_from_field_repr` and use them as the `ValueAtom`. This
//!     branch supports literal Compress preimages in the IR; in
//!     practice Compact emits the Variable form.
//!
//! ## Resolution rules at execution time
//!
//! 1. **Opcodes and structural metadata MUST resolve to immediates.**
//!    If an opcode operand references an unbound variable, decoding
//!    fails. In practice Compact always emits these as immediates.
//!
//! 2. **`Push` value operands and `Idx` `Key::Value` operands MUST
//!    resolve.** These flow into `query_context.query` and need to be
//!    concrete `StateValue<D>` / `AlignedValue`s.
//!
//! 3. **`Popeq` result operands are NOT resolved — they are skipped.**
//!    Gather-mode `Op::Popeq` carries `result: ()`. The result's
//!    operand positions are still part of the wire format (so the
//!    SNARK encoder can lift commits from them), but the runtime VM
//!    doesn't need them: `query` produces fresh read events live.
//!
//! ## Strict consumption
//!
//! The decoder consumes the entire `inputs` slice; trailing
//! unconsumed operands surface as decode failures (either via
//! "unknown opcode" or a truncation error mid-op).
//!
//! ## Limitations and round-trip caveats
//!
//! These are properties of the wire format itself (`onchain-vm/src/ops.rs`,
//! `transient-crypto/src/fab.rs`), not of this decoder. They're listed
//! here so callers know what to expect.
//!
//! 1. **`AlignmentAtom::Compress` preimages cannot be recovered from
//!    `Fr`s alone.** The forward encoding is
//!    `transient_commit(bytes, byte_len)` — a one-way commitment. The
//!    decoder gets the preimage from the operand layer:
//!     - `Operand::Variable` → `IrValue::Opaque { bytes, .. }` from
//!       `memory`.
//!     - `Operand::Immediate(byte_len)` followed by
//!       `ceil(byte_len / FR_BYTES_STORED)` immediate Frs → unpacked
//!       via `bytes_from_field_repr`.
//!
//!    There is no purely Fr-based path for Compress, and there can't
//!    be — this is fundamental to the encoding.
//!
//! 2. **`Op::Idx` paths are capped at 16 keys.** The forward encoder
//!    packs `path.len() - 1` into the low 4 bits of the opcode byte
//!    (`onchain-vm/src/ops.rs:453`). Paths of length 17 or more wrap
//!    and corrupt the opcode. This decoder produces `path_len ∈ [1, 16]`
//!    faithfully; longer paths are an encoder-side concern.
//!
//! 3. **`Op::Idx` with empty path is never seen by this decoder.** The
//!    forward encoder writes zero bytes for an empty-path `Idx`
//!    (`onchain-vm/src/ops.rs:447`), so empty-Idx ops are dropped at
//!    encode time. Correct by construction.
//!
//! 4. **`Op::Ins { n: 0 }` round-trips faithfully.** The forward
//!    encoder writes `0x90` / `0xa0`. Downstream
//!    `program_with_results` (the gather→verify translation) filters
//!    `Ins { n: 0 }` out of the verify program — that's a separate
//!    policy concern, not a decoder concern. The decoder produces it
//!    faithfully for `query` to consume.
//!
//! 5. **`Op::Noop` runs are fused back into a single `Op::Noop { n }`.**
//!    The forward encoder writes `n` zero bytes for a single
//!    `Op::Noop { n }` and the runtime gas formula
//!    (`onchain-vm/src/vm.rs:382`) is
//!    `noop_constant + n * noop_coeff_arg` per Op. Decoding N zero
//!    bytes as N separate `Noop { n: 1 }` ops would charge
//!    `N * noop_constant + N * noop_coeff_arg` — over-charging by
//!    `(N − 1) * noop_constant`. The fused form matches the gas a
//!    single op would have had.
//!
//!    The wire format can't distinguish a single `Noop { n: N }` from
//!    `N` separate `Noop { n: 1 }`s (both encode to the same N zero
//!    bytes), so fusion is the canonical inverse — it matches what
//!    Compact emits for an n-iteration noop run.
//!
//! 6. **`Popeq` / `Popeqc` result operands are not resolved or
//!    type-checked.** Gather-mode `Op::Popeq` carries `result: ()`;
//!    the runtime VM produces fresh read events. The decoder skips
//!    past the operand positions the alignment specifies (so the
//!    cursor advances correctly) but does not dereference them. An
//!    unbound `Operand::Variable` in a `Popeq` result slot is silently
//!    accepted at decode time — it's the SNARK side that pins those
//!    operand positions to commits via `Fr::try_from(IrValue::Opaque)`.

use anyhow::{Result, anyhow, bail};
use base_crypto::fab::{
    AlignedValue, Alignment, AlignmentAtom, AlignmentSegment, Value, ValueAtom,
};
use base_crypto::hash::HashOutput;
use onchain_vm::ops::{Key, Op};
use onchain_vm::result_mode::ResultModeGather;
use runtime_state::state::StateValue;
use std::collections::HashMap;
use storage::arena::Sp;
use storage::db::DB;
use storage::storage::{Array, HashMap as StoreHashMap};
use transient_crypto::curve::{FR_BYTES_STORED, Fr};
use transient_crypto::fab::AlignmentExt;
use transient_crypto::merkle_tree::MerkleTree;
use transient_crypto::repr::{FromFieldRepr, bytes_from_field_repr};

use crate::ir::{Identifier, Operand};
use crate::ir_types::IrValue;

/// Decode `inputs` (the `Impact` instruction's flat operand stream) into
/// gather-mode `Op` values for `query_context.query` execution. See module
/// docs for the wire-format and Compress-resolution semantics.
pub fn decode_impact_inputs<D: DB>(
    inputs: &[Operand],
    memory: &HashMap<Identifier, IrValue>,
) -> Result<Vec<Op<ResultModeGather, D>>> {
    let mut cursor = inputs;
    let mut out = Vec::new();
    while !cursor.is_empty() {
        out.push(decode_op::<D>(&mut cursor, memory)?);
    }
    Ok(out)
}

// ===========================================================================
// Op decoder
// ===========================================================================

fn decode_op<D: DB>(
    cursor: &mut &[Operand],
    memory: &HashMap<Identifier, IrValue>,
) -> Result<Op<ResultModeGather, D>> {
    let opcode = take_byte(cursor, memory)?;

    match opcode {
        // 0x00 Noop. The forward encoder writes `n` zero bytes for a
        // single `Op::Noop { n }` (see `onchain-vm/src/ops.rs:403`),
        // and the runtime VM charges
        // `noop_constant + n * noop_coeff_arg` per `Noop` op
        // (`onchain-vm/src/vm.rs:382`). Fusing a run of zero-immediates
        // back into a single `Op::Noop { n: count }` keeps the gas
        // charge equivalent to the original op: emitting N separate
        // `Noop { n: 1 }` ops would cost `N * noop_constant + N *
        // noop_coeff_arg`, over-charging by `(N - 1) * noop_constant`.
        //
        // The wire format itself can't distinguish a single
        // `Noop { n: N }` from `N` separate `Noop { n: 1 }` (both
        // encode to N zero bytes), so fusion is the canonical inverse:
        // it matches what the Compact compiler emits for an
        // n-iteration noop sequence, and it matches the gas charge a
        // single Op would have had.
        //
        // Fusion only consumes immediate-zero operands. A variable
        // operand resolving to zero terminates the run — Compact emits
        // opcodes as literal immediates and never as variables, so a
        // variable in the opcode position is structurally different
        // and shouldn't be folded silently.
        0x00 => {
            let mut n: u32 = 1;
            while let Some(Operand::Immediate(fr)) = cursor.first() {
                if *fr != Fr::from(0u64) {
                    break;
                }
                *cursor = &cursor[1..];
                n = n.checked_add(1).ok_or_else(|| {
                    anyhow!("decode_impact: Noop run exceeds u32::MAX")
                })?;
            }
            Ok(Op::Noop { n })
        }

        // 0x01..0x0b: trivial unit-byte opcodes.
        0x01 => Ok(Op::Lt),
        0x02 => Ok(Op::Eq),
        0x03 => Ok(Op::Type),
        0x04 => Ok(Op::Size),
        0x05 => Ok(Op::New),
        0x06 => Ok(Op::And),
        0x07 => Ok(Op::Or),
        0x08 => Ok(Op::Neg),
        0x09 => Ok(Op::Log),
        0x0a => Ok(Op::Root),
        0x0b => Ok(Op::Pop),

        // 0x0c / 0x0d: Popeq / Popeqc. Gather mode discards the
        // result; we still walk the alignment + value-operand
        // positions so the cursor advances past them. Since we don't
        // care about the actual value, we advance by exactly the
        // number of operand POSITIONS the alignment requires
        // (Compress=1, Field=1, Bytes(N)=ceil(N/31), Option=1+chosen
        // arity). For Compress slots the operand may be an Opaque
        // variable (we don't need to dereference) or an immediate
        // sequence (byte_len + N preimage Frs); both forms are
        // skipped uniformly here by walking the alignment-aware skip
        // helper.
        0x0c | 0x0d => {
            let cached = opcode == 0x0d;
            let alignment = decode_alignment(cursor, memory)?;
            skip_value_for_alignment(&alignment, cursor, memory)?;
            Ok(Op::Popeq { cached, result: () })
        }

        // 0x0e / 0x0f: Addi / Subi — one immediate u32.
        0x0e => Ok(Op::Addi {
            immediate: take_u32(cursor, memory)?,
        }),
        0x0f => Ok(Op::Subi {
            immediate: take_u32(cursor, memory)?,
        }),

        // 0x10 / 0x11: Push / Pushs — recursive StateValue payload.
        0x10 | 0x11 => {
            let storage = opcode == 0x11;
            let value = decode_state_value::<D>(cursor, memory)?;
            Ok(Op::Push { storage, value })
        }

        // 0x12 / 0x13: Branch / Jmp — one u32 skip.
        0x12 => Ok(Op::Branch {
            skip: take_u32(cursor, memory)?,
        }),
        0x13 => Ok(Op::Jmp {
            skip: take_u32(cursor, memory)?,
        }),

        // 0x14 / 0x15: Add / Sub.
        0x14 => Ok(Op::Add),
        0x15 => Ok(Op::Sub),

        // 0x16 / 0x17: Concat / Concatc — one u32 n.
        0x16 => Ok(Op::Concat {
            cached: false,
            n: take_u32(cursor, memory)?,
        }),
        0x17 => Ok(Op::Concat {
            cached: true,
            n: take_u32(cursor, memory)?,
        }),

        // 0x18: Member.
        0x18 => Ok(Op::Member),

        // 0x19 / 0x1a: Rem / Remc.
        0x19 => Ok(Op::Rem { cached: false }),
        0x1a => Ok(Op::Rem { cached: true }),

        // 0x30..0x3f: Dup with n in low nibble.
        b if b & 0xf0 == 0x30 => Ok(Op::Dup { n: b & 0x0f }),

        // 0x40..0x4f: Swap with n in low nibble.
        b if b & 0xf0 == 0x40 => Ok(Op::Swap { n: b & 0x0f }),

        // 0x50..0x8f: Idx with cached/push_path encoded in upper
        // nibble and `path.len() - 1` in lower nibble.
        b if b & 0xf0 == 0x50
            || b & 0xf0 == 0x60
            || b & 0xf0 == 0x70
            || b & 0xf0 == 0x80 =>
        {
            let (cached, push_path) = match b & 0xf0 {
                0x50 => (false, false),
                0x60 => (true, false),
                0x70 => (false, true),
                0x80 => (true, true),
                _ => unreachable!(),
            };
            let path_len = (b & 0x0f) as usize + 1;
            let mut keys: Vec<Key> = Vec::with_capacity(path_len);
            for _ in 0..path_len {
                keys.push(decode_key(cursor, memory)?);
            }
            Ok(Op::Idx {
                cached,
                push_path,
                path: keys.into_iter().collect::<Array<Key, D>>(),
            })
        }

        // 0x90..0x9f: Ins with n in low nibble.
        b if b & 0xf0 == 0x90 => Ok(Op::Ins {
            cached: false,
            n: b & 0x0f,
        }),
        // 0xa0..0xaf: Insc.
        b if b & 0xf0 == 0xa0 => Ok(Op::Ins {
            cached: true,
            n: b & 0x0f,
        }),

        // 0xff: Ckpt.
        0xff => Ok(Op::Ckpt),

        b => bail!("decode_impact: unknown opcode {:#04x}", b),
    }
}

/// Decode a complete `AlignedValue` from the operand stream. The
/// alignment metadata operands MUST resolve to immediates (length and
/// segment sentinels are structural). The value operands are dispatched
/// per-atom: `Field` and `Bytes(N)` resolve operands to Frs and parse
/// via the alignment; `Compress` consumes either an Opaque-typed
/// variable or an explicit `[byte_len, preimage_frs...]` immediate
/// sequence.
fn decode_aligned_value(
    cursor: &mut &[Operand],
    memory: &HashMap<Identifier, IrValue>,
) -> Result<AlignedValue> {
    let alignment = decode_alignment(cursor, memory)?;
    let value = decode_value_for(&alignment, cursor, memory)?;
    Ok(AlignedValue { value, alignment })
}

fn decode_alignment(
    cursor: &mut &[Operand],
    memory: &HashMap<Identifier, IrValue>,
) -> Result<Alignment> {
    let len = take_u32(cursor, memory)? as usize;
    let mut segments: Vec<AlignmentSegment> = Vec::with_capacity(len);
    for _ in 0..len {
        segments.push(decode_alignment_segment(cursor, memory)?);
    }
    Ok(Alignment(segments))
}

fn decode_alignment_segment(
    cursor: &mut &[Operand],
    memory: &HashMap<Identifier, IrValue>,
) -> Result<AlignmentSegment> {
    let first = take_fr(cursor, memory)?;
    if let Some(neg) = fr_to_small_negative(&first) {
        match neg {
            -1 => {
                return Ok(AlignmentSegment::Atom(AlignmentAtom::Compress))
            },
            -2 => {
                return Ok(AlignmentSegment::Atom(AlignmentAtom::Field))
            },
            -3 => {
                let opts_len = take_u32(cursor, memory)? as usize;
                let mut options: Vec<Alignment> = Vec::with_capacity(opts_len);
                for _ in 0..opts_len {
                    options.push(decode_alignment(cursor, memory)?);
                }
                return Ok(AlignmentSegment::Option(options));
            }
            other => bail!(
                "decode_impact: unknown alignment-segment sentinel {}",
                other
            ),
        }
    }
    let length = u32::try_from(first)
        .map_err(|_| anyhow!("decode_impact: alignment Bytes length out of u32 range"))?;
    Ok(AlignmentSegment::Atom(AlignmentAtom::Bytes { length }))
}

/// Decode the `Value` portion of an `AlignedValue` from the operand
/// cursor by walking each segment in turn and dispatching per atom kind.
/// Compress atoms see operand-level dispatch (Variable → Opaque memory;
/// Immediate → explicit byte_len + preimage Frs); other atoms resolve
/// operands to Frs and parse via the existing `Fr → ValueAtom`
/// reconstruction.
fn decode_value_for(
    alignment: &Alignment,
    cursor: &mut &[Operand],
    memory: &HashMap<Identifier, IrValue>,
) -> Result<Value> {
    let mut value: Vec<ValueAtom> = Vec::new();
    decode_segments_into(&alignment.0, cursor, memory, &mut value)?;
    Ok(Value(value))
}

fn decode_segments_into(
    segments: &[AlignmentSegment],
    cursor: &mut &[Operand],
    memory: &HashMap<Identifier, IrValue>,
    out: &mut Vec<ValueAtom>,
) -> Result<()> {
    for segment in segments.iter() {
        match segment {
            AlignmentSegment::Atom(atom) => {
                let atom_value = match atom {
                    AlignmentAtom::Compress => {
                        decode_compress_value(cursor, memory)?
                    },
                    AlignmentAtom::Field => {
                        let fr = take_fr(cursor, memory)?;
                        ValueAtom(fr.as_le_bytes()).normalize()
                    }
                    AlignmentAtom::Bytes { length } => {
                        let n_frs = (*length as usize).div_ceil(FR_BYTES_STORED);
                        let mut frs: Vec<Fr> = Vec::with_capacity(n_frs);
                        for _ in 0..n_frs {
                            frs.push(take_fr(cursor, memory)?);
                        }
                        let mut fr_cursor = &frs[..];
                        let bytes = bytes_from_field_repr(&mut fr_cursor, *length as usize)
                            .ok_or_else(|| {
                                anyhow!("decode_impact: failed to parse Bytes({}) atom", length)
                            })?;
                        ValueAtom(bytes)
                    }
                };
                out.push(atom_value);
            }
            AlignmentSegment::Option(options) => {
                let variant_fr = take_fr(cursor, memory)?;
                let variant = u16::try_from(variant_fr).map_err(|_| {
                    anyhow!("decode_impact: Option variant tag out of u16 range")
                })?;
                out.push(variant.into());
                let choice = options.get(variant as usize).ok_or_else(|| {
                    anyhow!(
                        "decode_impact: Option variant index {} out of range (have {} choices)",
                        variant,
                        options.len()
                    )
                })?;
                decode_segments_into(&choice.0, cursor, memory, out)?;
                let max_field_len =
                    options.iter().map(Alignment::field_len).max().unwrap_or(0);
                let padding = max_field_len - choice.field_len();
                for _ in 0..padding {
                    let pad = take_fr(cursor, memory)?;
                    if pad != Fr::from(0u64) {
                        bail!("decode_impact: Option padding non-zero");
                    }
                }
            }
        }
    }
    Ok(())
}

/// Decode a `Compress` slot's value. Two wire-format variants are
/// supported, distinguished by the variant of the operand at the
/// current cursor position:
///
/// * **Variable (the canonical Compact-emitted form)**: the next
///   operand is `Operand::Variable(id)` whose memory entry is
///   `IrValue::Opaque { bytes, .. }`. The decoder consumes one
///   operand position and uses `bytes` as the `ValueAtom`.
///
/// * **Immediate (literal preimage)**: the next operand is
///   `Operand::Immediate(byte_len_fr)`, followed by
///   `ceil(byte_len / FR_BYTES_STORED)` immediate Frs holding the
///   preimage in the canonical `bytes_from_field_repr` packing. The
///   decoder consumes `1 + n_frs` operand positions.
fn decode_compress_value(
    cursor: &mut &[Operand],
    memory: &HashMap<Identifier, IrValue>,
) -> Result<ValueAtom> {
    let next = cursor
        .first()
        .ok_or_else(|| anyhow!("decode_impact: truncated Compress atom value"))?
        .clone();

    match next {
        Operand::Variable(id) => {
            let val = memory.get(&id).cloned().ok_or_else(|| {
                anyhow!(
                    "decode_impact: Compress operand variable {:?} not bound in memory",
                    id
                )
            })?;
            // Use the explicit `Vec<u8>::try_from` impl on `IrValue`,
            // which matches only the `Opaque` variant. Any other
            // variant errors with a clear "cannot convert .. to
            // Opaque preimage bytes" message.
            let bytes: Vec<u8> = val.try_into().map_err(|e: anyhow::Error| {
                anyhow!(
                    "decode_impact: Compress operand variable {:?}: {}",
                    id,
                    e
                )
            })?;
            *cursor = &cursor[1..];
            Ok(ValueAtom(bytes))
        }
        Operand::Immediate(_) => {
            let byte_len = take_u32(cursor, memory)? as usize;
            let n_frs = byte_len.div_ceil(FR_BYTES_STORED);
            let mut frs: Vec<Fr> = Vec::with_capacity(n_frs);
            for _ in 0..n_frs {
                frs.push(take_fr(cursor, memory)?);
            }
            let mut fr_cursor = &frs[..];
            let bytes = bytes_from_field_repr(&mut fr_cursor, byte_len).ok_or_else(|| {
                anyhow!("decode_impact: malformed Compress preimage Frs (literal form)")
            })?;
            Ok(ValueAtom(bytes))
        }
    }
}

/// Skip past the value operands for a given alignment without
/// constructing a `Value`. Used for `Popeq` result fields, which the
/// gather-mode decoder doesn't need to materialize. The skip count for
/// each atom matches the number of operand POSITIONS its value
/// occupies in the wire format:
///
/// * `Field` — 1 position.
/// * `Bytes(N)` — `ceil(N / FR_BYTES_STORED)` positions.
/// * `Compress` — 1 position if the next operand is a Variable;
///   `1 + ceil(byte_len / FR_BYTES_STORED)` positions if it's an
///   Immediate sequence.
/// * `Option` — 1 position for the variant tag plus the chosen
///   alignment's positions, plus the padding to reach the longest
///   option's field_len (each padding Fr must be zero).
fn skip_value_for_alignment(
    alignment: &Alignment,
    cursor: &mut &[Operand],
    memory: &HashMap<Identifier, IrValue>,
) -> Result<()> {
    skip_segments(&alignment.0, cursor, memory)
}

fn skip_segments(
    segments: &[AlignmentSegment],
    cursor: &mut &[Operand],
    memory: &HashMap<Identifier, IrValue>,
) -> Result<()> {
    for segment in segments.iter() {
        match segment {
            AlignmentSegment::Atom(AlignmentAtom::Compress) => {
                // Match `decode_compress_value`'s position accounting.
                let next = cursor
                    .first()
                    .ok_or_else(|| anyhow!("skip_segments: truncated Compress atom"))?
                    .clone();
                match next {
                    Operand::Variable(_) => {
                        *cursor = &cursor[1..];
                    }
                    Operand::Immediate(_) => {
                        let byte_len = take_u32(cursor, memory)? as usize;
                        let n_frs = byte_len.div_ceil(FR_BYTES_STORED);
                        skip_operands(cursor, n_frs)?;
                    }
                }
            }
            AlignmentSegment::Atom(AlignmentAtom::Field) => {
                skip_operands(cursor, 1)?;
            }
            AlignmentSegment::Atom(AlignmentAtom::Bytes { length }) => {
                let n_frs = (*length as usize).div_ceil(FR_BYTES_STORED);
                skip_operands(cursor, n_frs)?;
            }
            AlignmentSegment::Option(options) => {
                let variant_fr = take_fr(cursor, memory)?;
                let variant = u16::try_from(variant_fr).map_err(|_| {
                    anyhow!("skip_segments: Option variant tag out of u16 range")
                })?;
                let choice = options.get(variant as usize).ok_or_else(|| {
                    anyhow!(
                        "skip_segments: Option variant index {} out of range",
                        variant
                    )
                })?;
                skip_segments(&choice.0, cursor, memory)?;
                let max_field_len =
                    options.iter().map(Alignment::field_len).max().unwrap_or(0);
                let padding = max_field_len - choice.field_len();
                skip_operands(cursor, padding)?;
            }
        }
    }
    Ok(())
}

/// Decode an `Idx` `Key`. The first operand resolves to `Fr`:
///   * `-1` → `Key::Stack`
///   * non-negative u32 → it was the alignment length of a
///     `Key::Value`'s `AlignedValue`; consume that many alignment
///     segments, then the value operands.
fn decode_key(
    cursor: &mut &[Operand],
    memory: &HashMap<Identifier, IrValue>,
) -> Result<Key> {
    let first = take_fr(cursor, memory)?;
    if let Some(-1) = fr_to_small_negative(&first) {
        return Ok(Key::Stack);
    }
    let len = u32::try_from(first)
        .map_err(|_| anyhow!("decode_impact: key alignment length out of u32 range"))?
        as usize;
    let mut segments: Vec<AlignmentSegment> = Vec::with_capacity(len);
    for _ in 0..len {
        segments.push(decode_alignment_segment(cursor, memory)?);
    }
    let alignment = Alignment(segments);
    let value = decode_value_for(&alignment, cursor, memory)?;
    Ok(Key::Value(AlignedValue { value, alignment }))
}

// ===========================================================================
// StateValue decoder (recursive, operand-resolving)
// ===========================================================================

fn decode_state_value<D: DB>(
    cursor: &mut &[Operand],
    memory: &HashMap<Identifier, IrValue>,
) -> Result<StateValue<D>> {
    let tag_fr = take_fr(cursor, memory)?;
    let tag = u128::try_from(tag_fr)
        .map_err(|_| anyhow!("decode_impact: StateValue tag out of u128 range"))?;
    let discriminator = (tag & 0x0F) as u8;
    match discriminator {
        // Null: tag exactly 0, no payload.
        0 => Ok(StateValue::Null),

        // Cell: tag = 1; payload is a single AlignedValue.
        1 => {
            let av = decode_aligned_value(cursor, memory)?;
            Ok(StateValue::Cell(Sp::new(av)))
        }

        // Map: tag = 2 | (size << 4); size entries of (AlignedValue,
        // StateValue) pairs.
        2 => {
            let size = (tag >> 4) as usize;
            let mut map: StoreHashMap<AlignedValue, StateValue<D>, D> = StoreHashMap::new();
            for _ in 0..size {
                let key = decode_aligned_value(cursor, memory)?;
                let val = decode_state_value::<D>(cursor, memory)?;
                map = map.insert(key, val);
            }
            Ok(StateValue::Map(map))
        }

        // Array: tag = 3 | (len << 4); len recursive StateValues.
        3 => {
            let len = (tag >> 4) as usize;
            let mut elems: Vec<StateValue<D>> = Vec::with_capacity(len);
            for _ in 0..len {
                elems.push(decode_state_value::<D>(cursor, memory)?);
            }
            Ok(StateValue::Array(
                elems.into_iter().collect::<Array<StateValue<D>, D>>(),
            ))
        }

        // BoundedMerkleTree: tag = 4 | (height << 4) | (entries.len << 12);
        // each entry is (u64 idx, HashOutput) (1 Fr + 2 Frs).
        4 => {
            let height = ((tag >> 4) & 0xFF) as u8;
            let entries_len = (tag >> 12) as usize;
            let mut tree: MerkleTree<(), D> = MerkleTree::blank(height);
            for _ in 0..entries_len {
                let idx = take_u64(cursor, memory)?;
                let hash = take_hash_output(cursor, memory)?;
                tree = tree
                    .try_update_hash(idx, hash, ())
                    .map_err(|e| anyhow!("decode_impact: try_update_hash: {:?}", e))?;
            }
            tree = tree.rehash();
            Ok(StateValue::BoundedMerkleTree(tree))
        }

        other => bail!(
            "decode_impact: unknown StateValue tag discriminator {}",
            other
        ),
    }
}

// ===========================================================================
// Operand-cursor primitive helpers
// ===========================================================================

fn take_operand(cursor: &mut &[Operand]) -> Result<Operand> {
    let Some((first, rest)) = cursor.split_first() else {
        bail!("decode_impact: unexpected end of operand stream");
    };
    *cursor = rest;
    Ok(first.clone())
}

/// Resolve an `Operand` to its `Fr` value.
///   * `Operand::Immediate(fr)` → `fr`.
///   * `Operand::Variable(id)` → `Fr::try_from(memory[id])`.
///     For `IrValue::Native` this is the inner Fr; for
///     `IrValue::Opaque` this is the precomputed commit (transparent
///     by design — see `ir_types.rs` for the rationale).
fn resolve_operand_fr(
    operand: &Operand,
    memory: &HashMap<Identifier, IrValue>,
) -> Result<Fr> {
    match operand {
        Operand::Immediate(fr) => Ok(*fr),
        Operand::Variable(id) => {
            let val = memory
                .get(id)
                .cloned()
                .ok_or_else(|| anyhow!("decode_impact: variable {:?} not bound in memory", id))?;
            Fr::try_from(val).map_err(|e| {
                anyhow!(
                    "decode_impact: variable {:?} could not convert to Fr: {e}",
                    id
                )
            })
        }
    }
}

fn take_fr(
    cursor: &mut &[Operand],
    memory: &HashMap<Identifier, IrValue>,
) -> Result<Fr> {
    let op = take_operand(cursor)?;
    resolve_operand_fr(&op, memory)
}

fn take_byte(
    cursor: &mut &[Operand],
    memory: &HashMap<Identifier, IrValue>,
) -> Result<u8> {
    let fr = take_fr(cursor, memory)?;
    fr_to_byte(&fr).ok_or_else(|| anyhow!("decode_impact: opcode out of byte range: {:?}", fr))
}

fn take_u32(
    cursor: &mut &[Operand],
    memory: &HashMap<Identifier, IrValue>,
) -> Result<u32> {
    let fr = take_fr(cursor, memory)?;
    u32::try_from(fr).map_err(|_| anyhow!("decode_impact: Fr does not fit in u32"))
}

fn take_u64(
    cursor: &mut &[Operand],
    memory: &HashMap<Identifier, IrValue>,
) -> Result<u64> {
    let fr = take_fr(cursor, memory)?;
    u64::try_from(fr).map_err(|_| anyhow!("decode_impact: Fr does not fit in u64"))
}

fn take_hash_output(
    cursor: &mut &[Operand],
    memory: &HashMap<Identifier, IrValue>,
) -> Result<HashOutput> {
    const N: usize = <HashOutput as FromFieldRepr>::FIELD_SIZE;
    if cursor.len() < N {
        bail!(
            "decode_impact: HashOutput truncated; need {} operands, have {}",
            N,
            cursor.len()
        );
    }
    let mut chunk: Vec<Fr> = Vec::with_capacity(N);
    for _ in 0..N {
        chunk.push(take_fr(cursor, memory)?);
    }
    HashOutput::from_field_repr(&chunk)
        .ok_or_else(|| anyhow!("decode_impact: failed to parse HashOutput"))
}

fn skip_operands(cursor: &mut &[Operand], n: usize) -> Result<()> {
    if cursor.len() < n {
        bail!(
            "decode_impact: cannot skip {} operands; only {} remaining",
            n,
            cursor.len()
        );
    }
    *cursor = &cursor[n..];
    Ok(())
}

fn fr_to_byte(fr: &Fr) -> Option<u8> {
    u8::try_from(*fr).ok()
}

fn fr_to_small_negative(fr: &Fr) -> Option<i32> {
    for n in 1..=8i32 {
        if *fr == Fr::from(-n as i64) {
            return Some(-n);
        }
    }
    None
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use storage::db::InMemoryDB;
    use transient_crypto::repr::FieldRepr;

    type GatherOp = Op<ResultModeGather, InMemoryDB>;

    /// Wrap a sequence of `Fr`s as `Operand::Immediate`s. Used for tests
    /// that don't need variable references.
    fn imm(frs: &[Fr]) -> Vec<Operand> {
        frs.iter().copied().map(Operand::Immediate).collect()
    }

    /// Decode the all-immediate form of a trivial opcode and assert the
    /// result.
    #[test]
    fn decode_lt() {
        let memory = HashMap::new();
        let inputs = imm(&[Fr::from(0x01u64)]);
        let ops: Vec<GatherOp> =
            decode_impact_inputs::<InMemoryDB>(&inputs, &memory).expect("decode");
        assert_eq!(ops.len(), 1);
        assert!(matches!(ops[0], Op::Lt));
    }

    #[test]
    fn decode_addi_with_immediate() {
        let memory = HashMap::new();
        let inputs = imm(&[Fr::from(0x0eu64), Fr::from(42u64)]);
        let ops: Vec<GatherOp> =
            decode_impact_inputs::<InMemoryDB>(&inputs, &memory).expect("decode");
        assert_eq!(ops.len(), 1);
        match &ops[0] {
            Op::Addi { immediate } => assert_eq!(*immediate, 42u32),
            other => panic!("expected Addi, got {other:?}"),
        }
    }

    /// Push of `Cell(Field, [%v])` where `%v` resolves to an
    /// `IrValue::Native(commit)`. The decoded AV's value bytes are the
    /// LE bytes of the resolved Fr (matching what the Field arm of
    /// `field_repr_unchecked` would write).
    #[test]
    fn decode_push_cell_field_with_variable() {
        let id = Identifier("%v".to_string());
        let mut memory: HashMap<Identifier, IrValue> = HashMap::new();
        memory.insert(id.clone(), IrValue::Native(Fr::from(7u64)));

        // [opcode=0x10, Cell tag=1, alignment_len=1, Field sentinel=-2, %v]
        let inputs = vec![
            Operand::Immediate(Fr::from(0x10u64)),
            Operand::Immediate(Fr::from(1u64)),
            Operand::Immediate(Fr::from(1u64)),
            Operand::Immediate(-Fr::from(2u64)),
            Operand::Variable(id),
        ];
        let ops: Vec<GatherOp> =
            decode_impact_inputs::<InMemoryDB>(&inputs, &memory).expect("decode");
        assert_eq!(ops.len(), 1);
        match &ops[0] {
            Op::Push {
                storage,
                value: StateValue::Cell(av),
            } => {
                assert!(!storage);
                assert_eq!(av.alignment.0.len(), 1);
                // Field arm produces ValueAtom(fr.as_le_bytes()).normalize().
                let mut expected = Fr::from(7u64).as_le_bytes();
                while expected.last() == Some(&0) {
                    expected.pop();
                }
                assert_eq!(av.value.0[0].0, expected);
            }
            other => panic!("expected Push Cell, got {other:?}"),
        }
    }

    /// **The load-bearing case for the new design.** Push of
    /// `Cell(Compress, [%opaque])` where `%opaque` resolves to
    /// `IrValue::Opaque { bytes: b"hello world", commit }`. The
    /// decoded AV's value bytes equal the preimage `b"hello world"`,
    /// not the commit.
    #[test]
    fn decode_push_cell_compress_with_opaque() {
        let id = Identifier("%s".to_string());
        let preimage = b"hello world".to_vec();
        let mut memory: HashMap<Identifier, IrValue> = HashMap::new();
        memory.insert(id.clone(), IrValue::opaque(preimage.clone()));

        // [opcode=0x10, Cell tag=1, alignment_len=1, Compress sentinel=-1, %s]
        let inputs = vec![
            Operand::Immediate(Fr::from(0x10u64)),
            Operand::Immediate(Fr::from(1u64)),
            Operand::Immediate(Fr::from(1u64)),
            Operand::Immediate(-Fr::from(1u64)),
            Operand::Variable(id),
        ];
        let ops: Vec<GatherOp> =
            decode_impact_inputs::<InMemoryDB>(&inputs, &memory).expect("decode");
        assert_eq!(ops.len(), 1);
        match &ops[0] {
            Op::Push {
                storage,
                value: StateValue::Cell(av),
            } => {
                assert!(!storage);
                assert_eq!(av.alignment.0.len(), 1);
                assert!(matches!(
                    av.alignment.0[0],
                    AlignmentSegment::Atom(AlignmentAtom::Compress)
                ));
                assert_eq!(av.value.0[0].0, preimage);
            }
            other => panic!("expected Push Cell, got {other:?}"),
        }
    }

    /// Compress-with-Opaque-variable failure mode: the operand is a
    /// Variable but resolves to a non-Opaque value. Must error.
    #[test]
    fn decode_push_compress_with_native_variable_errors() {
        let id = Identifier("%bad".to_string());
        let mut memory: HashMap<Identifier, IrValue> = HashMap::new();
        memory.insert(id.clone(), IrValue::Native(Fr::from(7u64)));

        let inputs = vec![
            Operand::Immediate(Fr::from(0x10u64)),
            Operand::Immediate(Fr::from(1u64)),
            Operand::Immediate(Fr::from(1u64)),
            Operand::Immediate(-Fr::from(1u64)),
            Operand::Variable(id),
        ];
        let err = decode_impact_inputs::<InMemoryDB>(&inputs, &memory)
            .expect_err("must error when Compress operand is Native variable");
        let msg = err.to_string();
        assert!(
            msg.contains("Compress") && msg.contains("Native"),
            "unexpected error: {msg}"
        );
    }

    /// Compress-with-Opaque-variable failure mode: the operand is a
    /// Variable not present in memory.
    #[test]
    fn decode_push_compress_with_unbound_variable_errors() {
        let memory: HashMap<Identifier, IrValue> = HashMap::new();
        let inputs = vec![
            Operand::Immediate(Fr::from(0x10u64)),
            Operand::Immediate(Fr::from(1u64)),
            Operand::Immediate(Fr::from(1u64)),
            Operand::Immediate(-Fr::from(1u64)),
            Operand::Variable(Identifier("%missing".to_string())),
        ];
        let err = decode_impact_inputs::<InMemoryDB>(&inputs, &memory)
            .expect_err("must error when Compress variable not bound");
        let msg = err.to_string();
        assert!(msg.contains("not bound"), "unexpected error: {msg}");
    }

    /// Push of `Cell(Compress, [byte_len_imm, preimage_fr_imm])`
    /// using the literal-immediate form. The decoded AV's value bytes
    /// equal the supplied preimage.
    #[test]
    fn decode_push_cell_compress_with_immediate_preimage() {
        // Preimage = b"hello" (5 bytes). N = ceil(5/31) = 1 Fr.
        // Pack: chunks(31).rev() of b"hello" = [b"hello"]. One Fr.
        let preimage = b"hello".to_vec();
        let preimage_fr = Fr::from_le_bytes(&preimage).expect("fits in Fr");

        let inputs = vec![
            Operand::Immediate(Fr::from(0x10u64)),
            Operand::Immediate(Fr::from(1u64)),
            Operand::Immediate(Fr::from(1u64)),
            Operand::Immediate(-Fr::from(1u64)),
            Operand::Immediate(Fr::from(preimage.len() as u64)),
            Operand::Immediate(preimage_fr),
        ];
        let memory = HashMap::new();
        let ops: Vec<GatherOp> =
            decode_impact_inputs::<InMemoryDB>(&inputs, &memory).expect("decode");
        assert_eq!(ops.len(), 1);
        match &ops[0] {
            Op::Push {
                value: StateValue::Cell(av),
                ..
            } => {
                assert_eq!(av.value.0[0].0, preimage);
            }
            other => panic!("expected Push Cell, got {other:?}"),
        }
    }

    /// `Popeq` with Compress alignment: gather mode skips past the
    /// preimage operand without resolving it. Must succeed even when
    /// the operand is an unbound variable (the runtime will produce
    /// the result live; the bytes only matter when the SNARK encoder
    /// runs, which is a separate path).
    #[test]
    fn decode_popeq_skips_compress_preimage_without_resolving() {
        let memory: HashMap<Identifier, IrValue> = HashMap::new();
        let inputs = vec![
            Operand::Immediate(Fr::from(0x0cu64)), // Popeq (uncached)
            Operand::Immediate(Fr::from(1u64)),    // alignment_len=1
            Operand::Immediate(-Fr::from(1u64)),   // Compress sentinel
            // Unbound variable in the preimage slot — gather mode
            // should NOT try to dereference it.
            Operand::Variable(Identifier("%public_input.future".to_string())),
        ];
        let ops: Vec<GatherOp> = decode_impact_inputs::<InMemoryDB>(&inputs, &memory)
            .expect("Popeq must skip unresolved Compress preimage");
        assert_eq!(ops.len(), 1);
        match &ops[0] {
            Op::Popeq { cached, result: () } => assert!(!cached),
            other => panic!("expected Popeq, got {other:?}"),
        }
    }

    /// Idx with a single `Key::Value` whose alignment is `Compress`
    /// resolves the Opaque variable's preimage and constructs a
    /// preimage-bearing key AV.
    #[test]
    fn decode_idx_key_value_compress_with_opaque() {
        let id = Identifier("%k".to_string());
        let preimage = b"id-42".to_vec();
        let mut memory: HashMap<Identifier, IrValue> = HashMap::new();
        memory.insert(id.clone(), IrValue::opaque(preimage.clone()));

        // Idx { cached: false, push_path: false, len: 1 }: opcode
        // 0x50 | 0 = 0x50. Then one Key::Value with alignment_len=1,
        // Compress sentinel, %k.
        let inputs = vec![
            Operand::Immediate(Fr::from(0x50u64)),
            Operand::Immediate(Fr::from(1u64)),  // alignment_len for the key's AV
            Operand::Immediate(-Fr::from(1u64)), // Compress sentinel
            Operand::Variable(id),
        ];
        let ops: Vec<GatherOp> =
            decode_impact_inputs::<InMemoryDB>(&inputs, &memory).expect("decode");
        assert_eq!(ops.len(), 1);
        match &ops[0] {
            Op::Idx {
                cached,
                push_path,
                path,
            } => {
                assert!(!cached);
                assert!(!push_path);
                assert_eq!(path.len(), 1);
                let key = path.get(0).expect("one key");
                match &*key {
                    Key::Value(av) => {
                        assert_eq!(av.value.0[0].0, preimage);
                    }
                    other => panic!("expected Key::Value, got {other:?}"),
                }
            }
            other => panic!("expected Idx, got {other:?}"),
        }
    }

    /// End-to-end: the load-bearing wire format from
    /// `compact/examples/compression-test/zkir/two.zkir` line 12,
    /// exercised against an Opaque-bearing memory.
    ///
    /// Wire stream (post-resolution shape):
    ///   `[0x10, 0x01, 0x01, 0x01, 0x01,        // Push Cell Bytes{1} value=1`
    ///   `0x11, 0x01, 0x01, -0x01, %s.0,        // Push (storage) Cell Compress %s.0`
    ///   `0x91]                                 // Ins{cached=false, n=1}`
    ///
    /// `%s.0` is an Opaque variable whose preimage is the user's
    /// string. The decoded sequence must produce three runtime ops
    /// where the second Push carries `Cell(Compress, value=preimage)`.
    #[test]
    fn decode_two_zkir_impact_pattern() {
        let s_id = Identifier("%s.0".to_string());
        let preimage = b"the quick brown fox".to_vec();
        let mut memory: HashMap<Identifier, IrValue> = HashMap::new();
        memory.insert(s_id.clone(), IrValue::opaque(preimage.clone()));

        let inputs = vec![
            // Push Cell Bytes{1} value=1 (kernel slot index)
            Operand::Immediate(Fr::from(0x10u64)),
            Operand::Immediate(Fr::from(1u64)), // Cell tag
            Operand::Immediate(Fr::from(1u64)), // alignment_len
            Operand::Immediate(Fr::from(1u64)), // Bytes{1}
            Operand::Immediate(Fr::from(1u64)), // value
            // Push (storage) Cell Compress %s.0
            Operand::Immediate(Fr::from(0x11u64)),
            Operand::Immediate(Fr::from(1u64)),  // Cell tag
            Operand::Immediate(Fr::from(1u64)),  // alignment_len
            Operand::Immediate(-Fr::from(1u64)), // Compress sentinel
            Operand::Variable(s_id),
            // Ins
            Operand::Immediate(Fr::from(0x91u64)),
        ];

        let ops: Vec<GatherOp> =
            decode_impact_inputs::<InMemoryDB>(&inputs, &memory).expect("decode");
        assert_eq!(ops.len(), 3);

        // First: Push of Cell with Bytes{1} value [1].
        match &ops[0] {
            Op::Push {
                storage: false,
                value: StateValue::Cell(av),
            } => {
                assert_eq!(av.value.0.len(), 1);
                assert_eq!(av.value.0[0].0, vec![1u8]);
            }
            other => panic!("expected Push Cell, got {other:?}"),
        }

        // Second: Push (storage) of Cell with Compress preimage.
        match &ops[1] {
            Op::Push {
                storage: true,
                value: StateValue::Cell(av),
            } => {
                assert!(matches!(
                    av.alignment.0[0],
                    AlignmentSegment::Atom(AlignmentAtom::Compress)
                ));
                assert_eq!(av.value.0[0].0, preimage);
            }
            other => panic!("expected Push (storage) Cell, got {other:?}"),
        }

        // Third: Ins{cached=false, n=1}.
        match &ops[2] {
            Op::Ins {
                cached: false,
                n: 1,
            } => {}
            other => panic!("expected Ins, got {other:?}"),
        }
    }

    /// A single zero-immediate decodes to `Op::Noop { n: 1 }`.
    #[test]
    fn decode_noop_single() {
        let memory = HashMap::new();
        let inputs = imm(&[Fr::from(0x00u64)]);
        let ops: Vec<GatherOp> =
            decode_impact_inputs::<InMemoryDB>(&inputs, &memory).expect("decode");
        assert_eq!(ops.len(), 1);
        match &ops[0] {
            Op::Noop { n } => assert_eq!(*n, 1),
            other => panic!("expected Noop, got {other:?}"),
        }
    }

    /// A run of N zero-immediates decodes to a single
    /// `Op::Noop { n: N }`. This is the gas-cost-faithful inverse of
    /// the forward encoding `Noop { n } → [0x00; n]`.
    #[test]
    fn decode_noop_fuses_run() {
        let memory = HashMap::new();
        let inputs = imm(&[
            Fr::from(0x00u64),
            Fr::from(0x00u64),
            Fr::from(0x00u64),
            Fr::from(0x00u64),
            Fr::from(0x00u64),
        ]);
        let ops: Vec<GatherOp> =
            decode_impact_inputs::<InMemoryDB>(&inputs, &memory).expect("decode");
        assert_eq!(ops.len(), 1, "five zeros must fuse into one Noop");
        match &ops[0] {
            Op::Noop { n } => assert_eq!(*n, 5),
            other => panic!("expected Noop, got {other:?}"),
        }
    }

    /// A run of zero-immediates followed by a different opcode fuses
    /// the zeros and then decodes the trailing opcode as its own Op.
    #[test]
    fn decode_noop_run_stops_at_non_zero() {
        let memory = HashMap::new();
        // Three Noops followed by Lt (0x01).
        let inputs = imm(&[
            Fr::from(0x00u64),
            Fr::from(0x00u64),
            Fr::from(0x00u64),
            Fr::from(0x01u64),
        ]);
        let ops: Vec<GatherOp> =
            decode_impact_inputs::<InMemoryDB>(&inputs, &memory).expect("decode");
        assert_eq!(ops.len(), 2);
        match &ops[0] {
            Op::Noop { n } => assert_eq!(*n, 3),
            other => panic!("expected fused Noop, got {other:?}"),
        }
        assert!(matches!(ops[1], Op::Lt));
    }

    /// A `Variable` operand resolving to zero does *not* participate in
    /// Noop fusion. Compact emits opcodes as literal immediates; a
    /// variable in the opcode slot is structurally different from a
    /// fused-Noop continuation, and folding it silently would conflate
    /// two distinct IR shapes.
    #[test]
    fn decode_noop_fusion_does_not_consume_zero_variable() {
        let id = Identifier("%zero".to_string());
        let mut memory: HashMap<Identifier, IrValue> = HashMap::new();
        memory.insert(id.clone(), IrValue::Native(Fr::from(0u64)));

        let inputs = vec![
            Operand::Immediate(Fr::from(0x00u64)),
            Operand::Variable(id), // Resolves to 0 but must NOT fuse.
        ];
        // The first byte (0x00) decodes as Noop{n:1}. The second
        // operand is then taken as the next opcode — it resolves to
        // 0x00 (via the Variable → IrValue::Native(0) → Fr::from(0)
        // → byte 0 path). On its own, a second `0x00` opcode parses
        // as another Noop. Since fusion is *not* applied across
        // immediate / variable boundaries, this produces TWO Noops.
        let ops: Vec<GatherOp> =
            decode_impact_inputs::<InMemoryDB>(&inputs, &memory).expect("decode");
        assert_eq!(ops.len(), 2, "variable must not fuse with prior immediate");
        match (&ops[0], &ops[1]) {
            (Op::Noop { n: n0 }, Op::Noop { n: n1 }) => {
                assert_eq!(*n0, 1);
                assert_eq!(*n1, 1);
            }
            other => panic!("expected two Noops, got {other:?}"),
        }
    }

    /// Empty input → empty output.
    #[test]
    fn empty_input_yields_empty_output() {
        let memory = HashMap::new();
        let decoded: Vec<GatherOp> =
            decode_impact_inputs::<InMemoryDB>(&[], &memory).expect("empty decode");
        assert!(decoded.is_empty());
    }

    /// Trailing garbage at the end of an input stream surfaces as a
    /// decode error (loop tries to decode another op and fails).
    #[test]
    fn trailing_garbage_errors() {
        let memory: HashMap<Identifier, IrValue> = HashMap::new();
        // Lt (one valid op) followed by an unknown opcode 0xee.
        let inputs = imm(&[Fr::from(0x01u64), Fr::from(0xeeu64)]);
        let err = decode_impact_inputs::<InMemoryDB>(&inputs, &memory)
            .expect_err("trailing garbage must error");
        let msg = format!("{err:#}");
        assert!(
            msg.contains("unknown opcode"),
            "expected unknown-opcode error, got: {msg}"
        );
    }

    /// `<AlignedValue as FieldRepr>::field_repr` of a Compress-bearing
    /// AV computes `transient_commit(preimage, len)`, which equals the
    /// commit `IrValue::opaque` precomputed. This is the consistency
    /// the simplified design relies on: preimage flows through
    /// `decode_impact_inputs` to construct the AV; the commit flows
    /// through `Fr::try_from(IrValue::Opaque)` to populate the SNARK
    /// PI vector; both agree at the field-repr layer.
    #[test]
    fn decoded_av_field_repr_matches_irvalue_commit() {
        let id = Identifier("%s".to_string());
        let preimage = b"check me".to_vec();
        let opaque = IrValue::opaque(preimage.clone());
        let expected_commit = match &opaque {
            IrValue::Opaque { commit, .. } => *commit,
            _ => unreachable!(),
        };
        let mut memory: HashMap<Identifier, IrValue> = HashMap::new();
        memory.insert(id.clone(), opaque);

        let inputs = vec![
            Operand::Immediate(Fr::from(0x10u64)),
            Operand::Immediate(Fr::from(1u64)),
            Operand::Immediate(Fr::from(1u64)),
            Operand::Immediate(-Fr::from(1u64)),
            Operand::Variable(id),
        ];
        let ops: Vec<GatherOp> =
            decode_impact_inputs::<InMemoryDB>(&inputs, &memory).expect("decode");
        let av = match &ops[0] {
            Op::Push {
                value: StateValue::Cell(av),
                ..
            } => av.clone(),
            _ => panic!("expected Push Cell"),
        };

        // field_repr of the Compress AV produces transient_commit,
        // matching the precomputed commit on the IrValue.
        let mut frs: Vec<Fr> = Vec::new();
        // For the Compress segment specifically, only the "value
        // atom" portion of field_repr is the commit. Rather than
        // calling AlignedValue::field_repr (which prepends the
        // alignment metadata), invoke the value-only path through
        // the AV. We can't call value_only_field_repr directly
        // (private), but the same effect is to call field_repr on
        // an AV whose alignment encodes to known bytes and check the
        // tail.
        av.field_repr(&mut frs);
        // alignment.field_repr writes [length=1, sentinel=-1] = 2
        // Frs; the value field_repr writes 1 Fr (the commit).
        assert_eq!(frs.len(), 3);
        assert_eq!(frs[0], Fr::from(1u64));
        assert_eq!(frs[1], -Fr::from(1u64));
        assert_eq!(
            frs[2], expected_commit,
            "AV field_repr Compress commit must equal IrValue::opaque commit"
        );
    }

    /// `Push { value: StateValue::Null }` — exercises the `tag = 0`
    /// discriminator of `decode_state_value` with no payload.
    #[test]
    fn decode_state_value_null() {
        let memory: HashMap<Identifier, IrValue> = HashMap::new();
        let inputs = imm(&[
            Fr::from(0x10u64), // Push opcode
            Fr::from(0u64),    // StateValue::Null tag
        ]);
        let ops: Vec<GatherOp> =
            decode_impact_inputs::<InMemoryDB>(&inputs, &memory).expect("decode");
        assert_eq!(ops.len(), 1);
        match &ops[0] {
            Op::Push {
                storage: false,
                value: StateValue::Null,
            } => {}
            other => panic!("expected Push Null, got {other:?}"),
        }
    }

    /// `Push { value: Array[Cell(field=5), Cell(field=7), Cell(field=42)] }` —
    /// exercises `tag = 3 | (3 << 4) = 0x33` and three recursive Cell
    /// decodes inside the Array arm of `decode_state_value`.
    #[test]
    fn decode_state_value_array_of_cells() {
        let memory: HashMap<Identifier, IrValue> = HashMap::new();
        // Per-cell encoding for Cell(AV{[Field], [v]}) is 4 Frs:
        //   1 (Cell tag) + 2 (alignment: len=1, Field sentinel=-2) + 1 (value)
        let cell = |v: u64| {
            vec![
                Fr::from(1u64),  // Cell tag
                Fr::from(1u64),  // alignment length = 1
                -Fr::from(2u64), // Field sentinel
                Fr::from(v),     // value
            ]
        };
        let mut frs: Vec<Fr> = vec![
            Fr::from(0x10u64), // Push opcode
            Fr::from(0x33u64), // Array tag, len=3
        ];
        frs.extend(cell(5));
        frs.extend(cell(7));
        frs.extend(cell(42));
        let inputs: Vec<Operand> = frs.into_iter().map(Operand::Immediate).collect();

        let ops: Vec<GatherOp> =
            decode_impact_inputs::<InMemoryDB>(&inputs, &memory).expect("decode");
        assert_eq!(ops.len(), 1);
        match &ops[0] {
            Op::Push {
                storage: false,
                value: StateValue::Array(arr),
            } => {
                assert_eq!(arr.len(), 3, "Array must have 3 elements");
                for (i, expected_byte) in [5u8, 7, 42].iter().enumerate() {
                    let elem: &StateValue<InMemoryDB> =
                        arr.get(i).expect("array index in range");
                    match elem {
                        StateValue::Cell(av) => {
                            assert_eq!(av.alignment.0.len(), 1);
                            assert!(matches!(
                                av.alignment.0[0],
                                AlignmentSegment::Atom(AlignmentAtom::Field)
                            ));
                            assert_eq!(av.value.0[0].0, vec![*expected_byte]);
                        }
                        other => panic!("expected Cell at index {i}, got {other:?}"),
                    }
                }
            }
            other => panic!("expected Push Array, got {other:?}"),
        }
    }

    /// `Push { value: Map[(av_0x42, Cell(field=42)), (av_0x99, Cell(field=99))] }`
    /// — exercises `tag = 2 | (2 << 4) = 0x22`, two recursive
    /// `(AlignedValue, StateValue)` decodes, and `StoreHashMap`
    /// insertion. Each Bytes<1> key encodes as 3 Frs:
    /// `[len=1, Bytes{1}=1, value_fr]`.
    #[test]
    fn decode_state_value_map_with_two_entries() {
        let memory: HashMap<Identifier, IrValue> = HashMap::new();
        // Map entries are encoded in sorted order; for AV{[Bytes<1>]}
        // keys with single-byte values, that's byte-ascending. We
        // emit (0x42, ...) first, (0x99, ...) second.
        let key_av = |b: u64| {
            vec![
                Fr::from(1u64), // alignment length = 1
                Fr::from(1u64), // Bytes{length=1}
                Fr::from(b),    // value packed as Fr
            ]
        };
        let cell_field = |v: u64| {
            vec![
                Fr::from(1u64),  // Cell tag
                Fr::from(1u64),  // alignment length = 1
                -Fr::from(2u64), // Field sentinel
                Fr::from(v),     // value
            ]
        };
        let mut frs: Vec<Fr> = vec![
            Fr::from(0x10u64), // Push opcode
            Fr::from(0x22u64), // Map tag, size=2
        ];
        frs.extend(key_av(0x42));
        frs.extend(cell_field(42));
        frs.extend(key_av(0x99));
        frs.extend(cell_field(99));
        let inputs: Vec<Operand> = frs.into_iter().map(Operand::Immediate).collect();

        let ops: Vec<GatherOp> =
            decode_impact_inputs::<InMemoryDB>(&inputs, &memory).expect("decode");
        assert_eq!(ops.len(), 1);
        match &ops[0] {
            Op::Push {
                storage: false,
                value: StateValue::Map(m),
            } => {
                assert_eq!(m.size(), 2, "Map must have 2 entries");
                // Inspect each entry by reconstructing the expected
                // key AV and looking it up.
                let bytes1_av = |b: u8| AlignedValue {
                    alignment: Alignment(vec![AlignmentSegment::Atom(
                        AlignmentAtom::Bytes { length: 1 },
                    )]),
                    value: Value(vec![ValueAtom(vec![b])]),
                };
                let v42 = m
                    .get(&bytes1_av(0x42))
                    .expect("entry with key 0x42 must exist");
                match &*v42 {
                    StateValue::Cell(av) => {
                        assert_eq!(av.value.0[0].0, vec![42u8]);
                    }
                    other => panic!("expected Cell at key 0x42, got {other:?}"),
                }
                let v99 = m
                    .get(&bytes1_av(0x99))
                    .expect("entry with key 0x99 must exist");
                match &*v99 {
                    StateValue::Cell(av) => {
                        assert_eq!(av.value.0[0].0, vec![99u8]);
                    }
                    other => panic!("expected Cell at key 0x99, got {other:?}"),
                }
            }
            other => panic!("expected Push Map, got {other:?}"),
        }
    }

    /// `Push { Array[Map[(0x01, Cell(field=10))], Map[(0x02, Null)]] }`
    /// — multi-level recursion (Array of Maps, with both Cell and
    /// Null as map values). Exercises the StateValue → AlignedValue
    /// → StateValue recursive descent at depth 2.
    #[test]
    fn decode_state_value_nested_array_of_maps() {
        let memory: HashMap<Identifier, IrValue> = HashMap::new();
        let key_av = |b: u64| {
            vec![
                Fr::from(1u64), // alignment length = 1
                Fr::from(1u64), // Bytes{length=1}
                Fr::from(b),
            ]
        };
        let cell_field = |v: u64| {
            vec![
                Fr::from(1u64),
                Fr::from(1u64),
                -Fr::from(2u64),
                Fr::from(v),
            ]
        };
        let mut frs: Vec<Fr> = vec![
            Fr::from(0x10u64), // Push opcode
            Fr::from(0x23u64), // Array tag, len=2
        ];
        // Element 0: Map[(0x01, Cell(field=10))]
        frs.push(Fr::from(0x12u64)); // Map tag, size=1
        frs.extend(key_av(0x01));
        frs.extend(cell_field(10));
        // Element 1: Map[(0x02, Null)]
        frs.push(Fr::from(0x12u64)); // Map tag, size=1
        frs.extend(key_av(0x02));
        frs.push(Fr::from(0u64)); // Null tag
        let inputs: Vec<Operand> = frs.into_iter().map(Operand::Immediate).collect();

        let ops: Vec<GatherOp> =
            decode_impact_inputs::<InMemoryDB>(&inputs, &memory).expect("decode");
        assert_eq!(ops.len(), 1);
        match &ops[0] {
            Op::Push {
                storage: false,
                value: StateValue::Array(arr),
            } => {
                assert_eq!(arr.len(), 2);
                let bytes1_av = |b: u8| AlignedValue {
                    alignment: Alignment(vec![AlignmentSegment::Atom(
                        AlignmentAtom::Bytes { length: 1 },
                    )]),
                    value: Value(vec![ValueAtom(vec![b])]),
                };

                // Element 0: Map[(0x01, Cell(field=10))]
                match arr.get(0).expect("element 0") {
                    StateValue::Map(m) => {
                        assert_eq!(m.size(), 1);
                        let v = m
                            .get(&bytes1_av(0x01))
                            .expect("entry at 0x01 must exist");
                        match &*v {
                            StateValue::Cell(av) => {
                                assert_eq!(av.value.0[0].0, vec![10u8]);
                            }
                            other => panic!("expected Cell, got {other:?}"),
                        }
                    }
                    other => panic!("expected Map at index 0, got {other:?}"),
                }

                // Element 1: Map[(0x02, Null)]
                match arr.get(1).expect("element 1") {
                    StateValue::Map(m) => {
                        assert_eq!(m.size(), 1);
                        let v = m
                            .get(&bytes1_av(0x02))
                            .expect("entry at 0x02 must exist");
                        match &*v {
                            StateValue::Null => {}
                            other => panic!("expected Null, got {other:?}"),
                        }
                    }
                    other => panic!("expected Map at index 1, got {other:?}"),
                }
            }
            other => panic!("expected Push Array, got {other:?}"),
        }
    }

    /// `Push { BoundedMerkleTree { height: 8, entries: [(0, h0), (5, h0)] } }`
    /// — exercises the `tag = 4 | (8 << 4) | (2 << 12) = 0x2084` arm
    /// and the per-entry `(u64, HashOutput)` decode (1 + 2 Frs each).
    /// `h0 = HashOutput([0; 32])` encodes as `[Fr(0), Fr(0)]`.
    #[test]
    fn decode_state_value_bmt_two_entries() {
        let memory: HashMap<Identifier, IrValue> = HashMap::new();
        // Tag layout: 4 (BMT) | (height=8) << 4 | (entries_len=2) << 12
        //           = 4 | 0x80 | 0x2000 = 0x2084.
        let mut frs: Vec<Fr> = vec![
            Fr::from(0x10u64),   // Push opcode
            Fr::from(0x2084u64), // BMT tag
        ];
        // Entry 0: (idx=0, HashOutput([0;32]))
        frs.push(Fr::from(0u64)); // u64 idx
        frs.push(Fr::from(0u64)); // HashOutput repr0
        frs.push(Fr::from(0u64)); // HashOutput repr1
        // Entry 1: (idx=5, HashOutput([0;32]))
        frs.push(Fr::from(5u64));
        frs.push(Fr::from(0u64));
        frs.push(Fr::from(0u64));
        let inputs: Vec<Operand> = frs.into_iter().map(Operand::Immediate).collect();

        let ops: Vec<GatherOp> =
            decode_impact_inputs::<InMemoryDB>(&inputs, &memory).expect("decode");
        assert_eq!(ops.len(), 1);
        match &ops[0] {
            Op::Push {
                storage: false,
                value: StateValue::BoundedMerkleTree(tree),
            } => {
                assert_eq!(tree.height(), 8, "BMT height must be 8");
                let entries: Vec<(u64, HashOutput)> = tree.iter().collect();
                assert_eq!(entries.len(), 2, "BMT must have 2 entries");
                // MerkleTreeIter yields entries in index-ascending
                // order, so this matches the insertion order.
                assert_eq!(entries[0].0, 0);
                assert_eq!(entries[0].1, HashOutput([0u8; 32]));
                assert_eq!(entries[1].0, 5);
                assert_eq!(entries[1].1, HashOutput([0u8; 32]));
            }
            other => panic!("expected Push BoundedMerkleTree, got {other:?}"),
        }
    }

    /// `Push { Cell(AV{[Field, Bytes<5>, Field]}) }` with concrete
    /// values — exercises a multi-segment alignment walked by
    /// `decode_segments_into` (not just a single-atom AV like the
    /// existing tests).
    #[test]
    fn decode_aligned_value_multi_atom() {
        let memory: HashMap<Identifier, IrValue> = HashMap::new();
        // Bytes<5> = [1, 2, 3, 4, 5] packs into a single Fr via
        // Fr::from_le_bytes (since 5 <= FR_BYTES_STORED).
        let bytes_fr = Fr::from_le_bytes(&[1u8, 2, 3, 4, 5])
            .expect("5 bytes fit in Fr");

        let inputs = imm(&[
            Fr::from(0x10u64), // Push opcode
            Fr::from(1u64),    // Cell tag
            Fr::from(3u64),    // alignment length = 3
            -Fr::from(2u64),   // Field sentinel
            Fr::from(5u64),    // Bytes{5}
            -Fr::from(2u64),   // Field sentinel
            Fr::from(1u64),    // value Field = 1
            bytes_fr,          // value Bytes<5> = [1,2,3,4,5]
            Fr::from(42u64),   // value Field = 42
        ]);

        let ops: Vec<GatherOp> =
            decode_impact_inputs::<InMemoryDB>(&inputs, &memory).expect("decode");
        assert_eq!(ops.len(), 1);
        match &ops[0] {
            Op::Push {
                storage: false,
                value: StateValue::Cell(av),
            } => {
                assert_eq!(av.alignment.0.len(), 3);
                assert!(matches!(
                    av.alignment.0[0],
                    AlignmentSegment::Atom(AlignmentAtom::Field)
                ));
                assert!(matches!(
                    av.alignment.0[1],
                    AlignmentSegment::Atom(AlignmentAtom::Bytes { length: 5 })
                ));
                assert!(matches!(
                    av.alignment.0[2],
                    AlignmentSegment::Atom(AlignmentAtom::Field)
                ));
                assert_eq!(av.value.0.len(), 3);
                assert_eq!(av.value.0[0].0, vec![1u8]);
                assert_eq!(av.value.0[1].0, vec![1u8, 2, 3, 4, 5]);
                assert_eq!(av.value.0[2].0, vec![42u8]);
            }
            other => panic!("expected Push Cell, got {other:?}"),
        }
    }

    /// `Push { Cell(AV{[Option[[Field], [Bytes<3>]]]}) }` with variant
    /// 1 (Bytes<3>) chosen — exercises the `AlignmentSegment::Option`
    /// arm of both `decode_alignment_segment` (alignment side) and
    /// `decode_segments_into` (value side), including the
    /// must-be-zero padding check.
    ///
    /// Both options have `field_len = 1` ([Field] = 1 Fr, [Bytes<3>] =
    /// `ceil(3/31) = 1` Fr), so no padding is required.
    #[test]
    fn decode_aligned_value_option_segment() {
        let memory: HashMap<Identifier, IrValue> = HashMap::new();
        let bytes_fr = Fr::from_le_bytes(&[9u8, 8, 7])
            .expect("3 bytes fit in Fr");

        let inputs = imm(&[
            Fr::from(0x10u64), // Push opcode
            Fr::from(1u64),    // Cell tag
            // Alignment: one Option segment containing two options.
            Fr::from(1u64),    // alignment length = 1
            -Fr::from(3u64),   // Option sentinel
            Fr::from(2u64),    // option count = 2
            // Option 0: [Field]
            Fr::from(1u64),    // sub-alignment length = 1
            -Fr::from(2u64),   // Field sentinel
            // Option 1: [Bytes<3>]
            Fr::from(1u64),    // sub-alignment length = 1
            Fr::from(3u64),    // Bytes{3}
            // Value: variant 1 (Bytes<3>) chosen.
            Fr::from(1u64),    // variant tag = 1
            bytes_fr,          // Bytes<3> = [9, 8, 7]
            // No padding (max_field_len 1 - chosen.field_len 1 = 0).
        ]);

        let ops: Vec<GatherOp> =
            decode_impact_inputs::<InMemoryDB>(&inputs, &memory).expect("decode");
        assert_eq!(ops.len(), 1);
        match &ops[0] {
            Op::Push {
                storage: false,
                value: StateValue::Cell(av),
            } => {
                assert_eq!(av.alignment.0.len(), 1);
                match &av.alignment.0[0] {
                    AlignmentSegment::Option(options) => {
                        assert_eq!(options.len(), 2);
                        assert_eq!(
                            options[0],
                            Alignment(vec![AlignmentSegment::Atom(
                                AlignmentAtom::Field
                            )])
                        );
                        assert_eq!(
                            options[1],
                            Alignment(vec![AlignmentSegment::Atom(
                                AlignmentAtom::Bytes { length: 3 }
                            )])
                        );
                    }
                    other => panic!("expected Option segment, got {other:?}"),
                }
                // Value has TWO entries:
                //   [0] variant tag = 1 → ValueAtom(vec![1])
                //   [1] Bytes<3> = [9, 8, 7] → ValueAtom(vec![9, 8, 7])
                assert_eq!(av.value.0.len(), 2);
                assert_eq!(av.value.0[0].0, vec![1u8]);
                assert_eq!(av.value.0[1].0, vec![9u8, 8, 7]);
            }
            other => panic!("expected Push Cell, got {other:?}"),
        }
    }
}

// ===========================================================================
// Property-based tests
// ===========================================================================
//
// The decoder satisfies three properties worth testing with random Op
// sequences. Each one exercises a slightly different slice of the
// behaviour:
//
//   1. `roundtrip_immediate_form` — *byte-level idempotency* of the
//      encode/decode pair. For any sequence of "decoder-safe" verify-
//      mode ops, encoding via `FieldRepr` to a stream of `Fr`s, wrapping
//      each as `Operand::Immediate`, decoding to gather-mode ops,
//      lifting back to verify-mode, and re-encoding produces a
//      bytewise-equal field representation. This is the correct round-
//      trip property because the wire format is *lossy* for non-
//      canonical inputs:
//        * `Fr::from_uniform_bytes` reduces Field atoms mod the field
//          prime (≈ 2^254), so a 64-byte Field `ValueAtom` becomes ≤32
//          bytes after one decode.
//        * The decoder pads `Bytes<N>` atoms to exactly `N` bytes,
//          whereas upstream `AlignedValue` normalization strips
//          trailing zeros from `ValueAtom.0`.
//      Hence `encode(decode(x))` is not necessarily byte-equal to
//      `encode(x)`, but once a sequence has been through one
//      encode-decode cycle it is in canonical form and is a fixed
//      point of subsequent cycles:
//          encode(decode(encode(x))) == encode(x) once x is canonical.
//      We assert this stronger invariant: `frs2 == frs1` where
//      `frs1 = encode(ops)` and `frs2 = encode(lift(decode(frs1)))`.
//
//   2. `substitution_invariance` — substituting an arbitrary subset of
//      operand positions with `Operand::Variable(id)` (binding `id` to
//      `IrValue::Native(fr)` in memory) does not change the decoded
//      result. The decoder must treat variable references that resolve
//      to the same `Fr` identically to literal immediates. The
//      property compares two *decoded* sequences (the immediate-only
//      decode and the partially-substituted decode), so wire-format
//      lossiness applies equally to both sides and cancels out. Noop
//      ops are filtered out because fusion is sensitive to the
//      Variable/Immediate distinction (a Variable terminates a Noop
//      run; see the comment on the Noop arm of `decode_op`), so the
//      property is well-defined only on non-Noop ops.
//
//   3. `noop_fusion_canonical` — the decoded output has no two
//      adjacent `Op::Noop` ops (fusion is canonical), and the sum of
//      `n` across all `Op::Noop`s in the decoded output equals the sum
//      across the input (no Noop steps are lost or gained).
//
// "Decoder-safe" excludes ops that the wire format can't faithfully
// round-trip:
//   * any `Op` carrying an `AlignedValue` with a `Compress` atom — the
//     forward encoding is one-way (`transient_commit(bytes, len)`), so
//     the operand layer (Variable→Opaque memory, or literal
//     `[byte_len, preimage_frs]`) is required to recover the preimage.
//     The existing hand-rolled tests in `mod tests` above cover that
//     case; the proptest filters it out.
//   * `Op::Popeq` is additionally filtered inside the round-trip test
//     (but not by `op_is_decoder_safe`, which is used by all three
//     properties): Popeq's verify-mode `result: AlignedValue` payload
//     is part of the wire encoding but is discarded by the decoder
//     (which produces gather-mode `Op`s with unit results). Lifting a
//     gather-mode `Popeq` back to verify-mode would require
//     fabricating that payload. The hand-rolled tests cover Popeq
//     directly.
//
// The proptest-derive `Arbitrary` impl on `Op<ResultModeVerify, _>`
// (see `onchain-vm/src/ops.rs`) already constrains:
//   * `Idx.path` to exactly one key (via `vec(Key::arbitrary(), 1..2)`);
//   * `Ins.n` to a non-zero value in `[1, 15]`;
//   * `Noop.n`, `Branch.skip`, etc. to bounded ranges.
//
// so the filter only needs to deal with Compress (plus the round-trip-
// specific Popeq filter).

#[cfg(all(test, feature = "proptest"))]
mod proptests {
    use super::*;
    use onchain_vm::result_mode::{ResultModeGather, ResultModeVerify};
    use proptest::collection::vec as prop_vec;
    use proptest::prelude::*;
    use storage::db::InMemoryDB;
    use transient_crypto::repr::FieldRepr;

    type VerifyOp = Op<ResultModeVerify, InMemoryDB>;
    type GatherOp = Op<ResultModeGather, InMemoryDB>;

    // -----------------------------------------------------------------------
    // Compress-atom detection (recursive walk over alignments + state values)
    // -----------------------------------------------------------------------

    fn alignment_contains_compress(a: &Alignment) -> bool {
        a.0.iter().any(segment_contains_compress)
    }

    fn segment_contains_compress(seg: &AlignmentSegment) -> bool {
        match seg {
            AlignmentSegment::Atom(AlignmentAtom::Compress) => true,
            AlignmentSegment::Atom(_) => false,
            AlignmentSegment::Option(options) => options.iter().any(alignment_contains_compress),
        }
    }

    fn av_contains_compress(av: &AlignedValue) -> bool {
        alignment_contains_compress(&av.alignment)
    }

    fn key_contains_compress(k: &Key) -> bool {
        match k {
            Key::Stack => false,
            Key::Value(av) => av_contains_compress(av),
        }
    }

    fn state_value_contains_compress(sv: &StateValue<InMemoryDB>) -> bool {
        match sv {
            StateValue::Null => false,
            // av: &Sp<AlignedValue, _>; reach &AlignedValue via double deref.
            StateValue::Cell(av) => av_contains_compress(&**av),
            // Map yields Sp<(Sp<K>, Sp<V>), _> per entry; access the
            // inner tuple via auto-deref-field-access, then deref each
            // Sp<_> to its target.
            StateValue::Map(m) => m.iter().any(|kv| {
                av_contains_compress(&*kv.0) || state_value_contains_compress(&*kv.1)
            }),
            // Array yields Sp<StateValue, _> per element.
            StateValue::Array(arr) => arr.iter().any(|s| state_value_contains_compress(&*s)),
            // BoundedMerkleTree leaves are `(u64, HashOutput)` — no AVs.
            StateValue::BoundedMerkleTree(_) => false,
            // `StateValue` is `#[non_exhaustive]`; default unknown
            // variants to "decoder-safe = true" (no Compress) so the
            // proptest filter doesn't accidentally reject everything
            // future variants might add. The decoder itself will fail
            // loudly on any unknown StateValue tag at runtime.
            _ => false,
        }
    }

    /// Filter: is this op losslessly round-trippable through the
    /// immediate-form decoder? Excludes only ops carrying Compress
    /// atoms; the proptest Arbitrary impl already enforces Idx.path
    /// length = 1 and Ins.n in [1, 15], so we don't need to filter
    /// those.
    fn op_is_decoder_safe(op: &VerifyOp) -> bool {
        use Op::*;
        match op {
            Push { value, .. } => !state_value_contains_compress(value),
            Popeq { result: av, .. } => !av_contains_compress(av),
            // Array iter yields Sp<Key, _>; deref to &Key for the
            // recursive check.
            Idx { path, .. } => path.iter().all(|k| !key_contains_compress(&*k)),
            _ => true,
        }
    }

    /// Flatten a `Vec<VerifyOp>` to its concatenated `FieldRepr`. Used
    /// on both sides of the byte-level idempotency check.
    fn field_repr_of(ops: &[VerifyOp]) -> Vec<Fr> {
        let mut frs = Vec::new();
        for op in ops {
            op.field_repr(&mut frs);
        }
        frs
    }

    proptest! {
        /// Byte-level idempotency of the encode/decode pair.
        ///
        /// The wire format is *lossy* for non-canonical inputs:
        /// `Fr::from_uniform_bytes` reduces Field atoms mod the field
        /// prime, and the decoder pads `Bytes<N>` atoms to exactly `N`
        /// bytes (overriding upstream normalization that strips
        /// trailing zeros). Op-level equality after one round-trip is
        /// therefore not a property of the decoder. The correct
        /// property is that re-encoding the decoded form reproduces
        /// the same field representation: the canonical image of one
        /// encode-decode cycle is a fixed point of the next.
        ///
        /// `Op::Popeq` is filtered here (not by `op_is_decoder_safe`,
        /// which the other properties also depend on) because lifting
        /// a gather-mode `Popeq` back to verify-mode would require
        /// fabricating its `AlignedValue` read-result payload.
        #[test]
        fn roundtrip_immediate_form(
            raw_ops in prop_vec(any::<VerifyOp>(), 0..16)
        ) {
            let ops: Vec<VerifyOp> = raw_ops
                .into_iter()
                .filter(op_is_decoder_safe)
                .filter(|o| !matches!(o, Op::Popeq { .. }))
                .collect();

            let frs1 = field_repr_of(&ops);
            let operands: Vec<Operand> =
                frs1.iter().cloned().map(Operand::Immediate).collect();
            let memory: HashMap<Identifier, IrValue> = HashMap::new();
            let decoded: Vec<GatherOp> =
                decode_impact_inputs::<InMemoryDB>(&operands, &memory)
                    .expect("decode must succeed for well-formed input");

            // Lift gather → verify. Popeq is filtered above, so the
            // `()` → `AlignedValue` closure is unreachable.
            let lifted: Vec<VerifyOp> = decoded
                .into_iter()
                .map(|op| op.translate(|_| unreachable!("Popeq filtered out above")))
                .collect();
            let frs2 = field_repr_of(&lifted);

            prop_assert_eq!(frs1, frs2);
        }

        /// Substituting an arbitrary subset of operand positions with
        /// `Operand::Variable(id)` (binding `id` to `IrValue::Native(fr)`
        /// in memory) does not change the decoded result.
        ///
        /// The property compares two *decoded* sequences (the
        /// immediate-only decode and the partially-substituted
        /// decode), not the substituted decode against the
        /// pre-encoding input — wire-format lossiness applies equally
        /// to both sides and cancels out.
        ///
        /// Noop ops are filtered out at generation time because
        /// fusion is sensitive to the Variable/Immediate distinction
        /// (see the Noop arm of `decode_op`).
        #[test]
        fn substitution_invariance(
            raw_ops in prop_vec(any::<VerifyOp>(), 0..16),
            substitute_mask in prop_vec(any::<bool>(), 0..1024usize),
        ) {
            let ops: Vec<VerifyOp> = raw_ops
                .into_iter()
                .filter(op_is_decoder_safe)
                .filter(|o| !matches!(o, Op::Noop { .. }))
                .collect();
            let frs = field_repr_of(&ops);

            // Pure-immediate decode: the baseline.
            let imm_operands: Vec<Operand> =
                frs.iter().cloned().map(Operand::Immediate).collect();
            let decoded_imm =
                decode_impact_inputs::<InMemoryDB>(&imm_operands, &HashMap::new())
                    .expect("immediate decode must succeed");

            // Partially-substituted decode: must produce the same
            // gather-mode op sequence as the pure-immediate decode.
            let mut memory: HashMap<Identifier, IrValue> = HashMap::new();
            let subst_operands: Vec<Operand> = frs
                .iter()
                .enumerate()
                .map(|(i, fr)| {
                    let should_subst =
                        substitute_mask.get(i).copied().unwrap_or(false);
                    if should_subst {
                        let id = Identifier(format!("%var_{}", i));
                        memory.insert(id.clone(), IrValue::Native(*fr));
                        Operand::Variable(id)
                    } else {
                        Operand::Immediate(*fr)
                    }
                })
                .collect();
            let decoded_subst =
                decode_impact_inputs::<InMemoryDB>(&subst_operands, &memory)
                    .expect("substitution decode must succeed");

            prop_assert_eq!(decoded_imm, decoded_subst);
        }

        /// The decoded output is in fused / canonical form: no two
        /// adjacent `Op::Noop`s, and the total Noop `n` count matches
        /// the input.
        #[test]
        fn noop_fusion_canonical(
            raw_ops in prop_vec(any::<VerifyOp>(), 0..16)
        ) {
            let ops: Vec<VerifyOp> = raw_ops
                .into_iter()
                .filter(op_is_decoder_safe)
                .collect();
            let frs = field_repr_of(&ops);
            let operands: Vec<Operand> = frs.into_iter().map(Operand::Immediate).collect();
            let memory: HashMap<Identifier, IrValue> = HashMap::new();
            let decoded = decode_impact_inputs::<InMemoryDB>(&operands, &memory)
                .expect("decode must succeed");

            // No two adjacent Noops in the decoded form.
            for w in decoded.windows(2) {
                prop_assert!(
                    !matches!((&w[0], &w[1]), (Op::Noop { .. }, Op::Noop { .. })),
                    "decoded form must have only fused Noops"
                );
            }

            // Total Noop "n" count is preserved across encode + decode.
            let input_total: u64 = ops.iter().filter_map(|o|
                if let Op::Noop { n } = o { Some(*n as u64) } else { None }
            ).sum();
            let decoded_total: u64 = decoded.iter().filter_map(|o|
                if let Op::Noop { n } = o { Some(*n as u64) } else { None }
            ).sum();
            prop_assert_eq!(input_total, decoded_total);
        }
    }
}
