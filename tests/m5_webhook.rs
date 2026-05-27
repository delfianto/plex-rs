//! M5.9 — Webhook ingest integration tests.
//!
//! Stands up an axum `Router` with a `WebhookPayload` extractor and
//! POSTs a multipart-form-data request the same shape Plex sends.
//! Asserts the extractor:
//! 1. parses the JSON `payload` field into a typed event
//! 2. captures the `thumb` binary attachment alongside it
//! 3. rejects malformed requests with a 400

#![cfg(feature = "webhook-axum")]

use std::sync::Arc;
use std::time::Duration;

use axum::{Router, body::Body, http::StatusCode, http::header, routing::post};
use plex_rs::webhook::{WebhookEvent, WebhookPayload};
use tokio::net::TcpListener;
use tokio::sync::Mutex;

/// Spin up an axum server on a random port that records the
/// extracted payload in the supplied `last_payload` slot, then
/// returns 204 No Content. Returns the bound URL.
async fn spawn_axum_server(
    last_payload: Arc<Mutex<Option<WebhookPayload>>>,
) -> (String, tokio::task::JoinHandle<()>) {
    let app = {
        let slot = last_payload.clone();
        Router::new().route(
            "/plex",
            post(move |payload: WebhookPayload| {
                let slot = slot.clone();
                async move {
                    *slot.lock().await = Some(payload);
                    StatusCode::NO_CONTENT
                }
            }),
        )
    };
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let url = format!("http://{addr}/plex");
    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (url, handle)
}

