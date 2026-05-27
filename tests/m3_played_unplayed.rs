//! M3.1 integration test — `PlayedUnplayed::mark_played` /
//! `mark_unplayed` hit the right scrobble endpoints.

use plex_rs::{PlayedUnplayed, PlexServer, PlexToken};
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
fn movies_body() -> serde_json::Value {
    serde_json::json!({
        "MediaContainer": {
            "size": 1,
            "Metadata": [{
                "ratingKey": "100",
                "key": "/library/metadata/100",
                "title": "Arrival",
                "viewCount": 0
            }]
        }
    })
}

async fn setup_with_movie() -> (MockServer, plex_rs::Movie) {
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
    let sections = plex.library().sections().await.unwrap();
    let movies = sections[0].movies().await.unwrap();
    (server, movies.into_iter().next().unwrap())
}

#[tokio::test]
async fn mark_played_hits_scrobble_with_correct_key_and_identifier() {
    let (server, movie) = setup_with_movie().await;
    Mock::given(method("GET"))
        .and(path("/:/scrobble"))
        .and(query_param("key", "100"))
        .and(query_param("identifier", "com.plexapp.plugins.library"))
        .respond_with(ResponseTemplate::new(200).set_body_string(""))
        .expect(1)
        .mount(&server)
        .await;

    movie.mark_played().await.unwrap();
}

#[tokio::test]
async fn mark_unplayed_hits_unscrobble() {
    let (server, movie) = setup_with_movie().await;
    Mock::given(method("GET"))
        .and(path("/:/unscrobble"))
        .and(query_param("key", "100"))
        .and(query_param("identifier", "com.plexapp.plugins.library"))
        .respond_with(ResponseTemplate::new(200).set_body_string(""))
        .expect(1)
        .mount(&server)
        .await;

    movie.mark_unplayed().await.unwrap();
}

#[tokio::test]
async fn is_played_reads_view_count() {
    let (_server, movie) = setup_with_movie().await;
    // The fixture has viewCount=0.
    assert!(!movie.is_played());
    // Verified via the trait too — both forms must agree.
    assert!(!PlayedUnplayed::is_played(&movie));
}
