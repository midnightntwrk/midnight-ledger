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

#![allow(dead_code)]
use std::borrow::Cow;

use base_crypto::fab::{
    AlignedValue, Alignment, AlignmentAtom, AlignmentSegment, Value, ValueAtom,
};
use hex::FromHex;
use js_sys::{BigInt, Function, JsString, Promise, Uint8Array};
use onchain_runtime::ops::Op;
use onchain_runtime::result_mode::ResultModeVerify;
use rand::rngs::OsRng;
use serde_wasm_bindgen::from_value;
use serialize::{tagged_deserialize, tagged_serialize};
use storage::db::InMemoryDB;
use transient_crypto::curve::FR_BYTES_STORED;
use transient_crypto::fab::AlignedValueExt;
use transient_crypto::proofs::Zkir as ZkirTrait;
use transient_crypto::repr::FieldRepr;
use transient_crypto::{
    curve::Fr,
    proofs::{
        KeyLocation, ParamsProver, ParamsProverProvider, ProofPreimage, ProvingKeyMaterial,
        Resolver,
    },
};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

/// Sanity check on a deserialized op vector — round-trip through tagged
/// serialization to make sure the JS payload is well-formed before we
/// hash it into the preimage. Same shape as
/// `onchain_runtime_wasm::ensure_ops_valid`.
fn ensure_ops_valid<M: onchain_runtime::result_mode::ResultMode<InMemoryDB>>(
    ops: &[Op<M, InMemoryDB>],
) -> Result<(), JsError>
where
    Op<M, InMemoryDB>: Eq,
{
    for op in ops.iter() {
        let mut ser = Vec::new();
        tagged_serialize(op, &mut ser)?;
        let op2: Op<M, InMemoryDB> = tagged_deserialize(&ser[..])?;
        if op != &op2 {
            return Err(JsError::new(
                "Operations didn't survive serialization check",
            ));
        }
    }
    Ok(())
}

