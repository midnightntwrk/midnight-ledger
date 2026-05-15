//! Minimal graphql-transport-ws client. One subscription per WS, no
//! multiplexing. Callers receive an async `Stream<Result<Value, _>>`
//! of `next.payload.data` JSON values; framing is hidden inside.

use std::time::Duration;

use futures::{SinkExt, Stream, StreamExt};
use serde_json::{Value, json};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{
        Message,
        client::IntoClientRequest,
        protocol::{CloseFrame, frame::coding::CloseCode},
    },
};

use super::UnshieldedError;

/// Subscribe over a fresh graphql-transport-ws WebSocket. Returns
/// an async stream of `next.payload.data` JSON values, ending when
/// the server sends `complete`/`error` or when the WS drops.
pub(crate) async fn subscribe(
    ws_url: &str,
    query: &str,
    variables: Value,
) -> Result<impl Stream<Item = Result<Value, UnshieldedError>>, UnshieldedError> {
    let mut req = ws_url
        .into_client_request()
        .map_err(|e| UnshieldedError::WsConnect(e.to_string()))?;
    req.headers_mut().insert(
        "Sec-WebSocket-Protocol",
        "graphql-transport-ws"
            .parse()
            .expect("static subprotocol header parses"),
    );

    let (mut ws, _resp) = tokio::time::timeout(Duration::from_secs(15), connect_async(req))
        .await
        .map_err(|_| UnshieldedError::WsConnect("connect timeout".into()))?
        .map_err(|e| UnshieldedError::WsConnect(e.to_string()))?;

    // Handshake: connection_init -> connection_ack
    let init = json!({ "type": "connection_init", "payload": {} }).to_string();
    ws.send(Message::Text(init))
        .await
        .map_err(|e| UnshieldedError::WsHandshake(e.to_string()))?;

    loop {
        let frame = tokio::time::timeout(Duration::from_secs(15), ws.next())
            .await
            .map_err(|_| UnshieldedError::WsHandshake("ack timeout".into()))?
            .ok_or_else(|| UnshieldedError::WsHandshake("ws closed before ack".into()))?
            .map_err(|e| UnshieldedError::WsHandshake(e.to_string()))?;
        match parse_text_frame(&frame)? {
            Some(v) => match frame_type(&v) {
                Some("connection_ack") => break,
                Some("ping") => {
                    let pong = json!({ "type": "pong" }).to_string();
                    ws.send(Message::Text(pong))
                        .await
                        .map_err(|e| UnshieldedError::WsHandshake(e.to_string()))?;
                }
                Some(t) => {
                    return Err(UnshieldedError::WsHandshake(format!(
                        "unexpected pre-ack frame type {t}"
                    )));
                }
                None => {
                    return Err(UnshieldedError::WsHandshake("frame missing type".into()));
                }
            },
            None => continue,
        }
    }

    // subscribe frame
    let sub = json!({
        "type": "subscribe",
        "id": "1",
        "payload": { "query": query, "variables": variables },
    })
    .to_string();
    ws.send(Message::Text(sub))
        .await
        .map_err(|e| UnshieldedError::WsHandshake(e.to_string()))?;

    Ok(into_data_stream(ws))
}

fn into_data_stream(
    ws: tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
) -> impl Stream<Item = Result<Value, UnshieldedError>> {
    async_stream::stream! {
        let mut ws = ws;
        loop {
            let item = match ws.next().await {
                None => break,
                Some(Err(e)) => {
                    yield Err(UnshieldedError::WsConnect(e.to_string()));
                    break;
                }
                Some(Ok(m)) => m,
            };
            match parse_text_frame(&item) {
                Err(e) => {
                    yield Err(e);
                    break;
                }
                Ok(None) => continue,
                Ok(Some(v)) => match frame_type(&v) {
                    Some("next") => {
                        match v.get("payload").and_then(|p| p.get("data")) {
                            Some(data) => yield Ok(data.clone()),
                            None => {
                                yield Err(UnshieldedError::Decode(
                                    "next frame missing payload.data".into(),
                                ));
                                break;
                            }
                        }
                    }
                    Some("error") => {
                        let payload = v.get("payload").map(|p| p.to_string())
                            .unwrap_or_else(|| "<no payload>".into());
                        yield Err(UnshieldedError::GqlError(payload));
                        break;
                    }
                    Some("complete") => break,
                    Some("ping") => {
                        let pong = json!({ "type": "pong" }).to_string();
                        if let Err(e) = ws.send(Message::Text(pong)).await {
                            yield Err(UnshieldedError::WsConnect(e.to_string()));
                            break;
                        }
                    }
                    Some("pong") => continue,
                    other => {
                        let _ = ws.close(Some(CloseFrame {
                            code: CloseCode::Normal,
                            reason: "unexpected frame".into(),
                        })).await;
                        yield Err(UnshieldedError::UnexpectedFrame(
                            format!("type={other:?}"),
                        ));
                        break;
                    }
                },
            }
        }
        let _ = ws.close(None).await;
    }
}

