//! Plex Home — sub-accounts on a family / household account.
//!
//! Plex Home lets a primary account create up to a configured
//! number of profile-style sub-accounts (PIN-protected, optional
//! parental controls). This module exposes the read surface for
//! enumerating those users.
//!
//! Mutation (`add_user`, `remove_user`, restrict / unrestrict)
//! is out of scope for this milestone — those workflows are
//! typically driven via Plex's web UI and require careful UX
//! around 2FA / PIN entry. The read path is sufficient for the
//! "show me my Home users" reporting case (which is the main
//! use case for local-LLM-agent integration).

use quick_xml::de::from_str;
use serde::Deserialize;

use crate::error::{Error, Result};
use crate::myplex::MyPlexClient;

// -----------------------------------------------------------------------------
// MyPlexHomeUser.
// -----------------------------------------------------------------------------

/// A Plex Home sub-account.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
// Plex emits 4 independent boolean flags per home user; bundling
// them is the natural wire mapping. Bitflag abstractions are
// unwarranted at this size.
#[allow(clippy::struct_excessive_bools)]
pub struct MyPlexHomeUser {
    /// plex.tv numeric id.
    pub id: u64,
    /// Display name (the title shown in the UI).
    pub title: String,
    /// Username (handle).
    pub username: Option<String>,
    /// Email when this is a non-managed (full plex.tv) account.
    pub email: Option<String>,
    /// Avatar URL.
    pub thumb: Option<String>,
    /// `true` for "managed" (restricted) Home users — typically
    /// child profiles without their own plex.tv account.
    pub restricted: bool,
    /// `true` when the profile has a PIN set for switching.
    pub protected: bool,
    /// `true` when this entry is the account owner / admin.
    pub admin: bool,
    /// `true` when the profile is a guest account.
    pub guest: bool,
}

// -----------------------------------------------------------------------------
// MyPlexClient impl.
// -----------------------------------------------------------------------------

impl MyPlexClient {
    /// List every Plex Home user on this account.
    ///
    /// Wire: `GET https://plex.tv/api/home/users`. Returns XML.
    ///
    /// # Errors
    /// Any [`Error`] variant.
    pub async fn home_users(&self) -> Result<Vec<MyPlexHomeUser>> {
        let url = format!("{}/api/home/users", self.base());
        let bytes = self.http().get_bytes(&url).await?;
        let body = std::str::from_utf8(&bytes)
            .map_err(|e| Error::Config(format!("home users body not utf-8: {e}")))?;
        parse_home_users(body)
    }
}

// -----------------------------------------------------------------------------
// XML parsing.
// -----------------------------------------------------------------------------

fn parse_home_users(body: &str) -> Result<Vec<MyPlexHomeUser>> {
    let mc: HomeContainer = from_str(body)?;
    Ok(mc.users.into_iter().map(HomeUserDto::into_domain).collect())
}

#[derive(Debug, Deserialize)]
#[serde(rename = "MediaContainer")]
struct HomeContainer {
    #[serde(rename = "User", default)]
    users: Vec<HomeUserDto>,
}

#[derive(Debug, Deserialize)]
struct HomeUserDto {
    #[serde(rename = "@id")]
    id: u64,
    #[serde(rename = "@title", default)]
    title: String,
    #[serde(rename = "@username", default)]
    username: Option<String>,
    #[serde(rename = "@email", default)]
    email: Option<String>,
    #[serde(rename = "@thumb", default)]
    thumb: Option<String>,
    #[serde(rename = "@restricted", default)]
    restricted: u8,
    #[serde(rename = "@protected", default)]
    protected: u8,
    #[serde(rename = "@admin", default)]
    admin: u8,
    #[serde(rename = "@guest", default)]
    guest: u8,
}

impl HomeUserDto {
    fn into_domain(self) -> MyPlexHomeUser {
        MyPlexHomeUser {
            id: self.id,
            title: self.title,
            username: self.username.filter(|s| !s.is_empty()),
            email: self.email.filter(|s| !s.is_empty()),
            thumb: self.thumb.filter(|s| !s.is_empty()),
            restricted: self.restricted == 1,
            protected: self.protected == 1,
            admin: self.admin == 1,
            guest: self.guest == 1,
        }
    }
}

// -----------------------------------------------------------------------------
// Tests.
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_XML: &str = r#"<?xml version="1.0"?>
<MediaContainer size="3">
  <User id="2001"
        title="Owner"
        username="owner"
        email="owner@example.com"
        admin="1"
        protected="0"
        restricted="0"
        guest="0"/>
  <User id="2002"
        title="Spouse"
        username="spouse"
        email="spouse@example.com"
        admin="0"
        protected="1"
        restricted="0"
        guest="0"/>
  <User id="2003"
        title="Kid"
        username=""
        email=""
        admin="0"
        protected="1"
        restricted="1"
        guest="0"/>
</MediaContainer>"#;

    #[test]
    fn parses_three_home_users() {
        let v = parse_home_users(SAMPLE_XML).unwrap();
        assert_eq!(v.len(), 3);
    }

    #[test]
    fn owner_account_has_admin_set() {
        let v = parse_home_users(SAMPLE_XML).unwrap();
        let owner = v.iter().find(|u| u.id == 2001).unwrap();
        assert!(owner.admin);
        assert!(!owner.protected);
        assert!(!owner.restricted);
        assert_eq!(owner.title, "Owner");
        assert_eq!(owner.email.as_deref(), Some("owner@example.com"));
    }

    #[test]
    fn protected_non_admin_account_recognised() {
        let v = parse_home_users(SAMPLE_XML).unwrap();
        let spouse = v.iter().find(|u| u.id == 2002).unwrap();
        assert!(!spouse.admin);
        assert!(spouse.protected);
        assert!(!spouse.restricted);
    }

    #[test]
    fn managed_restricted_account_has_no_email() {
        let v = parse_home_users(SAMPLE_XML).unwrap();
        let kid = v.iter().find(|u| u.id == 2003).unwrap();
        assert!(kid.restricted);
        assert!(kid.protected);
        assert!(!kid.admin);
        assert!(kid.email.is_none());
        assert!(kid.username.is_none());
    }

    #[test]
    fn empty_container_yields_empty_vec() {
        let xml = r"<MediaContainer/>";
        assert!(parse_home_users(xml).unwrap().is_empty());
    }

    #[test]
    fn malformed_xml_errors() {
        assert!(parse_home_users("not xml").is_err());
    }
}