/// Flatten an `AlignedValue` into the **preimage-bearing** form, appending
/// the resulting Frs to `out`.
///
/// This is the shared core used by every callsite that needs Compress
/// atoms to survive the wasm boundary with their preimage intact:
///
///   * `ProofPreimage.inputs` — the circuit input vector. The IR-side
///     `IrSource::preprocess` slices each declared `IrType::Opaque` input
///     out of `preimage.inputs` and materializes
///     `IrValue::Opaque { bytes, commit }` from this layout.
///
///   * `ProofPreimage.public_transcript_outputs` — popeq results. Each
///     `Op::Popeq { result, .. }` writes its `result` AV through this
///     helper. The IR-side `I::PublicInput` arm slices `IrType::Opaque`
///     entries out of the resulting Fr stream.
///
///   * `ProofPreimage.private_transcript` — witness AVs returned by
///     witness functions. Same shape, sliced by the IR's
///     `I::PrivateInput` arm.
///
/// For non-`Compress` segments this delegates to the standard
/// `value_only_field_repr` so the field representation is bit-for-bit
/// identical to what the existing `onchain-runtime-wasm`
/// `proofDataIntoSerializedPreimage` produces. The behaviour diverges
/// only at top-level `AlignmentAtom::Compress` atoms: instead of
/// emitting the `transient_commit(bytes, byte_len)` (which is what
/// `value_only_field_repr` does — see
/// `transient_crypto/src/fab.rs`), we emit `[byte_len_fr,
/// fr_0, ..., fr_{N-1}]` where the preimage Frs are packed via the
/// chunk-and-reverse layout `bytes_from_field_repr` inverts (i.e. the
/// same layout as `IrType::Opaque`'s `encode_offcircuit` arm in
/// `zkir-v3/src/ir_instructions/encode.rs`).
///
/// `AlignmentSegment::Option` is intentionally rejected: the
/// commit-bearing flatten pads each option to `max(option.field_len())`
/// so that the total length is fixed regardless of the chosen variant,
/// but a preimage-bearing flatten over Compress atoms inside an option
/// would have a data-dependent length, breaking that scheme. Compact's
/// current emission for `Opaque<...>` types puts the Compress atom at
/// the top level (no enclosing Option), so this is a real but
/// non-blocking gap. We fail loudly here rather than silently producing
/// a length that the IR-side decoder cannot slice.
fn flatten_av_with_opaque_preimages(av: &AlignedValue, out: &mut Vec<Fr>) -> Result<(), JsError> {
    let mut atoms: &[ValueAtom] = &av.value.0;
    for segment in &av.alignment.0 {
        match segment {
            AlignmentSegment::Atom(AlignmentAtom::Compress) => {
                let val_atom = consume_atom(&mut atoms, "Compress")?;
                let bytes = &val_atom.0;
                let byte_len_u32 = u32::try_from(bytes.len()).map_err(|_| {
                    JsError::new(
                        "flatten_av_with_opaque_preimages: Compress preimage longer \
                         than u32::MAX bytes — IR-side byte_len decoder cannot \
                         represent it",
                    )
                })?;
                out.push(Fr::from(u64::from(byte_len_u32)));
                let packed: Vec<Fr> = bytes
                    .chunks(FR_BYTES_STORED)
                    .map(|chunk| {
                        Fr::from_le_bytes(chunk).ok_or_else(|| {
                            JsError::new(
                                "flatten_av_with_opaque_preimages: Compress preimage \
                                 chunk does not fit into FR_BYTES_STORED bytes",
                            )
                        })
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                out.extend(packed.into_iter().rev());
            }
            AlignmentSegment::Atom(other) => {
                let val_atom = consume_atom(&mut atoms, "Atom(non-Compress)")?;
                let one_av = AlignedValue {
                    value: Value(vec![val_atom.clone()]),
                    alignment: Alignment(vec![AlignmentSegment::Atom(*other)]),
                };
                one_av.value_only_field_repr(out);
            }
            AlignmentSegment::Option(_) => {
                return Err(JsError::new(
                    "flatten_av_with_opaque_preimages: AlignmentSegment::Option is \
                     not supported. The fixed-length padding scheme used by the \
                     commit-bearing flatten breaks under variable-length Opaque \
                     preimages, and Compact's `Opaque<...>` emission does not \
                     currently nest Compress atoms inside Options. If/when this \
                     changes, both this flatten and the IR-side input slicer in \
                     `IrSource::preprocess` need to grow option-aware handling.",
                ));
            }
        }
    }
    if !atoms.is_empty() {
        return Err(JsError::new(
            "flatten_av_with_opaque_preimages: trailing value atoms not consumed \
             by alignment — alignment/value mismatch",
        ));
    }
    Ok(())
}

fn flatten_with_opaque_preimages(av: &AlignedValue) -> Result<Vec<Fr>, JsError> {
    let mut out = Vec::new();
    flatten_av_with_opaque_preimages(av, &mut out)?;
    Ok(out)
}

fn consume_atom<'a>(
    atoms: &mut &'a [ValueAtom],
    context: &'static str,
) -> Result<&'a ValueAtom, JsError> {
    let (head, tail) = atoms.split_first().ok_or_else(|| {
        JsError::new(&format!(
            "flatten_av_with_opaque_preimages: ran out of value atoms while \
             processing alignment segment ({context})"
        ))
    })?;
    *atoms = tail;
    Ok(head)
}

fn flatten_with_compress_commits(av: &AlignedValue, out: &mut Vec<Fr>) {
    av.value_only_field_repr(out);
}

struct JsKeyProvider(JsValue);

fn try_to_string(jsv: JsValue) -> String {
    let res = js_sys::Reflect::get(&jsv, &"toString".into())
        .and_then(|f| f.dyn_into::<Function>())
        .and_then(|f| f.call0(&jsv))
        .and_then(|s| s.dyn_into::<JsString>());
    match res {
        Ok(s) => s.into(),
        Err(_) => "<failed to stringify>".into(),
    }
}

fn err(msg: impl Into<String>) -> std::io::Error {
    std::io::Error::other(msg.into())
}

