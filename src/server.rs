//! [`PlexServer`] — a connected Plex Media Server handle.
//!
//! Construct via [`PlexServer::connect`] given a base URL and an auth
//! [`PlexToken`]. The constructor eagerly performs `GET /` to validate
//! the connection and harvest the server's [`ServerIdentity`] (machine
//! identifier, version, friendly name, capabilities). All further
//! method calls reuse the cached [`HttpClient`] and resolved base URL.

pub mod admin;
pub mod history;
pub mod sessions;
pub mod settings;
pub use admin::{
    Activity, BandwidthOptions, BandwidthStat, ButlerTask, ResourceStat, UpdateRelease,
    UpdaterStatus,
};
pub use history::{HistoryEntry, HistoryQuery, HistoryStream};
pub use sessions::{PlayState, PlayingSession, SessionPlayer, SessionUser, TranscodeSession};
pub use settings::{EnumValues, Setting, SettingKind, SettingValue, Settings};

use std::fmt;

use serde::Deserialize;
use url::Url;

use crate::client::HttpClient;
use crate::config::ClientConfig;
use crate::error::{Error, Result};
use crate::library::Library;
use crate::util::ids::{MachineIdentifier, PlexToken};
use crate::xml::MediaContainerMeta;

// -----------------------------------------------------------------------------
// ServerIdentity — the parsed snapshot of `GET /`.
// -----------------------------------------------------------------------------

/// Identifying metadata for a Plex Media Server.
///
/// Populated by [`PlexServer::connect`] from the root `MediaContainer`
/// returned by `GET /`. Every field is optional except
/// [`Self::machine_identifier`] and [`Self::version`] — Plex always
/// emits those.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
// Plex emits ~10 independent boolean capability flags on the root
// container; representing them as one bool per field is the most
// natural wire mapping. Collapsing them into a bitflag would be an
// abstraction not driven by the underlying schema.
#[allow(clippy::struct_excessive_bools)]
pub struct ServerIdentity {
    /// The PMS's stable 40-char identifier (`machineIdentifier`).
    pub machine_identifier: MachineIdentifier,
    /// Server software version (`1.40.x.NNNN-…`).
    pub version: String,
    /// User-set friendly name (e.g. "Living Room"). Optional.
    pub friendly_name: Option<String>,
    /// OS the server is running on (`Linux`, `Mac OS X`, `Windows`, …).
    pub platform: Option<String>,
    /// OS version string.
    pub platform_version: Option<String>,
    /// Whether the server is currently linked to a `plex.tv` account.
    pub my_plex: bool,
    /// Username of the linked plex.tv account, when present.
    pub my_plex_username: Option<String>,
    /// Whether the server is publicly reachable via plex.tv relay.
    pub my_plex_signin_state: Option<String>,
    /// Whether the linked account holds a Plex Pass subscription.
    pub my_plex_subscription: bool,
    /// Whether the server allows clients to delete media.
    pub allow_media_deletion: bool,
    /// Whether the server permits library sharing.
    pub allow_sharing: bool,
    /// Whether the server has Live TV / DVR features.
    pub livetv: Option<String>,
    /// Last modification time of the root container (epoch seconds).
    pub updated_at: Option<i64>,
}

/// JSON DTO for the `MediaContainer` returned by `GET /`.
///
/// Plex emits a *huge* attribute set on the root container; this DTO
/// captures only the fields [`ServerIdentity`] needs. Unknown fields
/// are silently ignored via serde's default handling.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RootContainerDto {
    machine_identifier: String,
    version: String,
    #[serde(default)]
    friendly_name: Option<String>,
    #[serde(default)]
    platform: Option<String>,
    #[serde(default)]
    platform_version: Option<String>,
    #[serde(default)]
    my_plex: Option<PlexBool>,
    #[serde(default)]
    my_plex_username: Option<String>,
    #[serde(default)]
    my_plex_signin_state: Option<String>,
    #[serde(default)]
    my_plex_subscription: Option<PlexBool>,
    #[serde(default)]
    allow_media_deletion: Option<PlexBool>,
    #[serde(default)]
    allow_sharing: Option<PlexBool>,
    #[serde(default)]
    livetv: Option<FlexString>,
    #[serde(default)]
    updated_at: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RootEnvelope {
    Wrapped {
        #[serde(rename = "MediaContainer")]
        container: RootContainerDto,
    },
}

