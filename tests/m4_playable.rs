//! M4.6 / Playable integration test — `direct_play_url()` builds a
//! token-bearing URL pointing at the first part's wire key.

use plex_rs::{Playable, PlexServer, PlexToken, Reload};
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
fn movies_listing_body() -> serde_json::Value {
    serde_json::json!({
        "MediaContainer": {
            "size": 1,
            "Metadata": [{
                "ratingKey": "100", "key": "/library/metadata/100",
                "title": "Arrival"
            }]
        }
    })
}
fn movie_detail_body() -> serde_json::Value {
    serde_json::json!({
        "MediaContainer": {
            "size": 1,
            "Metadata": [{
                "ratingKey": "100",
                "key": "/library/metadata/100",
                "title": "Arrival",
                "Media": [{
                    "id": 1,
                    "Part": [{
                        "id": 2,
                        "key": "/library/parts/2/1700000000/file.mkv"
                    }]
                }]
            }]
        }
    })
}

#[tokio::test]
async fn direct_play_url_embeds_token_after_reload() {
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
        .respond_with(ResponseTemplate::new(200).set_body_json(movies_listing_body()))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/library/metadata/100"))
        .respond_with(ResponseTemplate::new(200).set_body_json(movie_detail_body()))
        .mount(&server)
        .await;

    let plex = PlexServer::connect(
        Url::parse(&server.uri()).unwrap(),
        PlexToken::new("my-secret-token").unwrap(),
    )
    .await
    .unwrap();
    let partial = plex.library().sections().await.unwrap()[0]
        .movies()
        .await
        .unwrap()
        .into_iter()
        .next()
        .unwrap();

    // Before reload: media chain is empty, no playable URL.
    assert!(partial.direct_play_url().is_none());

    let full = partial.reload().await.unwrap();
    let url = full.direct_play_url().unwrap();
    assert_eq!(url.path(), "/library/parts/2/1700000000/file.mkv");
    let q = url.query().unwrap();
    assert!(q.contains("X-Plex-Token=my-secret-token"), "got query: {q}");
}