impl ParamsProverProvider for JsKeyProvider {
    async fn get_params(&self, k: u8) -> std::io::Result<ParamsProver> {
        let get_params = js_sys::Reflect::get(&self.0, &"getParams".into())
            .map_err(|_| err("could not get property 'getParams' on KeyMaterialProvider"))?
            .dyn_into::<Function>()
            .map_err(|_| err("property 'getParams' on KeyMaterialProvider is not a function"))?;
        let promise = get_params
            .call1(&self.0, &JsValue::from(k))
            .map_err(|e| err(format!("error calling getParams: {}", try_to_string(e))))?
            .dyn_into::<Promise>()
            .map_err(|_| err("result of getParams was not a promise"))?;
        let res = JsFuture::from(promise)
            .await
            .map_err(|e| {
                err(format!(
                    "getParams promise resolved to error: {}",
                    try_to_string(e)
                ))
            })?
            .dyn_into::<Uint8Array>()
            .map_err(|_| err("result of getParams was not a Uint8Array"))?
            .to_vec();
        ParamsProver::read(&res[..])
    }
}

impl Resolver for JsKeyProvider {
    async fn resolve_key(&self, key: KeyLocation) -> std::io::Result<Option<ProvingKeyMaterial>> {
        let lookup_key = js_sys::Reflect::get(&self.0, &"lookupKey".into())
            .map_err(|_| err("could not get property 'lookupKey' on KeyMaterialProvider"))?
            .dyn_into::<Function>()
            .map_err(|_| err("property 'lookupKey on KeyMaterialProvider is not a function"))?;
        let loc = JsValue::from(key.0.into_owned());
        let promise = lookup_key
            .call1(&self.0, &loc)
            .map_err(|e| err(format!("error calling lookupKey: {}", try_to_string(e))))?
            .dyn_into::<Promise>()
            .map_err(|_| err("result of lookupKey is not a promise"))?;
        let res = JsFuture::from(promise).await.map_err(|e| {
            err(format!(
                "lookupKey promise resolve to error: {}",
                try_to_string(e)
            ))
        })?;
        if res.is_undefined() || res.is_null() {
            return Ok(None);
        }
        let getprop = |prop: &str| {
            Ok::<_, std::io::Error>(
                js_sys::Reflect::get(&res, &prop.into())
                    .map_err(|_| {
                        err(format!(
                            "could not get property '{prop}' on ProvingKeyMaterial"
                        ))
                    })?
                    .dyn_into::<Uint8Array>()
                    .map_err(|_| {
                        err(format!(
                            "property '{prop}' on ProvingKeyMaterial is not a Uint8Array"
                        ))
                    })?
                    .to_vec(),
            )
        };
        let prover_key = getprop("proverKey")?;
        let verifier_key = getprop("verifierKey")?;
        let ir_source = getprop("ir")?;
        Ok(Some(ProvingKeyMaterial {
            prover_key,
            verifier_key,
            ir_source,
        }))
    }
}

fn fr_from_bigint(bigint: BigInt) -> Result<Fr, JsError> {
    let hex_str = String::from(
        bigint
            .to_string(16)
            .map_err(|err| JsError::new(&String::from(err.to_string())))?,
    );
    let padded_str = if hex_str.len() % 2 == 1 {
        "0".to_owned() + &hex_str
    } else {
        hex_str
    };
    let mut bytes = <Vec<u8>>::from_hex(padded_str.as_bytes())?;
    bytes.reverse();
    Fr::from_le_bytes(&bytes).ok_or_else(|| JsError::new("out of bounds for prime field"))
}

#[wasm_bindgen]
pub async fn prove(
    ser_preimage: Uint8Array,
    provider: JsValue,
    overwrite_binding_input: Option<BigInt>,
) -> Result<Uint8Array, JsError> {
    let mut preimage: ProofPreimage = tagged_deserialize(&mut &ser_preimage.to_vec()[..])?;
    if let Some(bi) = overwrite_binding_input {
        preimage.binding_input = fr_from_bigint(bi)?;
    }
    let provider = JsKeyProvider(provider);

    let proof = preimage
        .prove::<zkir_v3::IrSource>(OsRng, &provider, &provider)
        .await
        .map(|(proof, _)| proof)
        .map_err(|e| JsError::new(&e.to_string()))?;

    let mut res = Vec::new();
    tagged_serialize(&proof, &mut res)?;
    Ok(Uint8Array::from(&res[..]))
}

