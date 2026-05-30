//! `X-Plex-*` header construction.
//!
//! Plex identifies clients through a fixed set of `X-Plex-*` HTTP
//! headers documented in the `OpenAPI` spec `info.description`. Every
//! request — authenticated or not — must include at least the
//! identity headers (`Product`, `Version`, `Platform`,
//! `Client-Identifier`); requests against authenticated endpoints add
//! `X-Plex-Token`.
//!
//! The set we emit by default matches
//! `python-plexapi/plexapi/config.py:53-68`'s `BASE_HEADERS`, with a
//! few additions Plex documents on its developer portal:
//!
//! | Header                       | Required | Default                |
//! | ---------------------------- | :------: | ---------------------- |
//! | `X-Plex-Client-Identifier`   |   yes    | random UUID per build  |
//! | `X-Plex-Product`             |   yes    | `plex-rs`              |
//! | `X-Plex-Version`             |   yes    | crate version          |
//! | `X-Plex-Platform`            |   yes    | `std::env::consts::OS` |
//! | `X-Plex-Platform-Version`    |    no    | unset                  |
//! | `X-Plex-Device`              |    no    | "Other"                |
//! | `X-Plex-Device-Name`         |    no    | `hostname` if known    |
//! | `X-Plex-Provides`            |    no    | `controller`           |
//! | `X-Plex-Sync-Version`        |    no    | `2`                    |
//! | `X-Plex-Token`               |    no    | only when present      |
//!
//! All header values are constructed to be valid ASCII; non-ASCII
//! user-supplied values are rejected with [`Error::InvalidHeader`].

use http::header::{HeaderMap, HeaderName, HeaderValue};

use crate::error::{Error, Result};
use crate::util::ids::{ClientIdentifier, PlexToken};

// -----------------------------------------------------------------------------
// Header-name constants.
// -----------------------------------------------------------------------------

// Header-name constants are lowercase to satisfy
// `HeaderName::from_static`; HTTP headers are case-insensitive per
// RFC 7230 §3.2 so this is the canonical wire form.

/// `x-plex-token` header name (case-insensitive on the wire).
pub const HEADER_TOKEN: &str = "x-plex-token";

/// `x-plex-client-identifier` header name.
pub const HEADER_CLIENT_IDENTIFIER: &str = "x-plex-client-identifier";

/// `x-plex-product` header name.
pub const HEADER_PRODUCT: &str = "x-plex-product";

/// `x-plex-version` header name.
pub const HEADER_VERSION: &str = "x-plex-version";

/// `x-plex-platform` header name.
pub const HEADER_PLATFORM: &str = "x-plex-platform";

/// `x-plex-platform-version` header name.
pub const HEADER_PLATFORM_VERSION: &str = "x-plex-platform-version";

/// `x-plex-device` header name.
pub const HEADER_DEVICE: &str = "x-plex-device";

/// `x-plex-device-name` header name.
pub const HEADER_DEVICE_NAME: &str = "x-plex-device-name";

/// `x-plex-provides` header name.
pub const HEADER_PROVIDES: &str = "x-plex-provides";

/// `x-plex-sync-version` header name.
pub const HEADER_SYNC_VERSION: &str = "x-plex-sync-version";

/// `accept` header name.
pub const HEADER_ACCEPT: &str = "accept";

/// Default product name when callers don't override.
pub const DEFAULT_PRODUCT: &str = "plex-rs";

/// Default device name when callers don't override.
pub const DEFAULT_DEVICE: &str = "Other";

/// Default `X-Plex-Provides` advertisement.
pub const DEFAULT_PROVIDES: &str = "controller";

/// Sync version Plex expects from API clients.
pub const SYNC_VERSION: &str = "2";

/// `Accept` value to negotiate JSON, used everywhere except the few
/// endpoints that only emit XML.
pub const ACCEPT_JSON: &str = "application/json";

// -----------------------------------------------------------------------------
// PlexIdentity — value-typed bag of all the X-Plex-* identity headers.
// -----------------------------------------------------------------------------

/// Client identity broadcast on every request.
///
/// Build via [`PlexIdentity::new`] (requires a [`ClientIdentifier`])
/// and customise with the `with_*` setters. The identity is then
/// passed into `ClientConfig` (landing in M0.11) which applies it to
/// every outgoing request.
///
/// Plex relies on these values for device deduplication, sharing
/// permissions, and webhook routing.
#[derive(Debug, Clone)]
pub struct PlexIdentity {
    /// `X-Plex-Client-Identifier` — must be stable per install.
    pub client_identifier: ClientIdentifier,
    /// `X-Plex-Product` (display name).
    pub product: String,
    /// `X-Plex-Version` (this client's release/build version).
    pub version: String,
    /// `X-Plex-Platform`.
    pub platform: String,
    /// `X-Plex-Platform-Version`.
    pub platform_version: Option<String>,
    /// `X-Plex-Device` — device-class string (`Other`, `Roku`, …).
    pub device: String,
    /// `X-Plex-Device-Name` — user-friendly name.
    pub device_name: Option<String>,
    /// `X-Plex-Provides` — capabilities advertised to Plex.
    pub provides: String,
    /// `X-Plex-Sync-Version` — protocol version for sync operations.
    pub sync_version: String,
}

