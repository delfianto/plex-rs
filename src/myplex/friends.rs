//! Friends — accounts the signed-in user has shared a server with.
//!
//! Plex's "friends" endpoint at `https://plex.tv/api/users/`
//! returns every non-signed-in account that the current user has
//! granted access to one or more of their PMS instances. python's
//! `MyPlexAccount.users()` is the equivalent.
//!
//! The endpoint is XML-only. We parse with `quick-xml`'s serde
//! adapter — same approach as [`crate::MyPlexDevice`].

use std::fmt;

use quick_xml::de::from_str;
use serde::Deserialize;

use crate::error::{Error, Result};
use crate::myplex::MyPlexClient;
use crate::util::ids::PlexToken;

// -----------------------------------------------------------------------------
// MyPlexUser.
// -----------------------------------------------------------------------------

/// A friend / shared-with account.
#[derive(Clone)]
#[non_exhaustive]
// Plex emits 5 independent boolean flags per user; bundling them
// is the natural wire mapping. Bitflag abstractions are
// unwarranted at this size.
#[allow(clippy::struct_excessive_bools)]
pub struct MyPlexUser {
    /// plex.tv numeric id.
    pub id: u64,
    /// Username (display handle).
    pub username: String,
    /// Display title — often the same as `username`.
    pub title: String,
    /// Account email when visible to the signed-in user.
    pub email: Option<String>,
    /// Avatar URL.
    pub thumb: Option<String>,
    /// `true` when this account is also a Plex Home user.
    pub home: bool,
    /// `true` when restricted (Plex Home managed user).
    pub restricted: bool,
    /// `true` when the account can sync content offline.
    pub allow_sync: bool,
    /// `true` when the account has channels access.
    pub allow_channels: bool,
    /// `true` when the account can upload images.
    pub allow_camera_upload: bool,
    /// Per-share access token (when the signed-in user has shared
    /// a server with this account — Plex mints a per-friend token
    /// that's distinct from the account token).
    pub access_token: Option<PlexToken>,
}

impl fmt::Debug for MyPlexUser {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MyPlexUser")
            .field("id", &self.id)
            .field("username", &self.username)
            .field("title", &self.title)
            .field("email", &self.email)
            .field("home", &self.home)
            .field("restricted", &self.restricted)
            .field("access_token", &self.access_token)
            .finish_non_exhaustive()
    }
}

impl MyPlexClient {
    /// List every friend / shared-with account.
    ///
    /// Wire: `GET https://plex.tv/api/users/`.
    ///
    /// # Errors
    /// Any [`Error`] variant.
    pub async fn friends(&self) -> Result<Vec<MyPlexUser>> {
        let url = format!("{}/api/users/", self.base());
        let bytes = self.http().get_bytes(&url).await?;
        let body = std::str::from_utf8(&bytes)
            .map_err(|e| Error::Config(format!("friends body not utf-8: {e}")))?;
        parse_friends(body)
    }

    /// Remove a friend by their plex.tv id.
    ///
    /// Wire: `DELETE https://plex.tv/api/friends/<id>`.
    ///
    /// # Errors
    /// Any [`Error`] variant. [`Error::NotFound`] indicates the id
    /// isn't an active friend (already removed, or never was).
    pub async fn remove_friend(&self, user_id: u64) -> Result<()> {
        let url = format!("{}/api/friends/{}", self.base(), user_id);
        self.http().delete(&url).await
    }
}

// -----------------------------------------------------------------------------
// XML parsing.
// -----------------------------------------------------------------------------

fn parse_friends(body: &str) -> Result<Vec<MyPlexUser>> {
    let mc: UsersContainer = from_str(body)?;
    let mut out = Vec::with_capacity(mc.users.len());
    for dto in mc.users {
        out.push(dto.into_domain()?);
    }
    Ok(out)
}

#[derive(Debug, Deserialize)]
#[serde(rename = "MediaContainer")]
struct UsersContainer {
    #[serde(rename = "User", default)]
    users: Vec<UserDto>,
}

