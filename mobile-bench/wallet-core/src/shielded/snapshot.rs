//! Decode + fold for the `zswapLedgerEvents` subscription.
//!
//! Each stream element is a `ledger::events::Event<DefaultDB>` (via the
//! `raw` hex field). The fold mirrors `zswap::local::State::apply` but
//! is driven by the flattened `ZswapOutput` / `ZswapInput` events with
//! their explicit `mt_index`: every commitment is inserted into the
//! Merkle tree in index order, owned coins (decrypted via the wallet's
//! `SecretKeys`) are kept, others are collapsed to save memory, and
//! spent coins are removed on nullifier match.

use std::borrow::Cow;

use coin_structure::transfer::{Recipient, SenderEvidence};
use ledger::events::{Event, EventDetails};
use serde_json::Value;
use storage::DefaultDB;
use zswap::keys::SecretKeys;
use zswap::local::State as ZswapState;

use super::ShieldedError;

pub(super) const ZSWAP_LEDGER_EVENTS_QUERY: &str =
    include_str!("../../queries/midnight-indexer/zswap_ledger_events.subscription.graphql");

/// One decoded `zswapLedgerEvents` element: the ledger event plus the
/// indexer cursor (`id`) and "caught up" marker (`max_id`).
#[derive(Debug, Clone)]
pub(super) struct DecodedEvent {
    pub id: i64,
    pub max_id: i64,
    pub event: Event<DefaultDB>,
}

/// Decode one `next.payload.data.zswapLedgerEvents` JSON value.
pub(super) fn decode_event(data: &Value) -> Result<DecodedEvent, ShieldedError> {
    let obj = data
        .get("zswapLedgerEvents")
        .ok_or_else(|| ShieldedError::Decode("missing zswapLedgerEvents".into()))?;
    let id = obj
        .get("id")
        .and_then(Value::as_i64)
        .ok_or_else(|| ShieldedError::Decode("missing id".into()))?;
    let max_id = obj
        .get("maxId")
        .and_then(Value::as_i64)
        .ok_or_else(|| ShieldedError::Decode("missing maxId".into()))?;
    let raw_hex = obj
        .get("raw")
        .and_then(Value::as_str)
        .ok_or_else(|| ShieldedError::Decode("missing raw".into()))?;
    let raw_bytes = hex::decode(raw_hex.trim_start_matches("0x"))
        .map_err(|e| ShieldedError::Decode(format!("raw hex: {e}")))?;
    let event: Event<DefaultDB> = serialize::tagged_deserialize(&raw_bytes[..])
        .map_err(|e| ShieldedError::Decode(format!("raw tagged: {e}")))?;
    Ok(DecodedEvent { id, max_id, event })
}

/// Fold `events` into `start`, returning the updated zswap local state.
/// Mirrors `zswap::local::State::apply`: insert every commitment into
/// the tree (deferring `rehash` to the end), keep owned coins, collapse
/// the rest, and drop spent coins by nullifier.
pub(crate) fn replay_zswap_events<'a>(
    keys: &SecretKeys,
    start: ZswapState<DefaultDB>,
    events: impl Iterator<Item = &'a Event<DefaultDB>>,
) -> Result<ZswapState<DefaultDB>, ShieldedError> {
    let mut st = start;
    for ev in events {
        match &ev.content {
            EventDetails::ZswapOutput {
                commitment,
                preimage_evidence,
                mt_index,
                ..
            } => {
                let idx = *mt_index;
                st.merkle_tree = st
                    .merkle_tree
                    .try_update_hash(idx, commitment.0, ())
                    .map_err(|e| ShieldedError::Replay(format!("tree update @{idx}: {e:?}")))?;
                // Owned iff the preimage evidence decrypts/recipient-matches
                // AND the recomputed commitment matches (same belt-and-braces
                // check `State::apply` performs).
                let mine = preimage_evidence.try_with_keys(keys).filter(|coin| {
                    coin.commitment(&Recipient::User(keys.coin_public_key())) == *commitment
                });
                if let Some(coin) = mine {
                    let qci = coin.qualify(idx);
                    let nullifier = coin.nullifier(&SenderEvidence::User(Cow::Borrowed(
                        &keys.coin_secret_key,
                    )));
                    st.coins = st.coins.insert(nullifier, qci);
                } else {
                    st.merkle_tree = st.merkle_tree.collapse(idx, idx);
                }
                st.first_free = (idx + 1).max(st.first_free);
            }
            EventDetails::ZswapInput { nullifier, .. } => {
                st.coins = st.coins.remove(nullifier);
                st.pending_spends = st.pending_spends.remove(nullifier);
            }
            _ => {}
        }
    }
    st.merkle_tree = st.merkle_tree.rehash();
    Ok(st)
}

/// Map the shared WS transport error into a `ShieldedError`.
pub(super) fn translate_transport_error(e: crate::unshielded::UnshieldedError) -> ShieldedError {
    use crate::unshielded::UnshieldedError as U;
    match e {
        U::WsConnect(s) => ShieldedError::WsConnect(s),
        U::WsHandshake(s) => ShieldedError::WsHandshake(s),
        U::GqlError(s) => ShieldedError::GqlError(s),
        U::UnexpectedFrame(s) => ShieldedError::UnexpectedFrame(s),
        U::Decode(s) => ShieldedError::Decode(s),
        U::StreamClosedEarly => ShieldedError::StreamClosedEarly,
        other => ShieldedError::Decode(format!("{other:?}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn decode_missing_fields_errors() {
        assert!(matches!(
            decode_event(&json!({})),
            Err(ShieldedError::Decode(_))
        ));
        let err = decode_event(&json!({
            "zswapLedgerEvents": { "__typename": "ZswapOutput", "maxId": 1, "raw": "00" }
        }))
        .unwrap_err();
        match err {
            ShieldedError::Decode(m) => assert!(m.contains("id"), "{m}"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn decode_bad_raw_hex_errors() {
        let err = decode_event(&json!({
            "zswapLedgerEvents": { "id": 0, "maxId": 0, "raw": "zz" }
        }))
        .unwrap_err();
        assert!(matches!(err, ShieldedError::Decode(m) if m.contains("raw hex")));
    }
}
