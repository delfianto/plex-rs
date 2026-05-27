//! plex.tv (`MyPlex`) cloud client.
//!
//! [`MyPlexClient`] holds an authenticated handle to plex.tv and is
//! the entry point for everything that lives in the cloud rather than
//! on a specific Plex Media Server:
//!
//! - **Resources** — the list of servers and players the signed-in
//!   account can see, with the connection URIs needed to reach them
//!   ([`resources::MyPlexResource`]). The flagship method
//!   [`MyPlexResource::connect`](resources::MyPlexResource::connect)
//!   races concurrent probes across all known connection URIs and
//!   returns the first server that answers.
//! - **Devices**, **friends**, **home**, **webhooks**, **claim**,
//!   **sonos** — deferred (M5.4).
//!
//! ## Wire shape
//!
//! Every endpoint under `MyPlexClient` lives beneath the plex.tv
//! base URL (`https://plex.tv` by default) and is authenticated via
//! the standard `X-Plex-Token` header carried by the underlying
//! [`crate::HttpClient`]. The base URL is overridable via
//! [`MyPlexClient::with_base`] so integration tests can point the
//! client at a wiremock replica.

pub mod devices;
pub mod resources;
pub mod webhooks;

pub use devices::MyPlexDevice;
pub use resources::{ConnectOptions, MyPlexResource, ResourceConnection};

use serde::de::DeserializeOwned;

use crate::client::HttpClient;
use crate::config::ClientConfig;
use crate::error::Result;
use crate::headers::PlexIdentity;
use crate::util::ids::{ClientIdentifier, PlexToken};

/// Default plex.tv base URL.
const PLEX_TV_BASE: &str = "https://plex.tv";

/// Authenticated handle to plex.tv.
///
/// All `MyPlex` API calls (resources, devices, friends, webhooks)
/// hang off this type. Cheap to clone (`HttpClient` is internally
/// `Arc`-backed).
#[derive(Debug, Clone)]
pub struct MyPlexClient {
    http: HttpClient,
    base: String,
}

impl MyPlexClient {
    /// Build a fresh [`MyPlexClient`] from an auth token and a stable
    /// client identifier.
    ///
    /// `identity` overrides the default `X-Plex-*` headers when
    /// supplied; otherwise the crate's default identity is used.
    ///
    /// # Errors
    /// - [`crate::Error::Config`] if the identifier is empty.
    /// - [`crate::Error::InvalidHeader`] if `identity` contains a
    ///   non-ASCII header value.
    /// - [`crate::Error::Transport`] if the TLS / DNS stack fails to
    ///   initialise.
    pub fn new(
        token: PlexToken,
        client_identifier: ClientIdentifier,
        identity: Option<PlexIdentity>,
    ) -> Result<Self> {
        let mut cfg_b = ClientConfig::builder(client_identifier).token(Some(token));
        if let Some(id) = identity {
            cfg_b = cfg_b.identity(id);
        }
        let http = HttpClient::new(cfg_b.build()?)?;
        Ok(Self::with_client(http))
    }

    /// Build from a caller-supplied [`HttpClient`]. The client must
    /// already carry a valid `X-Plex-Token` in its identity headers.
    #[must_use]
    pub fn with_client(http: HttpClient) -> Self {
        Self {
            http,
            base: PLEX_TV_BASE.to_owned(),
        }
    }

    /// Override the plex.tv base URL. The supplied string is taken
    /// verbatim and used as a prefix; trailing slashes are stripped.
    #[must_use]
    pub fn with_base(mut self, base: impl Into<String>) -> Self {
        let mut s = base.into();
        while s.ends_with('/') {
            s.pop();
        }
        self.base = s;
        self
    }

    /// Borrow the configured plex.tv base URL.
    #[must_use]
    pub fn base(&self) -> &str {
        &self.base
    }

    /// Borrow the underlying [`HttpClient`]. Useful when a caller
    /// wants to reuse the same identity headers for direct PMS
    /// connections.
    #[must_use]
    pub const fn http(&self) -> &HttpClient {
        &self.http
    }

    /// Fetch the full list of resources (servers + players) visible
    /// to the signed-in account.
    ///
    /// Calls `GET /api/v2/resources?includeHttps=1&includeRelay=1`.
    /// The response includes every connection URI plex.tv knows
    /// about — direct LAN, public, and relay.
    ///
    /// # Errors
    /// Any [`crate::Error`] variant. [`crate::Error::Unauthorized`]
    /// indicates a stale token.
    pub async fn resources(&self) -> Result<Vec<MyPlexResource>> {
        let url = format!(
            "{}/api/v2/resources?includeHttps=1&includeRelay=1",
            self.base
        );
        let dtos: Vec<resources::MyPlexResourceDto> = self.get_json(&url).await?;
        let mut out = Vec::with_capacity(dtos.len());
        for dto in dtos {
            out.push(MyPlexResource::from_dto(dto)?);
        }
        Ok(out)
    }

    /// Find the first [`MyPlexResource`] whose name matches `name`
    /// (case-insensitive).
    ///
    /// # Errors
    /// As for [`Self::resources`]; returns `Ok(None)` when no match.
    pub async fn resource(&self, name: &str) -> Result<Option<MyPlexResource>> {
        let resources = self.resources().await?;
        Ok(resources
            .into_iter()
            .find(|r| r.name.eq_ignore_ascii_case(name)))
    }

    /// Crate-private JSON GET helper. Pulls in only because
    /// [`HttpClient::get_json`] already exists — this is a thin
    /// passthrough for ergonomic call sites.
    async fn get_json<T: DeserializeOwned>(&self, url: &str) -> Result<T> {
        self.http.get_json(url).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token() -> PlexToken {
        PlexToken::new("t").unwrap()
    }
    fn cid() -> ClientIdentifier {
        ClientIdentifier::new("c").unwrap()
    }

    #[test]
    fn with_base_strips_trailing_slashes() {
        let c = MyPlexClient::new(token(), cid(), None)
            .unwrap()
            .with_base("https://mock.example.com//");
        assert_eq!(c.base(), "https://mock.example.com");
    }

    #[test]
    fn default_base_is_plex_tv() {
        let c = MyPlexClient::new(token(), cid(), None).unwrap();
        assert_eq!(c.base(), "https://plex.tv");
    }
}
