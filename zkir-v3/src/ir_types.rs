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

use midnight_circuits::types::{AssignedNative, AssignedNativePoint, AssignedScalarOfNativeCurve};
use midnight_curves::{Fr as JubjubFr, JubjubExtended, JubjubSubgroup};
use midnight_proofs::plonk::Error;
#[cfg(feature = "proptest")]
use proptest_derive::Arbitrary;
use serde::{Deserialize, Serialize};
use serialize::{Deserializable, Serializable, Tagged};
use transient_crypto::curve::{Fr, outer};
use transient_crypto::hash::transient_commit;

type F = outer::Scalar;

/// Type of IR values
#[cfg_attr(feature = "proptest", derive(Arbitrary))]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Serializable)]
#[tag = "ir-type[v1]"]
pub enum IrType {
    /// Element of the BLS12-381 scalar field, a.k.a. the native field.
    /// This is also the base field of Jubjub.
    #[serde(rename = "Scalar<BLS12-381>")]
    Native,

    /// Point of the Jubjub elliptic curve.
    #[serde(rename = "Point<Jubjub>")]
    JubjubPoint,

    /// Element of the scalar field of Jubjub.
    #[serde(rename = "Scalar<Jubjub>")]
    JubjubScalar,

    /// Variable-length opaque preimage.
    ///
    /// `IrType::Opaque` appears in three places, all wired through the
    /// same preimage-bearing wire format:
    ///
    ///   * **`IrSource.inputs`** — the circuit input vector. Each
    ///     declared `Opaque` input slot reads
    ///     `[byte_len, fr_0, ..., fr_{N-1}]` from `preimage.inputs`
    ///     where `N = ceil(byte_len / FR_BYTES_STORED)` and the
    ///     preimage is packed via the same chunk-and-reverse layout
    ///     `AlignmentAtom::Bytes { length: byte_len }` uses. The
    ///     WASM-side bridge (`flatten_av_with_opaque_preimages` in
    ///     `zkir-v3-wasm/src/lib.rs`) produces this layout from the
    ///     user-supplied AlignedValue.
    ///
    ///   * **`I::PublicInput`** — bound from
    ///     `preimage.public_transcript_outputs`. The WASM bridge
    ///     flattens each `Op::Popeq { result, .. }` AV through the
    ///     same helper, so a Compress-aligned popeq result (i.e. a
    ///     ledger read of an `Opaque<...>` cell) lands here in
    ///     `[byte_len, ...]` form. The `IrSource::preprocess`
    ///     `I::PublicInput` arm calls
    ///     [`crate::ir_vm::transcript_slot_width`] to size the slice.
    ///
    ///   * **`I::PrivateInput`** — bound from
    ///     `preimage.private_transcript`. Same shape as above for
    ///     witness functions returning `Opaque<...>`.
    ///
    /// At decode time `decode_offcircuit` materializes an
    /// `IrValue::Opaque { bytes, commit }` whose `commit` is the
    /// `transient_commit(bytes, byte_len)` projection. The off-circuit
    /// preprocess pipeline and the in-circuit synthesis treat the
    /// variable as a single-Fr value whose `Fr::try_from` returns
    /// `commit` — meaning operands carrying Opaque values contribute
    /// the cached commit Fr to the byte-flat impact stream's
    /// `pis`/`public_transcript_inputs` exactly as if they were
    /// `IrValue::Native(commit_fr)`. The preimage `bytes` survive only
    /// at the IR-side memory layer for downstream consumers that
    /// genuinely need the bytes (e.g. building runtime-VM `AlignedValue`s
    /// from Compress preimages); the SNARK side never sees them.
    #[serde(rename = "Opaque")]
    Opaque,
}

impl IrType {
    /// Number of raw `Fr` elements needed to represent a value of this type.
    ///
    /// Panics for `IrType::Opaque`, which is variable-length at runtime —
    /// callers that need to slice an Opaque from an `Fr` stream must read
    /// the leading `byte_len` Fr first and consume `1 + ceil(byte_len /
    /// FR_BYTES_STORED)` Frs total. See the per-type slicer in
    /// `IrSource::preprocess`'s input loop for the canonical pattern.
    pub fn encoded_len(&self) -> usize {
        match self {
            IrType::Native => 1,
            IrType::JubjubPoint => 2,
            IrType::JubjubScalar => 1,
            IrType::Opaque => panic!(
                "IrType::Opaque has variable encoded length; callers must \
                 read byte_len explicitly. See IrSource::preprocess for the \
                 canonical pattern."
            ),
        }
    }
}

