//! On-chain contract state → `DidDocument` mapping.
//!
//! Mirrors `midnight-did/did/src/ledger-to-domain.ts`. We
//! `tagged_deserialize` the indexer-supplied `ContractState` bytes,
//! then walk the `StateValue` tree to pull out the DID Ledger
//! fields. Compact emits the contract's `ledger {}` block as a
//! 2-element root array: `[constants, mutable]`. The exact field
//! layout was extracted from
//! `midnight-did-contract/dist/managed/did/contract/index.js`'s
//! ledger-state accessors:
//!
//! ```text
//! root[0]  constants
//!   [0]    contractVersion        : Cell<bigint>
//!   [1]    controllerPublicKey    : Cell<Bytes32>
//! root[1]  mutable
//!   [0]    id                     : Cell<Bytes32>
//!   [1]    alsoKnownAs            : Map<string, ()>            (Set)
//!   [2]    version                : Cell<bigint>
//!   [3]    created                : Cell<bigint>
//!   [4]    updated                : Cell<bigint>
//!   [5]    deactivated            : Cell<bool>
//!   [6]    active                 : Cell<bool>
//!   [7]    operationCount         : Cell<bigint>
//!   [8]    verificationMethods    : Map<string, VerificationMethod>
//!   [9..13] relations             : Map<string, ()> ×5
//!   [14]   services               : Map<string, Service>
//! ```
//!
//! Phase 2b (this commit) decodes the *scalar* `Cell` fields:
//! contractVersion, controllerPublicKey, id, version, created,
//! updated, deactivated, active, operationCount.
//!
//! Phase 2c walks the `Map` subtrees for sets / VMs / services.

use std::time::{Duration, UNIX_EPOCH};

use base_crypto::fab::AlignedValue;
use onchain_state::state::{ContractState, StateValue};
use serialize::tagged_deserialize;
use storage::DefaultDB;

use crate::did::error::DidError;
use crate::did::id::{CONTRACT_ADDRESS_LEN, ContractAddressBytes, DidId};
use crate::did::types::{
    CurveType, DidDocument, KeyType, PublicKeyJwk, Service, ServiceEndpoint,
    VerificationMethod, VerificationMethodRef, VerificationMethodRelation,
    VerificationMethodType,
};

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub(crate) struct DidLedgerState {
    pub(crate) contract_version: u128,
    pub(crate) controller_public_key: [u8; 32],
    /// On-chain `id` — same 32-byte contract-address shape as the
    /// envelope address. Useful as a cross-check with the DID we
    /// asked for.
    pub(crate) id_bytes: ContractAddressBytes,
    pub(crate) version: u128,
    /// Compact `bigint` is a 256-bit field element; we expose it as
    /// 32 bytes here and let callers decide whether to interpret as
    /// u64 ms-timestamp, u128, or otherwise.
    pub(crate) created_raw: [u8; 32],
    pub(crate) updated_raw: [u8; 32],
    pub(crate) deactivated: bool,
    pub(crate) active: bool,
    pub(crate) operation_count: u128,
    /// Number of total fields the root array exposed at
    /// (`mutable` length). Diagnostic for layout drift.
    pub(crate) mutable_field_count: usize,
    pub(crate) also_known_as: Vec<String>,
    pub(crate) verification_methods: Vec<VerificationMethod>,
    pub(crate) authentication: Vec<String>,
    pub(crate) assertion_method: Vec<String>,
    pub(crate) key_agreement: Vec<String>,
    pub(crate) capability_invocation: Vec<String>,
    pub(crate) capability_delegation: Vec<String>,
    pub(crate) services: Vec<Service>,
}

/// Counter for the contract's maintenance authority, plucked out of
/// the same `ContractState` payload `decode_did_ledger_state` walks.
/// The next `MaintenanceUpdate` for this contract must use exactly
/// this value (the on-chain check increments after each accepted
/// update — see `ledger/src/structure.rs::ContractMaintenanceAuthority`).
pub(crate) fn decode_maintenance_counter(state_hex: &str) -> Result<u32, DidError> {
    let bytes = hex::decode(state_hex.trim_start_matches("0x"))
        .map_err(|e| DidError::DecodeState(format!("hex: {e}")))?;
    let state: ContractState<DefaultDB> = tagged_deserialize(&bytes[..])
        .map_err(|e| DidError::DecodeState(format!("tagged_deserialize: {e}")))?;
    Ok(state.maintenance_authority.counter)
}

