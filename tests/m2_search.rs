//! M2.8 integration tests — title search, recentlyAdded, onDeck.

use plex_rs::{LibraryItem, PlexServer, PlexToken};
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
            "Directory": [{"key": "1", "type": "movie", "title": "Movies", "uuid": "m-uuid"}]
        }
    })
}

#[tokio::test]
async fn title_search_yields_library_items() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(root_body()))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/library/sections"))
        .respond_with(ResponseTemplate::new(200).set_body_json(sections_body()))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/library/sections/1/all"))
        .and(query_param("title", "Blade Runner"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "MediaContainer": {
                "size": 2,
                "Metadata": [
                    {"ratingKey": "100", "key": "/library/metadata/100",
                     "type": "movie", "title": "Blade Runner", "year": 1982},
                    {"ratingKey": "200", "key": "/library/metadata/200",
                     "type": "movie", "title": "Blade Runner 2049", "year": 2017}
                ]
            }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let plex = PlexServer::connect(
        Url::parse(&server.uri()).unwrap(),
        PlexToken::new("tok").unwrap(),
    )
    .await
    .unwrap();
    let sections = plex.library().sections().await.unwrap();
    let items = sections[0].search("Blade Runner").await.unwrap();
    assert_eq!(items.len(), 2);
    assert!(matches!(items[0], LibraryItem::Movie(_)));
    assert_eq!(items[0].title(), "Blade Runner");
    assert_eq!(items[0].rating_key().get(), 100);
    if let LibraryItem::Movie(m) = &items[1] {
        assert_eq!(m.year, Some(2017));
    } else {
        panic!("expected Movie");
    }
}

#[tokio::test]
async fn recently_added_handles_mixed_types() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(root_body()))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/library/sections"))
        .respond_with(ResponseTemplate::new(200).set_body_json(sections_body()))
        .mount(&server)
        .await;
    // A show section's recentlyAdded typically returns episodes
    // (Plex returns leaf items by default). We're on a movie section
    // here, so all entries are movies — but the dispatch logic must
    // tolerate mixed responses from other endpoints.
    Mock::given(method("GET"))
        .and(path("/library/sections/1/recentlyAdded"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "MediaContainer": {
                "size": 2,
                "Metadata": [
                    {"ratingKey": "300", "key": "/library/metadata/300",
                     "type": "movie", "title": "New Release"},
                    {"ratingKey": "400", "key": "/library/metadata/400",
                     "type": "episode", "title": "Newest Episode",
                     "parentRatingKey": "401",
                     "grandparentRatingKey": "402"}
                ]
            }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let plex = PlexServer::connect(
        Url::parse(&server.uri()).unwrap(),
        PlexToken::new("tok").unwrap(),
    )
    .await
    .unwrap();
    let sections = plex.library().sections().await.unwrap();
    let items = sections[0].recently_added().await.unwrap();
    assert_eq!(items.len(), 2);
    assert!(matches!(items[0], LibraryItem::Movie(_)));
    assert!(matches!(items[1], LibraryItem::Episode(_)));
}

#[tokio::test]
async fn on_deck_endpoint_routed_correctly() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(root_body()))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/library/sections"))
        .respond_with(ResponseTemplate::new(200).set_body_json(sections_body()))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/library/sections/1/onDeck"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "MediaContainer": {"size": 0}
        })))
        .expect(1)
        .mount(&server)
        .await;

    let plex = PlexServer::connect(
        Url::parse(&server.uri()).unwrap(),
        PlexToken::new("tok").unwrap(),
    )
    .await
    .unwrap();
    let sections = plex.library().sections().await.unwrap();
    let items = sections[0].on_deck().await.unwrap();
    assert!(items.is_empty());
}

#[tokio::test]
async fn unknown_metadata_type_surfaces_as_error() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(root_body()))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/library/sections"))
        .respond_with(ResponseTemplate::new(200).set_body_json(sections_body()))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/library/sections/1/recentlyAdded"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "MediaContainer": {
                "size": 1,
                "Metadata": [{"ratingKey": "1", "key": "/library/metadata/1",
                              "type": "totally-new-kind", "title": "?"}]
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
    let err = sections[0].recently_added().await.unwrap_err();
    assert!(matches!(err, plex_rs::Error::Config(_)));
}
