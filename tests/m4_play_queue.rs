//! M4.4 — `PlayQueue` integration tests.
//!
//! Drives a wiremock-backed PMS through:
//!
//! 1. create from a single item (POST /playQueues with server:// uri)
//! 2. create from a list of items (uses library:///directory/ uri)
//! 3. get an existing queue by id
//! 4. add to "Up Next" (PUT)
//! 5. move item (PUT /items/{iid}/move)
//! 6. remove item (DELETE /items/{iid})
//! 7. clear (DELETE /items)

use plex_rs::{LibraryItem, PlayQueueId, PlexServer, PlexToken, RatingKey};
use url::Url;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn root_body() -> serde_json::Value {
    serde_json::json!({
        "MediaContainer": {
            "size": 0,
            "machineIdentifier": "abc123",
            "version": "v",
            "friendlyName": "Test"
        }
    })
}

fn sections_body() -> serde_json::Value {
    serde_json::json!({
        "MediaContainer": {
            "size": 1,
            "Directory": [{"key":"1","type":"movie","title":"Movies"}]
        }
    })
}

fn pq_body(id: u64, items: Vec<(u64, u64, &str)>) -> serde_json::Value {
    let metadata: Vec<serde_json::Value> = items
        .into_iter()
        .map(|(pqid, rk, title)| {
            serde_json::json!({
                "playQueueItemID": pqid,
                "ratingKey": rk.to_string(),
                "key": format!("/library/metadata/{rk}"),
                "title": title,
                "type": "movie",
                "librarySectionID": 1,
            })
        })
        .collect();
    serde_json::json!({
        "MediaContainer": {
            "identifier": "com.plexapp.plugins.library",
            "playQueueID": id,
            "playQueueVersion": 1,
            "playQueueTotalCount": metadata.len(),
            "playQueueSelectedItemID": metadata.first().and_then(|m| m.get("playQueueItemID").and_then(|v| v.as_u64())).unwrap_or(0),
            "playQueueSelectedItemOffset": 0,
            "playQueueSelectedMetadataItemID": 0,
            "playQueueShuffled": 0,
            "playQueueSourceURI": "server://abc123/com.plexapp.plugins.library/library/metadata/42",
            "Metadata": metadata,
        }
    })
}

fn movies_listing_body() -> serde_json::Value {
    serde_json::json!({
        "MediaContainer": {
            "size": 1,
            "Metadata": [{
                "ratingKey": "42",
                "key": "/library/metadata/42",
                "title": "Arrival",
                "type": "movie",
                "librarySectionID": 1,
            }, {
                "ratingKey": "43",
                "key": "/library/metadata/43",
                "title": "Dune",
                "type": "movie",
                "librarySectionID": 1,
            }]
        }
    })
}

async fn setup(server: &MockServer) -> PlexServer {
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
        PlexToken::new("token").unwrap(),
    )
    .await
    .unwrap()
}

async fn fetch_movies(plex: &PlexServer, server: &MockServer) -> Vec<LibraryItem> {
    Mock::given(method("GET"))
        .and(path("/library/sections/1/all"))
        .respond_with(ResponseTemplate::new(200).set_body_json(movies_listing_body()))
        .mount(server)
        .await;
    let sections = plex.library().sections().await.unwrap();
    sections[0]
        .movies()
        .await
        .unwrap()
        .into_iter()
        .map(LibraryItem::Movie)
        .collect()
}

#[tokio::test]
async fn create_from_item_posts_server_uri() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/playQueues"))
        .and(query_param("type", "video"))
        .and(query_param(
            "uri",
            "server://abc123/com.plexapp.plugins.library/library/metadata/42",
        ))
        .and(query_param("shuffle", "0"))
        .and(query_param("repeat", "0"))
        .and(query_param("continuous", "0"))
        .and(query_param("includeChapters", "1"))
        .and(query_param("includeRelated", "1"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(pq_body(500, vec![(100, 42, "Arrival")])),
        )
        .expect(1)
        .mount(&server)
        .await;

    let plex = setup(&server).await;
    let movies = fetch_movies(&plex, &server).await;
    let item = movies.iter().find(|m| m.rating_key().0 == 42).unwrap();
    let pq = plex
        .create_play_queue()
        .from_item(item)
        .execute()
        .await
        .unwrap();
    assert_eq!(pq.id, PlayQueueId(500));
    assert_eq!(pq.items.len(), 1);
    assert_eq!(pq.items[0].play_queue_item_id, 100);
}

#[tokio::test]
async fn create_from_items_uses_library_directory_uri() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/playQueues"))
        .and(query_param("type", "video"))
        .and(query_param(
            "uri",
            "library:///directory/%2Flibrary%2Fmetadata%2F42%2C43",
        ))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(pq_body(501, vec![(100, 42, "Arrival"), (101, 43, "Dune")])),
        )
        .expect(1)
        .mount(&server)
        .await;

    let plex = setup(&server).await;
    let movies = fetch_movies(&plex, &server).await;
    let a = &movies[0];
    let b = &movies[1];
    let items = [a, b];
    let pq = plex
        .create_play_queue()
        .from_items(&items)
        .execute()
        .await
        .unwrap();
    assert_eq!(pq.items.len(), 2);
}