pub(crate) fn decode_did_ledger_state(state_hex: &str) -> Result<DidLedgerState, DidError> {
    let bytes = hex::decode(state_hex.trim_start_matches("0x"))
        .map_err(|e| DidError::DecodeState(format!("hex: {e}")))?;
    let state: ContractState<DefaultDB> = tagged_deserialize(&bytes[..])
        .map_err(|e| DidError::DecodeState(format!("tagged_deserialize: {e}")))?;

    let root: &StateValue<DefaultDB> = &state.data.state;
    let StateValue::Array(root_arr) = root else {
        return Err(DidError::DecodeState(format!(
            "expected root StateValue::Array, got {}",
            state_value_kind(root)
        )));
    };
    if root_arr.len() != 2 {
        return Err(DidError::DecodeState(format!(
            "expected root array length 2, got {}",
            root_arr.len()
        )));
    }

    let constants = sub_array(root_arr, 0, "root[0]/constants")?;
    let mutable = sub_array(root_arr, 1, "root[1]/mutable")?;

    Ok(DidLedgerState {
        contract_version: cell_u128(constants, 0, "contractVersion")?,
        controller_public_key: cell_bytes32(constants, 1, "controllerPublicKey")?,
        id_bytes: cell_bytes32(mutable, 0, "id")?,
        version: cell_u128(mutable, 2, "version")?,
        created_raw: cell_bytes32_padded(mutable, 3, "created")?,
        updated_raw: cell_bytes32_padded(mutable, 4, "updated")?,
        deactivated: cell_bool(mutable, 5, "deactivated")?,
        active: cell_bool(mutable, 6, "active")?,
        operation_count: cell_u128(mutable, 7, "operationCount")?,
        mutable_field_count: mutable.len() as usize,
        also_known_as: decode_string_set(mutable, 1, "alsoKnownAs")?,
        verification_methods: decode_vm_map(mutable, 8, "verificationMethods")?,
        authentication: decode_string_set(mutable, 9, "authenticationRelation")?,
        assertion_method: decode_string_set(mutable, 10, "assertionMethodRelation")?,
        key_agreement: decode_string_set(mutable, 11, "keyAgreementRelation")?,
        capability_invocation: decode_string_set(mutable, 12, "capabilityInvocationRelation")?,
        capability_delegation: decode_string_set(mutable, 13, "capabilityDelegationRelation")?,
        services: decode_service_map(mutable, 14, "services")?,
    })
}

pub(crate) fn ledger_to_domain(ledger: &DidLedgerState, id: DidId) -> DidDocument {
    let created = decode_timestamp_ms(&ledger.created_raw);
    let updated = decode_timestamp_ms(&ledger.updated_raw);

    // Relations are stored on-chain as plain string sets, where each
    // entry is a verification-method fragment id (e.g. "key-0").
    // DID Core represents them as references to verification methods,
    // which we model as `VerificationMethodRef::Id` — full DID URL
    // form: `<did>#<fragment>`.
    let did_str = id.to_did_string();
    let to_refs = |frags: &[String]| -> Vec<VerificationMethodRef> {
        frags
            .iter()
            .map(|f| VerificationMethodRef::Id(format!("{did_str}#{f}")))
            .collect()
    };

    // Verification methods: same fragment-id → full DID URL
    // expansion. Controller of each VM defaults to the DID itself.
    let verification_method = ledger
        .verification_methods
        .iter()
        .cloned()
        .map(|mut vm| {
            if !vm.id.contains('#') {
                vm.id = format!("{did_str}#{}", vm.id);
            }
            vm.controller = id.clone();
            vm
        })
        .collect();

    DidDocument {
        id: id.clone(),
        // Self-controlling unless future phases surface a separate
        // controller DID via the on-chain controllerPublicKey ↔
        // DID-id resolution.
        controller: None,
        also_known_as: ledger.also_known_as.clone(),
        verification_method,
        authentication: to_refs(&ledger.authentication),
        assertion_method: to_refs(&ledger.assertion_method),
        key_agreement: to_refs(&ledger.key_agreement),
        capability_invocation: to_refs(&ledger.capability_invocation),
        capability_delegation: to_refs(&ledger.capability_delegation),
        service: ledger
            .services
            .iter()
            .cloned()
            .map(|mut s| {
                if !s.id.contains('#') {
                    s.id = format!("{did_str}#{}", s.id);
                }
                s
            })
            .collect(),
        deactivated: ledger.deactivated || !ledger.active,
        created,
        updated,
        version: ledger.version as u64,
    }
}

