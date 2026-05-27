//! M4.7 — history endpoint integration tests.
//!
//! Exercises pagination across multiple pages (via the
//! `X-Plex-Container-Start` request header), the filter builder, the
//! `max_results` cap, the streaming API, and the DELETE row path.

use futures_util::StreamExt;
use plex_rs::{LibraryItem, PlexServer, PlexToken, RatingKey};
use url::Url;
use wiremock::matchers::{header, header_exists, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn root_body() -> serde_json::Value {
    serde_json::json!({
        "MediaContainer": {
            "size": 0,
            "machineIdentifier": "m",
            "version": "v",
            "friendlyName": "Test"
        }
    })
}

fn sections_body() -> serde_json::Value {
    serde_json::json!({
        "MediaContainer": {
            "size": 0,
            "Directory": []
        }
    })
}

fn page_body(entries: Vec<serde_json::Value>, size: u32, total: u32) -> serde_json::Value {
    serde_json::json!({
        "MediaContainer": {
            "size": size,
            "totalSize": total,
            "Metadata": entries,
        }
    })
}

fn entry(rk: u64, viewed: i64, title: &str, account: u64) -> serde_json::Value {
    serde_json::json!({
        "accountID": account,
        "deviceID": 1,
        "historyKey": format!("/status/sessions/history/{rk}"),
        "viewedAt": viewed,
        "ratingKey": rk.to_string(),
        "key": format!("/library/metadata/{rk}"),
        "title": title,
        "type": "movie",
        "librarySectionID": 1,
    })
}

async fn connect(server: &MockServer) -> PlexServer {
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

#[tokio::test]
async fn collect_handles_single_page() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/status/sessions/history/all"))
        .and(query_param("sort", "viewedAt:desc"))
        .respond_with(ResponseTemplate::new(200).set_body_json(page_body(
            vec![
                entry(1, 1_700_000_000, "Arrival", 42),
                entry(2, 1_700_000_100, "Dune", 42),
            ],
            2,
            2,
        )))
        .expect(1)
        .mount(&server)
        .await;

    let plex = connect(&server).await;
    let entries = plex.history().collect().await.unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].account_id, 42);
    assert!(entries[0].viewed_at.is_some());
    match &entries[0].item {
        LibraryItem::Movie(m) => assert_eq!(m.title, "Arrival"),
        other => panic!("expected Movie, got {other:?}"),
    }
}

#[tokio::test]
async fn collect_paginates_via_container_start_header() {
    let server = MockServer::start().await;
    // Three pages of 2 entries each, totalSize=6.
    Mock::given(method("GET"))
        .and(path("/status/sessions/history/all"))
        .and(header("x-plex-container-start", "0"))
        .respond_with(ResponseTemplate::new(200).set_body_json(page_body(
            vec![entry(1, 100, "A", 1), entry(2, 200, "B", 1)],
            2,
            6,
        )))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/status/sessions/history/all"))
        .and(header("x-plex-container-start", "2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(page_body(
            vec![entry(3, 300, "C", 1), entry(4, 400, "D", 1)],
            2,
            6,
        )))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/status/sessions/history/all"))
        .and(header("x-plex-container-start", "4"))
        .respond_with(ResponseTemplate::new(200).set_body_json(page_body(
            vec![entry(5, 500, "E", 1), entry(6, 600, "F", 1)],
            2,
            6,
        )))
        .mount(&server)
        .await;

    let plex = connect(&server).await;
    let entries = plex.history().page_size(2).collect().await.unwrap();
    assert_eq!(entries.len(), 6);
    let titles: Vec<&str> = entries
        .iter()
        .map(|e| match &e.item {
            LibraryItem::Movie(m) => m.title.as_str(),
            _ => unreachable!(),
        })
        .collect();
    assert_eq!(titles, vec!["A", "B", "C", "D", "E", "F"]);
}

#[tokio::test]
async fn collect_honors_max_results_cap_mid_page() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/status/sessions/history/all"))
        .respond_with(ResponseTemplate::new(200).set_body_json(page_body(
            vec![
                entry(1, 100, "A", 1),
                entry(2, 200, "B", 1),
                entry(3, 300, "C", 1),
                entry(4, 400, "D", 1),
            ],
            4,
            4,
        )))
        .mount(&server)
        .await;

    let plex = connect(&server).await;
    let entries = plex.history().max_results(2).collect().await.unwrap();
    assert_eq!(entries.len(), 2);
}

#[tokio::test]
async fn filters_are_serialized_to_query_params() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/status/sessions/history/all"))
        .and(query_param("accountID", "42"))
        .and(query_param("librarySectionID", "1"))
        .and(query_param("metadataItemID", "100"))
        .respond_with(ResponseTemplate::new(200).set_body_json(page_body(
            vec![entry(100, 1_700_000_000, "Arrival", 42)],
            1,
            1,
        )))
        .expect(1)
        .mount(&server)
        .await;

    let plex = connect(&server).await;
    let entries = plex
        .history()
        .account(42)
        .library_section(1)
        .rating_key(RatingKey(100))
        .collect()
        .await
        .unwrap();
    assert_eq!(entries.len(), 1);
}

#[tokio::test]
async fn stream_yields_entries_lazily_across_pages() {
    let server = MockServer::start().await;
    // Two pages.
    Mock::given(method("GET"))
        .and(path("/status/sessions/history/all"))
        .and(header("x-plex-container-start", "0"))
        .and(header_exists("x-plex-container-size"))
        .respond_with(ResponseTemplate::new(200).set_body_json(page_body(
            vec![entry(1, 100, "A", 1), entry(2, 200, "B", 1)],
            2,
            4,
        )))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/status/sessions/history/all"))
        .and(header("x-plex-container-start", "2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(page_body(
            vec![entry(3, 300, "C", 1), entry(4, 400, "D", 1)],
            2,
            4,
        )))
        .mount(&server)
        .await;

    let plex = connect(&server).await;
    let mut s = plex.history().page_size(2).stream();
    let mut got = Vec::new();
    while let Some(item) = s.next().await {
        got.push(item.unwrap());
    }
    assert_eq!(got.len(), 4);
    assert_eq!(got[0].account_id, 1);
}

#[tokio::test]
async fn delete_targets_history_key() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/status/sessions/history/100"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/status/sessions/history/all"))
        .respond_with(ResponseTemplate::new(200).set_body_json(page_body(
            vec![entry(100, 1_700_000_000, "X", 1)],
            1,
            1,
        )))
        .mount(&server)
        .await;

    let plex = connect(&server).await;
    let entries = plex.history().collect().await.unwrap();
    let e = entries.into_iter().next().unwrap();
    e.delete(plex.http(), plex.base_url()).await.unwrap();
}