#[tokio::test]
async fn get_fetches_existing_queue_by_id() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/playQueues/777"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(pq_body(777, vec![(100, 42, "Arrival")])),
        )
        .expect(1)
        .mount(&server)
        .await;

    let plex = setup(&server).await;
    let pq = plex.play_queue(PlayQueueId(777)).await.unwrap();
    assert_eq!(pq.id, PlayQueueId(777));
}

#[tokio::test]
async fn add_item_puts_with_uri_and_optionally_next() {
    let server = MockServer::start().await;
    Mock::given(method("PUT"))
        .and(path("/playQueues/500"))
        .and(query_param(
            "uri",
            "server://./com.plexapp.plugins.library/library/metadata/43",
        ))
        .and(query_param("next", "1"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(pq_body(500, vec![(100, 42, "Arrival"), (102, 43, "Dune")])),
        )
        .expect(1)
        .mount(&server)
        .await;
    // The initial GET to bootstrap the queue.
    Mock::given(method("GET"))
        .and(path("/playQueues/500"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(pq_body(500, vec![(100, 42, "Arrival")])),
        )
        .mount(&server)
        .await;

    let plex = setup(&server).await;
    let pq = plex.play_queue(PlayQueueId(500)).await.unwrap();
    let movies = fetch_movies(&plex, &server).await;
    let new_item = movies.iter().find(|m| m.rating_key().0 == 43).unwrap();
    let pq = pq.add_item(new_item, true).await.unwrap();
    assert_eq!(pq.items.len(), 2);
    assert_eq!(pq.items[1].play_queue_item_id, 102);
}

#[tokio::test]
async fn move_item_puts_move_with_after() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/playQueues/500"))
        .respond_with(ResponseTemplate::new(200).set_body_json(pq_body(
            500,
            vec![(100, 42, "Arrival"), (101, 43, "Dune"), (102, 44, "Tenet")],
        )))
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .and(path("/playQueues/500/items/100/move"))
        .and(query_param("after", "101"))
        .respond_with(ResponseTemplate::new(200).set_body_json(pq_body(
            500,
            vec![(101, 43, "Dune"), (100, 42, "Arrival"), (102, 44, "Tenet")],
        )))
        .expect(1)
        .mount(&server)
        .await;

    let plex = setup(&server).await;
    let pq = plex.play_queue(PlayQueueId(500)).await.unwrap();
    let pq = pq.move_item(100, Some(101)).await.unwrap();
    assert_eq!(pq.items[0].play_queue_item_id, 101);
    assert_eq!(pq.items[1].play_queue_item_id, 100);
}

#[tokio::test]
async fn remove_item_deletes_single_item() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/playQueues/500"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(pq_body(500, vec![(100, 42, "Arrival"), (101, 43, "Dune")])),
        )
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/playQueues/500/items/101"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(pq_body(500, vec![(100, 42, "Arrival")])),
        )
        .expect(1)
        .mount(&server)
        .await;

    let plex = setup(&server).await;
    let pq = plex.play_queue(PlayQueueId(500)).await.unwrap();
    let pq = pq.remove_item(101).await.unwrap();
    assert_eq!(pq.items.len(), 1);
}

#[tokio::test]
async fn clear_deletes_all_items() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/playQueues/500"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(pq_body(500, vec![(100, 42, "Arrival"), (101, 43, "Dune")])),
        )
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/playQueues/500/items"))
        .respond_with(ResponseTemplate::new(200).set_body_json(pq_body(500, vec![])))
        .expect(1)
        .mount(&server)
        .await;

    let plex = setup(&server).await;
    let pq = plex.play_queue(PlayQueueId(500)).await.unwrap();
    let pq = pq.clear().await.unwrap();
    assert!(pq.items.is_empty());
}

#[tokio::test]
async fn refresh_re_fetches_queue() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/playQueues/500"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(pq_body(500, vec![(100, 42, "Arrival")])),
        )
        .expect(2)
        .mount(&server)
        .await;

    let plex = setup(&server).await;
    let pq = plex.play_queue(PlayQueueId(500)).await.unwrap();
    let _ = pq.refresh().await.unwrap();
}

#[tokio::test]
async fn execute_without_source_errors() {
    let server = MockServer::start().await;
    let plex = setup(&server).await;
    let err = plex.create_play_queue().execute().await.unwrap_err();
    use plex_rs::error::Error;
    assert!(
        matches!(err, Error::Config(ref msg) if msg.contains("source")),
        "expected Config error, got {err:?}"
    );
    let _ = RatingKey(0); // suppress unused import warning
}
