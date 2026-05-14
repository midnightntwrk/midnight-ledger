//! Snapshot driver: open a graphql-transport-ws subscription,
//! replay create/spend events into a `UtxoSet`, terminate on the
//! first `UnshieldedTransactionsProgress` event.

use futures::{Stream, StreamExt};
use serde_json::{Value, json};

use super::{TokenType, UnshieldedError, UnshieldedUtxo, UtxoId, UtxoSet, transport};

/// The subscription document, embedded at compile time. We don't
/// run graphql_client codegen for subscriptions — the WS protocol
/// is hand-rolled (`transport.rs`), and the response shape is
/// narrow enough to decode by walking serde_json::Value.
pub(super) const UNSHIELDED_TRANSACTIONS_QUERY: &str = include_str!(
    "../../queries/midnight-indexer/unshielded_transactions.subscription.graphql"
);

/// One decoded element of the subscription stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Event {
    Transaction {
        /// Indexer-assigned transaction id (`Transaction.id` in the
        /// schema). Used to compare against `Progress.highestTransactionId`
        /// for termination — see `fold_events`.
        transaction_id: i64,
        created: Vec<UnshieldedUtxo>,
        spent: Vec<UtxoId>,
    },
    /// The indexer's high-water-mark for this address. **Arrives
    /// first**, before any historical Transaction events, so it is
    /// NOT a "caught up" signal — it's an "I'm currently at id N for
    /// this address" marker. `fold_events` terminates once it has
    /// consumed Transaction events up to that id.
    Progress {
        highest_transaction_id: i64,
    },
}

/// Decode one `next.payload.data.unshieldedTransactions` JSON value.
pub(super) fn decode_event(data: &Value) -> Result<Event, UnshieldedError> {
    let obj = data
        .get("unshieldedTransactions")
        .ok_or_else(|| UnshieldedError::Decode("missing unshieldedTransactions".into()))?;
    let typename = obj
        .get("__typename")
        .and_then(Value::as_str)
        .ok_or_else(|| UnshieldedError::Decode("missing __typename".into()))?;
    match typename {
        "UnshieldedTransaction" => {
            let transaction_id = obj
                .get("transaction")
                .and_then(|t| t.get("id"))
                .and_then(Value::as_i64)
                .ok_or_else(|| UnshieldedError::Decode("missing transaction.id".into()))?;
            let created_raw = obj
                .get("createdUtxos")
                .and_then(Value::as_array)
                .ok_or_else(|| UnshieldedError::Decode("missing createdUtxos".into()))?;
            let spent_raw = obj
                .get("spentUtxos")
                .and_then(Value::as_array)
                .ok_or_else(|| UnshieldedError::Decode("missing spentUtxos".into()))?;
            let created = created_raw
                .iter()
                .map(decode_utxo)
                .collect::<Result<Vec<_>, _>>()?;
            let spent = spent_raw
                .iter()
                .map(decode_utxo_id)
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Event::Transaction { transaction_id, created, spent })
        }
        "UnshieldedTransactionsProgress" => {
            let high = obj
                .get("highestTransactionId")
                .and_then(Value::as_i64)
                .ok_or_else(|| UnshieldedError::Decode("missing highestTransactionId".into()))?;
            Ok(Event::Progress {
                highest_transaction_id: high,
            })
        }
        other => Err(UnshieldedError::UnexpectedFrame(format!(
            "__typename={other}"
        ))),
    }
}