#[wasm_bindgen]
pub async fn check(ser_preimage: Uint8Array, provider: JsValue) -> Result<Vec<JsValue>, JsError> {
    let preimage: ProofPreimage = tagged_deserialize(&mut &ser_preimage.to_vec()[..])?;
    let provider = JsKeyProvider(provider);
    let Some(data) = provider.resolve_key(preimage.key_location.clone()).await? else {
        return Err(JsError::new(&format!(
            "failed to resolve key at '{}'",
            &preimage.key_location.0
        )));
    };

    let ir = tagged_deserialize::<zkir_v3::IrSource>(&data.ir_source[..])
        .map_err(|e| JsError::new(&format!("Failed to deserialize ZKIR v3: {}", e)))?;
    let res = preimage
        .check(&ir)
        .map_err(|e| JsError::new(&e.to_string()))?;

    Ok(res
        .into_iter()
        .map(|val| match val {
            Some(val) => JsValue::from(BigInt::from(val)),
            None => JsValue::UNDEFINED,
        })
        .collect())
}

#[wasm_bindgen(js_name = "provingProvider")]
pub fn proving_provider(km_provider: JsValue) -> WrappedProvingProvider {
    WrappedProvingProvider { km_provider }
}

#[wasm_bindgen]
pub struct WrappedProvingProvider {
    km_provider: JsValue,
}

#[wasm_bindgen]
impl WrappedProvingProvider {
    pub async fn check(
        &self,
        ser_preimage: Uint8Array,
        _key_location: &str,
    ) -> Result<Vec<JsValue>, JsError> {
        check(ser_preimage, self.km_provider.clone()).await
    }
    pub async fn prove(
        &self,
        ser_preimage: Uint8Array,
        _key_location: &str,
        overwrite_binding_input: Option<BigInt>,
    ) -> Result<Uint8Array, JsError> {
        prove(
            ser_preimage,
            self.km_provider.clone(),
            overwrite_binding_input,
        )
        .await
    }
}

#[wasm_bindgen(js_name = "jsonIrToBinary")]
pub fn json_ir_to_binary(json: &str) -> Result<Uint8Array, JsError> {
    Zkir::from_json(json)?.serialize()
}

