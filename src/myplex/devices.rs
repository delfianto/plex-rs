//! `MyPlex` device registry.
//!
//! Every device that signs into a Plex account is tracked in
//! `plex.tv`'s device registry. This module exposes:
//!
//! - [`MyPlexClient::devices`] — list every registered device
//!   (servers, players, web clients, mobile apps, the lot).
//! - [`MyPlexDevice::delete`] — revoke a device's access token,
//!   forcing it to re-authenticate.
//!
//! ## Wire format
//!
//! `GET https://plex.tv/devices.xml` returns an XML
//! `<MediaContainer>` of `<Device>` elements. The endpoint is XML-only
//! — the v2 JSON resource endpoint (used by [`crate::MyPlexResource`])
//! returns a different shape that doesn't carry the integer `id`
//! field needed to DELETE individual devices.
//!
//! `DELETE https://plex.tv/devices/<id>.xml` removes one device.

use std::fmt;

use chrono::{DateTime, Utc};
use quick_xml::de::from_str;
use serde::Deserialize;

use crate::error::{Error, Result};
use crate::myplex::MyPlexClient;
use crate::util::ids::{ClientIdentifier, PlexToken};

// -----------------------------------------------------------------------------
// MyPlexDevice.
// -----------------------------------------------------------------------------

/// A device registered against the signed-in Plex account.
#[derive(Clone)]
#[non_exhaustive]
pub struct MyPlexDevice {
    /// plex.tv-internal numeric id. Required for [`Self::delete`].
    pub id: String,
    /// Friendly hostname / device name.
    pub name: Option<String>,
    /// Plex product (`Plex Media Server`, `Plex for iOS`, …).
    pub product: String,
    /// Product version string.
    pub product_version: Option<String>,
    /// OS the device runs on.
    pub platform: Option<String>,
    /// OS version.
    pub platform_version: Option<String>,
    /// Hardware device class (`Linux`, `iPad`, `AFTB`, …).
    pub device: Option<String>,
    /// Hardware model (`bueller`, `x86_64`, …).
    pub model: Option<String>,
    /// Hardware vendor.
    pub vendor: Option<String>,
    /// Capability list — `client`, `controller`, `sync-target`,
    /// `player`, `pubsub-player`, `server`. Comma-split into a `Vec`.
    pub provides: Vec<String>,
    /// Stable per-device identifier.
    pub client_identifier: ClientIdentifier,
    /// Unknown — Plex emits various forms here (`1`, `2`,
    /// `1.3.15`, …).
    pub version: Option<String>,
    /// Per-device access token. Distinct from the account token —
    /// can be revoked via [`Self::delete`] without affecting other
    /// devices.
    pub token: Option<PlexToken>,
    /// Public IPv4 / IPv6 last seen.
    pub public_address: Option<String>,
    /// Screen resolution (e.g. `750x1334`).
    pub screen_resolution: Option<String>,
    /// Screen pixel density hint.
    pub screen_density: Option<String>,
    /// First-seen timestamp.
    pub created_at: Option<DateTime<Utc>>,
    /// Most-recent-seen timestamp.
    pub last_seen_at: Option<DateTime<Utc>>,
    /// All connection URIs plex.tv knows about. Empty for devices
    /// that don't expose a remote-control HTTP endpoint (e.g. web
    /// browsers).
    pub connections: Vec<String>,
}

impl fmt::Debug for MyPlexDevice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MyPlexDevice")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("product", &self.product)
            .field("platform", &self.platform)
            .field("device", &self.device)
            .field("client_identifier", &self.client_identifier)
            .field("token", &self.token)
            .field("public_address", &self.public_address)
            .field("provides", &self.provides)
            .field("connections", &self.connections)
            .finish_non_exhaustive()
    }
}

impl MyPlexDevice {
    /// `true` when this device advertises the `server` capability.
    #[must_use]
    pub fn is_server(&self) -> bool {
        self.provides.iter().any(|p| p == "server")
    }

    /// `true` when this device advertises the `player` capability.
    #[must_use]
    pub fn is_player(&self) -> bool {
        self.provides.iter().any(|p| p == "player")
    }

    /// Revoke the device's access token, removing it from the
    /// account. The device must sign in again to be useful.
    ///
    /// # Errors
    /// Any transport [`Error`] variant. [`Error::NotFound`] suggests
    /// the device was already deleted from another session.
    pub async fn delete(&self, client: &MyPlexClient) -> Result<()> {
        let url = format!("{}/devices/{}.xml", client.base(), self.id);
        client.http().delete(&url).await
    }
}

