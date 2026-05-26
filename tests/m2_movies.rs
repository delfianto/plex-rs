//! M2.1 integration test — listing movies from a `MovieSection`.

use plex_rs::{PlexServer, PlexToken, SectionKind};
use url::Url;
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn root_body() -> serde_json::Value {
    serde_json::json!({
        "MediaContainer": {
            "size": 0,
            "machineIdentifier": "m",
            "version": "1.40.0"
        }
    })
}

fn sections_body() -> serde_json::Value {
    serde_json::json!({
        "MediaContainer": {
            "size": 1,
            "Directory": [{
                "key": "1", "type": "movie", "title": "Movies", "uuid": "movies-uuid"
            }]
        }
    })
}

fn movies_body() -> serde_json::Value {
    serde_json::json!({
        "MediaContainer": {
            "size": 2,
            "totalSize": 2,
            "Metadata": [
                {
                    "ratingKey": "100",
                    "key": "/library/metadata/100",
                    "title": "Arrival",
                    "year": 2016,
                    "duration": 6_963_000,
                    "viewCount": 1,
                    "rating": 7.9
                },
                {
                    "ratingKey": "200",
                    "key": "/library/metadata/200",
                    "title": "Blade Runner 2049",
                    "year": 2017,
                    "duration": 9_840_000,
                    "viewCount": 0
                }
            ]
        }
    })
}

async fn setup() -> (MockServer, PlexServer) {
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
    let url = Url::parse(&server.uri()).unwrap();
    let token = PlexToken::new("tok").unwrap();
    let plex = PlexServer::connect(url, token).await.unwrap();
    (server, plex)
}

#[tokio::test]
async fn lists_movies_in_a_movie_section() {
    let (server, plex) = setup().await;
    Mock::given(method("GET"))
        .and(path("/library/sections/1/all"))
        .and(query_param("type", "1"))
        .and(header("x-plex-token", "tok"))
        .respond_with(ResponseTemplate::new(200).set_body_json(movies_body()))
        .expect(1)
        .mount(&server)
        .await;

    let sections = plex.library().sections().await.unwrap();
    let section = sections
        .iter()
        .find(|s| s.kind == SectionKind::Movie)
        .unwrap();
    let movies = section.movies().await.unwrap();
    assert_eq!(movies.len(), 2);

    assert_eq!(movies[0].rating_key.get(), 100);
    assert_eq!(movies[0].title, "Arrival");
    assert_eq!(movies[0].year, Some(2016));
    assert_eq!(movies[0].rating, Some(7.9));
    assert_eq!(movies[0].duration_ms, Some(6_963_000));
    assert!(movies[0].is_played());

    assert_eq!(movies[1].title, "Blade Runner 2049");
    assert!(!movies[1].is_played());
}

#[tokio::test]
async fn movies_rejects_non_movie_section() {
    // A "show" section being asked for movies should fail cleanly.
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(root_body()))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/library/sections"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "MediaContainer": {
                "size": 1,
                "Directory": [{"key": "2", "type": "show", "title": "TV"}]
            }
        })))
        .mount(&server)
        .await;
    let plex = PlexServer::connect(
        Url::parse(&server.uri()).unwrap(),
        PlexToken::new("tok").unwrap(),
    )
    .await
    .unwrap();
    let sections = plex.library().sections().await.unwrap();
    let err = sections[0].movies().await.unwrap_err();
    assert!(matches!(err, plex_rs::Error::Config(_)));
}
