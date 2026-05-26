//! M0 smoke tests — exercise [`HttpClient`] against a `wiremock`
//! server to verify the foundation layer composes correctly.
//!
//! These tests cover behaviour that pure unit tests can't:
//! - Identity headers actually reach the wire.
//! - `Accept: application/json` is set by default.
//! - Status-to-Error mapping behaves end-to-end.
//! - The retry loop survives a transient 503.
//!
//! Each test spins up its own mock server (port 0) and tears it down
//! when the test function returns.

use std::time::Duration;

use plex_rs::{ClientConfig, ClientIdentifier, Error, HttpClient, PlexToken};
use serde::Deserialize;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Build a `HttpClient` configured with short timeouts and a
/// generous-enough retry budget for the integration tests.
fn client_with_token(token: Option<PlexToken>) -> HttpClient {
    let cfg = ClientConfig::builder(ClientIdentifier::new("integration-test").unwrap())
        .token(token)
        .request_timeout(Duration::from_secs(5))
        .connect_timeout(Duration::from_secs(2))
        .max_retries(2)
        .retry_base_delay(Duration::from_millis(1))
        .retry_max_delay(Duration::from_millis(50))
        .build()
        .unwrap();
    HttpClient::new(cfg).unwrap()
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
struct Identity {
    #[serde(rename = "MediaContainer")]
    container: IdentityContainer,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
struct IdentityContainer {
    #[serde(rename = "machineIdentifier")]
    machine_identifier: String,
    version: String,
}

#[tokio::test]
async fn identity_headers_reach_the_wire() {
    let server = MockServer::start().await;
    let token = PlexToken::new("test-secret-token").unwrap();

    Mock::given(method("GET"))
        .and(path("/identity"))
        .and(header("x-plex-client-identifier", "integration-test"))
        .and(header("x-plex-product", "plex-rs"))
        .and(header("x-plex-token", "test-secret-token"))
        .and(header("accept", "application/json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "MediaContainer": { "machineIdentifier": "MID-001", "version": "1.40.0" }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = client_with_token(Some(token));
    let url = format!("{}/identity", server.uri());
    let identity: Identity = client.get_json(&url).await.unwrap();
    assert_eq!(identity.container.machine_identifier, "MID-001");
    assert_eq!(identity.container.version, "1.40.0");
}

#[tokio::test]
async fn status_401_maps_to_unauthorized() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/needs-auth"))
        .respond_with(ResponseTemplate::new(401))
        .expect(1)
        .mount(&server)
        .await;

    let client = client_with_token(None);
    let url = format!("{}/needs-auth", server.uri());
    let err = client.get_bytes(&url).await.unwrap_err();
    assert!(matches!(err, Error::Unauthorized), "got {err:?}");
}

#[tokio::test]
async fn status_404_maps_to_not_found_with_path() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/missing"))
        .respond_with(ResponseTemplate::new(404))
        .expect(1)
        .mount(&server)
        .await;

    let client = client_with_token(None);
    let url = format!("{}/missing", server.uri());
    let err = client.get_bytes(&url).await.unwrap_err();
    match err {
        Error::NotFound { resource } => assert_eq!(resource, "/missing"),
        other => panic!("expected NotFound, got {other:?}"),
    }
}

#[tokio::test]
async fn transient_503_is_retried_then_succeeds() {
    let server = MockServer::start().await;

    // First two attempts get a 503, the third succeeds. The client is
    // configured with max_retries=2 which means 3 total attempts.
    Mock::given(method("GET"))
        .and(path("/flaky"))
        .respond_with(ResponseTemplate::new(503))
        .up_to_n_times(2)
        .expect(2)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/flaky"))
        .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
        .expect(1)
        .mount(&server)
        .await;

    let client = client_with_token(None);
    let url = format!("{}/flaky", server.uri());
    let body = client.get_bytes(&url).await.unwrap();
    assert_eq!(&body[..], b"ok");
}

#[tokio::test]
async fn retry_gives_up_after_max_attempts() {
    let server = MockServer::start().await;

    // Always 503; client should hit max_retries=2 → 3 attempts and then surface the error.
    Mock::given(method("GET"))
        .and(path("/always-bad"))
        .respond_with(ResponseTemplate::new(503))
        .expect(3)
        .mount(&server)
        .await;

    let client = client_with_token(None);
    let url = format!("{}/always-bad", server.uri());
    let err = client.get_bytes(&url).await.unwrap_err();
    match err {
        Error::Api { status, .. } => {
            assert_eq!(status, http::StatusCode::SERVICE_UNAVAILABLE);
        }
        other => panic!("expected Api(503), got {other:?}"),
    }
}

#[tokio::test]
async fn non_retryable_4xx_fails_fast() {
    let server = MockServer::start().await;

    // 400 is not in the retryable set. Client should NOT retry.
    Mock::given(method("GET"))
        .and(path("/bad-request"))
        .respond_with(ResponseTemplate::new(400).set_body_string("bad"))
        .expect(1)
        .mount(&server)
        .await;

    let client = client_with_token(None);
    let url = format!("{}/bad-request", server.uri());
    let err = client.get_bytes(&url).await.unwrap_err();
    match err {
        Error::Api { status, message } => {
            assert_eq!(status, http::StatusCode::BAD_REQUEST);
            assert!(message.contains("bad"));
        }
        other => panic!("expected Api(400), got {other:?}"),
    }
}

#[tokio::test]
async fn json_body_deserialises_on_200() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/data"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({"a": 1, "b": "two"})),
        )
        .expect(1)
        .mount(&server)
        .await;

    #[derive(Debug, Deserialize, PartialEq, Eq)]
    struct Payload {
        a: u32,
        b: String,
    }
    let client = client_with_token(None);
    let url = format!("{}/data", server.uri());
    let p: Payload = client.get_json(&url).await.unwrap();
    assert_eq!(
        p,
        Payload {
            a: 1,
            b: "two".to_owned()
        }
    );
}

#[tokio::test]
async fn json_parse_error_surfaces_as_error_json() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/bad-json"))
        .respond_with(ResponseTemplate::new(200).set_body_string("not json"))
        .expect(1)
        .mount(&server)
        .await;

    #[derive(Debug, Deserialize)]
    struct Payload {
        #[allow(dead_code)]
        a: u32,
    }
    let client = client_with_token(None);
    let url = format!("{}/bad-json", server.uri());
    let err = client.get_json::<Payload>(&url).await.unwrap_err();
    assert!(matches!(err, Error::Json(_)), "got {err:?}");
}
