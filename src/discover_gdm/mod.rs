//! GDM — local-network Plex Media Server discovery.
//!
//! GDM (G'Day Mate) is Plex's local discovery protocol. **It is not
//! mDNS** (analysis/09 §2): it's raw UDP carrying an HTTP/1.0
//! `M-SEARCH` request to a known multicast group.
//!
//! - **Server discovery** (this module): client sends a UDP packet
//!   to `239.0.0.250:32414` with body `M-SEARCH * HTTP/1.0\r\n\r\n`.
//!   Each Plex Media Server reachable on the LAN replies (from its
//!   own ephemeral port) with an HTTP-style response carrying its
//!   `Resource-Identifier`, `Name`, `Port`, `Version`, and a few
//!   other identifying headers.
//! - **Client discovery** (deferred): the reverse — servers asking
//!   "are there any Plex players around?" — uses broadcast
//!   `255.255.255.255:32412` and a similar payload.
//!
//! Behind the `discovery` Cargo feature so callers that don't need
//! local discovery don't pull in `tokio::net::UdpSocket`.

#![cfg(feature = "discovery")]

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;

use tokio::net::UdpSocket;
use tokio::time::{Instant, timeout};

use crate::error::{Error, Result};

/// IPv4 multicast group GDM servers listen on.
const SERVER_GROUP: Ipv4Addr = Ipv4Addr::new(239, 0, 0, 250);
/// Port for the server-discovery group.
const SERVER_PORT: u16 = 32414;
/// Wire payload — note HTTP/1.0, not 1.1.
const M_SEARCH: &[u8] = b"M-SEARCH * HTTP/1.0\r\n\r\n";

// -----------------------------------------------------------------------------
// GdmEntry.
// -----------------------------------------------------------------------------

/// One Plex Media Server discovered on the LAN.
///
/// Constructed by [`discover_local_servers`] from each server's
/// reply. Headers Plex doesn't recognise are dropped — see the
/// [`Self::headers`] field for the raw key/value bag if you need
/// to inspect unknown fields.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct GdmEntry {
    /// Address the server replied from.
    pub source: SocketAddr,
    /// `Resource-Identifier` header — Plex's `machineIdentifier`.
    pub machine_identifier: Option<String>,
    /// `Name` — user-set server name.
    pub name: Option<String>,
    /// `Port` — the HTTP port the server listens on (almost always
    /// 32400 but plex.tv supports overrides).
    pub port: Option<u16>,
    /// `Version` — Plex Media Server build string.
    pub version: Option<String>,
    /// `Content-Type` — typically `plex/media-server`.
    pub content_type: Option<String>,
    /// `Updated-At` — epoch seconds of last server-side change.
    pub updated_at: Option<i64>,
    /// Every header key/value pair the server returned, lowercased
    /// keys.
    pub headers: HashMap<String, String>,
}

impl GdmEntry {
    /// Construct a probable PMS base URL by joining the source IP
    /// with [`Self::port`].
    #[must_use]
    pub fn base_url(&self) -> Option<url::Url> {
        let port = self.port?;
        let scheme = "http";
        let ip = self.source.ip();
        let host = match ip {
            IpAddr::V4(a) => a.to_string(),
            IpAddr::V6(a) => format!("[{a}]"),
        };
        url::Url::parse(&format!("{scheme}://{host}:{port}/")).ok()
    }
}

// -----------------------------------------------------------------------------
// Discovery.
// -----------------------------------------------------------------------------

/// Send a GDM `M-SEARCH` and collect replies for `timeout`.
///
/// Replies are deduplicated by `Resource-Identifier` — Plex servers
/// emit one reply per network interface, so a multi-homed PMS would
/// otherwise appear multiple times.
///
/// Typical `timeout` is `1–3 seconds`. Plex servers reply within
/// hundreds of milliseconds on a healthy LAN; the timeout caps how
/// long the caller waits for late arrivals.
///
/// # Errors
/// - [`Error::Transport`] is not used (no `reqwest`) — IO errors
///   land in [`Error::Config`] with an explanatory message.
pub async fn discover_local_servers(wait: Duration) -> Result<Vec<GdmEntry>> {
    // Bind to all interfaces on an ephemeral port. Setting
    // `multicast_ttl_v4(1)` keeps the packet on-link.
    let socket = UdpSocket::bind(SocketAddr::from(([0, 0, 0, 0], 0)))
        .await
        .map_err(|e| Error::Config(format!("gdm: bind failed: {e}")))?;
    socket
        .set_multicast_ttl_v4(1)
        .map_err(|e| Error::Config(format!("gdm: set_multicast_ttl_v4: {e}")))?;
    let dest = SocketAddr::from((SERVER_GROUP, SERVER_PORT));
    socket
        .send_to(M_SEARCH, dest)
        .await
        .map_err(|e| Error::Config(format!("gdm: send_to {dest}: {e}")))?;

    let deadline = Instant::now() + wait;
    let mut buf = vec![0u8; 4096];
    let mut by_machine: HashMap<String, GdmEntry> = HashMap::new();
    let mut anon_entries: Vec<GdmEntry> = Vec::new();

    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        match timeout(remaining, socket.recv_from(&mut buf)).await {
            Err(_elapsed) => break,
            Ok(Ok((len, source))) => {
                if let Some(entry) = parse_reply(&buf[..len], source) {
                    if let Some(mid) = entry.machine_identifier.clone() {
                        by_machine.insert(mid, entry);
                    } else {
                        anon_entries.push(entry);
                    }
                }
            }
            Ok(Err(_e)) => {
                // Recv error mid-flight — treat as no more replies.
                break;
            }
        }
    }

    let mut out: Vec<GdmEntry> = by_machine.into_values().collect();
    out.extend(anon_entries);
    Ok(out)
}