/// ZKIR-v3-flavoured `proofDataIntoSerializedPreimage`.
///
/// This is intentionally NOT
/// `onchain_runtime_wasm::proof_data_into_serialized_preimage` — modifying
/// that function would change the preimage construction for every
/// consumer of the on-chain runtime, including ZKIR v2 callers that
/// require the existing commit-bearing `inputs` shape. The v3 IR side,
/// in contrast, needs `Compress`-aligned values to survive the wasm
/// boundary in **preimage-bearing** form so that downstream consumers
/// of `IrValue::Opaque` (the IR-level preimage carrier) have the
/// bytes available, not just the commit.
///
/// The two shapes coexist by partitioning the preimage:
///
///   * `ProofPreimage.inputs` — flattened with
///     [`flatten_with_opaque_preimages`]. For every top-level
///     `AlignmentAtom::Compress` atom in `input` we emit `[byte_len,
///     packed_preimage_frs ...]` (matching `IrType::Opaque`'s
///     `encode_offcircuit` arm). Non-Compress atoms emit the standard
///     `value_only_field_repr` Frs, so the layout is identical to the
///     existing v2 bridge for circuits that don't use Opaque inputs.
///
///   * `ProofPreimage.communications_commitment` — built from a
///     commit-bearing flatten via [`flatten_with_compress_commits`]
///     (which is exactly `value_only_field_repr`). The IR-side
///     `IrSource::preprocess` re-derives the same commit-bearing form
///     by walking declared input types via
///     `encode_offcircuit_for_commit` (which projects
///     `IrValue::Opaque { commit, .. }` to its cached `commit` Fr),
///     keeping the verification byte-for-byte aligned with what's
///     hashed here.
///
/// `ProofPreimage.public_transcript_outputs` and
/// `ProofPreimage.private_transcript` also use the preimage-bearing
/// flatten so that Compress-aligned popeq results (i.e. ledger reads of
/// `Opaque<...>` cells) and witness AVs returning Opaque values survive
/// the wasm boundary with their preimages intact. The IR-side
/// `I::PublicInput` and `I::PrivateInput` arms slice variable-width
/// Opaque entries out of these streams to materialize
/// `IrValue::Opaque { bytes, commit }` directly, eliminating the need
/// for a Native-as-Opaque output validator relaxation.
///
/// `ProofPreimage.public_transcript_inputs` MUST stay commit-bearing —
/// it has to match the byte-flat impact stream's resolved-to-Fr values
/// position-by-position, and operand resolution at impact time produces
/// exactly one Fr per operand position via `Fr::try_from(IrValue)`
/// (which returns the cached commit for Opaque). Switching this stream
/// would break the byte-equality invariant with `op.field_repr`.
///
/// `binding_input` carries no per-atom Compress/Opaque distinction and
/// is unchanged.
#[wasm_bindgen(js_name = "proofDataIntoSerializedPreimage")]
pub fn proof_data_into_serialized_preimage(
    input: JsValue,
    output: JsValue,
    public_transcript: JsValue,
    private_transcript_outputs: JsValue,
    key_location: Option<String>,
) -> Result<Uint8Array, JsError> {
    let input: AlignedValue = from_value(input)?;
    let output: AlignedValue = from_value(output)?;
    let public_transcript: Vec<Op<ResultModeVerify, InMemoryDB>> = from_value(public_transcript)?;
    ensure_ops_valid(&public_transcript)?;
    let private_transcript_outputs: Vec<AlignedValue> = from_value(private_transcript_outputs)?;

    let mut private_transcript = Vec::new();
    for entry in private_transcript_outputs.iter() {
        flatten_av_with_opaque_preimages(entry, &mut private_transcript)?;
    }

    let mut public_transcript_outputs = Vec::new();
    for op in public_transcript.iter() {
        if let Op::Popeq { result, .. } = op {
            flatten_av_with_opaque_preimages(result, &mut public_transcript_outputs)?;
        }
    }

    let mut public_transcript_inputs = Vec::new();
    for op in public_transcript.iter() {
        op.field_repr(&mut public_transcript_inputs);
    }

    let mut comm_comm_preimage = vec![Fr::from(0u64)];
    flatten_with_compress_commits(&input, &mut comm_comm_preimage);
    flatten_with_compress_commits(&output, &mut comm_comm_preimage);

    let inputs = flatten_with_opaque_preimages(&input)?;

    let preimage = ProofPreimage {
        inputs,
        binding_input: Fr::from(0u64),
        private_transcript,
        public_transcript_inputs,
        public_transcript_outputs,
        key_location: KeyLocation(
            key_location
                .map(Cow::Owned)
                .unwrap_or(Cow::Borrowed("dummy")),
        ),
        communications_commitment: Some((
            transient_crypto::hash::transient_hash(&comm_comm_preimage),
            Fr::from(0u64),
        )),
    };
    let mut buf = Vec::new();
    tagged_serialize(&preimage, &mut buf)?;
    Ok(buf[..].into())
}

#[wasm_bindgen]
struct Zkir(zkir_v3::IrSource);

#[wasm_bindgen]
impl Zkir {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Result<Zkir, JsError> {
        Err(JsError::new(
            "Zkir cannot be constructed directly through the WASM API.",
        ))
    }

    #[wasm_bindgen(js_name = "getK")]
    pub fn get_k(&self) -> u8 {
        self.0.k()
    }

    #[wasm_bindgen(js_name = "fromJson")]
    pub fn from_json(json: &str) -> Result<Self, JsError> {
        let ir = zkir_v3::IrSource::load(json.as_bytes())?;
        Ok(Self(ir))
    }

    #[wasm_bindgen]
    pub fn deserialize(bytes: Uint8Array) -> Result<Self, JsError> {
        let ir = tagged_deserialize::<zkir_v3::IrSource>(&mut &bytes.to_vec()[..])?;
        Ok(Self(ir))
    }

    pub fn serialize(&self) -> Result<Uint8Array, JsError> {
        let mut buf = Vec::new();
        tagged_serialize(&self.0, &mut buf)?;
        Ok(buf[..].into())
    }
}
