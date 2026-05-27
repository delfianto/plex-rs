//! M4.1 integration tests — playlist listing, items, deletion.

use plex_rs::{LibraryItem, PlaylistKind, PlexServer, PlexToken};
use url::Url;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn root_body() -> serde_json::Value {
    serde_json::json!({"MediaContainer": {"size": 0, "machineIdentifier": "m", "version": "v"}})
}

fn playlists_body() -> serde_json::Value {
    serde_json::json!({
        "MediaContainer": {
            "size": 2,
            "Metadata": [
                {
                    "ratingKey": "500", "key": "/playlists/500",
                    "title": "Workout", "playlistType": "audio",
                    "smart": false, "duration": 3_600_000, "leafCount": 12
                },
                {
                    "ratingKey": "600", "key": "/playlists/600",
                    "title": "Best Action 2024", "playlistType": "video",
                    "smart": "1",
                    "content": "library:///directory/encoded-filter"
                }
            ]
        }
    })
}

fn playlist_items_body() -> serde_json::Value {
    serde_json::json!({
        "MediaContainer": {
            "size": 2,
            "Metadata": [
                {
                    "ratingKey": "100", "key": "/library/metadata/100",
                    "type": "track", "title": "Track 1",
                    "parentRatingKey": "10", "grandparentRatingKey": "1",
                    "librarySectionID": 3
                },
                {
                    "ratingKey": "101", "key": "/library/metadata/101",
                    "type": "track", "title": "Track 2",
                    "parentRatingKey": "10", "grandparentRatingKey": "1",
                    "librarySectionID": 3
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
async fn playlists_returns_typed_list() {
    let (server, plex) = connected().await;
    Mock::given(method("GET"))
        .and(path("/playlists"))
        .respond_with(ResponseTemplate::new(200).set_body_json(playlists_body()))
        .expect(1)
        .mount(&server)
        .await;

    let playlists = plex.playlists().await.unwrap();
    assert_eq!(playlists.len(), 2);
    assert_eq!(playlists[0].title, "Workout");
    assert_eq!(playlists[0].kind, PlaylistKind::Audio);
    assert!(!playlists[0].smart);
    assert_eq!(playlists[0].leaf_count, Some(12));

    assert_eq!(playlists[1].kind, PlaylistKind::Video);
    assert!(playlists[1].smart);
    assert!(playlists[1].content_uri.is_some());
}

#[tokio::test]
async fn playlist_items_yields_library_items() {
    let (server, plex) = connected().await;
    Mock::given(method("GET"))
        .and(path("/playlists"))
        .respond_with(ResponseTemplate::new(200).set_body_json(playlists_body()))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/playlists/500/items"))
        .respond_with(ResponseTemplate::new(200).set_body_json(playlist_items_body()))
        .expect(1)
        .mount(&server)
        .await;

    let playlist = plex
        .playlists()
        .await
        .unwrap()
        .into_iter()
        .find(|p| p.title == "Workout")
        .unwrap();
    let items = playlist.items().await.unwrap();
    assert_eq!(items.len(), 2);
    assert!(matches!(items[0], LibraryItem::Track(_)));
    if let LibraryItem::Track(t) = &items[0] {
        assert_eq!(t.title, "Track 1");
        // librarySectionID was 3 — wired into the section back-ref.
        assert_eq!(t.section_ref.id, 3);
    }
}

#[tokio::test]
async fn playlist_delete_hits_delete_endpoint() {
    let (server, plex) = connected().await;
    Mock::given(method("GET"))
        .and(path("/playlists"))
        .respond_with(ResponseTemplate::new(200).set_body_json(playlists_body()))
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/playlists/500"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;

    let playlist = plex.playlists().await.unwrap().into_iter().next().unwrap();
    playlist.delete().await.unwrap();
}
