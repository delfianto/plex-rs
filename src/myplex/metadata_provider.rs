//! `metadata.provider.plex.tv` — cloud-catalogue per-user state.
//!
//! Where [`crate::PlayedUnplayed`] marks items watched on a single
//! PMS, the metadata provider tracks watched-state on the global
//! Plex Discover catalogue. Calling [`MyPlexClient::scrobble`]
//! propagates "this item is watched" to every PMS that subscribes
//! to the cloud catalogue.
//!
//! ## Wire endpoints
//!
//! | Method | Path                                                       | Purpose                |
//! | ------ | ---------------------------------------------------------- | ---------------------- |
//! | GET    | `/library/metadata/<rk>/userState`                         | Read per-user state    |
//! | GET    | `/actions/scrobble?key=<rk>&identifier=<provider>`         | Mark watched           |
//! | GET    | `/actions/unscrobble?key=<rk>&identifier=<provider>`       | Mark unwatched         |
//!
//! All against the Metadata Provider base
//! (`https://metadata.provider.plex.tv` by default). Override via
//! [`MyPlexClient::with_metadata_base`] for tests.
//!
//! ## Wire bug parity
//!
//! Plex's `scrobble` / `unscrobble` endpoints are HTTP `GET`
//! despite being mutating operations. We preserve that — the
//! crate doesn't second-guess the wire format.

use chrono::{DateTime, Utc};
use serde::Deserialize;

use crate::error::{Error, Result};
use crate::myplex::MyPlexClient;

/// Identifier sent to scrobble/unscrobble alongside the rating
/// key. Plex's cloud catalogue uses the standard library
/// identifier.
const SCROBBLE_IDENTIFIER: &str = "com.plexapp.plugins.library";

// -----------------------------------------------------------------------------
// UserState.
// -----------------------------------------------------------------------------

/// Per-user state for one item on the Plex cloud catalogue.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct UserState {
    /// Rating key of the affected item.
    pub rating_key: String,
    /// Wire `type` (`movie`, `show`, `episode`, …) when present.
    pub kind: Option<String>,
    /// Number of times this item has been played.
    pub view_count: u32,
    /// For shows / seasons: number of episodes marked watched.
    pub viewed_leaf_count: u32,
    /// Last-known playback position in milliseconds.
    pub view_offset_ms: u64,
    /// `true` when Plex marked this item fully complete
    /// (`viewState == "complete"`).
    pub view_state_complete: bool,
    /// When the item was last played, when known.
    pub last_viewed_at: Option<DateTime<Utc>>,
    /// When the item was added to the watchlist, when present.
    /// `Some` ⇔ on watchlist.
    pub watchlisted_at: Option<DateTime<Utc>>,
}

impl UserState {
    /// Convenience: `true` when the item has been played at least
    /// once (`view_count > 0`).
    #[must_use]
    pub const fn is_played(&self) -> bool {
        self.view_count > 0
    }

    /// Convenience: `true` when the item is on the user's
    /// watchlist (`watchlisted_at` is `Some`).
    #[must_use]
    pub const fn is_on_watchlist(&self) -> bool {
        self.watchlisted_at.is_some()
    }
}

// -----------------------------------------------------------------------------
// MyPlexClient impl.
// -----------------------------------------------------------------------------

impl MyPlexClient {
    /// Fetch the cloud user state for one item.
    ///
    /// `rating_key` is the hex segment of the item's GUID
    /// (e.g. `5d776b59ad5437001f796d8b`) — same value used by
    /// [`add_to_watchlist`](Self::add_to_watchlist).
    ///
    /// # Errors
    /// Any transport / parse [`Error`] variant.
    pub async fn user_state(&self, rating_key: &str) -> Result<UserState> {
        let url = format!(
            "{}/library/metadata/{}/userState",
            self.metadata_base(),
            encode_rating_key(rating_key),
        );
        let bytes = self.http().get_bytes(&url).await?;
        let body = std::str::from_utf8(&bytes)
            .map_err(|e| Error::Config(format!("userState body not utf-8: {e}")))?;
        parse_user_state(body)
    }