#[derive(Debug, Deserialize)]
struct UserDto {
    #[serde(rename = "@id")]
    id: u64,
    #[serde(rename = "@username", default)]
    username: String,
    #[serde(rename = "@title", default)]
    title: String,
    #[serde(rename = "@email", default)]
    email: Option<String>,
    #[serde(rename = "@thumb", default)]
    thumb: Option<String>,
    #[serde(rename = "@home", default)]
    home: u8,
    #[serde(rename = "@restricted", default)]
    restricted: u8,
    #[serde(rename = "@allowSync", default)]
    allow_sync: u8,
    #[serde(rename = "@allowChannels", default)]
    allow_channels: u8,
    #[serde(rename = "@allowCameraUpload", default)]
    allow_camera_upload: u8,
    #[serde(rename = "@accessToken", default)]
    access_token: Option<String>,
}

impl UserDto {
    fn into_domain(self) -> Result<MyPlexUser> {
        let access_token = self
            .access_token
            .filter(|s| !s.is_empty())
            .map(PlexToken::new)
            .transpose()?;
        Ok(MyPlexUser {
            id: self.id,
            username: self.username,
            title: self.title,
            email: self.email.filter(|s| !s.is_empty()),
            thumb: self.thumb.filter(|s| !s.is_empty()),
            home: self.home == 1,
            restricted: self.restricted == 1,
            allow_sync: self.allow_sync == 1,
            allow_channels: self.allow_channels == 1,
            allow_camera_upload: self.allow_camera_upload == 1,
            access_token,
        })
    }
}

// -----------------------------------------------------------------------------
// Tests.
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_XML: &str = r#"<?xml version="1.0"?>
<MediaContainer friendlyName="myaccount" identifier="com.plexapp.system" machineIdentifier="abc">
  <User id="1001"
        title="alice"
        username="alice"
        email="alice@example.com"
        thumb="https://plex.tv/users/alice"
        home="0"
        restricted="0"
        allowSync="1"
        allowChannels="1"
        allowCameraUpload="0"
        accessToken="share-token-alice"/>
  <User id="1002"
        title="bob"
        username="bob"
        email=""
        home="1"
        restricted="1"
        allowSync="0"
        allowChannels="0"
        allowCameraUpload="0"
        accessToken=""/>
</MediaContainer>"#;

    #[test]
    fn parses_two_users() {
        let v = parse_friends(SAMPLE_XML).unwrap();
        assert_eq!(v.len(), 2);
    }

    #[test]
    fn first_user_is_a_regular_friend_with_token() {
        let v = parse_friends(SAMPLE_XML).unwrap();
        let alice = v.iter().find(|u| u.id == 1001).unwrap();
        assert_eq!(alice.username, "alice");
        assert_eq!(alice.email.as_deref(), Some("alice@example.com"));
        assert!(!alice.home);
        assert!(!alice.restricted);
        assert!(alice.allow_sync);
        assert!(alice.access_token.is_some());
    }

    #[test]
    fn second_user_is_a_home_restricted_with_no_token() {
        let v = parse_friends(SAMPLE_XML).unwrap();
        let bob = v.iter().find(|u| u.id == 1002).unwrap();
        assert!(bob.home);
        assert!(bob.restricted);
        assert!(bob.email.is_none());
        assert!(bob.access_token.is_none());
    }

    #[test]
    fn debug_redacts_access_token() {
        let v = parse_friends(SAMPLE_XML).unwrap();
        let dbg = format!("{:?}", v[0]);
        assert!(!dbg.contains("share-token-alice"), "leaked: {dbg}");
        assert!(dbg.contains("***redacted***"));
    }

    #[test]
    fn empty_container_yields_empty_vec() {
        let xml = r"<MediaContainer/>";
        assert!(parse_friends(xml).unwrap().is_empty());
    }

    #[test]
    fn malformed_xml_errors() {
        assert!(parse_friends("not xml").is_err());
    }
}