impl PlexIdentity {
    /// Construct an identity with all reasonable defaults. The caller
    /// must supply a stable [`ClientIdentifier`].
    #[must_use]
    pub fn new(client_identifier: ClientIdentifier) -> Self {
        Self {
            client_identifier,
            product: DEFAULT_PRODUCT.to_owned(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
            platform: std::env::consts::OS.to_owned(),
            platform_version: None,
            device: DEFAULT_DEVICE.to_owned(),
            device_name: None,
            provides: DEFAULT_PROVIDES.to_owned(),
            sync_version: SYNC_VERSION.to_owned(),
        }
    }

    /// Override the product name (default: `plex-rs`).
    #[must_use]
    pub fn with_product(mut self, product: impl Into<String>) -> Self {
        self.product = product.into();
        self
    }

    /// Override the version string (default: `CARGO_PKG_VERSION`).
    #[must_use]
    pub fn with_version(mut self, version: impl Into<String>) -> Self {
        self.version = version.into();
        self
    }

    /// Override the platform string (default: `std::env::consts::OS`).
    #[must_use]
    pub fn with_platform(mut self, platform: impl Into<String>) -> Self {
        self.platform = platform.into();
        self
    }

    /// Set the optional platform version.
    #[must_use]
    pub fn with_platform_version(mut self, version: impl Into<String>) -> Self {
        self.platform_version = Some(version.into());
        self
    }

    /// Override the device class (default: `Other`).
    #[must_use]
    pub fn with_device(mut self, device: impl Into<String>) -> Self {
        self.device = device.into();
        self
    }

    /// Set the friendly device name.
    #[must_use]
    pub fn with_device_name(mut self, name: impl Into<String>) -> Self {
        self.device_name = Some(name.into());
        self
    }

    /// Override the capabilities advertisement (default: `controller`).
    #[must_use]
    pub fn with_provides(mut self, provides: impl Into<String>) -> Self {
        self.provides = provides.into();
        self
    }

    /// Override the sync protocol version (default: `2`).
    #[must_use]
    pub fn with_sync_version(mut self, version: impl Into<String>) -> Self {
        self.sync_version = version.into();
        self
    }

    /// Render this identity (and an optional [`PlexToken`]) as a fully
    /// populated [`HeaderMap`].
    ///
    /// The map always includes `Accept: application/json` so JSON
    /// negotiation happens by default. The token is rendered verbatim
    /// in the header value — be careful not to log a [`HeaderMap`]
    /// that has been through [`PlexIdentity::headers`].
    ///
    /// # Errors
    /// Returns [`Error::InvalidHeader`] when any field contains a
    /// character that is not a valid HTTP header value byte (CTL chars
    /// or non-ASCII). Per RFC 7230 only visible ASCII (and the few
    /// printable characters) is portable; the safest fix is to
    /// transliterate user-supplied names before passing them in.
    pub fn headers(&self, token: Option<&PlexToken>) -> Result<HeaderMap> {
        let mut h = HeaderMap::with_capacity(11);
        set(&mut h, HEADER_ACCEPT, ACCEPT_JSON)?;
        set(
            &mut h,
            HEADER_CLIENT_IDENTIFIER,
            self.client_identifier.as_str(),
        )?;
        set(&mut h, HEADER_PRODUCT, &self.product)?;
        set(&mut h, HEADER_VERSION, &self.version)?;
        set(&mut h, HEADER_PLATFORM, &self.platform)?;
        if let Some(v) = &self.platform_version {
            set(&mut h, HEADER_PLATFORM_VERSION, v)?;
        }
        set(&mut h, HEADER_DEVICE, &self.device)?;
        if let Some(v) = &self.device_name {
            set(&mut h, HEADER_DEVICE_NAME, v)?;
        }
        set(&mut h, HEADER_PROVIDES, &self.provides)?;
        set(&mut h, HEADER_SYNC_VERSION, &self.sync_version)?;
        if let Some(t) = token {
            set(&mut h, HEADER_TOKEN, t.expose())?;
        }
        Ok(h)
    }
}

/// Insert a single header, mapping every failure mode to
/// [`Error::InvalidHeader`] with a stable, redaction-safe message.
///
/// Beyond the validation `HeaderValue::from_str` already performs
/// (rejects CR / LF / NUL), this helper additionally rejects any
/// non-ASCII byte. Plex's documentation says non-ASCII identity
/// values "may not work" — see the
/// `OpenAPI` `info.description` "Headers"
/// section. Forcing ASCII at the boundary gives callers a
/// deterministic failure mode rather than a silent miscompare on the
/// other end.
fn set(map: &mut HeaderMap, name: &'static str, value: &str) -> Result<()> {
    if !value.is_ascii() {
        return Err(Error::InvalidHeader(format!(
            "value for {name:?} contains non-ASCII characters",
        )));
    }
    let name = HeaderName::from_static(name);
    let value = HeaderValue::from_str(value).map_err(|_| {
        Error::InvalidHeader(format!(
            "value for {name:?} contains control characters or other invalid bytes",
        ))
    })?;
    map.insert(name, value);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::ids::PlexToken;

    fn identity() -> PlexIdentity {
        PlexIdentity::new(ClientIdentifier::new("test-client-id").unwrap())
    }

    #[test]
    fn defaults_populate_required_headers() {
        let id = identity();
        let h = id.headers(None).unwrap();
        assert_eq!(h.get(HEADER_CLIENT_IDENTIFIER).unwrap(), "test-client-id");
        assert_eq!(h.get(HEADER_PRODUCT).unwrap(), DEFAULT_PRODUCT);
        assert_eq!(h.get(HEADER_VERSION).unwrap(), env!("CARGO_PKG_VERSION"));
        assert_eq!(h.get(HEADER_PLATFORM).unwrap(), std::env::consts::OS);
        assert_eq!(h.get(HEADER_DEVICE).unwrap(), DEFAULT_DEVICE);
        assert_eq!(h.get(HEADER_PROVIDES).unwrap(), DEFAULT_PROVIDES);
        assert_eq!(h.get(HEADER_SYNC_VERSION).unwrap(), SYNC_VERSION);
        assert_eq!(h.get(HEADER_ACCEPT).unwrap(), ACCEPT_JSON);
    }

    #[test]
    fn optional_headers_are_absent_by_default() {
        let h = identity().headers(None).unwrap();
        assert!(h.get(HEADER_PLATFORM_VERSION).is_none());
        assert!(h.get(HEADER_DEVICE_NAME).is_none());
        assert!(h.get(HEADER_TOKEN).is_none());
    }

    #[test]
    fn token_when_present_renders_into_x_plex_token_header() {
        let token = PlexToken::new("my-secret").unwrap();
        let h = identity().headers(Some(&token)).unwrap();
        assert_eq!(h.get(HEADER_TOKEN).unwrap(), "my-secret");
    }

    #[test]
    fn builder_methods_compose() {
        let id = identity()
            .with_product("custom-product")
            .with_version("9.9.9")
            .with_platform("Roku")
            .with_platform_version("4.3 build 1057")
            .with_device("Roku 3")
            .with_device_name("Living Room")
            .with_provides("controller,player")
            .with_sync_version("3");
        let h = id.headers(None).unwrap();
        assert_eq!(h.get(HEADER_PRODUCT).unwrap(), "custom-product");
        assert_eq!(h.get(HEADER_VERSION).unwrap(), "9.9.9");
        assert_eq!(h.get(HEADER_PLATFORM).unwrap(), "Roku");
        assert_eq!(h.get(HEADER_PLATFORM_VERSION).unwrap(), "4.3 build 1057");
        assert_eq!(h.get(HEADER_DEVICE).unwrap(), "Roku 3");
        assert_eq!(h.get(HEADER_DEVICE_NAME).unwrap(), "Living Room");
        assert_eq!(h.get(HEADER_PROVIDES).unwrap(), "controller,player");
        assert_eq!(h.get(HEADER_SYNC_VERSION).unwrap(), "3");
    }

    #[test]
    fn rejects_non_ascii_device_name() {
        // U+00E9 (é) is non-ASCII; HeaderValue::from_str will reject it.
        let id = identity().with_device_name("Salon — France");
        let err = id.headers(None).unwrap_err();
        assert!(matches!(err, Error::InvalidHeader(_)));
    }

    #[test]
    fn rejects_control_chars_in_product() {
        let id = identity().with_product("evil\rinjection");
        let err = id.headers(None).unwrap_err();
        assert!(matches!(err, Error::InvalidHeader(_)));
    }

    #[test]
    fn header_count_matches_set_fields() {
        // Required + Accept = 8 headers.
        let h = identity().headers(None).unwrap();
        assert_eq!(h.len(), 8);
        // With token, expect 9.
        let token = PlexToken::new("t").unwrap();
        let h = identity().headers(Some(&token)).unwrap();
        assert_eq!(h.len(), 9);
        // With all optional + token, expect 11.
        let h = identity()
            .with_platform_version("v")
            .with_device_name("D")
            .headers(Some(&token))
            .unwrap();
        assert_eq!(h.len(), 11);
    }
}
