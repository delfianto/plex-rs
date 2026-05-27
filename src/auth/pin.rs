//! [`MyPlexPinLogin`] — PIN / OAuth sign-in against plex.tv.
//!
//! Flow:
//!
//! 1. [`MyPlexPinLogin::start`] calls `POST https://plex.tv/api/v2/pins`
//!    to create a fresh PIN. plex.tv returns `{ id, code,
//!    authToken: null, expiresAt, … }`. The code is a four-character
//!    string the user types into `https://plex.tv/link`.
//! 2. The library polls `GET https://plex.tv/api/v2/pins/<id>`. As
//!    long as the user hasn't claimed the PIN, `authToken` stays
//!    null. Once they enter the code, plex.tv attaches the token.
//! 3. [`MyPlexPinLogin::wait`] handles the polling loop; callers
//!    that need finer-grained control can call
//!    [`MyPlexPinLogin::poll`] directly.
//!
//! All requests carry the standard `X-Plex-*` identity headers (in
//! particular `X-Plex-Client-Identifier`, which **must match
//! exactly** between the create and the poll — the synthesis lists
//! this as a common pitfall).

use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::Deserialize;

use crate::client::HttpClient;
use crate::config::ClientConfig;
use crate::error::{Error, Result};
use crate::headers::PlexIdentity;
use crate::util::ids::{ClientIdentifier, PlexToken};

/// plex.tv API endpoint for the PIN flow.
const PINS_URL: &str = "https://plex.tv/api/v2/pins";

// -----------------------------------------------------------------------------
// MyPlexPinLogin.
// -----------------------------------------------------------------------------

/// An in-progress PIN-based plex.tv sign-in.
///
/// Construct via [`MyPlexPinLogin::start`] — that call hits plex.tv
/// and returns a [`MyPlexPinLogin`] holding the freshly-issued
/// PIN. The user enters [`Self::code`] at `https://plex.tv/link`, and
/// the library polls plex.tv until the PIN is claimed (or expires).
#[derive(Debug)]
pub struct MyPlexPinLogin {
    http: HttpClient,
    /// PIN id assigned by plex.tv. Used to construct the poll URL.
    id: u64,
    /// 4-character PIN code shown to the user.
    code: String,
    /// Wall-clock expiry of the PIN.
    expires_at: DateTime<Utc>,
}

impl MyPlexPinLogin {
    /// Start a new PIN sign-in.
    ///
    /// `client_identifier` should be the same stable value the caller
    /// will use elsewhere — Plex deduplicates devices and sessions
    /// by it, and reuses a token across processes that share an id.
    ///
    /// Optional `identity` lets the caller customise the `X-Plex-*`
    /// product / device / platform headers; when `None`, the default
    /// `plex-rs` identity is used.
    ///
    /// # Errors
    /// Any transport [`Error`] variant. [`Error::Json`] when
    /// plex.tv's response can't be deserialised (e.g. plex.tv is
    /// returning HTML for the endpoint, indicating an outage).
    pub async fn start(
        client_identifier: ClientIdentifier,
        identity: Option<PlexIdentity>,
    ) -> Result<Self> {
        let mut cfg_builder = ClientConfig::builder(client_identifier);
        if let Some(id) = identity {
            cfg_builder = cfg_builder.identity(id);
        }
        let http = HttpClient::new(cfg_builder.build()?)?;
        let strong_url = format!("{PINS_URL}?strong=true");
        let dto: PinDto = http.post_json::<(), _>(&strong_url, &()).await?;
        Self::from_dto(http, dto)
    }

    /// Like [`Self::start`] but uses a caller-supplied
    /// [`HttpClient`] — useful when the application already has one
    /// configured with custom timeouts or retry behaviour. The
    /// client's identity headers must already be set up.
    ///
    /// # Errors
    /// See [`Self::start`].
    pub async fn start_with_client(http: HttpClient) -> Result<Self> {
        let strong_url = format!("{PINS_URL}?strong=true");
        let dto: PinDto = http.post_json::<(), _>(&strong_url, &()).await?;
        Self::from_dto(http, dto)
    }