    /// Mark `rating_key` as watched on the cloud catalogue.
    ///
    /// Despite being a mutation, the wire endpoint is a plain
    /// `GET` — we preserve that quirk.
    ///
    /// # Errors
    /// Any transport [`Error`] variant.
    pub async fn scrobble(&self, rating_key: &str) -> Result<()> {
        let url = format!(
            "{}/actions/scrobble?key={}&identifier={}",
            self.metadata_base(),
            encode_rating_key(rating_key),
            SCROBBLE_IDENTIFIER,
        );
        let _ = self.http().get_bytes(&url).await?;
        Ok(())
    }

    /// Mark `rating_key` as unwatched on the cloud catalogue.
    ///
    /// # Errors
    /// Any transport [`Error`] variant.
    pub async fn unscrobble(&self, rating_key: &str) -> Result<()> {
        let url = format!(
            "{}/actions/unscrobble?key={}&identifier={}",
            self.metadata_base(),
            encode_rating_key(rating_key),
            SCROBBLE_IDENTIFIER,
        );
        let _ = self.http().get_bytes(&url).await?;
        Ok(())
    }
}

// -----------------------------------------------------------------------------
// Helpers.
// -----------------------------------------------------------------------------

const HEX: &[u8; 16] = b"0123456789ABCDEF";

/// Plex rating keys are `[0-9a-f]+` hex. Defensive escape for
/// anything else.
fn encode_rating_key(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for byte in s.bytes() {
        if matches!(byte, b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~') {
            out.push(byte as char);
        } else {
            out.push('%');
            out.push(HEX[(byte >> 4) as usize] as char);
            out.push(HEX[(byte & 0xF) as usize] as char);
        }
    }
    out
}

fn parse_user_state(body: &str) -> Result<UserState> {
    let env: UserStateEnvelope = serde_json::from_str(body)?;
    let dto = env
        .container
        .user_state
        .into_iter()
        .next()
        .ok_or_else(|| Error::Config("userState body had no UserState entry".to_owned()))?;
    Ok(dto.into_domain())
}

/// Parse a timestamp that may be epoch seconds (string or number)
/// or ISO 8601.
fn parse_timestamp(raw: Option<&serde_json::Value>) -> Option<DateTime<Utc>> {
    let v = raw?;
    if let Some(n) = v.as_i64() {
        return DateTime::<Utc>::from_timestamp(n, 0);
    }
    if let Some(s) = v.as_str() {
        if let Ok(n) = s.parse::<i64>() {
            return DateTime::<Utc>::from_timestamp(n, 0);
        }
        return DateTime::parse_from_rfc3339(s)
            .ok()
            .map(|d| d.with_timezone(&Utc));
    }
    None
}

// -----------------------------------------------------------------------------
// DTOs.
// -----------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct UserStateEnvelope {
    #[serde(rename = "MediaContainer")]
    container: UserStateContainer,
}