// -----------------------------------------------------------------------------
// Reply parsing.
// -----------------------------------------------------------------------------

/// Parse a single GDM reply packet. Returns `None` when the body
/// isn't recognisably HTTP-shaped.
fn parse_reply(bytes: &[u8], source: SocketAddr) -> Option<GdmEntry> {
    let text = std::str::from_utf8(bytes).ok()?;
    let mut lines = text.lines();
    let status = lines.next()?;
    // First line is like `HTTP/1.0 200 OK` — accept any 2xx.
    if !status.starts_with("HTTP/") {
        return None;
    }
    let mut headers: HashMap<String, String> = HashMap::new();
    for line in lines {
        if line.is_empty() {
            continue;
        }
        if let Some((k, v)) = line.split_once(':') {
            headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_owned());
        }
    }
    Some(GdmEntry {
        source,
        machine_identifier: headers.get("resource-identifier").cloned(),
        name: headers.get("name").cloned(),
        port: headers.get("port").and_then(|p| p.parse().ok()),
        version: headers.get("version").cloned(),
        content_type: headers.get("content-type").cloned(),
        updated_at: headers.get("updated-at").and_then(|s| s.parse().ok()),
        headers,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_source() -> SocketAddr {
        "192.168.1.50:54321".parse().unwrap()
    }

    #[test]
    fn parse_reply_extracts_canonical_headers() {
        let body = b"HTTP/1.0 200 OK\r\n\
Content-Type: plex/media-server\r\n\
Resource-Identifier: abcdef0123456789\r\n\
Name: Living Room\r\n\
Port: 32400\r\n\
Version: 1.40.0.1234-deadbeef\r\n\
Updated-At: 1700000000\r\n\
\r\n";
        let entry = parse_reply(body, fake_source()).unwrap();
        assert_eq!(
            entry.machine_identifier.as_deref(),
            Some("abcdef0123456789")
        );
        assert_eq!(entry.name.as_deref(), Some("Living Room"));
        assert_eq!(entry.port, Some(32400));
        assert_eq!(entry.version.as_deref(), Some("1.40.0.1234-deadbeef"));
        assert_eq!(entry.content_type.as_deref(), Some("plex/media-server"));
        assert_eq!(entry.updated_at, Some(1_700_000_000));
    }

    #[test]
    fn parse_reply_returns_none_for_non_http() {
        assert!(parse_reply(b"not an http reply", fake_source()).is_none());
        assert!(parse_reply(&[0u8, 0x80, 0xff], fake_source()).is_none()); // not utf8
    }

    #[test]
    fn parse_reply_tolerates_unknown_headers() {
        let body = b"HTTP/1.0 200 OK\r\nX-Future: hello\r\n\r\n";
        let entry = parse_reply(body, fake_source()).unwrap();
        assert_eq!(
            entry.headers.get("x-future").map(String::as_str),
            Some("hello")
        );
        assert!(entry.machine_identifier.is_none());
    }

    #[test]
    fn base_url_constructs_from_port() {
        let body = b"HTTP/1.0 200 OK\r\nPort: 32400\r\n\r\n";
        let entry = parse_reply(body, fake_source()).unwrap();
        let url = entry.base_url().unwrap();
        assert_eq!(url.host_str(), Some("192.168.1.50"));
        assert_eq!(url.port(), Some(32400));
        assert_eq!(url.scheme(), "http");
    }

    #[test]
    fn base_url_returns_none_when_port_missing() {
        let body = b"HTTP/1.0 200 OK\r\nName: anon\r\n\r\n";
        let entry = parse_reply(body, fake_source()).unwrap();
        assert!(entry.base_url().is_none());
    }
}
