//! M2.3 integration tests — Artist → Album → Track hierarchy.

use plex_rs::{PlexServer, PlexToken, SectionKind};
use url::Url;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn root_body() -> serde_json::Value {
    serde_json::json!({"MediaContainer": {"size": 0, "machineIdentifier": "m", "version": "v"}})
}
fn sections_body() -> serde_json::Value {
    serde_json::json!({
        "MediaContainer": {
            "size": 1,
            "Directory": [{"key": "5", "type": "artist", "title": "Music", "uuid": "music-uuid"}]
        }
    })
}
fn artists_body() -> serde_json::Value {
    serde_json::json!({
        "MediaContainer": {
            "size": 1,
            "Metadata": [{
                "ratingKey": "2000",
                "key": "/library/metadata/2000",
                "title": "Daft Punk",
                "titleSort": "Daft Punk",
                "summary": "French electronic music duo.",
                "childCount": 4,
                "thumb": "/library/metadata/2000/thumb/1",
                "art": "/library/metadata/2000/art/1"
            }]
        }
    })
}
fn albums_body() -> serde_json::Value {
    serde_json::json!({
        "MediaContainer": {
            "size": 2,
            "Metadata": [
                {
                    "ratingKey": "2100",
                    "key": "/library/metadata/2100",
                    "title": "Discovery",
                    "year": 2001,
                    "studio": "Virgin",
                    "rating": 9.0,
                    "parentRatingKey": "2000",
                    "parentTitle": "Daft Punk",
                    "leafCount": 14,
                    "viewedLeafCount": 7,
                    "originallyAvailableAt": "2001-03-12",
                    "thumb": "/library/metadata/2100/thumb/1"
                },
                {
                    "ratingKey": "2200",
                    "key": "/library/metadata/2200",
                    "title": "Random Access Memories",
                    "year": 2013,
                    "parentRatingKey": "2000",
                    "leafCount": 13
                }
            ]
        }
    })
}
fn tracks_body() -> serde_json::Value {
    serde_json::json!({
        "MediaContainer": {
            "size": 2,
            "Metadata": [
                {
                    "ratingKey": "2101",
                    "key": "/library/metadata/2101",
                    "title": "One More Time",
                    "index": 1,
                    "parentIndex": 1,
                    "duration": 320_000,
                    "viewCount": 5,
                    "parentRatingKey": "2100",
                    "parentTitle": "Discovery",
                    "parentThumb": "/library/metadata/2100/thumb/1",
                    "grandparentRatingKey": "2000",
                    "grandparentTitle": "Daft Punk"
                },
                {
                    "ratingKey": "2102",
                    "key": "/library/metadata/2102",
                    "title": "Aerodynamic",
                    "index": 2,
                    "parentIndex": 1,
                    "duration": 207_000,
                    "viewCount": 0,
                    "parentRatingKey": "2100",
                    "grandparentRatingKey": "2000"
                }
            ]
        }
    })
}

async fn connected(server: &MockServer) -> PlexServer {
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(root_body()))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/library/sections"))
        .respond_with(ResponseTemplate::new(200).set_body_json(sections_body()))
        .mount(server)
        .await;
    PlexServer::connect(
        Url::parse(&server.uri()).unwrap(),
        PlexToken::new("tok").unwrap(),
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn artists_album_track_walk_round_trips() {
    let server = MockServer::start().await;
    let plex = connected(&server).await;
    Mock::given(method("GET"))
        .and(path("/library/sections/5/all"))
        .and(query_param("type", "8"))
        .respond_with(ResponseTemplate::new(200).set_body_json(artists_body()))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/library/metadata/2000/children"))
        .respond_with(ResponseTemplate::new(200).set_body_json(albums_body()))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/library/metadata/2100/children"))
        .respond_with(ResponseTemplate::new(200).set_body_json(tracks_body()))
        .expect(1)
        .mount(&server)
        .await;

    let sections = plex.library().sections().await.unwrap();
    let music = sections
        .iter()
        .find(|s| s.kind == SectionKind::Music)
        .unwrap();
    let artists = music.artists().await.unwrap();
    assert_eq!(artists.len(), 1);
    assert_eq!(artists[0].title, "Daft Punk");
    assert_eq!(artists[0].child_count, Some(4));

    let albums = artists[0].albums().await.unwrap();
    assert_eq!(albums.len(), 2);
    assert_eq!(albums[0].title, "Discovery");
    assert_eq!(albums[0].year, Some(2001));
    assert_eq!(albums[0].parent_rating_key.get(), 2000);
    assert_eq!(albums[0].leaf_count, Some(14));
    assert_eq!(albums[0].viewed_leaf_count, Some(7));
    assert_eq!(albums[0].studio.as_deref(), Some("Virgin"));

    let tracks = albums[0].tracks().await.unwrap();
    assert_eq!(tracks.len(), 2);

    let t1 = &tracks[0];
    assert_eq!(t1.rating_key.get(), 2101);
    assert_eq!(t1.title, "One More Time");
    assert_eq!(t1.index, Some(1));
    assert_eq!(t1.disc_number, Some(1));
    assert_eq!(t1.duration_ms, Some(320_000));
    assert!(t1.is_played());
    assert_eq!(t1.parent_rating_key.get(), 2100);
    assert_eq!(t1.grandparent_rating_key.get(), 2000);
    assert_eq!(t1.parent_title.as_deref(), Some("Discovery"));
    assert_eq!(t1.grandparent_title.as_deref(), Some("Daft Punk"));

    assert!(!tracks[1].is_played());
}

#[tokio::test]
async fn artists_rejects_non_music_section() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(root_body()))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/library/sections"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "MediaContainer": {
                "size": 1,
                "Directory": [{"key": "1", "type": "movie", "title": "Movies"}]
            }
        })))
        .mount(&server)
        .await;
    let plex = PlexServer::connect(
        Url::parse(&server.uri()).unwrap(),
        PlexToken::new("tok").unwrap(),
    )
    .await
    .unwrap();
    let sections = plex.library().sections().await.unwrap();
    let err = sections[0].artists().await.unwrap_err();
    assert!(matches!(err, plex_rs::Error::Config(_)));
}
