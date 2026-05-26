//! M2.2 integration tests — Show → Season → Episode hierarchy.

use plex_rs::{PlexServer, PlexToken, SectionKind};
use url::Url;
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn root_body() -> serde_json::Value {
    serde_json::json!({"MediaContainer": {"size": 0, "machineIdentifier": "m", "version": "v"}})
}

fn sections_body() -> serde_json::Value {
    serde_json::json!({
        "MediaContainer": {
            "size": 1,
            "Directory": [{"key": "3", "type": "show", "title": "TV Shows", "uuid": "shows-uuid"}]
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
                "studio": "Syfy",
                "year": 2015,
                "rating": 8.7,
                "audienceRating": 9.1,
                "contentRating": "TV-14",
                "childCount": 6,
                "leafCount": 62,
                "viewedLeafCount": 30,
                "addedAt": 1_600_000_000,
                "updatedAt": 1_700_000_000,
                "thumb": "/library/metadata/1000/thumb/123",
                "art": "/library/metadata/1000/art/123",
                "theme": "/library/metadata/1000/theme/123",
                "guid": "plex://show/abcd1234"
            }]
        }
    })
}

fn seasons_body() -> serde_json::Value {
    serde_json::json!({
        "MediaContainer": {
            "size": 2,
            "Metadata": [
                {
                    "ratingKey": "1100",
                    "key": "/library/metadata/1100",
                    "title": "Season 1",
                    "index": 1,
                    "parentRatingKey": "1000",
                    "parentKey": "/library/metadata/1000",
                    "parentTitle": "The Expanse",
                    "parentThumb": "/library/metadata/1000/thumb/123",
                    "leafCount": 10,
                    "viewedLeafCount": 10,
                    "thumb": "/library/metadata/1100/thumb/456"
                },
                {
                    "ratingKey": "1200",
                    "key": "/library/metadata/1200",
                    "title": "Season 2",
                    "index": 2,
                    "parentRatingKey": "1000",
                    "parentTitle": "The Expanse",
                    "leafCount": 13,
                    "viewedLeafCount": 5
                }
            ]
        }
    })
}

fn episodes_body() -> serde_json::Value {
    serde_json::json!({
        "MediaContainer": {
            "size": 2,
            "Metadata": [
                {
                    "ratingKey": "1101",
                    "key": "/library/metadata/1101",
                    "title": "Dulcinea",
                    "index": 1,
                    "summary": "A missing girl's case leads to interplanetary discoveries.",
                    "duration": 2_700_000,
                    "year": 2015,
                    "originallyAvailableAt": "2015-12-14",
                    "viewCount": 1,
                    "parentRatingKey": "1100",
                    "parentKey": "/library/metadata/1100",
                    "parentTitle": "Season 1",
                    "parentIndex": 1,
                    "parentThumb": "/library/metadata/1100/thumb/456",
                    "grandparentRatingKey": "1000",
                    "grandparentKey": "/library/metadata/1000",
                    "grandparentTitle": "The Expanse",
                    "grandparentThumb": "/library/metadata/1000/thumb/123",
                    "grandparentArt": "/library/metadata/1000/art/123",
                    "grandparentTheme": "/library/metadata/1000/theme/123",
                    "thumb": "/library/metadata/1101/thumb/789"
                },
                {
                    "ratingKey": "1102",
                    "key": "/library/metadata/1102",
                    "title": "The Big Empty",
                    "index": 2,
                    "duration": 2_700_000,
                    "viewCount": 0,
                    "parentRatingKey": "1100",
                    "parentTitle": "Season 1",
                    "parentIndex": 1,
                    "grandparentRatingKey": "1000",
                    "grandparentTitle": "The Expanse"
                }
            ]
        }
    })
}