// ── tree-walking helpers ───────────────────────────────────────────

fn sub_array<'a>(
    arr: &'a storage::storage::Array<StateValue<DefaultDB>, DefaultDB>,
    idx: usize,
    where_: &str,
) -> Result<&'a storage::storage::Array<StateValue<DefaultDB>, DefaultDB>, DidError> {
    let val = arr
        .get(idx)
        .ok_or_else(|| DidError::DecodeState(format!("{where_}: index {idx} out of bounds")))?;
    match val {
        StateValue::Array(inner) => Ok(inner),
        other => Err(DidError::DecodeState(format!(
            "{where_}: expected Array, got {}",
            state_value_kind(other)
        ))),
    }
}

fn cell_aligned<'a>(
    arr: &'a storage::storage::Array<StateValue<DefaultDB>, DefaultDB>,
    idx: usize,
    field: &str,
) -> Result<&'a AlignedValue, DidError> {
    let val = arr
        .get(idx)
        .ok_or_else(|| DidError::DecodeState(format!("{field}: index {idx} out of bounds")))?;
    match val {
        StateValue::Cell(sp) => Ok(&**sp),
        other => Err(DidError::DecodeState(format!(
            "{field}: expected Cell, got {}",
            state_value_kind(other)
        ))),
    }
}

fn cell_bool(
    arr: &storage::storage::Array<StateValue<DefaultDB>, DefaultDB>,
    idx: usize,
    field: &str,
) -> Result<bool, DidError> {
    let bytes = aligned_first_atom(cell_aligned(arr, idx, field)?, field)?;
    // ValueAtoms are normalised: trailing zeros are stripped, so
    // `false` (which encodes as `[0]`) becomes the empty atom on the
    // wire. Treat that as `false`. See `From<bool> for ValueAtom` in
    // `base-crypto/src/fab/conversions.rs`.
    match bytes.first() {
        None | Some(0) => Ok(false),
        Some(1) => Ok(true),
        Some(b) => Err(DidError::DecodeState(format!(
            "{field}: expected 0/1 bool, got {b}"
        ))),
    }
}

/// Decode a Compact `Uint`/`Counter`/`bigint` cell as a u128.
/// Compact stores integers as little-endian, normalised
/// `ValueAtom`s (trailing high zeros stripped); see
/// `From<u128> for ValueAtom` and `TryFrom<&ValueAtom> for u128`
/// in `base-crypto/src/fab/conversions.rs`. If the wire value is
/// wider than 16 bytes we lossily truncate to the low bytes —
/// callers that need the full 256-bit range should use
/// `cell_bytes32`.
fn cell_u128(
    arr: &storage::storage::Array<StateValue<DefaultDB>, DefaultDB>,
    idx: usize,
    field: &str,
) -> Result<u128, DidError> {
    let bytes = aligned_first_atom(cell_aligned(arr, idx, field)?, field)?;
    let mut buf = [0u8; 16];
    let take = bytes.len().min(16);
    buf[..take].copy_from_slice(&bytes[..take]);
    Ok(u128::from_le_bytes(buf))
}

fn cell_bytes32(
    arr: &storage::storage::Array<StateValue<DefaultDB>, DefaultDB>,
    idx: usize,
    field: &str,
) -> Result<[u8; CONTRACT_ADDRESS_LEN], DidError> {
    let bytes = aligned_first_atom(cell_aligned(arr, idx, field)?, field)?;
    // ValueAtoms are stored in normal form (trailing zeros stripped),
    // so a `Bytes<32>` cell whose tail is zero comes back shorter.
    // The alignment guarantees the original length is ≤ 32; pad the
    // tail with zeros to recover the wide form.
    if bytes.len() > CONTRACT_ADDRESS_LEN {
        return Err(DidError::DecodeState(format!(
            "{field}: value exceeds 32 bytes ({})",
            bytes.len()
        )));
    }
    let mut out = [0u8; CONTRACT_ADDRESS_LEN];
    out[..bytes.len()].copy_from_slice(bytes);
    Ok(out)
}