/// How long to wait after the most recent event before declaring the
/// snapshot done. Belt-and-braces for the case where a `Progress`
/// event was received with a high-water-mark we can never reach
/// (e.g. indexer reports a global tip rather than an address-scoped
/// one). Set comfortably above expected inter-frame latency.
const IDLE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// Fold an event stream into a `UtxoSet`, terminating when the
/// indexer has delivered the historical backlog.
///
/// Termination — verified against a live preprod-style standalone
/// indexer (`docs/superpowers/specs/2026-05-14-unshielded-sync-design.md`
/// open question #1):
/// the indexer emits `Progress { highest_transaction_id }` as the
/// **first** frame on the subscription — an "I'm currently at id N for
/// this address" marker — then streams the historical
/// `Transaction { transaction_id, … }` events. We track the highest
/// `transaction_id` consumed and exit once it meets or exceeds the
/// recorded target. Empty-history addresses fall out for free
/// because Progress arrives with `highest_transaction_id <= 0`, the
/// last_id starts at 0, and the comparison passes immediately.
///
/// An idle timeout (`IDLE_TIMEOUT`) is a backstop: if the indexer's
/// recorded high-water-mark is unreachable (shouldn't happen, but
/// would otherwise hang), the snapshot still returns once the
/// stream has been quiet for long enough — provided Progress has
/// been seen. Without Progress, an idle gap is still an error
/// (`StreamClosedEarly`).
///
/// Pulled out so this logic can be unit-tested against a hand-built
/// `Stream<Event>` with no live WS.
pub(super) async fn fold_events<S>(stream: S) -> Result<UtxoSet, UnshieldedError>
where
    S: Stream<Item = Result<Event, UnshieldedError>>,
{
    let mut set = UtxoSet::new();
    let mut target: Option<i64> = None;
    let mut last_seen_id: i64 = 0;
    futures::pin_mut!(stream);

    loop {
        // Early-exit check before each poll — handles the empty-history
        // address case where Progress(0) arrives with no transactions.
        if let Some(hi) = target {
            if last_seen_id >= hi {
                return Ok(set);
            }
        }

        let next = tokio::time::timeout(IDLE_TIMEOUT, stream.next()).await;
        match next {
            Ok(Some(item)) => match item? {
                Event::Transaction {
                    transaction_id,
                    created,
                    spent,
                } => {
                    for u in created {
                        set.insert(u);
                    }
                    for id in spent {
                        set.remove(&id);
                    }
                    if transaction_id > last_seen_id {
                        last_seen_id = transaction_id;
                    }
                }
                Event::Progress {
                    highest_transaction_id,
                } => {
                    target = Some(highest_transaction_id);
                }
            },
            // Stream closed cleanly — that's only OK if the indexer
            // explicitly told us we're caught up via `complete`,
            // which fold_events never sees directly. The transport
            // layer turns `complete` into stream-end, so we treat
            // an early end as `StreamClosedEarly` unless Progress
            // already told us we're done (handled by the early-exit
            // check at the top of the loop).
            Ok(None) => return Err(UnshieldedError::StreamClosedEarly),
            // Idle timeout. If Progress arrived earlier, treat the
            // silence as "no more historicals coming"; otherwise
            // surface the early stall as an error.
            Err(_elapsed) => {
                if target.is_some() {
                    return Ok(set);
                }
                return Err(UnshieldedError::StreamClosedEarly);
            }
        }
    }
}

/// Open a fresh graphql-transport-ws subscription against the
/// indexer, replay UTXO events into a `UtxoSet`, terminate on
/// the first `Progress` event. Closes the WS on return.
pub(crate) async fn snapshot(
    ws_url: &str,
    address: &str,
) -> Result<UtxoSet, UnshieldedError> {
    let stream = transport::subscribe(
        ws_url,
        UNSHIELDED_TRANSACTIONS_QUERY,
        json!({ "address": address, "transactionId": 0 }),
    )
    .await?;

    // Adapt the raw JSON stream into an `Event` stream so we can
    // reuse `fold_events`.
    let events = stream.map(|item| item.and_then(|v| decode_event(&v)));
    fold_events(events).await
}