/// Boolean fields Plex serialises as `"0"` / `"1"` / `0` / `1` / `true`.
///
/// Internal helper — exposed in DTO only.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub(crate) enum PlexBool {
    Numeric(u8),
    Native(bool),
    Str(String),
}

impl PlexBool {
    pub(crate) fn to_bool(&self) -> bool {
        match self {
            Self::Numeric(n) => *n != 0,
            Self::Native(b) => *b,
            Self::Str(s) => matches!(s.as_str(), "1" | "true" | "True"),
        }
    }
}

/// Crate-private re-export so sibling modules (`library`) can reuse
/// the same flexible-boolean parser without duplicating it.
pub(crate) type PlexBoolField = PlexBool;

/// A field Plex serialises as either a JSON string (`"7"`) or a JSON
/// number (`7`), depending on server version and endpoint (PMS 1.43+
/// emits the root `livetv` flag as a bare number). Normalised to
/// `String` so the public API stays stable across both wire forms.
///
/// `i64` (not `serde_json::Number`) is used for the numeric arm because
/// this type is nested inside the `#[serde(untagged)]` `RootEnvelope`,
/// and `serde_json::Number`/`Value` do not round-trip through serde's
/// untagged content buffer.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub(crate) enum FlexString {
    Str(String),
    Int(i64),
}

impl FlexString {
    pub(crate) fn into_string(self) -> String {
        match self {
            Self::Str(s) => s,
            Self::Int(n) => n.to_string(),
        }
    }
}

impl ServerIdentity {
    fn from_dto(dto: RootContainerDto) -> Result<Self> {
        Ok(Self {
            machine_identifier: MachineIdentifier::new(dto.machine_identifier)?,
            version: dto.version,
            friendly_name: dto.friendly_name,
            platform: dto.platform,
            platform_version: dto.platform_version,
            my_plex: dto.my_plex.is_some_and(|b| b.to_bool()),
            my_plex_username: dto.my_plex_username,
            my_plex_signin_state: dto.my_plex_signin_state,
            my_plex_subscription: dto.my_plex_subscription.is_some_and(|b| b.to_bool()),
            allow_media_deletion: dto.allow_media_deletion.is_some_and(|b| b.to_bool()),
            allow_sharing: dto.allow_sharing.is_some_and(|b| b.to_bool()),
            livetv: dto.livetv.map(FlexString::into_string),
            updated_at: dto.updated_at,
        })
    }
}

// -----------------------------------------------------------------------------
// PlexServer.
// -----------------------------------------------------------------------------

/// A connected Plex Media Server.
///
/// Cheap to clone (`HttpClient` is itself cheap-clone).
#[derive(Clone)]
pub struct PlexServer {
    base_url: Url,
    http: HttpClient,
    identity: ServerIdentity,
}

impl PlexServer {
    /// Connect to a PMS at `base_url` using `token` for auth.
    ///
    /// The implementation calls `GET /` and parses the root
    /// `MediaContainer` to populate [`ServerIdentity`]. The connection
    /// is therefore validated eagerly: a wrong token surfaces as
    /// [`Error::Unauthorized`], a wrong URL as a transport error.
    ///
    /// # Errors
    /// Any variant of [`Error`]. See [`HttpClient`] for details on
    /// status-to-error mapping.
    pub async fn connect(base_url: Url, token: PlexToken) -> Result<Self> {
        let config = ClientConfig::builder(crate::util::ids::ClientIdentifier::generated())
            .token(Some(token))
            .build()?;
        Self::connect_with_config(base_url, config).await
    }

    /// Like [`Self::connect`] but lets the caller pass a pre-built
    /// [`ClientConfig`] (useful for custom identity headers / timeouts
    /// / retry tuning).
    ///
    /// The config must already have its `token` set; passing one
    /// without a token will still succeed for unauthenticated PMS
    /// installations but most endpoints will reject the request.
    ///
    /// # Errors
    /// See [`Self::connect`].
    pub async fn connect_with_config(base_url: Url, config: ClientConfig) -> Result<Self> {
        let http = HttpClient::new(config)?;
        Self::from_http(base_url, http).await
    }