/// Like `cell_bytes32` but rejects oversize atoms instead of
/// truncating. Useful when callers want to read the field as a
/// little-endian unsigned integer up to 256 bits — short atoms get
/// the high bytes zero-padded so the LE decode (`from_le_bytes`)
/// returns the original value.
fn cell_bytes32_padded(
    arr: &storage::storage::Array<StateValue<DefaultDB>, DefaultDB>,
    idx: usize,
    field: &str,
) -> Result<[u8; CONTRACT_ADDRESS_LEN], DidError> {
    let bytes = aligned_first_atom(cell_aligned(arr, idx, field)?, field)?;
    if bytes.len() > CONTRACT_ADDRESS_LEN {
        return Err(DidError::DecodeState(format!(
            "{field}: value exceeds 32 bytes ({})",
            bytes.len()
        )));
    }
    let mut out = [0u8; CONTRACT_ADDRESS_LEN];
    out[..bytes.len()].copy_from_slice(bytes);
    Ok(out)
}

fn aligned_first_atom<'a>(
    av: &'a AlignedValue,
    field: &str,
) -> Result<&'a [u8], DidError> {
    aligned_atom_at(av, 0, field)
}

fn aligned_atom_at<'a>(
    av: &'a AlignedValue,
    idx: usize,
    field: &str,
) -> Result<&'a [u8], DidError> {
    let value: &base_crypto::fab::Value = av.as_ref();
    let atom = value.0.get(idx).ok_or_else(|| {
        DidError::DecodeState(format!(
            "{field}: AlignedValue atom {idx} missing (has {} total)",
            value.0.len()
        ))
    })?;
    Ok(atom.0.as_slice())
}

/// Read a Compact enum tag from atom `idx`. Compact serialises
/// the variant index as a minimal-byte little-endian unsigned
/// integer, so variant `0` ends up as an empty byte slice
/// (no bytes needed). Treat the empty case as tag `0`; anything
/// longer than a single byte is rejected — every Midnight DID
/// enum has well under 256 variants.
fn enum_tag_or_zero(av: &AlignedValue, idx: usize, field: &str) -> Result<u8, DidError> {
    let bytes = aligned_atom_at(av, idx, field)?;
    match bytes {
        [] => Ok(0),
        [b] => Ok(*b),
        more => Err(DidError::DecodeState(format!(
            "{field}: enum tag too wide ({} bytes)",
            more.len(),
        ))),
    }
}

/// Decode an [`AlignedValue`] atom as a UTF-8 string. Compact's
/// `OpaqueString` descriptor uses a single `compress` atom carrying
/// raw UTF-8 bytes (no length prefix — the atom boundary is the
/// length).
fn decode_string_atom(av: &AlignedValue, atom_idx: usize, field: &str) -> Result<String, DidError> {
    let bytes = aligned_atom_at(av, atom_idx, field)?;
    String::from_utf8(bytes.to_vec())
        .map_err(|e| DidError::DecodeState(format!("{field}: invalid UTF-8: {e}")))
}

/// Decode a `Set<string>` stored as a `StateValue::Map` whose keys
/// are string AlignedValues and values are `Null` (the unit type
/// `()`). Used for `alsoKnownAs` and the 5 relation sets.
fn decode_string_set(
    arr: &storage::storage::Array<StateValue<DefaultDB>, DefaultDB>,
    idx: usize,
    field: &str,
) -> Result<Vec<String>, DidError> {
    let m = map_at(arr, idx, field)?;
    let mut out = Vec::with_capacity(m.size());
    for entry in m.iter() {
        let (k_sp, _v_sp) = &*entry;
        let k: &AlignedValue = k_sp;
        out.push(decode_string_atom(k, 0, field)?);
    }
    out.sort();
    Ok(out)
}