impl MyPlexClient {
    /// List every device registered against this account.
    ///
    /// # Errors
    /// Any [`Error`] variant. [`Error::Unauthorized`] signals a
    /// stale token.
    pub async fn devices(&self) -> Result<Vec<MyPlexDevice>> {
        let url = format!("{}/devices.xml", self.base());
        let bytes = self.http().get_bytes(&url).await?;
        let body = std::str::from_utf8(&bytes)
            .map_err(|e| Error::Config(format!("devices body not utf-8: {e}")))?;
        parse_devices(body)
    }
}

// -----------------------------------------------------------------------------
// Parsing.
// -----------------------------------------------------------------------------

fn parse_devices(body: &str) -> Result<Vec<MyPlexDevice>> {
    let mc: MediaContainerDto = from_str(body)?;
    let mut out = Vec::with_capacity(mc.devices.len());
    for dto in mc.devices {
        out.push(dto.into_domain()?);
    }
    Ok(out)
}

#[derive(Debug, Deserialize)]
struct MediaContainerDto {
    #[serde(rename = "Device", default)]
    devices: Vec<DeviceDto>,
}

#[derive(Debug, Deserialize)]
struct DeviceDto {
    #[serde(rename = "@id")]
    id: String,
    #[serde(rename = "@name", default)]
    name: Option<String>,
    #[serde(rename = "@product", default)]
    product: String,
    #[serde(rename = "@productVersion", default)]
    product_version: Option<String>,
    #[serde(rename = "@platform", default)]
    platform: Option<String>,
    #[serde(rename = "@platformVersion", default)]
    platform_version: Option<String>,
    #[serde(rename = "@device", default)]
    device: Option<String>,
    #[serde(rename = "@model", default)]
    model: Option<String>,
    #[serde(rename = "@vendor", default)]
    vendor: Option<String>,
    #[serde(rename = "@provides", default)]
    provides: String,
    #[serde(rename = "@clientIdentifier")]
    client_identifier: String,
    #[serde(rename = "@version", default)]
    version: Option<String>,
    #[serde(rename = "@token", default)]
    token: Option<String>,
    #[serde(rename = "@publicAddress", default)]
    public_address: Option<String>,
    #[serde(rename = "@screenResolution", default)]
    screen_resolution: Option<String>,
    #[serde(rename = "@screenDensity", default)]
    screen_density: Option<String>,
    #[serde(rename = "@createdAt", default)]
    created_at: Option<String>,
    #[serde(rename = "@lastSeenAt", default)]
    last_seen_at: Option<String>,
    #[serde(rename = "Connection", default)]
    connections: Vec<ConnectionDto>,
}

#[derive(Debug, Deserialize)]
struct ConnectionDto {
    #[serde(rename = "@uri", default)]
    uri: String,
}

impl DeviceDto {
    fn into_domain(self) -> Result<MyPlexDevice> {
        let client_identifier = ClientIdentifier::new(self.client_identifier)?;
        let token = self
            .token
            .filter(|s| !s.is_empty())
            .map(PlexToken::new)
            .transpose()?;
        let provides = self
            .provides
            .split(',')
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty())
            .collect();
        let created_at = parse_epoch_seconds(self.created_at.as_deref());
        let last_seen_at = parse_epoch_seconds(self.last_seen_at.as_deref());
        let connections = self
            .connections
            .into_iter()
            .filter_map(|c| (!c.uri.is_empty()).then_some(c.uri))
            .collect();
        Ok(MyPlexDevice {
            id: self.id,
            name: self.name,
            product: self.product,
            product_version: self.product_version,
            platform: self.platform,
            platform_version: self.platform_version,
            device: self.device,
            model: self.model,
            vendor: self.vendor,
            provides,
            client_identifier,
            version: self.version,
            token,
            public_address: self.public_address,
            screen_resolution: self.screen_resolution,
            screen_density: self.screen_density,
            created_at,
            last_seen_at,
            connections,
        })
    }
}

/// Parse a timestamp that may arrive as an epoch-seconds string or
/// as an ISO-8601 string. plex.tv emits both depending on
/// endpoint; the device XML uses epoch seconds in practice.
fn parse_epoch_seconds(raw: Option<&str>) -> Option<DateTime<Utc>> {
    let s = raw?;
    if let Ok(epoch) = s.parse::<i64>() {
        return DateTime::<Utc>::from_timestamp(epoch, 0);
    }
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|d| d.with_timezone(&Utc))
}