    /// Construct from an already-built [`HttpClient`]. Useful for tests
    /// and for scenarios where the caller wants to reuse one client
    /// across multiple `PlexServer` connections.
    ///
    /// # Errors
    /// See [`Self::connect`].
    pub async fn from_http(base_url: Url, http: HttpClient) -> Result<Self> {
        let url = join_path(&base_url, "/")?;
        let env: RootEnvelope = http.get_json(url.as_str()).await?;
        let identity = match env {
            RootEnvelope::Wrapped { container } => ServerIdentity::from_dto(container)?,
        };
        Ok(Self {
            base_url,
            http,
            identity,
        })
    }

    /// Construct a [`PlexServer`] without performing the identity
    /// probe. **Test-only** — `pub` (not `pub(crate)`) so integration
    /// tests under `tests/` can call it, but `#[doc(hidden)]` so it
    /// stays out of the public docs.
    ///
    /// Production callers must use [`Self::connect`] /
    /// [`Self::connect_with_config`] / [`Self::from_http`] which
    /// validate the connection.
    #[doc(hidden)]
    #[must_use]
    pub const fn __test_new(base_url: Url, http: HttpClient, identity: ServerIdentity) -> Self {
        Self {
            base_url,
            http,
            identity,
        }
    }

    /// Borrow the cached [`ServerIdentity`].
    #[must_use]
    pub const fn identity(&self) -> &ServerIdentity {
        &self.identity
    }

    /// Borrow the base URL of the server.
    #[must_use]
    pub const fn base_url(&self) -> &Url {
        &self.base_url
    }

    /// Borrow the underlying [`HttpClient`]. Public so callers and
    /// sub-modules can issue ad-hoc requests against the same
    /// configured client.
    #[must_use]
    pub const fn http(&self) -> &HttpClient {
        &self.http
    }

    /// Convenience: return a [`Library`] handle bound to this server.
    /// Cheap — `Library` is a thin wrapper around a cloned
    /// [`HttpClient`] and the base URL.
    #[must_use]
    pub fn library(&self) -> Library {
        Library::new(self.http.clone(), self.base_url.clone())
    }

    /// Issue a "ping" — `GET /identity`. Useful as a lightweight
    /// reachability probe; the response is a tiny `MediaContainer`
    /// with no nested entities.
    ///
    /// # Errors
    /// See [`Self::connect`].
    pub async fn ping(&self) -> Result<MediaContainerMeta> {
        #[derive(Deserialize)]
        struct Env {
            #[serde(rename = "MediaContainer")]
            container: MediaContainerMeta,
        }
        let url = join_path(&self.base_url, "/identity")?;
        let env: Env = self.http.get_json(url.as_str()).await?;
        Ok(env.container)
    }

    /// List every playlist on the server.
    ///
    /// Calls `GET /playlists` and parses the `<Playlist>` children.
    /// Use [`crate::Playlist::items`] on each returned playlist to
    /// fetch its items, [`crate::Playlist::delete`] to remove it.
    ///
    /// # Errors
    /// Any transport [`Error`] variant.
    pub async fn playlists(&self) -> Result<Vec<crate::Playlist>> {
        let url = join_path(&self.base_url, "/playlists")?;
        let body = self.http.get_bytes(url.as_str()).await?;
        let body_str = std::str::from_utf8(&body)
            .map_err(|e| Error::Config(format!("/playlists body not utf-8: {e}")))?;
        let mc: crate::xml::MediaContainer<crate::media::playlist::PlaylistDto> =
            crate::xml::MediaContainer::from_json(body_str, "Metadata")?;
        mc.items
            .into_iter()
            .map(|dto| dto.into_domain(self.http.clone(), self.base_url.clone()))
            .collect()
    }
}

impl fmt::Debug for PlexServer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PlexServer")
            .field("base_url", &self.base_url)
            .field("identity", &self.identity)
            .field("http", &self.http)
            .finish()
    }
}

// -----------------------------------------------------------------------------
// Helpers.
// -----------------------------------------------------------------------------

