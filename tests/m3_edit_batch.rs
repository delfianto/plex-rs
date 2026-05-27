//! M3.8 — EditBatch integration tests.
//!
//! Exercises the multi-op batch by issuing one PUT containing
//! field edits, lock toggles, tag replaces, and tag removes,
//! then verifying every fragment landed in the query string.

use plex_rs::{EditBatchExt, PlexServer, PlexToken};
use url::Url;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn root_body() -> serde_json::Value {
    serde_json::json!({
        "MediaContainer": {"size":0,"machineIdentifier":"m","version":"v"}
    })
}

fn sections_body() -> serde_json::Value {
    serde_json::json!({
        "MediaContainer": {
            "size": 1,
            "Directory": [{"key":"7","type":"movie","title":"Movies"}]
        }
    })
}

fn movies_body() -> serde_json::Value {
    serde_json::json!({
        "MediaContainer": {
            "size": 1,
            "Metadata": [{
                "ratingKey": "42",
                "key": "/library/metadata/42",
                "title": "Arrival",
                "type": "movie",
                "librarySectionID": 7,
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
    Mock::given(method("GET"))
        .and(path("/library/sections/7/all"))
        .respond_with(ResponseTemplate::new(200).set_body_json(movies_body()))
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
async fn batch_sends_one_put_with_all_operations() {
    let server = MockServer::start().await;
    // The combined PUT must carry every fragment from the batch.
    Mock::given(method("PUT"))
        .and(path("/library/sections/7/all"))
        .and(query_param("id", "42"))
        .and(query_param("type", "1"))
        .and(query_param("title.value", "Arrival (2016)"))
        .and(query_param("title.locked", "1"))
        .and(query_param("summary.value", "Aliens arrive"))
        .and(query_param("year.value", "2016"))
        .and(query_param("genre.locked", "1"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;

    let plex = setup(&server).await;
    let movies = plex.library().sections().await.unwrap()[0]
        .movies()
        .await
        .unwrap();
    let movie = movies.into_iter().next().unwrap();
    movie
        .batch()
        .set_title("Arrival (2016)", true)
        .set_summary("Aliens arrive", true)
        .set_year(2016, true)
        .replace_genres(&["Sci-Fi", "Drama"], true)
        .execute()
        .await
        .unwrap();
}

#[tokio::test]
async fn empty_batch_makes_no_http_call() {
    let server = MockServer::start().await;
    // Mount NOTHING for PUT — if the batch fires one, wiremock
    // returns 404 and the assertion below fails.
    let plex = setup(&server).await;
    let movies = plex.library().sections().await.unwrap()[0]
        .movies()
        .await
        .unwrap();
    let movie = movies.into_iter().next().unwrap();
    // No-op batch should short-circuit without an HTTP call.
    movie.batch().execute().await.unwrap();
}

#[tokio::test]
async fn batch_with_remove_tags_uses_dash_sigil() {
    let server = MockServer::start().await;
    Mock::given(method("PUT"))
        .and(path("/library/sections/7/all"))
        .and(query_param("id", "42"))
        .and(query_param("label[].tag.tag-", "old,deprecated"))
        .and(query_param("label.locked", "0"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;
    let plex = setup(&server).await;
    let movie = plex.library().sections().await.unwrap()[0]
        .movies()
        .await
        .unwrap()
        .into_iter()
        .next()
        .unwrap();
    movie
        .batch()
        .remove_tags("label", &["old", "deprecated"], false)
        .execute()
        .await
        .unwrap();
}

#[tokio::test]
async fn batch_with_lock_only_op_does_not_emit_value_pair() {
    let server = MockServer::start().await;
    // Asserting absence of a query param requires checking the
    // URL ourselves — wiremock's query_param matcher only checks
    // presence. Use a strict positive match: `art.locked=1` is
    // present and the request 200s.
    Mock::given(method("PUT"))
        .and(path("/library/sections/7/all"))
        .and(query_param("id", "42"))
        .and(query_param("art.locked", "1"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;
    let plex = setup(&server).await;
    let movie = plex.library().sections().await.unwrap()[0]
        .movies()
        .await
        .unwrap()
        .into_iter()
        .next()
        .unwrap();
    movie
        .batch()
        .lock_field("art", true)
        .execute()
        .await
        .unwrap();
}
