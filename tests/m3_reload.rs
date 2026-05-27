//! M3.2 integration test — `Reload::reload()` upgrades a partial
//! item to the full-detail record from
//! `/library/metadata/<rating_key>`.

use plex_rs::{PlexServer, PlexToken, Reload};
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
/// Partial listing — what a section's `/all` endpoint typically returns.
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
/// Full detail — what `/library/metadata/<rk>` returns: same scalar
/// fields but with Media[] / tags / etc. populated.
fn movie_detail_body() -> serde_json::Value {
    serde_json::json!({
        "MediaContainer": {
            "size": 1,
            "Metadata": [{
                "ratingKey": "100",
                "key": "/library/metadata/100",
                "title": "Arrival",
                "summary": "First contact, on linguists' terms.",
                "year": 2016,
                "duration": 6_963_000,
                "rating": 7.9,
                "Genre": [{"tag": "Sci-Fi"}, {"tag": "Drama"}],
                "Director": [{"tag": "Denis Villeneuve"}],
                "Media": [{
                    "id": 1,
                    "duration": 6_963_000,
                    "videoCodec": "h264",
                    "Part": [{"id": 2, "key": "/library/parts/2/file.mkv"}]
                }]
            }]
        }
    })
}

#[tokio::test]
async fn reload_upgrades_partial_movie_to_full_detail() {
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
        .expect(1)
        .mount(&server)
        .await;

    let plex = PlexServer::connect(
        Url::parse(&server.uri()).unwrap(),
        PlexToken::new("tok").unwrap(),
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

    // Partial: tags + media should be empty.
    assert!(partial.tags.is_empty());
    assert!(partial.media.is_empty());
    assert!(partial.summary.is_none());

    let full = partial.reload().await.unwrap();
    assert_eq!(full.title, "Arrival");
    assert_eq!(full.year, Some(2016));
    assert_eq!(full.rating, Some(7.9));
    assert_eq!(full.tags.len(), 3); // 2 genres + 1 director
    assert_eq!(full.media.len(), 1);
    assert!(full.summary.is_some());
}

#[tokio::test]
async fn reload_surfaces_404_as_not_found() {
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
        .respond_with(ResponseTemplate::new(404))
        .expect(1)
        .mount(&server)
        .await;

    let plex = PlexServer::connect(
        Url::parse(&server.uri()).unwrap(),
        PlexToken::new("tok").unwrap(),
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
    let err = partial.reload().await.unwrap_err();
    assert!(matches!(err, plex_rs::Error::NotFound { .. }));
}
