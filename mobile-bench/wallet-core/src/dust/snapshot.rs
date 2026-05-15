//! Snapshot driver: subscribe to `dustLedgerEvents`, decode each
//! into a `ledger::events::Event<DefaultDB>` via the `raw` field,
//! collect them, then call `DustLocalState::replay_events` to
//! hydrate the wallet's DUST state. See the design spec for the
//! termination semantics.

use serde_json::Value;
use storage::DefaultDB;

use super::DustError;

#[allow(dead_code)] // Used by snapshot (Task 5) and tests.
pub(super) const DUST_LEDGER_EVENTS_QUERY: &str = include_str!(
    "../../queries/midnight-indexer/dust_ledger_events.subscription.graphql"
);

/// One decoded element of the subscription stream. The ledger's
/// `Event<D>` carries the actual variant — we keep our outer
/// envelope minimal (just the id we need for termination).
#[allow(dead_code)] // Used by snapshot (Task 5) and tests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct DecodedEvent {
    pub id: i64,
    pub max_id: i64,
    pub event: ledger::events::Event<DefaultDB>,
}

/// Decode one `next.payload.data.dustLedgerEvents` JSON value.
#[allow(dead_code)] // Used by snapshot (Task 5) and tests.
pub(super) fn decode_event(data: &Value) -> Result<DecodedEvent, DustError> {
    let obj = data
        .get("dustLedgerEvents")
        .ok_or_else(|| DustError::Decode("missing dustLedgerEvents".into()))?;
    let id = obj
        .get("id")
        .and_then(Value::as_i64)
        .ok_or_else(|| DustError::Decode("missing id".into()))?;
    let max_id = obj
        .get("maxId")
        .and_then(Value::as_i64)
        .ok_or_else(|| DustError::Decode("missing maxId".into()))?;
    let raw_hex = obj
        .get("raw")
        .and_then(Value::as_str)
        .ok_or_else(|| DustError::Decode("missing raw".into()))?;
    let raw_bytes = hex::decode(raw_hex.trim_start_matches("0x"))
        .map_err(|e| DustError::Decode(format!("raw hex: {e}")))?;
    let event: ledger::events::Event<DefaultDB> =
        serialize::tagged_deserialize(&raw_bytes[..])
            .map_err(|e| DustError::Decode(format!("raw tagged: {e}")))?;
    Ok(DecodedEvent { id, max_id, event })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// We can't easily synthesise a valid scale-encoded
    /// ledger::events::Event<D> in a unit test without dragging
    /// in the full ledger machinery, so the decoder is exercised
    /// end-to-end against the live indexer in Task 12. Here we
    /// only test the JSON-walking layer with bad payloads.

    #[test]
    fn decode_missing_root_errors() {
        let data = json!({});
        let err = decode_event(&data).unwrap_err();
        assert!(matches!(err, DustError::Decode(_)));
    }

    #[test]
    fn decode_missing_id_errors() {
        let data = json!({
            "dustLedgerEvents": {
                "__typename": "DustInitialUtxo",
                "maxId": 10,
                "raw": "00"
            }
        });
        let err = decode_event(&data).unwrap_err();
        match err {
            DustError::Decode(msg) => assert!(msg.contains("id"), "msg={msg}"),
            other => panic!("expected Decode, got {other:?}"),
        }
    }

    #[test]
    fn decode_bad_raw_hex_errors() {
        let data = json!({
            "dustLedgerEvents": {
                "__typename": "DustInitialUtxo",
                "id": 0,
                "maxId": 0,
                "raw": "not-hex"
            }
        });
        let err = decode_event(&data).unwrap_err();
        match err {
            DustError::Decode(msg) => assert!(msg.contains("raw hex"), "msg={msg}"),
            other => panic!("expected Decode, got {other:?}"),
        }
    }
}