/// Resolve a relative path against a PMS base URL.
///
/// Plex base URLs are expected to be of the form
/// `http(s)://host:port/`. `Url::join` with a leading `/` replaces the
/// full path, which is what we want.
pub(crate) fn join_path(base: &Url, path: &str) -> Result<Url> {
    base.join(path).map_err(Error::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plex_bool_handles_numeric_str_and_native() {
        assert!(PlexBool::Numeric(1).to_bool());
        assert!(!PlexBool::Numeric(0).to_bool());
        assert!(PlexBool::Native(true).to_bool());
        assert!(!PlexBool::Native(false).to_bool());
        assert!(PlexBool::Str("1".to_owned()).to_bool());
        assert!(PlexBool::Str("true".to_owned()).to_bool());
        assert!(!PlexBool::Str("0".to_owned()).to_bool());
        assert!(!PlexBool::Str("false".to_owned()).to_bool());
    }

    #[test]
    fn join_path_replaces_path() {
        let base = Url::parse("http://example.com:32400/").unwrap();
        let url = join_path(&base, "/library/sections").unwrap();
        assert_eq!(url.path(), "/library/sections");
    }

    #[test]
    fn server_identity_from_minimal_dto() {
        let body = r#"{
            "MediaContainer": {
                "machineIdentifier": "abc-machine",
                "version": "1.40.0"
            }
        }"#;
        let env: RootEnvelope = serde_json::from_str(body).unwrap();
        let id = match env {
            RootEnvelope::Wrapped { container } => ServerIdentity::from_dto(container).unwrap(),
        };
        assert_eq!(id.machine_identifier.as_str(), "abc-machine");
        assert_eq!(id.version, "1.40.0");
        assert!(id.friendly_name.is_none());
        assert!(!id.my_plex);
    }

    #[test]
    fn server_identity_parses_string_booleans() {
        let body = r#"{
            "MediaContainer": {
                "machineIdentifier": "m",
                "version": "v",
                "myPlex": "1",
                "myPlexSubscription": "1",
                "allowMediaDeletion": "0",
                "allowSharing": 1
            }
        }"#;
        let env: RootEnvelope = serde_json::from_str(body).unwrap();
        let id = match env {
            RootEnvelope::Wrapped { container } => ServerIdentity::from_dto(container).unwrap(),
        };
        assert!(id.my_plex);
        assert!(id.my_plex_subscription);
        assert!(!id.allow_media_deletion);
        assert!(id.allow_sharing);
    }

    #[test]
    fn server_identity_rejects_empty_machine_identifier() {
        let body = r#"{"MediaContainer":{"machineIdentifier":"","version":"v"}}"#;
        let env: RootEnvelope = serde_json::from_str(body).unwrap();
        let result = match env {
            RootEnvelope::Wrapped { container } => ServerIdentity::from_dto(container),
        };
        assert!(matches!(result, Err(Error::Config(_))));
    }

    fn parse_identity(body: &str) -> ServerIdentity {
        let env: RootEnvelope = serde_json::from_str(body).unwrap();
        match env {
            RootEnvelope::Wrapped { container } => ServerIdentity::from_dto(container).unwrap(),
        }
    }

    #[test]
    fn flex_string_into_string_for_both_arms() {
        assert_eq!(FlexString::Str("7".to_owned()).into_string(), "7");
        assert_eq!(FlexString::Int(7).into_string(), "7");
    }

    #[test]
    fn server_identity_parses_livetv_as_quoted_string() {
        // Pre-1.43 servers quote the livetv flag.
        let id = parse_identity(
            r#"{"MediaContainer":{"machineIdentifier":"m","version":"v","livetv":"7"}}"#,
        );
        assert_eq!(id.livetv.as_deref(), Some("7"));
    }

    #[test]
    fn server_identity_parses_livetv_as_bare_number() {
        // PMS 1.43+ emits the livetv flag as a bare JSON number; it must
        // normalise to the same "7" string.
        let id = parse_identity(
            r#"{"MediaContainer":{"machineIdentifier":"m","version":"v","livetv":7}}"#,
        );
        assert_eq!(id.livetv.as_deref(), Some("7"));
    }

    #[test]
    fn server_identity_livetv_absent_is_none() {
        let id = parse_identity(r#"{"MediaContainer":{"machineIdentifier":"m","version":"v"}}"#);
        assert!(id.livetv.is_none());
    }
}
