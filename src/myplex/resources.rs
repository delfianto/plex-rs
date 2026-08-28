//! [`MyPlexResource`] — a server or player advertised by plex.tv.
//!
//! plex.tv tracks every PMS and player that's signed in to the
//! account, along with every connection URI it knows about for each
//! one. To go from "I have an auth token" to "I have a working
//! [`PlexServer`] handle", a caller would normally need to:
//!
//! 1. fetch the list of resources,
//! 2. pick the one they want by name or `clientIdentifier`,
//! 3. iterate the connection URIs (LAN address, public address,
//!    plex.tv relay) and try each one in priority order.
//!
//! Step 3 is fiddly: LAN URIs are unreachable from the public
//! internet, public URIs are unreachable behind some NATs, and the
//! relay is slow. [`MyPlexResource::connect`] races every URI in
//! parallel and returns the first [`PlexServer`] that successfully
//! answers `GET /identity`, applying a configurable per-attempt
//! timeout so a single slow URI doesn't block the others.
//!
//! ## Priority order
//!
//! Default ordering matches `python-plexapi`:
//!
//! 1. **Location:** `local` → `remote` → `relay`
//! 2. **Scheme:** `https` → `http`
//!
//! For shared resources (i.e. resources the account does **not**
//! own), local URIs are skipped — they wouldn't be reachable from
//! the calling host anyway. Override via [`ConnectOptions`].

use std::time::Duration;

use futures_util::stream::{FuturesUnordered, StreamExt};
use serde::Deserialize;
use url::Url;

use crate::config::ClientConfig;
use crate::error::{Error, Result};
use crate::headers::PlexIdentity;
use crate::server::PlexServer;
use crate::util::ids::{ClientIdentifier, MachineIdentifier, PlexToken};

// -----------------------------------------------------------------------------
// MyPlexResource.
// -----------------------------------------------------------------------------

/// A server or player advertised by plex.tv.
///
/// Built from a single entry in the JSON array returned by
/// `GET https://plex.tv/api/v2/resources`.
#[derive(Debug, Clone)]
#[non_exhaustive]
// Plex emits ~10 independent boolean flags on the resource entry;
// representing them as one bool per field is the most natural wire
// mapping. Collapsing them into a bitflag would be an abstraction
// not driven by the underlying schema.
#[allow(clippy::struct_excessive_bools)]
pub struct MyPlexResource {
    /// Friendly name set by the owner (e.g. "Living Room").
    pub name: String,
    /// Product type — e.g. "Plex Media Server", "Plex for iOS".
    pub product: String,
    /// Product version string.
    pub product_version: Option<String>,
    /// Operating system the resource runs on.
    pub platform: Option<String>,
    /// OS version string.
    pub platform_version: Option<String>,
    /// Hardware device class as detected by plex.tv (`PC`, `PS4`, …).
    pub device: Option<String>,
    /// Stable client identifier. Equivalent to a PMS's
    /// `machineIdentifier`.
    pub client_identifier: MachineIdentifier,
    /// Comma-split list of capabilities the resource advertises —
    /// `server`, `player`, `controller`, etc.
    pub provides: Vec<String>,
    /// `true` when the signed-in account owns this resource.
    pub owned: bool,
    /// `true` when the resource is currently online.
    pub presence: bool,
    /// `true` when plex.tv asserts the resource requires HTTPS.
    pub https_required: bool,
    /// `true` when the public IP of the resource matches the calling
    /// host's public IP — hint that direct connection is possible.
    pub public_address_matches: bool,
    /// `true` when the resource has the Plex Relay enabled.
    pub relay: bool,
    /// `true` when the server has DNS rebinding protection enabled
    /// (forces the `*.plex.direct` hostnames over raw IPs).
    pub dns_rebinding_protection: bool,
    /// Per-resource access token for this resource, when plex.tv mints
    /// one. Distinct from the account-level `MyPlex` token — Plex mints a
    /// separate token per resource so a shared server can be revoked
    /// without invalidating the user's main session. `None` for resources
    /// plex.tv lists without a token (e.g. players you merely control,
    /// or other accounts' shared resources).
    pub access_token: Option<PlexToken>,
    /// Public IP address plex.tv last saw the resource on.
    pub public_address: Option<String>,
    /// All connection URIs plex.tv knows about for this resource.
    /// Ordering is not significant — use
    /// [`Self::preferred_connections`] to pick.
    pub connections: Vec<ResourceConnection>,
}