/// Off-circuit IR value carrying actual data.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IrValue {
    /// BLS12-381 scalar field element.
    Native(Fr),

    /// Jubjub point.
    JubjubPoint(JubjubSubgroup),

    /// Jubjub scalar field value.
    JubjubScalar(JubjubFr),

    /// Opaque preimage carried alongside its `transient_commit` commitment.
    /// The `commit` field is `transient_commit(&bytes,
    /// Fr::from(bytes.len() as u64))`, computed once at construction time
    /// (see [`IrValue::opaque`]). Subsequent `Fr::try_from(IrValue::Opaque
    /// { commit, .. })` returns `commit` transparently, which is what the
    /// off-circuit preprocess and in-circuit synthesis Impact arms read
    /// for Compress-slot operands. The `bytes` field is preserved for
    /// downstream IR-level consumers that need the preimage (e.g. when
    /// reconstructing runtime-VM `AlignedValue`s for Compress slots);
    /// the proving side itself only ever consumes `commit`.
    Opaque { bytes: Vec<u8>, commit: Fr },
}

impl IrValue {
    /// Construct an `IrValue::Opaque` from a preimage byte sequence,
    /// computing the commitment once. This is the only public constructor
    /// for the Opaque variant; we don't accept `From<Vec<u8>>` because the
    /// commitment must be derived alongside `bytes`.
    ///
    /// The empty-preimage case (`bytes.is_empty()`) is special-cased to
    /// produce `commit == Fr::from(0u64)`, matching the runtime's
    /// `<ValueAtom as ValueAtomExt>::field_repr_unchecked` Compress arm
    /// at `transient_crypto/src/fab.rs` ("Special case for the
    /// empty string to make defaults work well"). This keeps
    /// `IrValue::default(&IrType::Opaque)` and the runtime's
    /// `<AlignedValue as FieldRepr>::field_repr` of an empty Compress AV
    /// in agreement on the same Fr.
    pub fn opaque(bytes: Vec<u8>) -> Self {
        let commit = if bytes.is_empty() {
            Fr::from(0u64)
        } else {
            let len_fr: Fr = (bytes.len() as u64).into();
            transient_commit(&bytes[..], len_fr)
        };
        IrValue::Opaque { bytes, commit }
    }

    pub(crate) fn get_type(&self) -> IrType {
        match self {
            IrValue::Native(_) => IrType::Native,
            IrValue::JubjubPoint(_) => IrType::JubjubPoint,
            IrValue::JubjubScalar(_) => IrType::JubjubScalar,
            IrValue::Opaque { .. } => IrType::Opaque,
        }
    }

    pub(crate) fn default(val_t: &IrType) -> Self {
        match val_t {
            IrType::Native => IrValue::Native(Fr::default()),
            IrType::JubjubPoint => IrValue::JubjubPoint(JubjubSubgroup::default()),
            IrType::JubjubScalar => IrValue::JubjubScalar(JubjubFr::default()),
            // Empty preimage; commit matches the runtime
            // `<AlignedValue as FieldRepr>::field_repr` Compress arm's
            // empty-AV special case.
            IrType::Opaque => IrValue::opaque(Vec::new()),
        }
    }
}

/// In-circuit IR value, this is a placeholder for an [IrValue], a circuit
/// variable that does not necessarily carry actual data (it will carry data
/// during the proving process, but not during the circuit compilation)
#[derive(Clone, Debug)]
pub enum CircuitValue {
    Native(AssignedNative<F>),
    JubjubPoint(AssignedNativePoint<JubjubExtended>),
    JubjubScalar(AssignedScalarOfNativeCurve<JubjubExtended>),
    /// In-circuit projection of an `IrValue::Opaque`. We carry only the
    /// `commit` assigned-Fr because the preimage has no role inside the
    /// circuit — the commit was computed off-circuit at preprocess
    /// time (see `IrValue::opaque`) and is witnessed directly here. From
    /// the perspective of the existing Impact arm in `IrSource::circuit`,
    /// `try_into::<AssignedNative>()` returns this `commit`, making
    /// Opaque indistinguishable from Native in-circuit.
    Opaque {
        commit: AssignedNative<F>,
    },
}

impl CircuitValue {
    pub fn get_type(&self) -> IrType {
        match self {
            CircuitValue::Native(_) => IrType::Native,
            CircuitValue::JubjubPoint(_) => IrType::JubjubPoint,
            CircuitValue::JubjubScalar(_) => IrType::JubjubScalar,
            CircuitValue::Opaque { .. } => IrType::Opaque,
        }
    }
}

