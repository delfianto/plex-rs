//! M4.2 integration tests — Collection listing, items, deletion.

use plex_rs::{LibraryItem, PlexServer, PlexToken, SectionKind};
use url::Url;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn root_body() -> serde_json::Value {
    serde_json::json!({"MediaContainer": {"size": 0, "machineIdentifier": "m", "version": "v"}})
}
fn sections_body() -> serde_json::Value {
    serde_json::json!({
        "MediaContainer": {
            "size": 1,
            "Directory": [{"key": "1", "type": "movie", "title": "Movies"}]
        }
    })
}
fn collections_body() -> serde_json::Value {
    serde_json::json!({
        "MediaContainer": {
            "size": 2,
            "Metadata": [
                {
                    "ratingKey": "700", "key": "/library/collections/700",
                    "title": "Best of 2024", "subtype": "movie",
                    "childCount": 8, "smart": false,
                    "collectionMode": "default", "collectionSort": "alpha",
                    "thumb": "/library/metadata/700/thumb/1700000000"
                },
                {
                    "ratingKey": "701", "key": "/library/collections/701",
                    "title": "Sci-Fi Smart", "subtype": "movie",
                    "smart": "1", "leafCount": 42
                }
            ]
        }
    })
}
fn collection_items_body() -> serde_json::Value {
    serde_json::json!({
        "MediaContainer": {
            "size": 2,
            "Metadata": [
                {
                    "ratingKey": "100", "key": "/library/metadata/100",
                    "type": "movie", "title": "Movie One"
                },
                {
                    "ratingKey": "101", "key": "/library/metadata/101",
                    "type": "movie", "title": "Movie Two"
                }
            ]
        }
    })
}

async fn connected_with_section() -> (MockServer, plex_rs::LibrarySection) {
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
    let plex = PlexServer::connect(
        Url::parse(&server.uri()).unwrap(),
        PlexToken::new("tok").unwrap(),
    )
    .await
    .unwrap();
    let section = plex
        .library()
        .sections()
        .await
        .unwrap()
        .into_iter()
        .find(|s| s.kind == SectionKind::Movie)
        .unwrap();
    (server, section)
}

#[tokio::test]
async fn collections_returns_typed_list() {
    let (server, section) = connected_with_section().await;
    Mock::given(method("GET"))
        .and(path("/library/sections/1/collections"))
        .respond_with(ResponseTemplate::new(200).set_body_json(collections_body()))
        .expect(1)
        .mount(&server)
        .await;

    let collections = section.collections().await.unwrap();
    assert_eq!(collections.len(), 2);
    assert_eq!(collections[0].title, "Best of 2024");
    assert_eq!(collections[0].subtype.as_deref(), Some("movie"));
    assert_eq!(collections[0].child_count, Some(8));
    assert!(!collections[0].smart);

    assert!(collections[1].smart);
    assert_eq!(collections[1].leaf_count, Some(42));
}

#[tokio::test]
async fn collection_items_yields_library_items() {
    let (server, section) = connected_with_section().await;
    Mock::given(method("GET"))
        .and(path("/library/sections/1/collections"))
        .respond_with(ResponseTemplate::new(200).set_body_json(collections_body()))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/library/collections/700/children"))
        .respond_with(ResponseTemplate::new(200).set_body_json(collection_items_body()))
        .expect(1)
        .mount(&server)
        .await;

    let collection = section
        .collections()
        .await
        .unwrap()
        .into_iter()
        .next()
        .unwrap();
    let items = collection.items().await.unwrap();
    assert_eq!(items.len(), 2);
    assert!(matches!(items[0], LibraryItem::Movie(_)));
}

#[tokio::test]
async fn collection_delete_hits_delete_endpoint() {
    let (server, section) = connected_with_section().await;
    Mock::given(method("GET"))
        .and(path("/library/sections/1/collections"))
        .respond_with(ResponseTemplate::new(200).set_body_json(collections_body()))
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/library/collections/700"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;

    let collection = section
        .collections()
        .await
        .unwrap()
        .into_iter()
        .next()
        .unwrap();
    collection.delete().await.unwrap();
}
