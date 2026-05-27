//! M5.4 — devices list + delete integration tests.

use plex_rs::{ClientIdentifier, MyPlexClient, PlexToken};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn client_pointing_at(plex_tv_mock_uri: &str) -> MyPlexClient {
    let cid = ClientIdentifier::new("devices-test").unwrap();
    MyPlexClient::new(PlexToken::new("acct-token").unwrap(), cid, None)
        .unwrap()
        .with_base(plex_tv_mock_uri)
}

const SAMPLE_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<MediaContainer publicAddress="1.2.3.4" size="2">
  <Device name="Living Room PMS"
          product="Plex Media Server"
          productVersion="1.40.2.0"
          platform="Linux"
          device="PC"
          provides="server"
          clientIdentifier="abcdef0123456789abcdef0123456789abcd0001"
          id="1001"
          token="token-for-server">
    <Connection uri="https://10-0-0-5.x.plex.direct:32400"/>
  </Device>
  <Device name="Phone"
          product="Plex for iOS"
          platform="iOS"
          device="iPhone"
          provides="client,player"
          clientIdentifier="abcdef0123456789abcdef0123456789abcd0002"
          id="1002"
          token="token-for-phone"/>
</MediaContainer>"#;

#[tokio::test]
async fn devices_lists_registered_entries() {
    let plex_tv = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/devices.xml"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("Content-Type", "application/xml")
                .set_body_string(SAMPLE_XML),
        )
        .expect(1)
        .mount(&plex_tv)
        .await;

    let client = client_pointing_at(&plex_tv.uri());
    let devices = client.devices().await.unwrap();
    assert_eq!(devices.len(), 2);
    assert!(devices.iter().any(|d| d.id == "1001" && d.is_server()));
    assert!(devices.iter().any(|d| d.id == "1002" && d.is_player()));
}

#[tokio::test]
async fn device_delete_hits_devices_id_endpoint() {
    let plex_tv = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/devices.xml"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("Content-Type", "application/xml")
                .set_body_string(SAMPLE_XML),
        )
        .mount(&plex_tv)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/devices/1002.xml"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&plex_tv)
        .await;

    let client = client_pointing_at(&plex_tv.uri());
    let devices = client.devices().await.unwrap();
    let phone = devices.iter().find(|d| d.id == "1002").unwrap();
    phone.delete(&client).await.unwrap();
}

#[tokio::test]
async fn devices_empty_when_container_empty() {
    let plex_tv = MockServer::start().await;
    let empty = r#"<?xml version="1.0"?><MediaContainer size="0"></MediaContainer>"#;
    Mock::given(method("GET"))
        .and(path("/devices.xml"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("Content-Type", "application/xml")
                .set_body_string(empty),
        )
        .expect(1)
        .mount(&plex_tv)
        .await;
    let devices = client_pointing_at(&plex_tv.uri()).devices().await.unwrap();
    assert!(devices.is_empty());
}