/// Implements both `From<T> for Enum` (wrap) and `TryFrom<Enum> for T` (unwrap)
/// for the specified enum variants.
macro_rules! impl_enum_from_try_from {
    ($enum:ident, $error:ty, $error_constructor:expr; $($variant:ident => $t:ty),* $(,)? ) => {
        $(
            // Wrap: From<T> -> Enum
            impl From<$t> for $enum {
                fn from(value: $t) -> Self {
                    $enum::$variant(value)
                }
            }

            // Unwrap: TryFrom<Enum> -> T
            impl std::convert::TryFrom<$enum> for $t {
                type Error = $error;

                fn try_from(value: $enum) -> Result<Self, Self::Error> {
                    match &value {
                        $enum::$variant(inner) => Ok(inner.clone()),
                        other => Err($error_constructor(
                            format!("cannot convert {:?} to {:?}",
                                     other.get_type(), stringify!($variant)),
                            )
                        ),
                    }
                }
            }
        )*
    };
}

// Derives implementations for non-Native variants. We don't use the macro
// for `Native => Fr` because we want `Fr::try_from(IrValue)` to succeed for
// both `Native` (returning the inner Fr) and `Opaque` (returning the
// precomputed commit) — the macro can only produce one impl per
// (variant, type) pair, so the Native + Opaque dispatch is written manually
// below.
impl_enum_from_try_from!(IrValue, anyhow::Error, anyhow::Error::msg;
    JubjubPoint => JubjubSubgroup,
    JubjubScalar => JubjubFr,
);

// Manual impl: `From<Fr> for IrValue` constructs the `Native` variant. The
// `Opaque` variant has no `From<Vec<u8>>` (`IrValue::opaque` is the only
// constructor) because the commit must be derived alongside the bytes.
impl From<Fr> for IrValue {
    fn from(value: Fr) -> Self {
        IrValue::Native(value)
    }
}

// Manual impl: `TryFrom<IrValue> for Fr` succeeds for both `Native` and
// `Opaque`. For `Opaque`, the returned Fr is the precomputed commit. This
// makes Opaque values transparent at the operand-resolution layer
// (preprocess and in-circuit Impact arms call `try_into::<Fr>()` on the
// resolved value and get the commit for free).
impl TryFrom<IrValue> for Fr {
    type Error = anyhow::Error;
    fn try_from(value: IrValue) -> Result<Self, Self::Error> {
        match value {
            IrValue::Native(fr) => Ok(fr),
            IrValue::Opaque { commit, .. } => Ok(commit),
            other => Err(anyhow::Error::msg(format!(
                "cannot convert {:?} to Native",
                other.get_type()
            ))),
        }
    }
}

// `TryFrom<IrValue> for Vec<u8>` — extract the preimage bytes from an
// `Opaque` value. Unlike `Fr::try_from`, this is *not* a transparent
// fallback: only the Opaque variant produces a preimage, so non-Opaque
// inputs error.
impl TryFrom<IrValue> for Vec<u8> {
    type Error = anyhow::Error;
    fn try_from(value: IrValue) -> Result<Self, Self::Error> {
        match value {
            IrValue::Opaque { bytes, .. } => Ok(bytes),
            other => Err(anyhow::Error::msg(format!(
                "cannot convert {:?} to Opaque preimage bytes",
                other.get_type()
            ))),
        }
    }
}

// Same shape as IrValue, but for CircuitValue. The macro handles the
// non-Native variants; `From<AssignedNative<F>>` for `Native` and
// `TryFrom<CircuitValue> for AssignedNative<F>` (handling Native + Opaque)
// are written manually.
impl_enum_from_try_from!(CircuitValue, Error, Error::Synthesis;
    JubjubPoint => AssignedNativePoint<JubjubExtended>,
    JubjubScalar => AssignedScalarOfNativeCurve<JubjubExtended>,
);

impl From<AssignedNative<F>> for CircuitValue {
    fn from(value: AssignedNative<F>) -> Self {
        CircuitValue::Native(value)
    }
}