async fn connected(server: &MockServer) -> PlexServer {
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
        PlexToken::new("tok").unwrap(),
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn shows_lists_typed_show_section() {
    let server = MockServer::start().await;
    let plex = connected(&server).await;
    Mock::given(method("GET"))
        .and(path("/library/sections/3/all"))
        .and(query_param("type", "2"))
        .and(header("x-plex-token", "tok"))
        .respond_with(ResponseTemplate::new(200).set_body_json(shows_body()))
        .expect(1)
        .mount(&server)
        .await;

    let sections = plex.library().sections().await.unwrap();
    let tv = sections
        .iter()
        .find(|s| s.kind == SectionKind::Show)
        .unwrap();
    let shows = tv.shows().await.unwrap();
    assert_eq!(shows.len(), 1);
    let s = &shows[0];
    assert_eq!(s.rating_key.get(), 1000);
    assert_eq!(s.title, "The Expanse");
    assert_eq!(s.studio.as_deref(), Some("Syfy"));
    assert_eq!(s.year, Some(2015));
    assert_eq!(s.child_count, Some(6));
    assert_eq!(s.leaf_count, Some(62));
    assert_eq!(s.viewed_leaf_count, Some(30));
    let progress = s.watch_progress().unwrap();
    assert!((progress - (30.0 / 62.0)).abs() < 0.001);
    assert!(s.theme.is_some());
}

#[tokio::test]
async fn show_seasons_lists_typed_seasons() {
    let server = MockServer::start().await;
    let plex = connected(&server).await;
    Mock::given(method("GET"))
        .and(path("/library/sections/3/all"))
        .respond_with(ResponseTemplate::new(200).set_body_json(shows_body()))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/library/metadata/1000/children"))
        .respond_with(ResponseTemplate::new(200).set_body_json(seasons_body()))
        .expect(1)
        .mount(&server)
        .await;

    let sections = plex.library().sections().await.unwrap();
    let tv = sections
        .iter()
        .find(|s| s.kind == SectionKind::Show)
        .unwrap();
    let shows = tv.shows().await.unwrap();
    let seasons = shows[0].seasons().await.unwrap();
    assert_eq!(seasons.len(), 2);
    assert_eq!(seasons[0].index, Some(1));
    assert_eq!(seasons[0].parent_rating_key.get(), 1000);
    assert_eq!(seasons[0].parent_title.as_deref(), Some("The Expanse"));
    assert_eq!(seasons[0].leaf_count, Some(10));
    assert_eq!(seasons[1].index, Some(2));
    assert_eq!(seasons[1].leaf_count, Some(13));
}

#[tokio::test]
async fn season_episodes_carries_parent_grandparent_back_links() {
    let server = MockServer::start().await;
    let plex = connected(&server).await;
    Mock::given(method("GET"))
        .and(path("/library/sections/3/all"))
        .respond_with(ResponseTemplate::new(200).set_body_json(shows_body()))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/library/metadata/1000/children"))
        .respond_with(ResponseTemplate::new(200).set_body_json(seasons_body()))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/library/metadata/1100/children"))
        .respond_with(ResponseTemplate::new(200).set_body_json(episodes_body()))
        .expect(1)
        .mount(&server)
        .await;

    let sections = plex.library().sections().await.unwrap();
    let tv = sections
        .iter()
        .find(|s| s.kind == SectionKind::Show)
        .unwrap();
    let shows = tv.shows().await.unwrap();
    let seasons = shows[0].seasons().await.unwrap();
    let s1 = &seasons[0];
    let episodes = s1.episodes().await.unwrap();
    assert_eq!(episodes.len(), 2);

    let ep1 = &episodes[0];
    assert_eq!(ep1.rating_key.get(), 1101);
    assert_eq!(ep1.title, "Dulcinea");
    assert_eq!(ep1.index, Some(1));
    assert_eq!(ep1.parent_index, Some(1));
    assert_eq!(ep1.parent_rating_key.get(), 1100);
    assert_eq!(ep1.grandparent_rating_key.get(), 1000);
    assert_eq!(ep1.grandparent_title.as_deref(), Some("The Expanse"));
    assert_eq!(ep1.duration_ms, Some(2_700_000));
    assert!(ep1.is_played());
    assert_eq!(ep1.season_episode_label().as_deref(), Some("S01E01"));

    let ep2 = &episodes[1];
    assert!(!ep2.is_played());
    assert_eq!(ep2.season_episode_label().as_deref(), Some("S01E02"));
}

#[tokio::test]
async fn shows_rejects_non_show_section() {
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
                "Directory": [{"key": "1", "type": "movie", "title": "Movies"}]
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
    let err = sections[0].shows().await.unwrap_err();
    assert!(matches!(err, plex_rs::Error::Config(_)));
}
