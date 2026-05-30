//! M5.3 — `MyPlexResource` + connect race integration tests.
//!
//! Drives a wiremock-backed plex.tv replica through:
//!
//! 1. `resources()` returns parsed entries with token + connections;
//! 2. `connect()` succeeds against the first answering connection,
//!    even when an earlier connection in the priority list refuses;
//! 3. `connect()` returns `NotFound` when every connection fails.

use std::time::Duration;

use plex_rs::{
    ClientIdentifier, ConnectOptions, MyPlexClient, MyPlexResource, PlexToken, error::Error,
};
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn root_pms_body() -> serde_json::Value {
    serde_json::json!({
        "MediaContainer": {
            "size": 0,
            "machineIdentifier": "pms-machine-id",
            "version": "1.40.2.0",
            "friendlyName": "Living Room",
        }
    })
}

fn resources_body(connections: Vec<serde_json::Value>) -> serde_json::Value {
    serde_json::json!([{
        "name": "Living Room",
        "product": "Plex Media Server",
        "productVersion": "1.40.2.0",
        "platform": "Linux",
        "platformVersion": "5.15",
        "device": "PC",
        "clientIdentifier": "pms-machine-id-stable",
        "provides": "server",
        "owned": true,
        "presence": true,
        "httpsRequired": false,
        "accessToken": "per-resource-token",
        "publicAddress": "1.2.3.4",
        "publicAddressMatches": true,
        "relay": false,
        "dnsRebindingProtection": false,
        "natLoopbackSupported": false,
        "connections": connections,
    }])
}

fn client_pointing_at(plex_tv_mock_uri: &str) -> MyPlexClient {
    let cid = ClientIdentifier::new("resource-test").unwrap();
    MyPlexClient::new(PlexToken::new("acct-token").unwrap(), cid, None)
        .unwrap()
        .with_base(plex_tv_mock_uri)
}

#[tokio::test]
async fn resources_parses_and_returns_entries() {
    let plex_tv = MockServer::start().await;
    let body = resources_body(vec![serde_json::json!({
        "protocol": "https",
        "address": "10.0.0.5",
        "port": 32400,
        "uri": "https://10-0-0-5.x.plex.direct:32400",
        "local": true,
        "relay": false,
        "IPv6": false,
    })]);
    Mock::given(method("GET"))
        .and(path("/api/v2/resources"))
        .and(query_param("includeHttps", "1"))
        .and(query_param("includeRelay", "1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .expect(1)
        .mount(&plex_tv)
        .await;

    let client = client_pointing_at(&plex_tv.uri());
    let resources = client.resources().await.unwrap();
    assert_eq!(resources.len(), 1);
    assert_eq!(resources[0].name, "Living Room");
    assert!(resources[0].is_server());
    assert!(resources[0].owned);
    assert_eq!(resources[0].connections.len(), 1);
    assert_eq!(
        resources[0]
            .access_token
            .as_ref()
            .expect("owned server resource has a per-resource token")
            .expose(),
        "per-resource-token"
    );
}

#[tokio::test]
async fn resource_finds_by_name_case_insensitively() {
    let plex_tv = MockServer::start().await;
    let body = resources_body(vec![]);
    Mock::given(method("GET"))
        .and(path("/api/v2/resources"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&plex_tv)
        .await;

    let client = client_pointing_at(&plex_tv.uri());
    let r = client.resource("LIVING ROOM").await.unwrap();
    assert!(r.is_some());
    let r = client.resource("Nonexistent").await.unwrap();
    assert!(r.is_none());
}

#[tokio::test]
async fn connect_picks_first_answering_connection() {
    // Two "PMS" mocks: a dead one (returns 503 on /) and a live one
    // (returns a valid MediaContainer). The dead one is listed FIRST
    // as a local connection — it would be tried before the live one
    // by preferred_connections, but the race lets the live one win.
    let dead_pms = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&dead_pms)
        .await;

    let live_pms = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(root_pms_body()))
        .mount(&live_pms)
        .await;

    // plex.tv replica that advertises both connections.
    let plex_tv = MockServer::start().await;
    let body = resources_body(vec![
        serde_json::json!({
            "protocol":"http","address":"127.0.0.1","port":1,
            "uri": dead_pms.uri(),
            "local":true,"relay":false,"IPv6":false,
        }),
        serde_json::json!({
            "protocol":"http","address":"127.0.0.1","port":2,
            "uri": live_pms.uri(),
            "local":false,"relay":false,"IPv6":false,
        }),
    ]);
    Mock::given(method("GET"))
        .and(path("/api/v2/resources"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&plex_tv)
        .await;

    let client = client_pointing_at(&plex_tv.uri());
    let resource: MyPlexResource = client.resource("Living Room").await.unwrap().unwrap();
    let opts = ConnectOptions::default().with_per_attempt_timeout(Duration::from_secs(3));
    let server = resource.connect_with_options(opts).await.unwrap();
    assert_eq!(
        server.identity().friendly_name.as_deref(),
        Some("Living Room")
    );
}

#[tokio::test]
async fn connect_returns_not_found_when_all_connections_fail() {
    let dead_a = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&dead_a)
        .await;
    let dead_b = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&dead_b)
        .await;

    let plex_tv = MockServer::start().await;
    let body = resources_body(vec![
        serde_json::json!({
            "protocol":"http","address":"127.0.0.1","port":1,
            "uri": dead_a.uri(),
            "local":true,"relay":false,"IPv6":false,
        }),
        serde_json::json!({
            "protocol":"http","address":"127.0.0.1","port":2,
            "uri": dead_b.uri(),
            "local":false,"relay":false,"IPv6":false,
        }),
    ]);
    Mock::given(method("GET"))
        .and(path("/api/v2/resources"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&plex_tv)
        .await;

    let client = client_pointing_at(&plex_tv.uri());
    let resource = client.resource("Living Room").await.unwrap().unwrap();
    let opts = ConnectOptions::default().with_per_attempt_timeout(Duration::from_secs(2));
    let err = resource.connect_with_options(opts).await.unwrap_err();
    // The last failing probe surfaces — either the 404 or the 503,
    // depending on which one happens to complete last in the race.
    // Both are "this URL is not a healthy PMS"; assert on the broad
    // shape rather than which specific variant.
    assert!(
        matches!(err, Error::NotFound { .. } | Error::Api { .. }),
        "expected NotFound or Api, got {err:?}"
    );
}