fn decode_utxo(v: &Value) -> Result<UnshieldedUtxo, UnshieldedError> {
    let owner = v
        .get("owner")
        .and_then(Value::as_str)
        .ok_or_else(|| UnshieldedError::Decode("utxo.owner".into()))?
        .to_string();
    let token_hex = v
        .get("tokenType")
        .and_then(Value::as_str)
        .ok_or_else(|| UnshieldedError::Decode("utxo.tokenType".into()))?;
    let token_bytes = hex::decode(token_hex.trim_start_matches("0x"))
        .map_err(|e| UnshieldedError::Decode(format!("utxo.tokenType: {e}")))?;
    let value_str = v
        .get("value")
        .and_then(Value::as_str)
        .ok_or_else(|| UnshieldedError::Decode("utxo.value".into()))?;
    let value: u128 = value_str
        .parse()
        .map_err(|e| UnshieldedError::Decode(format!("utxo.value: {e}")))?;
    let intent_hash = decode_hash32("created.intentHash", v.get("intentHash"))?;
    let output_index = v
        .get("outputIndex")
        .and_then(Value::as_i64)
        .ok_or_else(|| UnshieldedError::Decode("utxo.outputIndex".into()))?;
    if !(0..=u32::MAX as i64).contains(&output_index) {
        return Err(UnshieldedError::Decode(format!(
            "utxo.outputIndex out of u32 range: {output_index}"
        )));
    }
    let ctime = v.get("ctime").and_then(Value::as_i64).and_then(|s| {
        if s >= 0 { Some(s as u64) } else { None }
    });
    let initial_nonce = decode_hash32("utxo.initialNonce", v.get("initialNonce"))?;

    Ok(UnshieldedUtxo {
        owner,
        token_type: TokenType(token_bytes),
        value,
        id: UtxoId {
            intent_hash,
            output_index: output_index as u32,
        },
        ctime,
        initial_nonce,
    })
}

fn decode_utxo_id(v: &Value) -> Result<UtxoId, UnshieldedError> {
    let intent_hash = decode_hash32("spent.intentHash", v.get("intentHash"))?;
    let output_index = v
        .get("outputIndex")
        .and_then(Value::as_i64)
        .ok_or_else(|| UnshieldedError::Decode("spent.outputIndex".into()))?;
    if !(0..=u32::MAX as i64).contains(&output_index) {
        return Err(UnshieldedError::Decode(format!(
            "spent.outputIndex out of u32 range: {output_index}"
        )));
    }
    Ok(UtxoId {
        intent_hash,
        output_index: output_index as u32,
    })
}

