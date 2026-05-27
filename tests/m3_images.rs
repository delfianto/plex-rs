//! M3.6 integration tests — image URL building and lock toggles.

use plex_rs::{HasArtLock, HasArtUrl, HasPosterUrl, PlexServer, PlexToken};
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
                "ratingKey": "100",
                "key": "/library/metadata/100",
                "title": "Arrival",
                "thumb": "/library/metadata/100/thumb/1700000000",
                "art": "/library/metadata/100/art/1700000000"
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
async fn art_url_resolves_against_base() {
    let (_server, movie) = setup().await;
    let url = movie.art_url().unwrap().unwrap();
    assert_eq!(url.path(), "/library/metadata/100/art/1700000000");
    // Host comes from the wiremock server.
    assert!(url.host_str().is_some());
}

#[tokio::test]
async fn poster_url_resolves_against_base() {
    let (_server, movie) = setup().await;
    let url = movie.poster_url().unwrap().unwrap();
    assert_eq!(url.path(), "/library/metadata/100/thumb/1700000000");
}

#[tokio::test]
async fn lock_art_emits_art_locked_one() {
    let (server, movie) = setup().await;
    // Lock toggles emit JUST `<field>.locked=<0|1>` — no `.value`
    // pair (analysis/08 §6).
    Mock::given(method("PUT"))
        .and(path("/library/sections/1/all"))
        .and(query_param("id", "100"))
        .and(query_param("art.locked", "1"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;

    movie.lock_art().await.unwrap();
}

#[tokio::test]
async fn unlock_art_emits_art_locked_zero() {
    let (server, movie) = setup().await;
    Mock::given(method("PUT"))
        .and(path("/library/sections/1/all"))
        .and(query_param("id", "100"))
        .and(query_param("art.locked", "0"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;

    movie.unlock_art().await.unwrap();
}