/// Decode a `Map<string, Service>`. Each entry's key is a string
/// AlignedValue; the value is a `StateValue::Cell` carrying a Service
/// AlignedValue (3 atoms: id, typ, serviceEndpoint).
fn decode_service_map(
    arr: &storage::storage::Array<StateValue<DefaultDB>, DefaultDB>,
    idx: usize,
    field: &str,
) -> Result<Vec<Service>, DidError> {
    let m = map_at(arr, idx, field)?;
    let mut out = Vec::with_capacity(m.size());
    for entry in m.iter() {
        let (k_sp, v_sp) = &*entry;
        let key = decode_string_atom(k_sp, 0, &format!("{field}.<key>"))?;
        let v_state: &StateValue<DefaultDB> = v_sp;
        let av = match v_state {
            StateValue::Cell(sp) => &**sp,
            other => {
                return Err(DidError::DecodeState(format!(
                    "{field}[{key}]: expected Cell value, got {}",
                    state_value_kind(other)
                )));
            }
        };
        let id = decode_string_atom(av, 0, &format!("{field}.id"))?;
        let typ = decode_string_atom(av, 1, &format!("{field}.typ"))?;
        let endpoint = decode_string_atom(av, 2, &format!("{field}.serviceEndpoint"))?;
        out.push(Service {
            id,
            typ,
            service_endpoint: ServiceEndpoint::Uri(endpoint),
        });
        // The map key is the same as the service id; ignore the
        // duplicate copy. We keep the assertion as a soft check.
        debug_assert!(out.last().map(|s| s.id.as_str()) == Some(&key.as_str()[..]));
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

/// Decode a `Map<string, VerificationMethod>`. The value AlignedValue
/// has 6 atoms in this order:
///   0: id (string)
///   1: typ (1-byte enum: 0=Undefined, 1=JsonWebKey)
///   2..5: PublicKeyJwk { kty (1B enum), crv (1B enum), x (32B field), y (32B field) }
fn decode_vm_map(
    arr: &storage::storage::Array<StateValue<DefaultDB>, DefaultDB>,
    idx: usize,
    field: &str,
) -> Result<Vec<VerificationMethod>, DidError> {
    let m = map_at(arr, idx, field)?;
    let mut out = Vec::with_capacity(m.size());
    for entry in m.iter() {
        let (k_sp, v_sp) = &*entry;
        let key = decode_string_atom(k_sp, 0, &format!("{field}.<key>"))?;
        let v_state: &StateValue<DefaultDB> = v_sp;
        let av = match v_state {
            StateValue::Cell(sp) => &**sp,
            other => {
                return Err(DidError::DecodeState(format!(
                    "{field}[{key}]: expected Cell value, got {}",
                    state_value_kind(other)
                )));
            }
        };
        let id = decode_string_atom(av, 0, &format!("{field}.id"))?;
        // Compact serialises a small-int enum tag as a
        // minimal-byte LEB128: variant 0 → 0 bytes; variant N → 1
        // byte. So `None` (empty atom) means "tag 0" — treat it
        // the same as `Some(0)`.
        let typ_tag = enum_tag_or_zero(av, 1, &format!("{field}.typ"))?;
        let typ = match typ_tag {
            0 => VerificationMethodType::JsonWebKey, // Undefined → fall back
            1 => VerificationMethodType::JsonWebKey,
            other => {
                return Err(DidError::DecodeState(format!(
                    "{field}.typ: unexpected enum tag {other}"
                )));
            }
        };
        let kty_tag = enum_tag_or_zero(av, 2, &format!("{field}.kty"))?;
        let kty = match kty_tag {
            0 => KeyType::EC,
            1 => KeyType::EC, // RSA — not represented in our enum, fall back
            2 => KeyType::EC, // oct
            3 => KeyType::OKP,
            other => {
                return Err(DidError::DecodeState(format!(
                    "{field}.kty: unexpected enum tag {other}"
                )));
            }
        };
        let crv_tag = enum_tag_or_zero(av, 3, &format!("{field}.crv"))?;
        let crv = match crv_tag {
            0 => CurveType::Ed25519,
            1 => CurveType::Jubjub,
            2 => CurveType::P256,
            other => {
                return Err(DidError::DecodeState(format!(
                    "{field}.crv: unexpected enum tag {other}"
                )));
            }
        };
        let x = aligned_atom_at(av, 4, &format!("{field}.x"))?.to_vec();
        let y = aligned_atom_at(av, 5, &format!("{field}.y"))?.to_vec();

        out.push(VerificationMethod {
            id,
            typ,
            // Filled in by ledger_to_domain — VMs always have the
            // owning DID as their controller.
            controller: DidId::new(crate::Network::Mainnet, [0u8; 32]),
            public_key_jwk: PublicKeyJwk {
                kty,
                crv,
                x: base64url(&x),
                y: Some(base64url(&y)),
            },
        });
        debug_assert!(out.last().map(|v| v.id.as_str()) == Some(&key.as_str()[..]));
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

#[allow(dead_code)]
const _: VerificationMethodRelation = VerificationMethodRelation::Authentication;

/// URL-safe base64 without padding — DID Core spec for JWK
/// coordinates.
fn base64url(bytes: &[u8]) -> String {
    use std::fmt::Write;
    const ALPHABET: &[u8] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk.len();
        let b = (chunk[0] as u32) << 16
            | (chunk.get(1).copied().unwrap_or(0) as u32) << 8
            | (chunk.get(2).copied().unwrap_or(0) as u32);
        out.push(ALPHABET[((b >> 18) & 63) as usize] as char);
        out.push(ALPHABET[((b >> 12) & 63) as usize] as char);
        if n >= 2 {
            out.push(ALPHABET[((b >> 6) & 63) as usize] as char);
        }
        if n >= 3 {
            out.push(ALPHABET[(b & 63) as usize] as char);
        }
    }
    let _ = write!(&mut out, ""); // suppress unused-import warning on `Write`
    out
}

fn map_at<'a>(
    arr: &'a storage::storage::Array<StateValue<DefaultDB>, DefaultDB>,
    idx: usize,
    field: &str,
) -> Result<&'a storage::storage::HashMap<AlignedValue, StateValue<DefaultDB>, DefaultDB>, DidError>
{
    let val = arr
        .get(idx)
        .ok_or_else(|| DidError::DecodeState(format!("{field}: index {idx} out of bounds")))?;
    match val {
        StateValue::Map(m) => Ok(m),
        other => Err(DidError::DecodeState(format!(
            "{field}: expected Map, got {}",
            state_value_kind(other)
        ))),
    }
}

fn state_value_kind(v: &StateValue<DefaultDB>) -> &'static str {
    match v {
        StateValue::Null => "Null",
        StateValue::Cell(_) => "Cell",
        StateValue::Map(_) => "Map",
        StateValue::Array(_) => "Array",
        StateValue::BoundedMerkleTree(_) => "BoundedMerkleTree",
        // StateValue is `#[non_exhaustive]` — fall back to "Other"
        // for variants added upstream.
        _ => "Other",
    }
}

pub(crate) fn decode_timestamp_ms(raw: &[u8; 32]) -> Option<std::time::SystemTime> {
    // `cell_bytes32_padded` returns the LE-padded form: the value's
    // bytes occupy the low end of the buffer with zeros padded into
    // the high end. The Compact constructor stores the timestamp in
    // a `Uint<64>` slot, so we read the low 8 bytes as a LE u64.
    // Reject a value outside ~1970..2286 so genuine garbage doesn't
    // surface as a bogus date.
    let lo: [u8; 8] = raw[..8].try_into().ok()?;
    let ms = u64::from_le_bytes(lo);
    if ms == 0 || ms > 10_000_000_000_000 {
        return None;
    }
    // Anything in raw[8..] would be a value wider than u64.
    if raw[8..].iter().any(|b| *b != 0) {
        return None;
    }
    Some(UNIX_EPOCH + Duration::from_millis(ms))
}

// ── tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_hex_is_rejected() {
        let err = decode_did_ledger_state("").unwrap_err();
        assert!(matches!(err, DidError::DecodeState(_)));
    }

    #[test]
    fn invalid_hex_is_rejected() {
        let err = decode_did_ledger_state("zzzz").unwrap_err();
        assert!(matches!(err, DidError::DecodeState(_)));
    }

    #[test]
    fn timestamp_zero_is_none() {
        assert!(decode_timestamp_ms(&[0u8; 32]).is_none());
    }

    #[test]
    fn timestamp_2026_decodes() {
        // 2026-04-30 00:00 UTC = 1777737600000 ms.
        // `cell_bytes32_padded` produces the LE-padded form: ms
        // bytes at the low end, high bytes zero.
        let ms = 1_777_737_600_000_u64;
        let mut raw = [0u8; 32];
        raw[..8].copy_from_slice(&ms.to_le_bytes());
        let ts = decode_timestamp_ms(&raw).unwrap();
        let ts_ms = ts
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        assert_eq!(ts_ms, ms);
    }
}
