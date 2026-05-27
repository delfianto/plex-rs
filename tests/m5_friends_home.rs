//! M5.4 — friends + home integration tests.

use plex_rs::{ClientIdentifier, MyPlexClient, PlexToken};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn client_pointing_at(uri: &str) -> MyPlexClient {
    let cid = ClientIdentifier::new("friends-home-test").unwrap();
    MyPlexClient::new(PlexToken::new("acct-token").unwrap(), cid, None)
        .unwrap()
        .with_base(uri)
}

const FRIENDS_XML: &str = r#"<?xml version="1.0"?>
<MediaContainer>
  <User id="1001" title="alice" username="alice" email="alice@e.com"
        home="0" restricted="0" allowSync="1" allowChannels="1" allowCameraUpload="0"
        accessToken="share-tok-alice"/>
  <User id="1002" title="bob" username="bob" email=""
        home="1" restricted="1" allowSync="0" allowChannels="0" allowCameraUpload="0"
        accessToken=""/>
</MediaContainer>"#;

const HOME_XML: &str = r#"<?xml version="1.0"?>
<MediaContainer>
  <User id="2001" title="Owner" username="owner" email="owner@e.com"
        admin="1" protected="0" restricted="0" guest="0"/>
  <User id="2002" title="Kid" username="" email=""
        admin="0" protected="1" restricted="1" guest="0"/>
</MediaContainer>"#;

#[tokio::test]
async fn friends_lists_shared_accounts() {
    let plex_tv = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/users/"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("Content-Type", "application/xml")
                .set_body_string(FRIENDS_XML),
        )
        .expect(1)
        .mount(&plex_tv)
        .await;
    let friends = client_pointing_at(&plex_tv.uri()).friends().await.unwrap();
    assert_eq!(friends.len(), 2);
    assert_eq!(friends[0].id, 1001);
    assert!(friends[0].access_token.is_some());
    assert_eq!(friends[1].id, 1002);
    assert!(friends[1].restricted);
}

#[tokio::test]
async fn remove_friend_deletes_at_id_path() {
    let plex_tv = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/api/friends/1001"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&plex_tv)
        .await;
    client_pointing_at(&plex_tv.uri())
        .remove_friend(1001)
        .await
        .unwrap();
}

#[tokio::test]
async fn home_users_lists_subaccounts() {
    let plex_tv = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/home/users"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("Content-Type", "application/xml")
                .set_body_string(HOME_XML),
        )
        .expect(1)
        .mount(&plex_tv)
        .await;
    let users = client_pointing_at(&plex_tv.uri())
        .home_users()
        .await
        .unwrap();
    assert_eq!(users.len(), 2);
    let owner = users.iter().find(|u| u.id == 2001).unwrap();
    assert!(owner.admin);
    let kid = users.iter().find(|u| u.id == 2002).unwrap();
    assert!(kid.restricted);
    assert!(kid.protected);
    assert!(kid.email.is_none());
}
