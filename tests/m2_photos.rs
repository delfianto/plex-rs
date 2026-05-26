//! M2.4 integration tests — Photoalbum with mixed (album + photo)
//! children.

use plex_rs::{PhotoEntry, PlexServer, PlexToken, SectionKind};
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
            "Directory": [{"key": "7", "type": "photo", "title": "Photos", "uuid": "photos-uuid"}]
        }
    })
}
fn top_albums_body() -> serde_json::Value {
    serde_json::json!({
        "MediaContainer": {
            "size": 1,
            "Metadata": [{
                "ratingKey": "3000",
                "key": "/library/metadata/3000",
                "type": "photoalbum",
                "title": "Vacations",
                "summary": "Family trips",
                "childCount": 2,
                "addedAt": 1_700_000_000
            }]
        }
    })
}
fn mixed_children_body() -> serde_json::Value {
    serde_json::json!({
        "MediaContainer": {
            "size": 3,
            "Metadata": [
                {
                    "ratingKey": "3100",
                    "key": "/library/metadata/3100",
                    "type": "photoalbum",
                    "title": "2023 Hawaii",
                    "parentRatingKey": "3000"
                },
                {
                    "ratingKey": "3200",
                    "key": "/library/metadata/3200",
                    "type": "photo",
                    "title": "beach.jpg",
                    "parentRatingKey": "3000",
                    "year": 2023
                },
                {
                    "ratingKey": "3201",
                    "key": "/library/metadata/3201",
                    "type": "photo",
                    "title": "sunset.jpg",
                    "parentRatingKey": "3000"
                }
            ]
        }
    })
}

#[tokio::test]
async fn photoalbum_children_yields_mixed_albums_and_photos() {
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
        .and(path("/library/sections/7/all"))
        .respond_with(ResponseTemplate::new(200).set_body_json(top_albums_body()))
        .mount(&server)
        .await;
    // We hit /children three times below: once via .children(), once via
    // .sub_albums(), once via .photos(). Each call is independent —
    // exercising the convenience-filter helpers verifies they really do
    // round-trip the same data.
    Mock::given(method("GET"))
        .and(path("/library/metadata/3000/children"))
        .respond_with(ResponseTemplate::new(200).set_body_json(mixed_children_body()))
        .expect(3)
        .mount(&server)
        .await;

    let plex = PlexServer::connect(
        Url::parse(&server.uri()).unwrap(),
        PlexToken::new("tok").unwrap(),
    )
    .await
    .unwrap();
    let sections = plex.library().sections().await.unwrap();
    let photos_section = sections
        .iter()
        .find(|s| s.kind == SectionKind::Photo)
        .unwrap();
    let top = photos_section.photoalbums().await.unwrap();
    assert_eq!(top.len(), 1);
    assert_eq!(top[0].title, "Vacations");
    assert_eq!(top[0].child_count, Some(2));

    let entries = top[0].children().await.unwrap();
    assert_eq!(entries.len(), 3);
    // First child is a sub-album.
    assert!(matches!(entries[0], PhotoEntry::Album(_)));
    // Next two are photos.
    assert!(matches!(entries[1], PhotoEntry::Photo(_)));
    assert!(matches!(entries[2], PhotoEntry::Photo(_)));

    // Convenience filters: sub_albums() and photos() partition the same set.
    let subs = top[0].sub_albums().await.unwrap();
    assert_eq!(subs.len(), 1);
    assert_eq!(subs[0].title, "2023 Hawaii");
    let direct = top[0].photos().await.unwrap();
    assert_eq!(direct.len(), 2);
    assert_eq!(direct[0].title, "beach.jpg");
    assert_eq!(direct[0].year, Some(2023));
}
