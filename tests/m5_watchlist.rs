//! M5.5 — plex.tv Watchlist integration tests.

use plex_rs::{
    ClientIdentifier, MyPlexClient, PlexToken, WatchlistFilter, WatchlistKind, WatchlistOptions,
};
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn client_pointing_at(discover_mock_uri: &str) -> MyPlexClient {
    let cid = ClientIdentifier::new("watchlist-test").unwrap();
    MyPlexClient::new(PlexToken::new("acct-token").unwrap(), cid, None)
        .unwrap()
        .with_discover_base(discover_mock_uri)
}

fn watchlist_body(items: Vec<serde_json::Value>) -> serde_json::Value {
    serde_json::json!({
        "MediaContainer": {
            "size": items.len(),
            "Metadata": items,
        }
    })
}

fn movie(guid_hex: &str, title: &str) -> serde_json::Value {
    serde_json::json!({
        "guid": format!("plex://movie/{guid_hex}"),
        "type": "movie",
        "title": title,
        "year": 2016,
        "rating": 8.5,
    })
}

#[tokio::test]
async fn watchlist_default_lists_all_filter() {
    let discover = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/library/sections/watchlist/all"))
        .and(query_param("includeCollections", "1"))
        .and(query_param("includeExternalMedia", "1"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(watchlist_body(vec![
                movie("aaaa", "Arrival"),
                movie("bbbb", "Dune"),
            ])),
        )
        .expect(1)
        .mount(&discover)
        .await;

    let client = client_pointing_at(&discover.uri());
    let items = client.watchlist().await.unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].rating_key, "aaaa");
    assert_eq!(items[0].title, "Arrival");
    assert_eq!(items[1].rating_key, "bbbb");
}

#[tokio::test]
async fn watchlist_with_filter_released_threads_path() {
    let discover = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/library/sections/watchlist/released"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(watchlist_body(vec![movie("xxxx", "X")])),
        )
        .expect(1)
        .mount(&discover)
        .await;
    let opts = WatchlistOptions::default().with_filter(WatchlistFilter::Released);
    let items = client_pointing_at(&discover.uri())
        .watchlist_with(&opts)
        .await
        .unwrap();
    assert_eq!(items.len(), 1);
}

#[tokio::test]
async fn watchlist_with_kind_filter_serializes_numeric_type() {
    let discover = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/library/sections/watchlist/all"))
        .and(query_param("type", "1")) // 1 = movie
        .respond_with(ResponseTemplate::new(200).set_body_json(watchlist_body(vec![])))
        .expect(1)
        .mount(&discover)
        .await;
    let opts = WatchlistOptions::default().with_kind(WatchlistKind::Movie);
    let items = client_pointing_at(&discover.uri())
        .watchlist_with(&opts)
        .await
        .unwrap();
    assert!(items.is_empty());
}

#[tokio::test]
async fn watchlist_caps_results_to_max_results() {
    let discover = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/library/sections/watchlist/all"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(watchlist_body(vec![
                movie("a", "A"),
                movie("b", "B"),
                movie("c", "C"),
                movie("d", "D"),
            ])),
        )
        .expect(1)
        .mount(&discover)
        .await;
    let opts = WatchlistOptions::default().with_max_results(2);
    let items = client_pointing_at(&discover.uri())
        .watchlist_with(&opts)
        .await
        .unwrap();
    assert_eq!(items.len(), 2);
}

#[tokio::test]
async fn add_to_watchlist_puts_with_rating_key() {
    let discover = MockServer::start().await;
    Mock::given(method("PUT"))
        .and(path("/actions/addToWatchlist"))
        .and(query_param("ratingKey", "5d776b59ad5437001f796d8b"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&discover)
        .await;
    client_pointing_at(&discover.uri())
        .add_to_watchlist("5d776b59ad5437001f796d8b")
        .await
        .unwrap();
}

#[tokio::test]
async fn remove_from_watchlist_puts_with_rating_key() {
    let discover = MockServer::start().await;
    Mock::given(method("PUT"))
        .and(path("/actions/removeFromWatchlist"))
        .and(query_param("ratingKey", "deadbeef"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&discover)
        .await;
    client_pointing_at(&discover.uri())
        .remove_from_watchlist("deadbeef")
        .await
        .unwrap();
}
