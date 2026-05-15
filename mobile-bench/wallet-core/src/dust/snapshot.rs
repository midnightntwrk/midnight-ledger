//! Snapshot driver: subscribe to `dustLedgerEvents`, decode each
//! into a `ledger::events::Event<DefaultDB>` via the `raw` field,
//! collect them, then call `DustLocalState::replay_events` to
//! hydrate the wallet's DUST state. See the design spec for the
//! termination semantics.

use futures::{Stream, StreamExt};
use ledger::dust::DustLocalState;
use serde_json::{Value, json};
use storage::DefaultDB;

use crate::unshielded::transport;
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

const IDLE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// Fold the event stream into an ordered Vec<ledger::events::Event>,
/// then replay against an empty `DustLocalState` to produce the
/// hydrated state. Termination: stop after seeing an event with
/// `id == max_id` (the indexer's own "you're caught up" marker).
/// Idle timeout (5s) is the belt-and-braces backstop.
pub(super) async fn fold_events<S>(
    stream: S,
    sk: &ledger::dust::DustSecretKey,
    params: ledger::dust::DustParameters,
) -> Result<DustLocalState<DefaultDB>, DustError>
where
    S: Stream<Item = Result<DecodedEvent, DustError>>,
{
    let mut events: Vec<ledger::events::Event<DefaultDB>> = Vec::new();
    let mut last_id: i64 = -1;
    let mut target_max: Option<i64> = None;
    futures::pin_mut!(stream);

    loop {
        // Early exit on caught-up.
        if let Some(max) = target_max {
            if last_id >= max {
                break;
            }
        }

        let next = tokio::time::timeout(IDLE_TIMEOUT, stream.next()).await;
        match next {
            Ok(Some(item)) => {
                let DecodedEvent { id, max_id, event } = item?;
                events.push(event);
                last_id = id;
                target_max = Some(max_id);
            }
            Ok(None) => {
                if target_max.is_some() {
                    break;
                }
                return Err(DustError::StreamClosedEarly);
            }
            Err(_) => {
                if target_max.is_some() {
                    break;
                }
                return Err(DustError::StreamClosedEarly);
            }
        }
    }

    let state = DustLocalState::new(params);
    state
        .replay_events(sk, events.iter())
        .map_err(|e| DustError::Replay(e.to_string()))
}

/// Open a subscription, fold, hydrate. Caller supplies the dust
/// secret key + params.
pub(crate) async fn snapshot(
    ws_url: &str,
    sk: &ledger::dust::DustSecretKey,
    params: ledger::dust::DustParameters,
) -> Result<DustLocalState<DefaultDB>, DustError> {
    let stream = transport::subscribe(
        ws_url,
        DUST_LEDGER_EVENTS_QUERY,
        json!({ "id": 0 }),
    )
    .await
    .map_err(translate_unshielded_error)?;

    let events = stream.map(|item| {
        item.map_err(translate_unshielded_error)
            .and_then(|v| decode_event(&v))
    });
    fold_events(events, sk, params).await
}

fn translate_unshielded_error(e: crate::unshielded::UnshieldedError) -> DustError {
    use crate::unshielded::UnshieldedError as U;
    match e {
        U::WsConnect(s) => DustError::WsConnect(s),
        U::WsHandshake(s) => DustError::WsHandshake(s),
        U::GqlError(s) => DustError::GqlError(s),
        U::UnexpectedFrame(s) => DustError::UnexpectedFrame(s),
        U::Decode(s) => DustError::Decode(s),
        U::StreamClosedEarly => DustError::StreamClosedEarly,
        U::InvalidAddress(s) => DustError::InvalidPublicKey(s),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    use futures::stream;

    /// Empty event stream with no Progress observed → StreamClosedEarly.
    /// The real happy path runs against live events in Task 12.
    #[tokio::test]
    async fn fold_returns_stream_closed_early_when_no_events() {
        let events: Vec<Result<DecodedEvent, DustError>> = vec![];
        let mut rng = rand::rngs::OsRng;
        let sk = ledger::dust::DustSecretKey::sample(&mut rng);
        let params = ledger::structure::INITIAL_PARAMETERS.dust;
        let result = fold_events(stream::iter(events), &sk, params).await;
        assert!(matches!(result, Err(DustError::StreamClosedEarly)));
    }
}
