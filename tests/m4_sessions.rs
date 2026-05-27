//! M4.3 integration tests — sessions listing + termination.

use plex_rs::{LibraryItem, PlayState, PlexServer, PlexToken};
use url::Url;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn root_body() -> serde_json::Value {
    serde_json::json!({"MediaContainer": {"size": 0, "machineIdentifier": "m", "version": "v"}})
}

fn sessions_body() -> serde_json::Value {
    serde_json::json!({
        "MediaContainer": {
            "size": 2,
            "Metadata": [
                {
                    "ratingKey": "100",
                    "key": "/library/metadata/100",
                    "type": "movie",
                    "title": "Arrival",
                    "sessionKey": "12345",
                    "viewOffset": 1_500_000,
                    "User": {"id": 1, "title": "alice", "thumb": "/u/1/avatar"},
                    "Player": {
                        "address": "192.168.1.50",
                        "device": "Roku 3",
                        "machineIdentifier": "roku-abc",
                        "model": "4200X",
                        "product": "Plex for Roku",
                        "state": "playing",
                        "title": "Living Room",
                        "platform": "Roku",
                        "version": "7.4.0",
                        "local": "1",
                        "controllable": "1"
                    },
                    "TranscodeSession": {
                        "key": "tx-1",
                        "throttled": "1",
                        "progress": 47.5,
                        "duration": 7_200_000,
                        "remaining": 1_800_000,
                        "speed": 1.4,
                        "sourceVideoCodec": "h264",
                        "videoCodec": "h264",
                        "audioCodec": "aac",
                        "container": "mkv",
                        "transcodeHwRequested": "1"
                    }
                },
                {
                    "ratingKey": "200",
                    "key": "/library/metadata/200",
                    "type": "track",
                    "title": "Drift",
                    "sessionKey": "67890",
                    "parentRatingKey": "20",
                    "grandparentRatingKey": "2",
                    "viewOffset": 120_000,
                    "Player": {
                        "device": "iPhone",
                        "state": "paused",
                        "local": false
                    }
                }
            ]
        }
    })
}

async fn connected() -> (MockServer, PlexServer) {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(root_body()))
        .mount(&server)
        .await;
    let plex = PlexServer::connect(
        Url::parse(&server.uri()).unwrap(),
        PlexToken::new("tok").unwrap(),
    )
    .await
    .unwrap();
    (server, plex)
}

#[tokio::test]
async fn sessions_parses_mixed_session_types() {
    let (server, plex) = connected().await;
    Mock::given(method("GET"))
        .and(path("/status/sessions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(sessions_body()))
        .expect(1)
        .mount(&server)
        .await;

    let sessions = plex.sessions().await.unwrap();
    assert_eq!(sessions.len(), 2);

    let movie_sess = &sessions[0];
    assert_eq!(movie_sess.session_key, "12345");
    assert_eq!(movie_sess.view_offset_ms, Some(1_500_000));
    assert_eq!(movie_sess.user.id, 1);
    assert_eq!(movie_sess.user.title.as_deref(), Some("alice"));
    assert_eq!(movie_sess.player.state, PlayState::Playing);
    assert_eq!(movie_sess.player.device.as_deref(), Some("Roku 3"));
    assert!(movie_sess.player.local);
    assert!(movie_sess.player.controllable);
    let tx = movie_sess.transcode.as_ref().unwrap();
    assert!(tx.throttled);
    assert_eq!(tx.video_codec.as_deref(), Some("h264"));
    assert_eq!(tx.speed, Some(1.4));
    assert!(matches!(movie_sess.item, LibraryItem::Movie(_)));

    let track_sess = &sessions[1];
    assert_eq!(track_sess.session_key, "67890");
    assert_eq!(track_sess.player.state, PlayState::Paused);
    assert!(!track_sess.player.local);
    assert!(track_sess.transcode.is_none());
    assert!(matches!(track_sess.item, LibraryItem::Track(_)));
}

#[tokio::test]
async fn session_stop_hits_terminate_with_session_id_and_reason() {
    let (server, plex) = connected().await;
    Mock::given(method("GET"))
        .and(path("/status/sessions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(sessions_body()))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/status/sessions/terminate"))
        .and(query_param("sessionId", "12345"))
        .and(query_param("reason", "End of demo"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;

    let sessions = plex.sessions().await.unwrap();
    sessions[0].stop(Some("End of demo")).await.unwrap();
}

#[tokio::test]
async fn session_stop_works_without_reason() {
    let (server, plex) = connected().await;
    Mock::given(method("GET"))
        .and(path("/status/sessions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(sessions_body()))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/status/sessions/terminate"))
        .and(query_param("sessionId", "67890"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;

    let sessions = plex.sessions().await.unwrap();
    sessions[1].stop(None).await.unwrap();
}
