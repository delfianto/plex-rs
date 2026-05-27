//! M5.6 — metadata_provider userState + scrobble integration tests.

use plex_rs::{ClientIdentifier, MyPlexClient, PlexToken};
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn client_pointing_at(metadata_mock_uri: &str) -> MyPlexClient {
    let cid = ClientIdentifier::new("meta-test").unwrap();
    MyPlexClient::new(PlexToken::new("acct-token").unwrap(), cid, None)
        .unwrap()
        .with_metadata_base(metadata_mock_uri)
}

fn user_state_body() -> serde_json::Value {
    serde_json::json!({
        "MediaContainer": {
            "size": 1,
            "UserState": [{
                "ratingKey": "abc123",
                "type": "movie",
                "viewCount": 2,
                "viewOffset": 1234,
                "viewState": "complete",
                "lastViewedAt": 1_700_000_000,
                "watchlistedAt": 1_690_000_000
            }]
        }
    })
}

#[tokio::test]
async fn user_state_fetches_and_parses() {
    let meta = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/library/metadata/abc123/userState"))
        .respond_with(ResponseTemplate::new(200).set_body_json(user_state_body()))
        .expect(1)
        .mount(&meta)
        .await;

    let state = client_pointing_at(&meta.uri())
        .user_state("abc123")
        .await
        .unwrap();
    assert_eq!(state.rating_key, "abc123");
    assert_eq!(state.kind.as_deref(), Some("movie"));
    assert_eq!(state.view_count, 2);
    assert_eq!(state.view_offset_ms, 1234);
    assert!(state.view_state_complete);
    assert!(state.is_played());
    assert!(state.is_on_watchlist());
}

#[tokio::test]
async fn scrobble_hits_actions_scrobble_with_identifier() {
    let meta = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/actions/scrobble"))
        .and(query_param("key", "abc123"))
        .and(query_param("identifier", "com.plexapp.plugins.library"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&meta)
        .await;
    client_pointing_at(&meta.uri())
        .scrobble("abc123")
        .await
        .unwrap();
}

#[tokio::test]
async fn unscrobble_hits_actions_unscrobble_with_identifier() {
    let meta = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/actions/unscrobble"))
        .and(query_param("key", "abc123"))
        .and(query_param("identifier", "com.plexapp.plugins.library"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&meta)
        .await;
    client_pointing_at(&meta.uri())
        .unscrobble("abc123")
        .await
        .unwrap();
}

#[tokio::test]
async fn user_state_returns_config_error_on_empty_container() {
    let meta = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/library/metadata/x/userState"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "MediaContainer": {"size": 0, "UserState": []}
        })))
        .expect(1)
        .mount(&meta)
        .await;
    use plex_rs::error::Error;
    let err = client_pointing_at(&meta.uri())
        .user_state("x")
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Config(ref m) if m.contains("UserState")));
}
