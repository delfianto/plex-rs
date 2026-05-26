//! M1 integration tests — `PlexServer::connect` → identity → library
//! sections, exercised against a `wiremock` mock PMS.
//!
//! Each test spins up a mock PMS that responds to the exact endpoints
//! the M1 surface uses (`GET /`, `GET /identity`, `GET /library/sections`),
//! verifies the request shape, and asserts on the parsed domain
//! objects.

use plex_rs::{PlexServer, PlexToken, SectionKind};
use url::Url;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn root_body() -> serde_json::Value {
    serde_json::json!({
        "MediaContainer": {
            "size": 0,
            "machineIdentifier": "abc123machine",
            "version": "1.40.0.1234-deadbeef",
            "friendlyName": "Living Room",
            "platform": "Linux",
            "platformVersion": "Ubuntu 24.04",
            "myPlex": "1",
            "myPlexUsername": "user@example.com",
            "myPlexSubscription": 1,
            "allowMediaDeletion": "0",
            "allowSharing": true,
            "livetv": "7",
            "updatedAt": 1700000000
        }
    })
}

fn sections_body() -> serde_json::Value {
    serde_json::json!({
        "MediaContainer": {
            "size": 4,
            "allowSync": false,
            "title1": "Plex Library",
            "Directory": [
                {
                    "key": "1",
                    "type": "movie",
                    "title": "Movies",
                    "uuid": "movies-uuid",
                    "agent": "com.plexapp.agents.imdb",
                    "scanner": "Plex Movie Scanner",
                    "language": "en",
                    "createdAt": 1700000000,
                    "updatedAt": 1700000100,
                    "allowSync": "1",
                    "refreshing": false
                },
                {
                    "key": "2",
                    "type": "show",
                    "title": "TV Shows",
                    "uuid": "shows-uuid"
                },
                {
                    "key": "3",
                    "type": "artist",
                    "title": "Music",
                    "uuid": "music-uuid"
                },
                {
                    "key": "4",
                    "type": "podcast",
                    "title": "Podcasts",
                    "uuid": "pod-uuid"
                }
            ]
        }
    })
}

#[tokio::test]
async fn connect_parses_root_identity() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/"))
        .and(header("x-plex-token", "test-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(root_body()))
        .expect(1)
        .mount(&server)
        .await;

    let url = Url::parse(&server.uri()).unwrap();
    let token = PlexToken::new("test-token").unwrap();
    let plex = PlexServer::connect(url, token).await.unwrap();

    let id = plex.identity();
    assert_eq!(id.machine_identifier.as_str(), "abc123machine");
    assert_eq!(id.version, "1.40.0.1234-deadbeef");
    assert_eq!(id.friendly_name.as_deref(), Some("Living Room"));
    assert_eq!(id.platform.as_deref(), Some("Linux"));
    assert!(id.my_plex);
    assert!(id.my_plex_subscription);
    assert!(!id.allow_media_deletion);
    assert!(id.allow_sharing);
    assert_eq!(id.my_plex_username.as_deref(), Some("user@example.com"));
}

#[tokio::test]
async fn connect_propagates_401_as_unauthorized() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(401))
        .expect(1)
        .mount(&server)
        .await;

    let url = Url::parse(&server.uri()).unwrap();
    let token = PlexToken::new("bad-token").unwrap();
    let err = PlexServer::connect(url, token).await.unwrap_err();
    assert!(matches!(err, plex_rs::Error::Unauthorized));
}

#[tokio::test]
async fn library_sections_returns_typed_sections() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(root_body()))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/library/sections"))
        .and(header("x-plex-token", "test-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(sections_body()))
        .expect(1)
        .mount(&server)
        .await;

    let url = Url::parse(&server.uri()).unwrap();
    let token = PlexToken::new("test-token").unwrap();
    let plex = PlexServer::connect(url, token).await.unwrap();
    let sections = plex.library().sections().await.unwrap();

    assert_eq!(sections.len(), 4);

    assert_eq!(sections[0].id(), 1);
    assert_eq!(sections[0].kind, SectionKind::Movie);
    assert_eq!(sections[0].title, "Movies");
    assert_eq!(sections[0].uuid, "movies-uuid");
    assert_eq!(
        sections[0].agent.as_deref(),
        Some("com.plexapp.agents.imdb")
    );
    assert!(sections[0].allow_sync);

    assert_eq!(sections[1].kind, SectionKind::Show);
    assert_eq!(sections[2].kind, SectionKind::Music);
    // Unknown wire type lands in Other(...) — forward compatibility.
    assert_eq!(sections[3].kind, SectionKind::Other("podcast".to_owned()));
}

#[tokio::test]
async fn library_section_ref_carries_back_link_to_server() {
    // Ensures that LibrarySectionRef::url() builds the section-level
    // mutation URL that the M3 edit traits will need.
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
    let token = PlexToken::new("test-token").unwrap();
    let plex = PlexServer::connect(url, token).await.unwrap();
    let sections = plex.library().sections().await.unwrap();
    let movies = &sections[0];
    let edit_url = movies
        .section_ref
        .url("/all?id=12345&title.value=Foo&title.locked=1")
        .unwrap();
    assert_eq!(edit_url.path(), "/library/sections/1/all");
    assert!(edit_url.query().unwrap().contains("id=12345"));
    assert!(edit_url.query().unwrap().contains("title.value=Foo"));
    assert!(edit_url.query().unwrap().contains("title.locked=1"));
}

#[tokio::test]
async fn ping_hits_identity_endpoint() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(root_body()))
        .mount(&server)
        .await;
    // /identity returns a minimal MediaContainer with size only.
    Mock::given(method("GET"))
        .and(path("/identity"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "MediaContainer": {"size": 0, "title": "Identity"}
        })))
        .expect(1)
        .mount(&server)
        .await;

    let url = Url::parse(&server.uri()).unwrap();
    let token = PlexToken::new("test-token").unwrap();
    let plex = PlexServer::connect(url, token).await.unwrap();
    let meta = plex.ping().await.unwrap();
    assert_eq!(meta.size, 0);
    assert_eq!(meta.title.as_deref(), Some("Identity"));
}