fn decode_hash32(field: &str, v: Option<&Value>) -> Result<[u8; 32], UnshieldedError> {
    let hex_str = v
        .and_then(Value::as_str)
        .ok_or_else(|| UnshieldedError::Decode(format!("{field}: missing")))?;
    let bytes = hex::decode(hex_str.trim_start_matches("0x"))
        .map_err(|e| UnshieldedError::Decode(format!("{field}: {e}")))?;
    if bytes.len() != 32 {
        return Err(UnshieldedError::Decode(format!(
            "{field}: expected 32 bytes, got {}",
            bytes.len()
        )));
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&bytes);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn intent_hex(b: u8) -> String {
        hex::encode([b; 32])
    }
    fn nonce_hex(b: u8) -> String {
        hex::encode([b; 32])
    }

    #[test]
    fn decode_progress_event() {
        let data = json!({
            "unshieldedTransactions": {
                "__typename": "UnshieldedTransactionsProgress",
                "highestTransactionId": 42
            }
        });
        let ev = decode_event(&data).expect("decode");
        assert_eq!(
            ev,
            Event::Progress { highest_transaction_id: 42 }
        );
    }

    #[test]
    fn decode_transaction_with_created_and_spent() {
        let data = json!({
            "unshieldedTransactions": {
                "__typename": "UnshieldedTransaction",
                "transaction": { "id": 7 },
                "createdUtxos": [{
                    "owner": "mn_addr_test1abcd",
                    "tokenType": hex::encode([0xAB]),
                    "value": "1000000",
                    "intentHash": intent_hex(0x11),
                    "outputIndex": 0,
                    "ctime": 1_700_000_000,
                    "initialNonce": nonce_hex(0x22)
                }],
                "spentUtxos": [{
                    "intentHash": intent_hex(0x33),
                    "outputIndex": 1
                }]
            }
        });
        let ev = decode_event(&data).expect("decode");
        match ev {
            Event::Transaction { transaction_id, created, spent } => {
                assert_eq!(transaction_id, 7);
                assert_eq!(created.len(), 1);
                assert_eq!(created[0].value, 1_000_000);
                assert_eq!(created[0].id.output_index, 0);
                assert_eq!(created[0].id.intent_hash, [0x11; 32]);
                assert_eq!(created[0].initial_nonce, [0x22; 32]);
                assert_eq!(created[0].token_type.0, vec![0xAB]);
                assert_eq!(spent.len(), 1);
                assert_eq!(spent[0].intent_hash, [0x33; 32]);
                assert_eq!(spent[0].output_index, 1);
            }
            other => panic!("expected Transaction, got {other:?}"),
        }
    }

    #[test]
    fn decode_transaction_with_empty_arrays() {
        let data = json!({
            "unshieldedTransactions": {
                "__typename": "UnshieldedTransaction",
                "transaction": { "id": 0 },
                "createdUtxos": [],
                "spentUtxos": []
            }
        });
        let ev = decode_event(&data).expect("decode");
        match ev {
            Event::Transaction { transaction_id, created, spent } => {
                assert_eq!(transaction_id, 0);
                assert!(created.is_empty());
                assert!(spent.is_empty());
            }
            other => panic!("expected Transaction, got {other:?}"),
        }
    }

    #[test]
    fn decode_missing_transaction_id_errors() {
        let data = json!({
            "unshieldedTransactions": {
                "__typename": "UnshieldedTransaction",
                "createdUtxos": [],
                "spentUtxos": []
            }
        });
        let err = decode_event(&data).unwrap_err();
        match err {
            UnshieldedError::Decode(msg) => {
                assert!(msg.contains("transaction.id"), "msg={msg}");
            }
            other => panic!("expected Decode, got {other:?}"),
        }
    }

    #[test]
    fn decode_unknown_typename_errors() {
        let data = json!({
            "unshieldedTransactions": {
                "__typename": "SomethingElse"
            }
        });
        let err = decode_event(&data).unwrap_err();
        assert!(matches!(err, UnshieldedError::UnexpectedFrame(_)));
    }

    #[test]
    fn decode_missing_root_errors() {
        let data = json!({});
        let err = decode_event(&data).unwrap_err();
        assert!(matches!(err, UnshieldedError::Decode(_)));
    }

    #[test]
    fn decode_bad_intent_hash_length() {
        let data = json!({
            "unshieldedTransactions": {
                "__typename": "UnshieldedTransaction",
                "transaction": { "id": 1 },
                "createdUtxos": [],
                "spentUtxos": [{
                    "intentHash": hex::encode([0x44; 16]),
                    "outputIndex": 0
                }]
            }
        });
        let err = decode_event(&data).unwrap_err();
        assert!(matches!(err, UnshieldedError::Decode(_)));
    }

    #[test]
    fn decode_negative_output_index_errors() {
        let data = json!({
            "unshieldedTransactions": {
                "__typename": "UnshieldedTransaction",
                "transaction": { "id": 1 },
                "createdUtxos": [],
                "spentUtxos": [{
                    "intentHash": intent_hex(0x55),
                    "outputIndex": -1
                }]
            }
        });
        let err = decode_event(&data).unwrap_err();
        match err {
            UnshieldedError::Decode(msg) => {
                assert!(msg.contains("outputIndex"), "msg={msg}");
            }
            other => panic!("expected Decode, got {other:?}"),
        }
    }

    #[test]
    fn decode_value_not_string_errors() {
        // The indexer documents `value: String` (u128-as-decimal).
        // A bare JSON number would slip past `as_str`; verify we
        // reject it with a field-named error.
        let data = json!({
            "unshieldedTransactions": {
                "__typename": "UnshieldedTransaction",
                "transaction": { "id": 1 },
                "createdUtxos": [{
                    "owner": "mn_addr_test1abcd",
                    "tokenType": hex::encode([0xAB]),
                    "value": 1_000_000,
                    "intentHash": intent_hex(0x11),
                    "outputIndex": 0,
                    "ctime": 1_700_000_000,
                    "initialNonce": nonce_hex(0x22)
                }],
                "spentUtxos": []
            }
        });
        let err = decode_event(&data).unwrap_err();
        match err {
            UnshieldedError::Decode(msg) => {
                assert!(msg.contains("utxo.value"), "msg={msg}");
            }
            other => panic!("expected Decode, got {other:?}"),
        }
    }

    #[test]
    fn decode_missing_token_type_errors() {
        let data = json!({
            "unshieldedTransactions": {
                "__typename": "UnshieldedTransaction",
                "transaction": { "id": 1 },
                "createdUtxos": [{
                    "owner": "mn_addr_test1abcd",
                    "value": "100",
                    "intentHash": intent_hex(0x11),
                    "outputIndex": 0,
                    "initialNonce": nonce_hex(0x22)
                }],
                "spentUtxos": []
            }
        });
        let err = decode_event(&data).unwrap_err();
        match err {
            UnshieldedError::Decode(msg) => {
                assert!(msg.contains("tokenType"), "msg={msg}");
            }
            other => panic!("expected Decode, got {other:?}"),
        }
    }

    #[test]
    fn decode_bad_initial_nonce_length_errors() {
        let data = json!({
            "unshieldedTransactions": {
                "__typename": "UnshieldedTransaction",
                "transaction": { "id": 1 },
                "createdUtxos": [{
                    "owner": "mn_addr_test1abcd",
                    "tokenType": hex::encode([0xAB]),
                    "value": "100",
                    "intentHash": intent_hex(0x11),
                    "outputIndex": 0,
                    "initialNonce": hex::encode([0xCC; 8])
                }],
                "spentUtxos": []
            }
        });
        let err = decode_event(&data).unwrap_err();
        match err {
            UnshieldedError::Decode(msg) => {
                assert!(msg.contains("initialNonce"), "msg={msg}");
            }
            other => panic!("expected Decode, got {other:?}"),
        }
    }

    use crate::unshielded::{TokenType, UnshieldedUtxo, UtxoId};
    use futures::stream;

    fn utxo(intent: u8, idx: u32, token: u8, value: u128) -> UnshieldedUtxo {
        UnshieldedUtxo {
            owner: "mn_addr_test1abcd".to_string(),
            token_type: TokenType(vec![token]),
            value,
            id: UtxoId {
                intent_hash: [intent; 32],
                output_index: idx,
            },
            ctime: Some(1_700_000_000),
            initial_nonce: [0u8; 32],
        }
    }

    #[tokio::test]
    async fn fold_progress_first_then_transactions_terminates_at_target() {
        // Real-indexer ordering (verified by direct WS probe against a
        // local standalone stack — open question #1 from the design
        // spec): `Progress { hi }` arrives BEFORE the historical
        // `Transaction` events. fold_events must keep consuming
        // until `last_seen_id >= hi`, not bail on the first Progress.
        // Beyond the id that matches the target, further events are
        // not consumed — they belong to the post-snapshot live tail.
        let events: Vec<Result<Event, UnshieldedError>> = vec![
            Ok(Event::Progress { highest_transaction_id: 9 }),
            Ok(Event::Transaction {
                transaction_id: 4,
                created: vec![utxo(1, 0, 0xAB, 100)],
                spent: vec![],
            }),
            Ok(Event::Transaction {
                transaction_id: 6,
                created: vec![utxo(2, 0, 0xAB, 100)],
                spent: vec![],
            }),
            Ok(Event::Transaction {
                transaction_id: 9,
                created: vec![utxo(3, 0, 0xAB, 100)],
                spent: vec![],
            }),
            // Below should never be reached: fold should have
            // terminated on the id=9 event matching target=9.
            Ok(Event::Transaction {
                transaction_id: 11,
                created: vec![utxo(4, 0, 0xAB, 9999)],
                spent: vec![],
            }),
        ];
        let set = fold_events(stream::iter(events)).await.expect("ok");
        assert_eq!(set.len(), 3);
        assert_eq!(set.total_for(&TokenType(vec![0xAB])), 300);
    }

    #[tokio::test]
    async fn fold_empty_address_progress_zero_terminates_immediately() {
        // Empty-history address: Progress(0) arrives, no Transaction
        // events follow. last_seen_id starts at 0, so the early-exit
        // check at the top of the loop fires on the next iteration.
        let events: Vec<Result<Event, UnshieldedError>> = vec![
            Ok(Event::Progress { highest_transaction_id: 0 }),
            // Should never be reached:
            Ok(Event::Transaction {
                transaction_id: 99,
                created: vec![utxo(1, 0, 0xAB, 9999)],
                spent: vec![],
            }),
        ];
        let set = fold_events(stream::iter(events)).await.expect("ok");
        assert!(set.is_empty());
    }

    #[tokio::test]
    async fn fold_applies_create_then_spend() {
        let id_a = UtxoId {
            intent_hash: [0x11; 32],
            output_index: 0,
        };
        let events: Vec<Result<Event, UnshieldedError>> = vec![
            Ok(Event::Progress { highest_transaction_id: 2 }),
            Ok(Event::Transaction {
                transaction_id: 1,
                created: vec![utxo(0x11, 0, 0xAB, 100)],
                spent: vec![],
            }),
            Ok(Event::Transaction {
                transaction_id: 2,
                created: vec![],
                spent: vec![id_a],
            }),
        ];
        let set = fold_events(stream::iter(events)).await.expect("ok");
        assert!(set.is_empty());
    }

    #[tokio::test]
    async fn fold_returns_stream_closed_early_without_progress() {
        // Stream ends without ever emitting a Progress event. We
        // don't know whether we're caught up, so the snapshot fails
        // rather than silently returning a partial set.
        let events: Vec<Result<Event, UnshieldedError>> = vec![
            Ok(Event::Transaction {
                transaction_id: 1,
                created: vec![utxo(1, 0, 0xAB, 100)],
                spent: vec![],
            }),
        ];
        let err = fold_events(stream::iter(events)).await.unwrap_err();
        assert!(matches!(err, UnshieldedError::StreamClosedEarly));
    }

    #[tokio::test]
    async fn fold_propagates_first_error() {
        let events: Vec<Result<Event, UnshieldedError>> = vec![
            Ok(Event::Transaction {
                transaction_id: 1,
                created: vec![utxo(1, 0, 0xAB, 100)],
                spent: vec![],
            }),
            Err(UnshieldedError::Decode("boom".into())),
            Ok(Event::Progress { highest_transaction_id: 1 }),
        ];
        let err = fold_events(stream::iter(events)).await.unwrap_err();
        assert!(matches!(err, UnshieldedError::Decode(_)));
    }

    #[tokio::test]
    async fn fold_handles_empty_transaction_events() {
        let events: Vec<Result<Event, UnshieldedError>> = vec![
            Ok(Event::Progress { highest_transaction_id: 1 }),
            Ok(Event::Transaction {
                transaction_id: 1,
                created: vec![],
                spent: vec![],
            }),
        ];
        let set = fold_events(stream::iter(events)).await.expect("ok");
        assert!(set.is_empty());
    }
}
