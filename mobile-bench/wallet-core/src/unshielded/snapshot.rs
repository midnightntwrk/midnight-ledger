//! Snapshot driver: open a graphql-transport-ws subscription,
//! replay create/spend events into a `UtxoSet`, terminate on the
//! first `UnshieldedTransactionsProgress` event.

use serde_json::Value;

use super::{TokenType, UnshieldedError, UnshieldedUtxo, UtxoId};

/// The subscription document, embedded at compile time. We don't
/// run graphql_client codegen for subscriptions — the WS protocol
/// is hand-rolled (`transport.rs`), and the response shape is
/// narrow enough to decode by walking serde_json::Value.
#[allow(dead_code)] // Used by snapshot driver in Task 4 and tests.
pub(super) const UNSHIELDED_TRANSACTIONS_QUERY: &str = include_str!(
    "../../queries/midnight-indexer/unshielded_transactions.subscription.graphql"
);

/// One decoded element of the subscription stream.
#[allow(dead_code)] // Used by snapshot driver in Task 4 and tests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Event {
    Transaction {
        created: Vec<UnshieldedUtxo>,
        spent: Vec<UtxoId>,
    },
    /// The indexer's "you're caught up" signal. The carried
    /// `highest_transaction_id` is informational — we terminate on
    /// any Progress event regardless of value.
    Progress {
        highest_transaction_id: i64,
    },
}

/// Decode one `next.payload.data.unshieldedTransactions` JSON value.
#[allow(dead_code)] // Used by snapshot driver in Task 4 and tests.
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
            Ok(Event::Transaction { created, spent })
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

#[allow(dead_code)] // Used by snapshot driver in Task 4 and tests.
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

#[allow(dead_code)] // Used by snapshot driver in Task 4 and tests.
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

#[allow(dead_code)] // Used by snapshot driver in Task 4 and tests.
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
            Event::Transaction { created, spent } => {
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
                "createdUtxos": [],
                "spentUtxos": []
            }
        });
        let ev = decode_event(&data).expect("decode");
        match ev {
            Event::Transaction { created, spent } => {
                assert!(created.is_empty());
                assert!(spent.is_empty());
            }
            other => panic!("expected Transaction, got {other:?}"),
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
}
