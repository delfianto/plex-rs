//! M4.5 — `PlexClient` remote-control integration tests.
//!
//! Drives a wiremock-backed *player* (not PMS) through:
//!
//! 1. each of the core navigation commands (`X-Plex-Target-Client-Identifier`
//!    header + `commandID` query param verified)
//! 2. each of the core playback commands (`type=` query param verified)
//! 3. command-ID monotonicity across a sequence of commands
//! 4. `play_media()` composition with a `PlayQueue` from a separate
//!    PMS mock — verifies the full `protocol/address/port/key/
//!    containerKey/token` payload.

use plex_rs::{
    ClientIdentifier, LibraryItem, MachineIdentifier, MediaType, PlayQueueId, PlexClient,
    PlexServer, PlexToken, RepeatMode,
};
use url::Url;
use wiremock::matchers::{header, header_exists, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const TARGET_HEADER: &str = "X-Plex-Target-Client-Identifier";
const PLAYER_MACHINE_ID: &str = "player-machine-id";

fn build_client(player_mock_uri: &str) -> PlexClient {
    PlexClient::connect(
        Url::parse(player_mock_uri).unwrap(),
        PlexToken::new("player-token").unwrap(),
        MachineIdentifier::new(PLAYER_MACHINE_ID).unwrap(),
        ClientIdentifier::new("plex-rs-test").unwrap(),
    )
    .unwrap()
}

#[tokio::test]
async fn move_up_sends_navigation_command_with_target_header() {
    let player = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/player/navigation/moveUp"))
        .and(header(
            TARGET_HEADER.to_ascii_lowercase().as_str(),
            PLAYER_MACHINE_ID,
        ))
        .and(query_param("commandID", "1"))
        .respond_with(ResponseTemplate::new(200).set_body_string("OK"))
        .expect(1)
        .mount(&player)
        .await;
    build_client(&player.uri()).move_up().await.unwrap();
}

#[tokio::test]
async fn navigation_select_back_home_each_target_their_own_path() {
    let player = MockServer::start().await;
    for cmd in [
        "select",
        "back",
        "home",
        "moveDown",
        "moveLeft",
        "moveRight",
    ] {
        Mock::given(method("GET"))
            .and(path(format!("/player/navigation/{cmd}")))
            .respond_with(ResponseTemplate::new(200).set_body_string("OK"))
            .expect(1)
            .mount(&player)
            .await;
    }
    let c = build_client(&player.uri());
    c.select().await.unwrap();
    c.back().await.unwrap();
    c.go_to_home().await.unwrap();
    c.move_down().await.unwrap();
    c.move_left().await.unwrap();
    c.move_right().await.unwrap();
}

#[tokio::test]
async fn play_pause_stop_send_playback_commands_with_type() {
    let player = MockServer::start().await;
    for cmd in ["play", "pause", "stop"] {
        Mock::given(method("GET"))
            .and(path(format!("/player/playback/{cmd}")))
            .and(query_param("type", "video"))
            .respond_with(ResponseTemplate::new(200).set_body_string("OK"))
            .expect(1)
            .mount(&player)
            .await;
    }
    let c = build_client(&player.uri());
    c.play(MediaType::Video).await.unwrap();
    c.pause(MediaType::Video).await.unwrap();
    c.stop(MediaType::Video).await.unwrap();
}

#[tokio::test]
async fn seek_to_threads_offset_as_milliseconds() {
    let player = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/player/playback/seekTo"))
        .and(query_param("offset", "120000"))
        .and(query_param("type", "video"))
        .respond_with(ResponseTemplate::new(200).set_body_string("OK"))
        .expect(1)
        .mount(&player)
        .await;
    build_client(&player.uri())
        .seek_to(120_000, MediaType::Video)
        .await
        .unwrap();
}

#[tokio::test]
async fn set_volume_clamps_to_100_and_sends_set_parameters() {
    let player = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/player/playback/setParameters"))
        .and(query_param("volume", "100"))
        .and(query_param("type", "music"))
        .respond_with(ResponseTemplate::new(200).set_body_string("OK"))
        .expect(1)
        .mount(&player)
        .await;
    build_client(&player.uri())
        .set_volume(255, MediaType::Music)
        .await
        .unwrap();
}

#[tokio::test]
async fn set_repeat_threads_numeric_mode() {
    let player = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/player/playback/setParameters"))
        .and(query_param("repeat", "2"))
        .and(query_param("type", "music"))
        .respond_with(ResponseTemplate::new(200).set_body_string("OK"))
        .expect(1)
        .mount(&player)
        .await;
    build_client(&player.uri())
        .set_repeat(RepeatMode::All, MediaType::Music)
        .await
        .unwrap();
}

#[tokio::test]
async fn set_shuffle_threads_boolean_as_numeric() {
    let player = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/player/playback/setParameters"))
        .and(query_param("shuffle", "1"))
        .respond_with(ResponseTemplate::new(200).set_body_string("OK"))
        .expect(1)
        .mount(&player)
        .await;
    build_client(&player.uri())
        .set_shuffle(true, MediaType::Music)
        .await
        .unwrap();
}

#[tokio::test]
async fn command_id_increments_monotonically_across_commands() {
    let player = MockServer::start().await;
    // Mount three matchers, each requiring a specific commandID.
    for expected_id in ["1", "2", "3"] {
        Mock::given(method("GET"))
            .and(path("/player/playback/play"))
            .and(query_param("commandID", expected_id))
            .respond_with(ResponseTemplate::new(200).set_body_string("OK"))
            .up_to_n_times(1)
            .expect(1)
            .mount(&player)
            .await;
    }
    let c = build_client(&player.uri());
    c.play(MediaType::Video).await.unwrap();
    c.play(MediaType::Video).await.unwrap();
    c.play(MediaType::Video).await.unwrap();
}

#[tokio::test]
async fn play_media_composes_full_payload_from_play_queue() {
    // Stand up a "PMS" mock for connect + library section listing.
    let pms = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "MediaContainer": {
                "size": 0,
                "machineIdentifier": "pms-machine-id-abc",
                "version": "v",
            }
        })))
        .mount(&pms)
        .await;
    Mock::given(method("GET"))
        .and(path("/library/sections"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "MediaContainer": {
                "size": 1,
                "Directory": [{"key":"1","type":"movie","title":"Movies"}]
            }
        })))
        .mount(&pms)
        .await;
    Mock::given(method("GET"))
        .and(path("/library/sections/1/all"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "MediaContainer": {
                "size": 1,
                "Metadata": [{
                    "ratingKey": "42",
                    "key": "/library/metadata/42",
                    "title": "Arrival",
                    "type": "movie",
                    "librarySectionID": 1,
                }]
            }
        })))
        .mount(&pms)
        .await;
    Mock::given(method("POST"))
        .and(path("/playQueues"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "MediaContainer": {
                "playQueueID": 999,
                "playQueueVersion": 1,
                "playQueueTotalCount": 1,
                "Metadata": [{
                    "playQueueItemID": 100,
                    "ratingKey": "42",
                    "key": "/library/metadata/42",
                    "title": "Arrival",
                    "type": "movie",
                    "librarySectionID": 1,
                }]
            }
        })))
        .mount(&pms)
        .await;

    let plex = PlexServer::connect(
        Url::parse(&pms.uri()).unwrap(),
        PlexToken::new("pms-secret-token").unwrap(),
    )
    .await
    .unwrap();
    let movie = plex.library().sections().await.unwrap()[0]
        .movies()
        .await
        .unwrap()
        .into_iter()
        .next()
        .unwrap();
    let item = LibraryItem::Movie(movie);
    let queue = plex
        .create_play_queue()
        .from_item(&item)
        .execute()
        .await
        .unwrap();
    assert_eq!(queue.id, PlayQueueId(999));

    // Now stand up the player mock and verify the playMedia payload.
    let player = MockServer::start().await;
    let pms_url = Url::parse(&pms.uri()).unwrap();
    let pms_host = pms_url.host_str().unwrap().to_owned();
    let pms_port = pms_url.port_or_known_default().unwrap().to_string();
    Mock::given(method("GET"))
        .and(path("/player/playback/playMedia"))
        .and(header(
            TARGET_HEADER.to_ascii_lowercase().as_str(),
            PLAYER_MACHINE_ID,
        ))
        .and(query_param(
            "providerIdentifier",
            "com.plexapp.plugins.library",
        ))
        .and(query_param("machineIdentifier", "pms-machine-id-abc"))
        .and(query_param("protocol", "http"))
        .and(query_param("address", pms_host.as_str()))
        .and(query_param("port", pms_port.as_str()))
        .and(query_param("offset", "30000"))
        .and(query_param("key", "/library/metadata/42"))
        .and(query_param("type", "video"))
        .and(query_param(
            "containerKey",
            "/playQueues/999?window=100&own=1",
        ))
        .and(query_param("token", "pms-secret-token"))
        .and(header_exists("x-plex-target-client-identifier"))
        .respond_with(ResponseTemplate::new(200).set_body_string("OK"))
        .expect(1)
        .mount(&player)
        .await;
    build_client(&player.uri())
        .play_media(&plex, &queue, 30_000)
        .await
        .unwrap();
}

#[tokio::test]
async fn play_media_errors_when_queue_is_empty() {
    let pms = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "MediaContainer": {"size":0,"machineIdentifier":"m","version":"v"}
        })))
        .mount(&pms)
        .await;
    let plex = PlexServer::connect(
        Url::parse(&pms.uri()).unwrap(),
        PlexToken::new("t").unwrap(),
    )
    .await
    .unwrap();
    // Build an empty queue by hand by going through a mocked GET.
    Mock::given(method("GET"))
        .and(path("/playQueues/1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "MediaContainer": {
                "playQueueID": 1,
                "playQueueVersion": 1,
                "playQueueTotalCount": 0,
                "Metadata": []
            }
        })))
        .mount(&pms)
        .await;
    let queue = plex.play_queue(PlayQueueId(1)).await.unwrap();
    let player = MockServer::start().await;
    let err = build_client(&player.uri())
        .play_media(&plex, &queue, 0)
        .await
        .unwrap_err();
    use plex_rs::error::Error;
    assert!(
        matches!(err, Error::Config(ref msg) if msg.contains("no items")),
        "expected Config error mentioning 'no items', got {err:?}"
    );
}
