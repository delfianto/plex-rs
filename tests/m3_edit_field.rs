//! M3.4 integration tests — EditField / EditTitle / EditSummary
//! hit the right section-keyed PUT endpoint.

use plex_rs::{EditSummary, EditTitle, PlexServer, PlexToken};
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
            "Directory": [{"key": "1", "type": "movie", "title": "Movies"}]
        }
    })
}
fn movies_body() -> serde_json::Value {
    serde_json::json!({
        "MediaContainer": {
            "size": 1,
            "Metadata": [{
                "ratingKey": "100", "key": "/library/metadata/100",
                "title": "Old Title", "summary": "Old summary"
            }]
        }
    })
}

async fn setup() -> (MockServer, plex_rs::Movie) {
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
        .respond_with(ResponseTemplate::new(200).set_body_json(movies_body()))
        .mount(&server)
        .await;
    let plex = PlexServer::connect(
        Url::parse(&server.uri()).unwrap(),
        PlexToken::new("tok").unwrap(),
    )
    .await
    .unwrap();
    let movie = plex.library().sections().await.unwrap()[0]
        .movies()
        .await
        .unwrap()
        .into_iter()
        .next()
        .unwrap();
    (server, movie)
}

#[tokio::test]
async fn edit_title_routes_through_section_endpoint() {
    let (server, movie) = setup().await;
    // The edit endpoint is on the SECTION, not the metadata item — see
    // analysis/11 §2.4. The id, type, field.value, and field.locked
    // pairs are all in the query string.
    Mock::given(method("PUT"))
        .and(path("/library/sections/1/all"))
        .and(query_param("id", "100"))
        .and(query_param("type", "1"))
        .and(query_param("title.value", "New Title"))
        .and(query_param("title.locked", "1"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;

    movie.edit_title("New Title", true).await.unwrap();
}

#[tokio::test]
async fn edit_summary_unlocks_after_setting() {
    let (server, movie) = setup().await;
    Mock::given(method("PUT"))
        .and(path("/library/sections/1/all"))
        .and(query_param("id", "100"))
        .and(query_param("summary.value", "Fresh summary"))
        .and(query_param("summary.locked", "0"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;

    movie.edit_summary("Fresh summary", false).await.unwrap();
}

#[tokio::test]
async fn edit_title_pct_encodes_value() {
    let (server, movie) = setup().await;
    Mock::given(method("PUT"))
        .and(path("/library/sections/1/all"))
        // wiremock decodes the param value for matching.
        .and(query_param("title.value", "Title with spaces & symbols=!"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;

    movie
        .edit_title("Title with spaces & symbols=!", true)
        .await
        .unwrap();
}