    fn from_dto(http: HttpClient, dto: PinDto) -> Result<Self> {
        let expires_at = dto
            .expires_at
            .parse::<DateTime<Utc>>()
            .map_err(|e| Error::Config(format!("pin expiresAt unparseable: {e}")))?;
        Ok(Self {
            http,
            id: dto.id,
            code: dto.code,
            expires_at,
        })
    }

    /// The 4-character PIN code. Show this to the user along with
    /// instructions to visit `https://plex.tv/link`.
    #[must_use]
    pub fn code(&self) -> &str {
        &self.code
    }

    /// The PIN id assigned by plex.tv (useful for diagnostics /
    /// logging — do not display to users).
    #[must_use]
    pub const fn id(&self) -> u64 {
        self.id
    }

    /// When the PIN expires. After this point [`Self::poll`] will
    /// return [`Error::Auth`] indicating the user must restart the
    /// flow.
    #[must_use]
    pub const fn expires_at(&self) -> DateTime<Utc> {
        self.expires_at
    }

    /// Check whether the PIN has been claimed yet.
    ///
    /// Returns `Ok(Some(token))` when the user has entered the code
    /// at plex.tv/link and plex.tv has minted an auth token.
    /// Returns `Ok(None)` while the PIN is still pending.
    /// Returns `Err(_)` on transport / parse failures, or
    /// [`Error::Auth`] when the PIN has expired.
    ///
    /// # Errors
    /// Any [`Error`] variant.
    pub async fn poll(&self) -> Result<Option<PlexToken>> {
        let url = format!("{PINS_URL}/{}", self.id);
        let dto: PinDto = self.http.get_json(&url).await?;
        if let Some(t) = dto.auth_token {
            // Edge case: plex.tv occasionally returns an empty
            // string instead of null. Treat as "not yet".
            if t.is_empty() {
                return Ok(None);
            }
            return Ok(Some(PlexToken::new(t)?));
        }
        // Check expiry — plex.tv keeps returning the same payload
        // forever; we time it out client-side.
        if Utc::now() > self.expires_at {
            return Err(Error::Auth(
                "PIN expired before being claimed by the user".to_owned(),
            ));
        }
        Ok(None)
    }

    /// Poll plex.tv repeatedly until the PIN is claimed, the PIN
    /// expires, or `timeout` elapses.
    ///
    /// `interval` is how often to poll plex.tv. plex.tv rate-limits
    /// PIN polls — staying at or above 1 second is recommended.
    ///
    /// # Errors
    /// - [`Error::Auth`] when the PIN expires or `timeout` elapses
    ///   without a claim.
    /// - Any transport [`Error`].
    pub async fn wait(&self, timeout: Duration, interval: Duration) -> Result<PlexToken> {
        let start = std::time::Instant::now();
        loop {
            if let Some(token) = self.poll().await? {
                return Ok(token);
            }
            if start.elapsed() >= timeout {
                return Err(Error::Auth(format!(
                    "PIN sign-in timed out after {timeout:?}"
                )));
            }
            tokio::time::sleep(interval).await;
        }
    }
}

// -----------------------------------------------------------------------------
// DTO.
// -----------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PinDto {
    id: u64,
    code: String,
    expires_at: String,
    #[serde(default)]
    auth_token: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pin_dto_parses_create_response() {
        // Plex's create response: authToken is null until claimed.
        let body = serde_json::json!({
            "id": 12345,
            "code": "ABCD",
            "expiresAt": "2026-12-31T23:59:59Z",
            "authToken": null
        });
        let dto: PinDto = serde_json::from_value(body).unwrap();
        assert_eq!(dto.id, 12345);
        assert_eq!(dto.code, "ABCD");
        assert!(dto.auth_token.is_none());
    }

    #[test]
    fn pin_dto_parses_claimed_response() {
        let body = serde_json::json!({
            "id": 12345,
            "code": "ABCD",
            "expiresAt": "2026-12-31T23:59:59Z",
            "authToken": "the-minted-token"
        });
        let dto: PinDto = serde_json::from_value(body).unwrap();
        assert_eq!(dto.auth_token.as_deref(), Some("the-minted-token"));
    }
}