impl TryFrom<CircuitValue> for AssignedNative<F> {
    type Error = Error;
    fn try_from(value: CircuitValue) -> Result<Self, Self::Error> {
        match value {
            CircuitValue::Native(x) => Ok(x),
            CircuitValue::Opaque { commit } => Ok(commit),
            other => Err(Error::Synthesis(format!(
                "cannot convert {:?} to Native",
                other.get_type()
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `IrValue::opaque(bytes)` materializes the precomputed `commit` field
    /// alongside the preimage. For non-empty preimages the commit is
    /// `transient_commit(&bytes, len)`; for the empty case it's
    /// `Fr::from(0u64)` to match the runtime's `field_repr_unchecked`
    /// special case.
    #[test]
    fn opaque_constructor_caches_commit() {
        let preimage = b"hello world".to_vec();
        let v = IrValue::opaque(preimage.clone());
        match v {
            IrValue::Opaque { ref bytes, commit } => {
                assert_eq!(bytes, &preimage);
                let expected: Fr = transient_commit(&preimage[..], (preimage.len() as u64).into());
                assert_eq!(commit, expected, "commit must equal transient_commit");
            }
            _ => panic!("opaque() did not return Opaque variant"),
        }
    }

    #[test]
    fn opaque_empty_commit_is_zero() {
        // Matches the `if self.0.is_empty() { writer.write(&[0.into()]) }`
        // arm at transient_crypto/src/fab.rs:489-491.
        let v = IrValue::opaque(Vec::new());
        match v {
            IrValue::Opaque { ref bytes, commit } => {
                assert!(bytes.is_empty());
                assert_eq!(commit, Fr::from(0u64));
            }
            _ => panic!("opaque(empty) did not return Opaque variant"),
        }
    }

    /// `Fr::try_from(IrValue::Opaque { .. })` returns the precomputed
    /// commit. This is the load-bearing transparency that lets the
    /// existing preprocess and in-circuit `I::Impact` arms keep working
    /// unchanged when an operand resolves to an Opaque variable.
    #[test]
    fn fr_try_from_opaque_returns_commit() {
        let v = IrValue::opaque(b"hello".to_vec());
        let expected_commit = match &v {
            IrValue::Opaque { commit, .. } => *commit,
            _ => unreachable!(),
        };
        let fr: Fr = v.try_into().expect("Opaque must convert to Fr (commit)");
        assert_eq!(fr, expected_commit);
    }

    /// `Fr::try_from(IrValue::Native(_))` still works.
    #[test]
    fn fr_try_from_native_unchanged() {
        let v = IrValue::Native(Fr::from(42u64));
        let fr: Fr = v.try_into().expect("Native -> Fr must succeed");
        assert_eq!(fr, Fr::from(42u64));
    }

    /// `Fr::try_from` errors for JubjubPoint/JubjubScalar — unchanged from
    /// the previous (macro-generated) behavior.
    #[test]
    fn fr_try_from_jubjub_point_errors() {
        let v = IrValue::JubjubPoint(JubjubSubgroup::default());
        let err = Fr::try_from(v).expect_err("JubjubPoint must NOT convert to Fr");
        let msg = err.to_string();
        assert!(
            msg.contains("cannot convert") && msg.contains("Native"),
            "expected cannot-convert-to-Native error, got: {msg}"
        );
    }

    /// `Vec<u8>::try_from(IrValue::Opaque { bytes, .. })` returns the preimage bytes.
    #[test]
    fn vec_u8_try_from_opaque_returns_bytes() {
        let v = IrValue::opaque(b"hello".to_vec());
        let bytes: Vec<u8> = v.try_into().expect("Opaque -> Vec<u8> must succeed");
        assert_eq!(bytes, b"hello".to_vec());
    }

    /// Non-Opaque `IrValue` rejects `Vec<u8>::try_from`. Only Opaque values carry a preimage.
    #[test]
    fn vec_u8_try_from_native_errors() {
        let v = IrValue::Native(Fr::from(42u64));
        let err = Vec::<u8>::try_from(v).expect_err("Native must NOT convert to Vec<u8>");
        let msg = err.to_string();
        assert!(
            msg.contains("cannot convert") && msg.contains("Opaque"),
            "expected cannot-convert-to-Opaque error, got: {msg}"
        );
    }

    /// `IrValue::default(&IrType::Opaque)` produces an empty preimage with
    /// commit `0`, in line with the runtime's empty-AV special case.
    #[test]
    fn default_opaque_is_empty_with_zero_commit() {
        let v = IrValue::default(&IrType::Opaque);
        match v {
            IrValue::Opaque { ref bytes, commit } => {
                assert!(bytes.is_empty());
                assert_eq!(commit, Fr::from(0u64));
            }
            _ => panic!("default(opaque) must yield Opaque variant"),
        }
    }

    /// `IrValue::Opaque { .. }.get_type()` is `IrType::Opaque`.
    #[test]
    fn get_type_opaque() {
        let v = IrValue::opaque(b"x".to_vec());
        assert_eq!(v.get_type(), IrType::Opaque);
    }

    /// `IrType::Opaque.encoded_len()` panics (callers must read byte_len
    /// dynamically; see IrSource::preprocess for the canonical pattern).
    #[test]
    #[should_panic(expected = "IrType::Opaque has variable encoded length")]
    fn encoded_len_opaque_panics() {
        let _ = IrType::Opaque.encoded_len();
    }
}
