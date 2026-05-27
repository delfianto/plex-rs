//! M4.8 — `Settings` integration tests.
//!
//! Exercises:
//! 1. `GET /:/prefs` returns a typed collection with the expected
//!    settings and groups
//! 2. `set(id, value)` performs a `PUT` with the right query
//!    parameters and reloads
//! 3. `set_many(...)` batches multiple updates into one PUT
//! 4. validation: wrong type / unknown id / out-of-enum rejection
//!    happens client-side without a network round-trip

use plex_rs::error::Error;
use plex_rs::{PlexServer, PlexToken, SettingKind, SettingValue};
use url::Url;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn root_body() -> serde_json::Value {
    serde_json::json!({
        "MediaContainer": {"size":0,"machineIdentifier":"m","version":"v"}
    })
}

fn prefs_body() -> serde_json::Value {
    serde_json::json!({
        "MediaContainer": {
            "size": 4,
            "Setting": [
                {"id":"TranscoderQuality","label":"Quality","summary":"Streaming",
                 "type":"int","default":"100","value":"60","group":"transcoder"},
                {"id":"FriendlyName","label":"Name","summary":"Server name",
                 "type":"text","default":"","value":"Living Room","group":"general"},
                {"id":"logDebug","label":"Debug logs","summary":"",
                 "type":"bool","default":"false","value":"true","group":"general",
                 "advanced":true},
                {"id":"LanNetworksBandwidth","label":"LAN bw","summary":"",
                 "type":"enum","default":"low","value":"high","group":"transcoder",
                 "enumValues":"low|medium|high"}
            ]
        }
    })
}

async fn connect(server: &MockServer) -> PlexServer {
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(root_body()))
        .mount(server)
        .await;
    PlexServer::connect(
        Url::parse(&server.uri()).unwrap(),
        PlexToken::new("token").unwrap(),
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn settings_load_returns_typed_collection() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/:/prefs"))
        .respond_with(ResponseTemplate::new(200).set_body_json(prefs_body()))
        .expect(1)
        .mount(&server)
        .await;

    let plex = connect(&server).await;
    let settings = plex.settings().await.unwrap();
    assert_eq!(settings.len(), 4);
    assert_eq!(
        settings.get("TranscoderQuality").unwrap().kind,
        SettingKind::Int
    );
    assert_eq!(
        settings.get("TranscoderQuality").unwrap().value.as_int(),
        Some(60)
    );
}

#[tokio::test]
async fn set_writes_via_put_and_reloads() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/:/prefs"))
        .respond_with(ResponseTemplate::new(200).set_body_json(prefs_body()))
        .expect(2) // initial load + reload after set
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .and(path("/:/prefs"))
        .and(query_param("TranscoderQuality", "80"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;

    let plex = connect(&server).await;
    let settings = plex.settings().await.unwrap();
    let _ = settings
        .set(&plex, "TranscoderQuality", SettingValue::Int(80))
        .await
        .unwrap();
}

#[tokio::test]
async fn set_many_batches_multiple_updates_in_one_put() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/:/prefs"))
        .respond_with(ResponseTemplate::new(200).set_body_json(prefs_body()))
        .expect(2)
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .and(path("/:/prefs"))
        .and(query_param("TranscoderQuality", "60"))
        .and(query_param("FriendlyName", "Den"))
        .and(query_param("logDebug", "false"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;

    let plex = connect(&server).await;
    let settings = plex.settings().await.unwrap();
    let _ = settings
        .set_many(
            &plex,
            vec![
                ("TranscoderQuality", SettingValue::Int(60)),
                ("FriendlyName", SettingValue::Text("Den".to_owned())),
                ("logDebug", SettingValue::Bool(false)),
            ],
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn set_unknown_id_rejected_client_side() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/:/prefs"))
        .respond_with(ResponseTemplate::new(200).set_body_json(prefs_body()))
        .mount(&server)
        .await;
    // No PUT mock — if validation works, no PUT happens.
    let plex = connect(&server).await;
    let settings = plex.settings().await.unwrap();
    let err = settings
        .set(&plex, "NoSuchSetting", SettingValue::Int(1))
        .await
        .unwrap_err();
    assert!(
        matches!(&err, Error::NotFound { resource } if resource.contains("NoSuchSetting")),
        "got {err:?}"
    );
}

#[tokio::test]
async fn set_wrong_value_kind_rejected_client_side() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/:/prefs"))
        .respond_with(ResponseTemplate::new(200).set_body_json(prefs_body()))
        .mount(&server)
        .await;
    let plex = connect(&server).await;
    let settings = plex.settings().await.unwrap();
    let err = settings
        .set(
            &plex,
            "TranscoderQuality",
            SettingValue::Text("high".to_owned()),
        )
        .await
        .unwrap_err();
    assert!(
        matches!(err, Error::Config(ref msg) if msg.contains("Int")),
        "got {err:?}"
    );
}

#[tokio::test]
async fn set_enum_value_outside_options_rejected_client_side() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/:/prefs"))
        .respond_with(ResponseTemplate::new(200).set_body_json(prefs_body()))
        .mount(&server)
        .await;
    let plex = connect(&server).await;
    let settings = plex.settings().await.unwrap();
    let err = settings
        .set(
            &plex,
            "LanNetworksBandwidth",
            SettingValue::Text("ludicrous".to_owned()),
        )
        .await
        .unwrap_err();
    assert!(
        matches!(err, Error::Config(ref msg) if msg.contains("ludicrous")),
        "got {err:?}"
    );
}

#[tokio::test]
async fn set_many_empty_rejected_client_side() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/:/prefs"))
        .respond_with(ResponseTemplate::new(200).set_body_json(prefs_body()))
        .mount(&server)
        .await;
    let plex = connect(&server).await;
    let settings = plex.settings().await.unwrap();
    let err = settings.set_many(&plex, vec![]).await.unwrap_err();
    assert!(
        matches!(err, Error::Config(ref msg) if msg.contains("at least one")),
        "got {err:?}"
    );
}