impl MyPlexResource {
    /// `true` when this resource advertises the `server` capability.
    /// Players advertise `player`/`controller` and won't accept a
    /// PMS-style connection.
    #[must_use]
    pub fn is_server(&self) -> bool {
        self.provides.iter().any(|p| p == "server")
    }

    /// Build a sorted priority list of connection URIs.
    ///
    /// Ordering: location (`local`, `remote`, `relay`) outer,
    /// scheme (`https`, `http`) inner. For shared resources, local
    /// URIs are excluded (they wouldn't be reachable from this host).
    ///
    /// `ssl == Some(true)` keeps only HTTPS URIs;
    /// `ssl == Some(false)` keeps only HTTP URIs;
    /// `ssl == None` returns both.
    #[must_use]
    pub fn preferred_connections(&self, ssl: Option<bool>) -> Vec<String> {
        let mut out = Vec::with_capacity(self.connections.len());
        for &location in &[Location::Local, Location::Remote, Location::Relay] {
            for &https in &[true, false] {
                if let Some(want_ssl) = ssl
                    && want_ssl != https
                {
                    continue;
                }
                for c in &self.connections {
                    if c.location() != location {
                        continue;
                    }
                    if c.is_https() != https {
                        continue;
                    }
                    // Skip local URIs for shared (non-owned) resources;
                    // python-plexapi's preferred_connections gate.
                    if location == Location::Local && !self.owned {
                        continue;
                    }
                    out.push(c.uri.clone());
                }
            }
        }
        out
    }

    /// Race concurrent probes against every preferred connection URI
    /// and return the first [`PlexServer`] that answers. Default
    /// options.
    ///
    /// # Errors
    /// - [`Error::NotFound`] when every connection URI fails.
    /// - The last failing probe's error if every probe failed.
    pub async fn connect(&self) -> Result<PlexServer> {
        self.connect_with_options(ConnectOptions::default()).await
    }

    /// Same as [`Self::connect`] with caller-tuned options.
    ///
    /// # Errors
    /// See [`Self::connect`].
    pub async fn connect_with_options(&self, opts: ConnectOptions) -> Result<PlexServer> {
        let urls = self.preferred_connections(opts.ssl);
        if urls.is_empty() {
            return Err(Error::NotFound {
                resource: format!("no usable connections for resource '{}'", self.name),
            });
        }
        let Some(token) = self.access_token.clone() else {
            return Err(Error::NotFound {
                resource: format!(
                    "resource '{}' has no access token to connect with",
                    self.name
                ),
            });
        };
        let mut probes = FuturesUnordered::new();
        for raw in urls {
            let token = token.clone();
            let identifier = opts.client_identifier.clone();
            let identity = opts.identity.clone();
            let per_attempt = opts.per_attempt_timeout;
            probes.push(async move {
                connect_one(&raw, token, identifier, identity, per_attempt).await
            });
        }
        let mut last_err: Option<Error> = None;
        while let Some(res) = probes.next().await {
            match res {
                Ok(server) => return Ok(server),
                Err(e) => last_err = Some(e),
            }
        }
        Err(last_err.unwrap_or(Error::NotFound {
            resource: format!("resource '{}' had no answering connection", self.name),
        }))
    }

    /// Crate-private constructor used by [`crate::myplex::MyPlexClient`].
    pub(crate) fn from_dto(dto: MyPlexResourceDto) -> Result<Self> {
        let client_identifier = MachineIdentifier::new(dto.client_identifier)?;
        let access_token = dto
            .access_token
            .filter(|s| !s.is_empty())
            .map(PlexToken::new)
            .transpose()?;
        let provides = dto
            .provides
            .split(',')
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty())
            .collect();
        Ok(Self {
            name: dto.name,
            product: dto.product,
            product_version: dto.product_version,
            platform: dto.platform,
            platform_version: dto.platform_version,
            device: dto.device,
            client_identifier,
            provides,
            owned: dto.owned.unwrap_or(false),
            presence: dto.presence.unwrap_or(false),
            https_required: dto.https_required.unwrap_or(false),
            public_address_matches: dto.public_address_matches.unwrap_or(false),
            relay: dto.relay.unwrap_or(false),
            dns_rebinding_protection: dto.dns_rebinding_protection.unwrap_or(false),
            public_address: dto.public_address,
            access_token,
            connections: dto
                .connections
                .into_iter()
                .map(ResourceConnection::from_dto)
                .collect(),
        })
    }
}