/// Returns `Ok(Some(v))` for parsed text frames, `Ok(None)` for
/// non-text we ignore (binary, pong, close), or `Err` on bad JSON.
pub(super) fn parse_text_frame(m: &Message) -> Result<Option<Value>, UnshieldedError> {
    match m {
        Message::Text(s) => {
            let v: Value = serde_json::from_str(s)
                .map_err(|e| UnshieldedError::UnexpectedFrame(format!("bad json: {e}")))?;
            Ok(Some(v))
        }
        Message::Binary(_) | Message::Pong(_) | Message::Ping(_) | Message::Frame(_) => Ok(None),
        // Close: delegate to the next ws.next() returning None to end
        // the stream — tungstenite drives the close handshake.
        Message::Close(_) => Ok(None),
    }
}

pub(super) fn frame_type(v: &Value) -> Option<&str> {
    v.get("type").and_then(Value::as_str)
}

/// Render the `connection_init` payload as wire text. Pulled out
/// for unit tests.
#[cfg(test)]
pub(super) fn connection_init_frame() -> String {
    json!({ "type": "connection_init", "payload": {} }).to_string()
}

/// Render the `subscribe` payload as wire text. Pulled out
/// for unit tests.
#[cfg(test)]
pub(super) fn subscribe_frame(query: &str, variables: Value) -> String {
    json!({
        "type": "subscribe",
        "id": "1",
        "payload": { "query": query, "variables": variables },
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tokio_tungstenite::tungstenite::Message;

    #[test]
    fn connection_init_frame_shape() {
        let s = connection_init_frame();
        let v: Value = serde_json::from_str(&s).unwrap();
        assert_eq!(v.get("type").and_then(Value::as_str), Some("connection_init"));
        assert!(v.get("payload").is_some());
    }

    #[test]
    fn subscribe_frame_shape() {
        let s = subscribe_frame("subscription X { x }", json!({"a": 1}));
        let v: Value = serde_json::from_str(&s).unwrap();
        assert_eq!(v.get("type").and_then(Value::as_str), Some("subscribe"));
        assert_eq!(v.get("id").and_then(Value::as_str), Some("1"));
        let payload = v.get("payload").unwrap();
        assert_eq!(
            payload.get("query").and_then(Value::as_str),
            Some("subscription X { x }")
        );
        assert_eq!(payload.get("variables").unwrap(), &json!({"a": 1}));
    }

    #[test]
    fn parse_next_frame() {
        let raw = json!({
            "type": "next",
            "id": "1",
            "payload": { "data": { "foo": 42 } }
        })
        .to_string();
        let m = Message::Text(raw);
        let v = parse_text_frame(&m).unwrap().unwrap();
        assert_eq!(frame_type(&v), Some("next"));
    }

    #[test]
    fn parse_complete_frame() {
        let raw = json!({"type": "complete", "id": "1"}).to_string();
        let v = parse_text_frame(&Message::Text(raw)).unwrap().unwrap();
        assert_eq!(frame_type(&v), Some("complete"));
    }

    #[test]
    fn parse_binary_returns_none() {
        let m = Message::Binary(vec![1, 2, 3]);
        assert!(parse_text_frame(&m).unwrap().is_none());
    }

    #[test]
    fn parse_bad_json_errors() {
        let m = Message::Text("not json".into());
        let err = parse_text_frame(&m).unwrap_err();
        assert!(matches!(err, UnshieldedError::UnexpectedFrame(_)));
    }
}