#[derive(Debug, Deserialize, Default)]
struct UserStateContainer {
    #[serde(rename = "UserState", default)]
    user_state: Vec<UserStateDto>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UserStateDto {
    #[serde(default)]
    rating_key: String,
    #[serde(default, rename = "type")]
    kind: Option<String>,
    #[serde(default)]
    view_count: u32,
    #[serde(default)]
    viewed_leaf_count: u32,
    #[serde(default)]
    view_offset: u64,
    #[serde(default)]
    view_state: Option<String>,
    #[serde(default)]
    last_viewed_at: Option<serde_json::Value>,
    #[serde(default)]
    watchlisted_at: Option<serde_json::Value>,
}

impl UserStateDto {
    fn into_domain(self) -> UserState {
        UserState {
            rating_key: self.rating_key,
            kind: self.kind,
            view_count: self.view_count,
            viewed_leaf_count: self.viewed_leaf_count,
            view_offset_ms: self.view_offset,
            view_state_complete: self.view_state.as_deref() == Some("complete"),
            last_viewed_at: parse_timestamp(self.last_viewed_at.as_ref()),
            watchlisted_at: parse_timestamp(self.watchlisted_at.as_ref()),
        }
    }
}

// -----------------------------------------------------------------------------
// Tests.
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_user_state_with_all_fields() {
        let body = serde_json::json!({
            "MediaContainer": {
                "size": 1,
                "UserState": [{
                    "ratingKey": "abc123",
                    "type": "movie",
                    "viewCount": 3,
                    "viewedLeafCount": 0,
                    "viewOffset": 0,
                    "viewState": "complete",
                    "lastViewedAt": 1_700_000_000,
                    "watchlistedAt": 1_690_000_000
                }]
            }
        });
        let s = parse_user_state(&body.to_string()).unwrap();
        assert_eq!(s.rating_key, "abc123");
        assert_eq!(s.kind.as_deref(), Some("movie"));
        assert_eq!(s.view_count, 3);
        assert!(s.view_state_complete);
        assert!(s.is_played());
        assert!(s.is_on_watchlist());
        assert_eq!(s.last_viewed_at.unwrap().timestamp(), 1_700_000_000);
        assert_eq!(s.watchlisted_at.unwrap().timestamp(), 1_690_000_000);
    }

    #[test]
    fn parses_user_state_with_missing_optional_fields() {
        let body = serde_json::json!({
            "MediaContainer": {
                "UserState": [{
                    "ratingKey": "xyz"
                }]
            }
        });
        let s = parse_user_state(&body.to_string()).unwrap();
        assert_eq!(s.rating_key, "xyz");
        assert!(s.kind.is_none());
        assert_eq!(s.view_count, 0);
        assert!(!s.view_state_complete);
        assert!(!s.is_played());
        assert!(!s.is_on_watchlist());
        assert!(s.last_viewed_at.is_none());
        assert!(s.watchlisted_at.is_none());
    }

    #[test]
    fn parse_user_state_rejects_empty_container() {
        let body = serde_json::json!({"MediaContainer": {"UserState": []}});
        let err = parse_user_state(&body.to_string()).unwrap_err();
        assert!(matches!(err, Error::Config(ref msg) if msg.contains("UserState")));
    }

    #[test]
    fn parse_timestamp_handles_epoch_number() {
        let v = serde_json::json!(1_700_000_000);
        let t = parse_timestamp(Some(&v)).unwrap();
        assert_eq!(t.timestamp(), 1_700_000_000);
    }

    #[test]
    fn parse_timestamp_handles_epoch_string() {
        let v = serde_json::json!("1700000000");
        let t = parse_timestamp(Some(&v)).unwrap();
        assert_eq!(t.timestamp(), 1_700_000_000);
    }

    #[test]
    fn parse_timestamp_handles_iso8601() {
        let v = serde_json::json!("2023-11-14T22:13:20+00:00");
        let t = parse_timestamp(Some(&v)).unwrap();
        assert_eq!(t.timestamp(), 1_700_000_000);
    }

    #[test]
    fn encode_rating_key_passes_hex_unchanged() {
        assert_eq!(encode_rating_key("abc123def"), "abc123def");
    }

    #[test]
    fn encode_rating_key_escapes_special() {
        assert_eq!(encode_rating_key("a/b"), "a%2Fb");
    }

    #[test]
    fn is_played_threshold_is_one_view() {
        let s = UserState {
            rating_key: "x".into(),
            kind: None,
            view_count: 0,
            viewed_leaf_count: 0,
            view_offset_ms: 0,
            view_state_complete: false,
            last_viewed_at: None,
            watchlisted_at: None,
        };
        assert!(!s.is_played());
        let s2 = UserState { view_count: 1, ..s };
        assert!(s2.is_played());
    }
}
