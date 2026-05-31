//! M3.6 image-trait coverage for the Show type — theme URL + theme /
//! art / poster lock toggles that the movie-only `m3_images` suite
//! doesn't exercise.

use plex_rs::{
    HasArtLock, HasArtUrl, HasPosterLock, HasPosterUrl, HasThemeLock, HasThemeUrl, PlexServer,
    PlexToken, SectionKind,
};
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
            "Directory": [{"key": "3", "type": "show", "title": "TV Shows"}]
        }
    })
}
fn shows_body() -> serde_json::Value {
    serde_json::json!({
        "MediaContainer": {
            "size": 1,
            "Metadata": [{
                "ratingKey": "1000",
                "key": "/library/metadata/1000",
                "title": "The Expanse",
                "thumb": "/library/metadata/1000/thumb/123",
                "art": "/library/metadata/1000/art/123",
                "theme": "/library/metadata/1000/theme/123"
            }]
        }
    })
}

async fn setup() -> (MockServer, plex_rs::Show) {
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
        .and(path("/library/sections/3/all"))
        .respond_with(ResponseTemplate::new(200).set_body_json(shows_body()))
        .mount(&server)
        .await;
    let plex = PlexServer::connect(
        Url::parse(&server.uri()).unwrap(),
        PlexToken::new("tok").unwrap(),
    )
    .await
    .unwrap();
    let show = plex
        .library()
        .sections()
        .await
        .unwrap()
        .into_iter()
        .find(|s| s.kind == SectionKind::Show)
        .unwrap()
        .shows()
        .await
        .unwrap()
        .into_iter()
        .next()
        .unwrap();
    (server, show)
}

#[tokio::test]
async fn show_image_urls_resolve_against_base() {
    let (_server, show) = setup().await;
    assert_eq!(
        show.art_url().unwrap().unwrap().path(),
        "/library/metadata/1000/art/123"
    );
    assert_eq!(
        show.poster_url().unwrap().unwrap().path(),
        "/library/metadata/1000/thumb/123"
    );
    assert_eq!(
        show.theme_url().unwrap().unwrap().path(),
        "/library/metadata/1000/theme/123"
    );
}

#[tokio::test]
async fn show_lock_theme_emits_theme_locked_one() {
    let (server, show) = setup().await;
    Mock::given(method("PUT"))
        .and(path("/library/sections/3/all"))
        .and(query_param("id", "1000"))
        .and(query_param("theme.locked", "1"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;
    show.lock_theme().await.unwrap();
}

#[tokio::test]
async fn show_unlock_theme_emits_theme_locked_zero() {
    let (server, show) = setup().await;
    Mock::given(method("PUT"))
        .and(path("/library/sections/3/all"))
        .and(query_param("id", "1000"))
        .and(query_param("theme.locked", "0"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;
    show.unlock_theme().await.unwrap();
}

#[tokio::test]
async fn show_lock_poster_emits_thumb_locked_one() {
    let (server, show) = setup().await;
    Mock::given(method("PUT"))
        .and(path("/library/sections/3/all"))
        .and(query_param("id", "1000"))
        .and(query_param("thumb.locked", "1"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;
    show.lock_poster().await.unwrap();
}

#[tokio::test]
async fn show_unlock_art_emits_art_locked_zero() {
    let (server, show) = setup().await;
    Mock::given(method("PUT"))
        .and(path("/library/sections/3/all"))
        .and(query_param("id", "1000"))
        .and(query_param("art.locked", "0"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;
    show.unlock_art().await.unwrap();
}
