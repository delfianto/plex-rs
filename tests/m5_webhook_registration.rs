//! M5.4 — webhook registration integration tests.
//!
//! Drives a wiremock-backed plex.tv replica through the GET / POST
//! flow used by `MyPlexClient::webhooks` / `add_webhook` /
//! `delete_webhook` / `set_webhooks`.

use plex_rs::error::Error;
use plex_rs::{ClientIdentifier, MyPlexClient, PlexToken};
use wiremock::matchers::{body_string_contains, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn client_pointing_at(plex_tv_mock_uri: &str) -> MyPlexClient {
    let cid = ClientIdentifier::new("webhook-test").unwrap();
    MyPlexClient::new(PlexToken::new("acct-token").unwrap(), cid, None)
        .unwrap()
        .with_base(plex_tv_mock_uri)
}

fn list_body(urls: &[&str]) -> serde_json::Value {
    serde_json::json!({
        "webhooks": urls.iter().map(|u| serde_json::json!({"url": u})).collect::<Vec<_>>()
    })
}

#[tokio::test]
async fn webhooks_returns_registered_urls() {
    let plex_tv = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v2/user/webhooks"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(list_body(&["https://a.example/", "https://b.example/"])),
        )
        .expect(1)
        .mount(&plex_tv)
        .await;
    let urls = client_pointing_at(&plex_tv.uri()).webhooks().await.unwrap();
    assert_eq!(urls, vec!["https://a.example/", "https://b.example/"]);
}

#[tokio::test]
async fn add_webhook_posts_merged_list_then_reloads() {
    let plex_tv = MockServer::start().await;
    // 1st GET returns the existing single URL.
    // POST receives the merged list (a + b).
    // 2nd GET (after POST) returns the new list.
    Mock::given(method("GET"))
        .and(path("/api/v2/user/webhooks"))
        .respond_with(ResponseTemplate::new(200).set_body_json(list_body(&["https://a/"])))
        .up_to_n_times(1)
        .expect(1)
        .mount(&plex_tv)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/v2/user/webhooks"))
        .and(header("content-type", "application/x-www-form-urlencoded"))
        .and(body_string_contains("urls%5B%5D=https%3A%2F%2Fa%2F"))
        .and(body_string_contains("urls%5B%5D=https%3A%2F%2Fb%2F"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&plex_tv)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v2/user/webhooks"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(list_body(&["https://a/", "https://b/"])),
        )
        .expect(1)
        .mount(&plex_tv)
        .await;

    let urls = client_pointing_at(&plex_tv.uri())
        .add_webhook("https://b/")
        .await
        .unwrap();
    assert_eq!(urls, vec!["https://a/", "https://b/"]);
}

#[tokio::test]
async fn add_webhook_is_idempotent_when_url_already_present() {
    let plex_tv = MockServer::start().await;
    // GET only — no POST, since the URL is already there.
    Mock::given(method("GET"))
        .and(path("/api/v2/user/webhooks"))
        .respond_with(ResponseTemplate::new(200).set_body_json(list_body(&["https://a/"])))
        .expect(1)
        .mount(&plex_tv)
        .await;
    let urls = client_pointing_at(&plex_tv.uri())
        .add_webhook("https://a/")
        .await
        .unwrap();
    assert_eq!(urls, vec!["https://a/"]);
}

#[tokio::test]
async fn delete_webhook_posts_filtered_list() {
    let plex_tv = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v2/user/webhooks"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(list_body(&["https://a/", "https://b/"])),
        )
        .up_to_n_times(1)
        .expect(1)
        .mount(&plex_tv)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/v2/user/webhooks"))
        .and(body_string_contains("urls%5B%5D=https%3A%2F%2Fa%2F"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&plex_tv)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v2/user/webhooks"))
        .respond_with(ResponseTemplate::new(200).set_body_json(list_body(&["https://a/"])))
        .expect(1)
        .mount(&plex_tv)
        .await;

    let urls = client_pointing_at(&plex_tv.uri())
        .delete_webhook("https://b/")
        .await
        .unwrap();
    assert_eq!(urls, vec!["https://a/"]);
}

#[tokio::test]
async fn delete_webhook_returns_not_found_when_url_absent() {
    let plex_tv = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v2/user/webhooks"))
        .respond_with(ResponseTemplate::new(200).set_body_json(list_body(&["https://a/"])))
        .expect(1)
        .mount(&plex_tv)
        .await;
    // No POST mock — if the validation hits, no POST happens.
    let err = client_pointing_at(&plex_tv.uri())
        .delete_webhook("https://not-registered/")
        .await
        .unwrap_err();
    assert!(
        matches!(&err, Error::NotFound { resource } if resource.contains("not-registered")),
        "got {err:?}"
    );
}

#[tokio::test]
async fn set_webhooks_empty_posts_clear_marker() {
    let plex_tv = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v2/user/webhooks"))
        .and(body_string_contains("urls="))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&plex_tv)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v2/user/webhooks"))
        .respond_with(ResponseTemplate::new(200).set_body_json(list_body(&[])))
        .expect(1)
        .mount(&plex_tv)
        .await;
    let urls = client_pointing_at(&plex_tv.uri())
        .set_webhooks(&[])
        .await
        .unwrap();
    assert!(urls.is_empty());
}