/// Build a multipart body the same shape Plex sends (boundary
/// chosen at random; one `payload` text field, optional `thumb`
/// binary field).
fn build_multipart(payload_json: &str, thumb: Option<&[u8]>) -> (String, Vec<u8>) {
    let boundary = "----plex-rs-test-boundary-deadbeef";
    let mut body = Vec::new();
    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body.extend_from_slice(
        b"Content-Disposition: form-data; name=\"payload\"\r\n\
          Content-Type: application/json\r\n\r\n",
    );
    body.extend_from_slice(payload_json.as_bytes());
    body.extend_from_slice(b"\r\n");
    if let Some(thumb_bytes) = thumb {
        body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        body.extend_from_slice(
            b"Content-Disposition: form-data; name=\"thumb\"; filename=\"poster.jpg\"\r\n\
              Content-Type: image/jpeg\r\n\r\n",
        );
        body.extend_from_slice(thumb_bytes);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    let content_type = format!("multipart/form-data; boundary={boundary}");
    (content_type, body)
}

fn sample_play_payload_json() -> String {
    serde_json::json!({
        "event": "media.play",
        "user": true,
        "owner": true,
        "Account": {"id": 1, "title": "alice"},
        "Server": {"title": "Living Room", "uuid": "abc"},
        "Player": {"local": true, "publicAddress": "1.2.3.4",
                   "title": "Apple TV", "uuid": "player"},
        "Metadata": {"ratingKey": "42", "key": "/library/metadata/42",
                     "type": "movie", "title": "Arrival"}
    })
    .to_string()
}

async fn post_multipart(url: &str, content_type: &str, body: Vec<u8>) -> reqwest::Response {
    reqwest::Client::new()
        .post(url)
        .header(header::CONTENT_TYPE, content_type)
        .body(body)
        .send()
        .await
        .unwrap()
}

#[tokio::test]
async fn axum_extractor_parses_media_play_payload() {
    let slot: Arc<Mutex<Option<WebhookPayload>>> = Arc::new(Mutex::new(None));
    let (url, handle) = spawn_axum_server(slot.clone()).await;
    // Tiny delay to ensure axum is ready.
    tokio::time::sleep(Duration::from_millis(50)).await;
    let (content_type, body) = build_multipart(&sample_play_payload_json(), None);
    let resp = post_multipart(&url, &content_type, body).await;
    assert_eq!(resp.status(), 204);
    let got = slot.lock().await.take().unwrap();
    assert_eq!(got.event, WebhookEvent::MediaPlay);
    assert!(got.user);
    assert!(got.owner);
    assert_eq!(got.account.unwrap().title.as_deref(), Some("alice"));
    let meta = got.metadata.unwrap();
    assert_eq!(meta.rating_key.as_deref(), Some("42"));
    assert_eq!(meta.title.as_deref(), Some("Arrival"));
    assert!(got.thumb_bytes.is_none());
    handle.abort();
}

#[tokio::test]
async fn axum_extractor_captures_thumb_attachment() {
    let slot: Arc<Mutex<Option<WebhookPayload>>> = Arc::new(Mutex::new(None));
    let (url, handle) = spawn_axum_server(slot.clone()).await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    let fake_jpeg: &[u8] = b"\xff\xd8\xff\xe0fake-jpeg-bytes";
    let (content_type, body) = build_multipart(&sample_play_payload_json(), Some(fake_jpeg));
    let resp = post_multipart(&url, &content_type, body).await;
    assert_eq!(resp.status(), 204);
    let got = slot.lock().await.take().unwrap();
    let thumb = got.thumb_bytes.as_ref().expect("thumb missing");
    assert_eq!(&thumb[..], fake_jpeg);
    handle.abort();
}

#[tokio::test]
async fn axum_extractor_rejects_non_multipart_with_400() {
    let slot: Arc<Mutex<Option<WebhookPayload>>> = Arc::new(Mutex::new(None));
    let (url, handle) = spawn_axum_server(slot.clone()).await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    let resp = reqwest::Client::new()
        .post(&url)
        .header(header::CONTENT_TYPE, "application/json")
        .body(sample_play_payload_json())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    // Slot remains empty because the extractor never produced a value.
    assert!(slot.lock().await.is_none());
    handle.abort();
}

#[tokio::test]
async fn axum_extractor_rejects_missing_payload_field_with_400() {
    let slot: Arc<Mutex<Option<WebhookPayload>>> = Arc::new(Mutex::new(None));
    let (url, handle) = spawn_axum_server(slot.clone()).await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    // Multipart with only a `thumb` field, no `payload`.
    let boundary = "----plex-rs-test-boundary";
    let mut body = Vec::new();
    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body.extend_from_slice(
        b"Content-Disposition: form-data; name=\"thumb\"; filename=\"x.jpg\"\r\n\
          Content-Type: image/jpeg\r\n\r\n\
          fake\r\n",
    );
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    let content_type = format!("multipart/form-data; boundary={boundary}");
    let resp = post_multipart(&url, &content_type, body).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    assert!(slot.lock().await.is_none());
    handle.abort();
}

#[tokio::test]
async fn axum_extractor_rejects_invalid_payload_json_with_400() {
    let slot: Arc<Mutex<Option<WebhookPayload>>> = Arc::new(Mutex::new(None));
    let (url, handle) = spawn_axum_server(slot.clone()).await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    let (content_type, body) = build_multipart("not json at all", None);
    let resp = post_multipart(&url, &content_type, body).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    assert!(slot.lock().await.is_none());
    handle.abort();
}

#[tokio::test]
async fn axum_extractor_handles_library_new_event() {
    let slot: Arc<Mutex<Option<WebhookPayload>>> = Arc::new(Mutex::new(None));
    let (url, handle) = spawn_axum_server(slot.clone()).await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    let payload = serde_json::json!({
        "event": "library.new",
        "user": false,
        "owner": true,
        "Server": {"title": "PMS", "uuid": "x"},
        "Metadata": {
            "ratingKey": "99",
            "type": "episode",
            "title": "Pilot",
            "grandparentTitle": "Severance",
            "parentTitle": "Season 1",
            "librarySectionType": "show"
        }
    })
    .to_string();
    let (content_type, body) = build_multipart(&payload, None);
    let resp = post_multipart(&url, &content_type, body).await;
    assert_eq!(resp.status(), 204);
    let got = slot.lock().await.take().unwrap();
    assert_eq!(got.event, WebhookEvent::LibraryNew);
    let meta = got.metadata.unwrap();
    assert_eq!(meta.grandparent_title.as_deref(), Some("Severance"));
    assert_eq!(meta.library_section_type.as_deref(), Some("show"));
    handle.abort();
}

// Suppress unused import on `Body` when other tests don't construct one.
const _: fn() -> Body = || Body::empty();
