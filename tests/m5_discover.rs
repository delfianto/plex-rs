//! M5.5 — Discover search integration tests.

use plex_rs::{ClientIdentifier, DiscoverKind, DiscoverOptions, MyPlexClient, PlexToken};
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn client_pointing_at(discover_mock_uri: &str) -> MyPlexClient {
    let cid = ClientIdentifier::new("discover-test").unwrap();
    MyPlexClient::new(PlexToken::new("acct-token").unwrap(), cid, None)
        .unwrap()
        .with_discover_base(discover_mock_uri)
}

fn search_body(hits: Vec<serde_json::Value>) -> serde_json::Value {
    serde_json::json!({
        "MediaContainer": {
            "size": hits.len(),
            "SearchResults": [{
                "id": "external",
                "SearchResult": hits,
            }]
        }
    })
}

fn movie_hit(rk: &str, title: &str, year: u16) -> serde_json::Value {
    serde_json::json!({
        "score": 0.9,
        "Metadata": {
            "guid": format!("plex://movie/{rk}"),
            "type": "movie",
            "title": title,
            "year": year,
            "rating": 8.0,
        }
    })
}

#[tokio::test]
async fn discover_search_default_options() {
    let discover = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/library/search"))
        .and(query_param("query", "arrival"))
        .and(query_param("limit", "30"))
        .and(query_param("searchTypes", "movies,tv"))
        .and(query_param("searchProviders", "discover"))
        .and(query_param("includeMetadata", "1"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(search_body(vec![movie_hit("aaaa", "Arrival", 2016)])),
        )
        .expect(1)
        .mount(&discover)
        .await;

    let items = client_pointing_at(&discover.uri())
        .discover_search("arrival", &DiscoverOptions::default())
        .await
        .unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].rating_key, "aaaa");
    assert_eq!(items[0].title, "Arrival");
    assert_eq!(items[0].year, Some(2016));
    assert_eq!(items[0].score, Some(0.9));
}

#[tokio::test]
async fn discover_search_with_kind_filter() {
    let discover = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/library/search"))
        .and(query_param("searchTypes", "tv"))
        .respond_with(ResponseTemplate::new(200).set_body_json(search_body(vec![])))
        .expect(1)
        .mount(&discover)
        .await;
    let opts = DiscoverOptions::default().with_kind(DiscoverKind::Show);
    let items = client_pointing_at(&discover.uri())
        .discover_search("severance", &opts)
        .await
        .unwrap();
    assert!(items.is_empty());
}

#[tokio::test]
async fn discover_search_with_custom_providers() {
    let discover = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/library/search"))
        .and(query_param("searchProviders", "discover,PLEXAVOD"))
        .respond_with(ResponseTemplate::new(200).set_body_json(search_body(vec![])))
        .expect(1)
        .mount(&discover)
        .await;
    let opts = DiscoverOptions::default().with_providers("discover,PLEXAVOD");
    let items = client_pointing_at(&discover.uri())
        .discover_search("free movies", &opts)
        .await
        .unwrap();
    assert!(items.is_empty());
}

#[tokio::test]
async fn discover_search_with_limit() {
    let discover = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/library/search"))
        .and(query_param("limit", "5"))
        .respond_with(ResponseTemplate::new(200).set_body_json(search_body(vec![
            movie_hit("a", "A", 2020),
            movie_hit("b", "B", 2021),
        ])))
        .expect(1)
        .mount(&discover)
        .await;
    let opts = DiscoverOptions::default().with_limit(5);
    let items = client_pointing_at(&discover.uri())
        .discover_search("test", &opts)
        .await
        .unwrap();
    assert_eq!(items.len(), 2);
}
