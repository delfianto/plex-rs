//! M2.9 integration test — end-to-end `FilterBuilder` execution.

use plex_rs::{FilterBuilder, LibraryItem, PlexServer, PlexToken};
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
async fn filter_builder_emits_canonical_wire_form() {
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
        // Plex's wire ops: type, equal, gt, sort, limit.
        .and(query_param("type", "1"))
        .and(query_param("genre", "Action"))
        // wiremock decodes the wire `>>=` once -> field name "year>>" with value "2010".
        .and(query_param("year>>", "2010"))
        .and(query_param("sort", "rating:desc"))
        .and(query_param("limit", "5"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "MediaContainer": {
                "size": 1,
                "Metadata": [
                    {"ratingKey": "100", "key": "/library/metadata/100",
                     "type": "movie", "title": "Sicario", "year": 2015, "rating": 7.6}
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
    let filter = FilterBuilder::new()
        .libtype(1)
        .equal("genre", "Action")
        .gt("year", 2010)
        .sort_by_desc("rating")
        .limit(5);
    let items = sections[0].filter(&filter).await.unwrap();
    assert_eq!(items.len(), 1);
    if let LibraryItem::Movie(m) = &items[0] {
        assert_eq!(m.title, "Sicario");
        assert_eq!(m.year, Some(2015));
    } else {
        panic!("expected Movie variant");
    }
}

#[tokio::test]
async fn empty_filter_hits_all_without_query() {
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
    let items = sections[0].filter(&FilterBuilder::new()).await.unwrap();
    assert!(items.is_empty());
}