/// Connect to a single URI and build a `PlexServer`.
async fn connect_one(
    raw: &str,
    token: PlexToken,
    identifier: ClientIdentifier,
    identity: Option<PlexIdentity>,
    per_attempt_timeout: Duration,
) -> Result<PlexServer> {
    let url = Url::parse(raw)?;
    let mut cfg_b = ClientConfig::builder(identifier)
        .token(Some(token))
        .request_timeout(per_attempt_timeout)
        .connect_timeout(per_attempt_timeout);
    if let Some(id) = identity {
        cfg_b = cfg_b.identity(id);
    }
    let cfg = cfg_b.build()?;
    PlexServer::connect_with_config(url, cfg).await
}

// -----------------------------------------------------------------------------
// ResourceConnection.
// -----------------------------------------------------------------------------

/// A single connection URI plex.tv knows about for a resource.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ResourceConnection {
    /// `https` or `http`.
    pub protocol: String,
    /// Raw IP address or hostname (e.g. `192.168.1.10` or
    /// `1-2-3-4.abcd.plex.direct`).
    pub address: String,
    /// TCP port.
    pub port: u16,
    /// Fully-formed URI as advertised by plex.tv. Use this
    /// rather than reconstructing from `protocol`/`address`/`port`.
    pub uri: String,
    /// `true` when this is a LAN address.
    pub local: bool,
    /// `true` when this URI routes through plex.tv's relay.
    pub relay: bool,
    /// `true` when `address` is an IPv6 literal.
    pub ipv6: bool,
}

impl ResourceConnection {
    fn from_dto(dto: ResourceConnectionDto) -> Self {
        Self {
            protocol: dto.protocol,
            address: dto.address,
            port: dto.port,
            uri: dto.uri,
            local: dto.local.unwrap_or(false),
            relay: dto.relay.unwrap_or(false),
            ipv6: dto.ipv6.unwrap_or(false),
        }
    }

    /// `true` when the URI uses HTTPS.
    #[must_use]
    pub fn is_https(&self) -> bool {
        self.protocol.eq_ignore_ascii_case("https")
    }

