//! M5.7 — Alerts WebSocket integration tests.
//!
//! Stands up a tiny tokio WebSocket "PMS" replica that:
//! 1. accepts a connection at `/:/websockets/notifications`
//! 2. verifies the `X-Plex-Token` query parameter on the request
//! 3. emits a handful of pre-crafted `NotificationContainer`
//!    frames in sequence
//! 4. closes the socket cleanly
//!
//! and asserts the `Alerts` stream yields the expected
//! `AlertEvent` variants in order.

#![cfg(feature = "alerts")]

use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use plex_rs::alerts::{AlertEvent, Alerts, TranscodeLifecycle};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::{Message, handshake::server::Request};

/// Spin up a one-shot WebSocket server on a random localhost port.
///
/// Returns the `ws://...` URL prefix and a join handle. The server:
/// - records the request URI for handshake assertions
/// - emits each frame in `frames` in order
/// - closes the connection cleanly afterwards
async fn spawn_ws_server(frames: Vec<String>) -> (String, tokio::task::JoinHandle<Option<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let prefix = format!("ws://{addr}");
    let frames = Arc::new(frames);
    let handle = tokio::spawn(async move {
        let (stream, _peer) = listener.accept().await.expect("accept");
        let captured_uri: Arc<std::sync::Mutex<Option<String>>> =
            Arc::new(std::sync::Mutex::new(None));
        let captured_clone = captured_uri.clone();
        #[allow(clippy::result_large_err)]
        let ws = tokio_tungstenite::accept_hdr_async(stream, |req: &Request, resp| {
            *captured_clone.lock().unwrap() = Some(req.uri().to_string());
            Ok(resp)
        })
        .await
        .expect("ws handshake");
        let (mut writer, _reader) = ws.split();
        for frame in frames.iter() {
            writer
                .send(Message::Text(frame.clone().into()))
                .await
                .expect("send");
        }
        let _ = writer.send(Message::Close(None)).await;
        captured_uri.lock().unwrap().clone()
    });
    (prefix, handle)
}

fn playing_frame() -> String {
    r#"{"NotificationContainer":{
        "type":"playing","size":1,
        "PlaySessionStateNotification":[{
            "sessionKey":"42","ratingKey":"100",
            "key":"/library/metadata/100",
            "viewOffset":12345,"state":"playing"
        }]
    }}"#
    .to_owned()
}

fn timeline_frame() -> String {
    r#"{"NotificationContainer":{
        "type":"timeline","size":2,
        "TimelineEntry":[
            {"itemID":1,"state":5,"title":"Arrival"},
            {"itemID":2,"state":9,"title":"Dune"}
        ]
    }}"#
    .to_owned()
}

fn transcode_start_frame() -> String {
    r#"{"NotificationContainer":{
        "type":"transcodeSession.start","size":1,
        "TranscodeSession":[{"key":"/transcode/session/X","throttled":false}]
    }}"#
    .to_owned()
}

#[tokio::test]
async fn alerts_stream_yields_decoded_events_in_order_end_to_end() {
    let (ws_prefix, server_handle) = spawn_ws_server(vec![
        playing_frame(),
        timeline_frame(),
        transcode_start_frame(),
    ])
    .await;
    let ws_url = format!("{ws_prefix}/:/websockets/notifications?X-Plex-Token=ws-test-token");
    let mut stream = Alerts::connect_with_url(&ws_url).await.unwrap();
    let mut events: Vec<AlertEvent> = Vec::new();
    while let Ok(Some(ev)) = tokio::time::timeout(Duration::from_secs(2), stream.next()).await {
        match ev {
            Ok(e) => events.push(e),
            Err(_) => break,
        }
    }
    // playing (1) + timeline (2) + transcode (1) = 4 events
    assert_eq!(events.len(), 4, "got events: {events:?}");
    match &events[0] {
        AlertEvent::Playing(p) => {
            assert_eq!(p.session_key, "42");
            assert_eq!(p.state, "playing");
        }
        other => panic!("expected Playing, got {other:?}"),
    }
    match &events[1] {
        AlertEvent::Timeline(t) => assert_eq!(t.item_id, 1),
        other => panic!("expected Timeline, got {other:?}"),
    }
    match &events[2] {
        AlertEvent::Timeline(t) => assert_eq!(t.item_id, 2),
        other => panic!("expected Timeline, got {other:?}"),
    }
    match &events[3] {
        AlertEvent::TranscodeSession(t) => {
            assert_eq!(t.lifecycle, TranscodeLifecycle::Start);
        }
        other => panic!("expected TranscodeSession, got {other:?}"),
    }
    // Confirm the handshake URI carried the token query param.
    let captured = server_handle.await.unwrap().unwrap();
    assert!(
        captured.contains("X-Plex-Token=ws-test-token"),
        "captured uri: {captured}"
    );
    assert!(
        captured.starts_with("/:/websockets/notifications"),
        "captured uri: {captured}"
    );
}

#[tokio::test]
async fn alerts_stream_terminates_on_clean_close() {
    let (ws_prefix, server_handle) = spawn_ws_server(vec![playing_frame()]).await;
    let ws_url = format!("{ws_prefix}/:/websockets/notifications?X-Plex-Token=t");
    let mut stream = Alerts::connect_with_url(&ws_url).await.unwrap();
    let first = tokio::time::timeout(Duration::from_secs(2), stream.next())
        .await
        .unwrap();
    assert!(matches!(first, Some(Ok(AlertEvent::Playing(_)))));
    // Drain — after the close frame the stream should end.
    let second = tokio::time::timeout(Duration::from_secs(2), stream.next())
        .await
        .unwrap();
    assert!(
        matches!(second, None | Some(Err(_))),
        "expected end of stream, got {second:?}"
    );
    server_handle.await.unwrap();
}

#[tokio::test]
async fn alerts_connect_returns_error_for_unreachable_url() {
    // Port 1 on localhost is reserved and refuses connections.
    let err = Alerts::connect_with_url("ws://127.0.0.1:1/")
        .await
        .unwrap_err();
    use plex_rs::error::Error;
    assert!(
        matches!(err, Error::Config(ref msg) if msg.contains("websocket")),
        "expected Config error mentioning 'websocket', got {err:?}"
    );
}