// -----------------------------------------------------------------------------
// Tests.
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_devices_xml() -> &'static str {
        r#"<?xml version="1.0" encoding="UTF-8"?>
<MediaContainer publicAddress="1.2.3.4" size="2">
  <Device name="Living Room PMS"
          publicAddress="1.2.3.4"
          product="Plex Media Server"
          productVersion="1.40.2.0"
          platform="Linux"
          platformVersion="5.15"
          device="PC"
          model="x86_64"
          vendor="ubuntu"
          provides="server"
          clientIdentifier="abcdef0123456789abcdef0123456789abcd0001"
          version="1.40.2.0"
          id="1001"
          token="token-for-server"
          createdAt="1577836800"
          lastSeenAt="1716422400">
    <Connection uri="https://10-0-0-5.x.plex.direct:32400"/>
    <Connection uri="https://1-2-3-4.x.plex.direct:32400"/>
  </Device>
  <Device name="Alice's iPhone"
          publicAddress="5.6.7.8"
          product="Plex for iOS"
          productVersion="9.5.0"
          platform="iOS"
          platformVersion="17.1"
          device="iPhone"
          model="iPhone14,5"
          vendor=""
          provides="client,player,pubsub-player"
          clientIdentifier="abcdef0123456789abcdef0123456789abcd0002"
          version="9.5.0"
          id="1002"
          token="token-for-phone"
          screenResolution="1170x2532"
          screenDensity="3"
          createdAt="1700000000"
          lastSeenAt="1716000000"/>
</MediaContainer>"#
    }

    #[test]
    fn parse_devices_yields_two_entries() {
        let v = parse_devices(sample_devices_xml()).unwrap();
        assert_eq!(v.len(), 2);
    }

    #[test]
    fn server_device_carries_server_capability_and_connections() {
        let v = parse_devices(sample_devices_xml()).unwrap();
        let server = v.iter().find(|d| d.id == "1001").unwrap();
        assert_eq!(server.product, "Plex Media Server");
        assert!(server.is_server());
        assert!(!server.is_player());
        assert_eq!(server.connections.len(), 2);
        assert!(server.connections[0].contains("10-0-0-5"));
        assert!(server.connections[1].contains("1-2-3-4"));
    }

    #[test]
    fn player_device_carries_client_player_capabilities() {
        let v = parse_devices(sample_devices_xml()).unwrap();
        let phone = v.iter().find(|d| d.id == "1002").unwrap();
        assert!(phone.is_player());
        assert!(!phone.is_server());
        assert_eq!(phone.provides, vec!["client", "player", "pubsub-player"]);
        assert_eq!(phone.screen_resolution.as_deref(), Some("1170x2532"));
        assert_eq!(phone.screen_density.as_deref(), Some("3"));
    }

    #[test]
    fn device_token_is_redacted_in_debug() {
        let v = parse_devices(sample_devices_xml()).unwrap();
        let dbg = format!("{:?}", v[0]);
        assert!(!dbg.contains("token-for-server"), "leaked token: {dbg}");
        assert!(dbg.contains("***redacted***"));
    }

    #[test]
    fn timestamps_parsed_from_epoch_seconds() {
        let v = parse_devices(sample_devices_xml()).unwrap();
        let server = v.iter().find(|d| d.id == "1001").unwrap();
        let created = server.created_at.unwrap();
        // 2020-01-01T00:00:00Z = 1577836800.
        assert_eq!(created.timestamp(), 1_577_836_800);
    }

    #[test]
    fn empty_token_attribute_yields_none() {
        let xml = r#"<MediaContainer><Device
            id="1" product="X" clientIdentifier="abcd1234abcd1234abcd1234abcd1234abcd0099"
            provides="client" token=""/></MediaContainer>"#;
        let v = parse_devices(xml).unwrap();
        assert!(v[0].token.is_none());
    }

    #[test]
    fn empty_container_yields_empty_vec() {
        let xml = r"<MediaContainer></MediaContainer>";
        let v = parse_devices(xml).unwrap();
        assert!(v.is_empty());
    }

    #[test]
    fn malformed_xml_returns_xml_error() {
        let v = parse_devices("not xml");
        assert!(v.is_err());
    }

    #[test]
    fn provides_csv_trims_whitespace() {
        let xml = r#"<MediaContainer><Device
            id="1" product="X" clientIdentifier="abcd1234abcd1234abcd1234abcd1234abcd0099"
            provides="client, player , pubsub-player"/></MediaContainer>"#;
        let v = parse_devices(xml).unwrap();
        assert_eq!(v[0].provides, vec!["client", "player", "pubsub-player"]);
    }
}