    /// Classify the connection as local / remote / relay for the
    /// preference-ordering pass.
    #[must_use]
    pub(crate) const fn location(&self) -> Location {
        if self.relay {
            Location::Relay
        } else if self.local {
            Location::Local
        } else {
            Location::Remote
        }
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub(crate) enum Location {
    Local,
    Remote,
    Relay,
}

// -----------------------------------------------------------------------------
// ConnectOptions.
// -----------------------------------------------------------------------------

/// Tunables for [`MyPlexResource::connect_with_options`].
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ConnectOptions {
    /// Filter connections by scheme. `None` (default) tries both.
    pub ssl: Option<bool>,
    /// Per-attempt request + connect timeout. Default: 8 seconds.
    pub per_attempt_timeout: Duration,
    /// Stable client identifier used by every probe. Default:
    /// [`ClientIdentifier::generated`].
    pub client_identifier: ClientIdentifier,
    /// Optional override for the X-Plex-* identity headers. When
    /// `None`, the crate default identity is used.
    pub identity: Option<PlexIdentity>,
}

impl Default for ConnectOptions {
    fn default() -> Self {
        Self {
            ssl: None,
            per_attempt_timeout: Duration::from_secs(8),
            client_identifier: ClientIdentifier::generated(),
            identity: None,
        }
    }
}

impl ConnectOptions {
    /// Set [`Self::ssl`] (builder style).
    #[must_use]
    pub const fn with_ssl(mut self, ssl: Option<bool>) -> Self {
        self.ssl = ssl;
        self
    }

    /// Set [`Self::per_attempt_timeout`] (builder style).
    #[must_use]
    pub const fn with_per_attempt_timeout(mut self, timeout: Duration) -> Self {
        self.per_attempt_timeout = timeout;
        self
    }

    /// Set [`Self::client_identifier`] (builder style).
    #[must_use]
    pub fn with_client_identifier(mut self, id: ClientIdentifier) -> Self {
        self.client_identifier = id;
        self
    }

    /// Set [`Self::identity`] (builder style).
    #[must_use]
    pub fn with_identity(mut self, identity: PlexIdentity) -> Self {
        self.identity = Some(identity);
        self
    }
}

// -----------------------------------------------------------------------------
// DTOs.
// -----------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MyPlexResourceDto {
    name: String,
    product: String,
    #[serde(default)]
    product_version: Option<String>,
    #[serde(default)]
    platform: Option<String>,
    #[serde(default)]
    platform_version: Option<String>,
    #[serde(default)]
    device: Option<String>,
    client_identifier: String,
    #[serde(default)]
    provides: String,
    #[serde(default)]
    owned: Option<bool>,
    #[serde(default)]
    presence: Option<bool>,
    #[serde(default)]
    https_required: Option<bool>,
    #[serde(default)]
    public_address_matches: Option<bool>,
    #[serde(default)]
    relay: Option<bool>,
    #[serde(default)]
    dns_rebinding_protection: Option<bool>,
    #[serde(default)]
    public_address: Option<String>,
    // Null/absent for resources plex.tv lists without a per-resource token
    // (players, other accounts' shared resources).
    #[serde(default)]
    access_token: Option<String>,
    #[serde(default)]
    connections: Vec<ResourceConnectionDto>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResourceConnectionDto {
    protocol: String,
    address: String,
    port: u16,
    uri: String,
    #[serde(default)]
    local: Option<bool>,
    #[serde(default)]
    relay: Option<bool>,
    // plex.tv emits `IPv6` (PascalCase); rename_all alone doesn't
    // catch this case, so override explicitly.
    #[serde(default, rename = "IPv6")]
    ipv6: Option<bool>,
}

// -----------------------------------------------------------------------------
// Tests.
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_dto() -> MyPlexResourceDto {
        // A representative entry: owned server with three connections
        // (LAN https, public https, plex.tv relay).
        serde_json::from_value(serde_json::json!({
            "name": "Living Room",
            "product": "Plex Media Server",
            "productVersion": "1.40.2.0",
            "platform": "Linux",
            "platformVersion": "5.15",
            "device": "PC",
            "clientIdentifier": "abcd1234abcd1234abcd1234abcd1234abcd1234",
            "createdAt": "2020-01-01T00:00:00Z",
            "lastSeenAt": "2026-05-27T00:00:00Z",
            "provides": "server",
            "owned": true,
            "accessToken": "resource-access-token",
            "publicAddress": "1.2.3.4",
            "httpsRequired": false,
            "synced": false,
            "relay": false,
            "dnsRebindingProtection": false,
            "natLoopbackSupported": false,
            "publicAddressMatches": true,
            "presence": true,
            "connections": [
                {"protocol":"https","address":"10.0.0.5","port":32400,
                 "uri":"https://10-0-0-5.abc.plex.direct:32400",
                 "local":true,"relay":false,"IPv6":false},
                {"protocol":"https","address":"1.2.3.4","port":32400,
                 "uri":"https://1-2-3-4.abc.plex.direct:32400",
                 "local":false,"relay":false,"IPv6":false},
                {"protocol":"https","address":"relay.plex.tv","port":443,
                 "uri":"https://relay.plex.tv:443/abcd",
                 "local":false,"relay":true,"IPv6":false}
            ]
        }))
        .unwrap()
    }

    #[test]
    fn dto_parses_full_entry() {
        let dto = sample_dto();
        let r = MyPlexResource::from_dto(dto).unwrap();
        assert_eq!(r.name, "Living Room");
        assert_eq!(r.provides, vec!["server"]);
        assert!(r.is_server());
        assert!(r.owned);
        assert_eq!(r.connections.len(), 3);
        assert!(r.connections[0].local);
        assert!(r.connections[2].relay);
    }

    #[test]
    fn preferred_connections_orders_local_then_remote_then_relay() {
        let r = MyPlexResource::from_dto(sample_dto()).unwrap();
        let order = r.preferred_connections(None);
        assert_eq!(order.len(), 3);
        assert!(order[0].contains("10-0-0-5"), "local first: {order:?}");
        assert!(order[1].contains("1-2-3-4"), "remote second: {order:?}");
        assert!(order[2].contains("relay.plex.tv"), "relay last: {order:?}");
    }

    #[test]
    fn preferred_connections_skips_local_for_shared_resources() {
        let mut r = MyPlexResource::from_dto(sample_dto()).unwrap();
        r.owned = false;
        let order = r.preferred_connections(None);
        assert_eq!(order.len(), 2, "local LAN URI should drop: {order:?}");
        assert!(order[0].contains("1-2-3-4"));
        assert!(order[1].contains("relay.plex.tv"));
    }

    #[test]
    fn preferred_connections_filters_by_scheme() {
        let r = MyPlexResource::from_dto(sample_dto()).unwrap();
        assert_eq!(r.preferred_connections(Some(true)).len(), 3);
        // All connections in the sample are https, so http-only is empty.
        assert!(r.preferred_connections(Some(false)).is_empty());
    }

    #[test]
    fn preferred_connections_orders_https_before_http() {
        // Synthesize a single-location resource with mixed schemes.
        let dto: MyPlexResourceDto = serde_json::from_value(serde_json::json!({
            "name": "T", "product": "PMS",
            "clientIdentifier": "abcd1234abcd1234abcd1234abcd1234abcd1234",
            "provides": "server", "owned": true, "presence": true,
            "accessToken": "tk",
            "connections": [
                {"protocol":"http","address":"a","port":32400,"uri":"http://a:32400",
                 "local":false,"relay":false,"IPv6":false},
                {"protocol":"https","address":"a","port":32400,"uri":"https://a:32400",
                 "local":false,"relay":false,"IPv6":false}
            ]
        }))
        .unwrap();
        let r = MyPlexResource::from_dto(dto).unwrap();
        let order = r.preferred_connections(None);
        assert_eq!(order.len(), 2);
        assert!(order[0].starts_with("https://"), "https first: {order:?}");
        assert!(order[1].starts_with("http://"));
    }

    #[test]
    fn provides_is_csv_split() {
        let dto: MyPlexResourceDto = serde_json::from_value(serde_json::json!({
            "name":"Phone","product":"Plex for iOS",
            "clientIdentifier":"abcdefabcdefabcdefabcdefabcdefabcdefabcd",
            "provides":"player, controller, pubsub-player",
            "presence": true, "accessToken":"x",
        }))
        .unwrap();
        let r = MyPlexResource::from_dto(dto).unwrap();
        assert_eq!(r.provides, vec!["player", "controller", "pubsub-player"]);
        assert!(!r.is_server());
    }

    #[test]
    fn ipv6_pascalcase_field_deserializes() {
        let dto: MyPlexResourceDto = serde_json::from_value(serde_json::json!({
            "name":"S","product":"PMS",
            "clientIdentifier":"abcdefabcdefabcdefabcdefabcdefabcdefabcd",
            "provides":"server","presence": true, "accessToken":"x",
            "connections":[
                {"protocol":"https","address":"::1","port":32400,
                 "uri":"https://[::1]:32400","IPv6":true}
            ]
        }))
        .unwrap();
        let r = MyPlexResource::from_dto(dto).unwrap();
        assert!(r.connections[0].ipv6);
    }

    #[test]
    fn access_token_is_redacted_in_debug() {
        let r = MyPlexResource::from_dto(sample_dto()).unwrap();
        let dbg = format!("{r:?}");
        assert!(
            !dbg.contains("resource-access-token"),
            "Debug leaked token: {dbg}"
        );
        assert!(dbg.contains("***redacted***"));
    }

    #[tokio::test]
    async fn connect_returns_not_found_when_no_connections() {
        let mut r = MyPlexResource::from_dto(sample_dto()).unwrap();
        r.connections.clear();
        let err = r.connect().await.unwrap_err();
        assert!(
            matches!(err, Error::NotFound { .. }),
            "expected NotFound, got {err:?}"
        );
    }
}
