//! Plex URI scheme handling.
//!
//! Plex uses URI strings as opaque references in query parameters and
//! as the persisted `content` field on smart playlists and
//! collections. From the inventory in
//! [`analysis/07-playback-and-playlists.md`](../analysis/07-playback-and-playlists.md)
//! §8 there are seven distinct shapes. [`PlexUri`] models all of
//! them as a typed enum so the wire form can be discriminated at
//! parse time and reconstructed via [`Display`](fmt::Display) without losing
//! round-trip stability.
//!
//! ```text
//! server://<machineId>/com.plexapp.plugins.library/library/metadata/12345
//! library://<sectionUUID>/item/library/metadata/12345
//! library:///directory/<urlencoded(path)>
//! playlist:///<urlencoded(guid)>
//! /playQueues/<id>[?own=1&window=N]               (containerKey, not a URI scheme)
//! https://plex.tv/devices/<clientId>/sync_items[/<id>]
//! /security/token?type=delegation&scope=all       (security token endpoint)
//! ```
//!
//! Note that the last three aren't strictly URI schemes — they're
//! paths / absolute URLs / endpoints — but `python-plexapi` treats
//! them as interchangeable references in `containerKey`-shaped
//! parameters, so we collapse them into the same enum to make
//! round-tripping symmetric.

use std::fmt;
use std::str::FromStr;

use uuid::Uuid;

use crate::error::Error;
use crate::util::ids::{ClientIdentifier, MachineIdentifier, PlayQueueId, RatingKey};

/// Typed Plex URI.
///
/// Constructed via [`PlexUri::parse`] (or `str::parse`) and rendered
/// with [`Display`](fmt::Display). Round-trip stability is exercised by unit tests.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PlexUri {
    /// `server://<machineId>/com.plexapp.plugins.library<key>`.
    ///
    /// The most common form — references any library item on a
    /// specific PMS. `key` is typically `/library/metadata/<rk>`,
    /// possibly comma-joined for multi-item references.
    ///
    /// Cites: `playqueue.py:185`, `playlist.py:360`, `collection.py:435`.
    Server {
        /// The target server's stable identifier.
        machine_id: MachineIdentifier,
        /// The library key, including the leading slash.
        key: String,
    },

    /// `library://<sectionUUID>/item/library/metadata/<rk>`.
    ///
    /// Single item by section UUID; used for `PlayQueue` mutations.
    ///
    /// Cites: `playqueue.py:251-252`.
    LibraryItem {
        /// The owning library section's UUID.
        section_uuid: Uuid,
        /// The item's rating key.
        rating_key: RatingKey,
    },

    /// `library:///directory/<urlencoded(path)>` (three-slash form).
    ///
    /// A directory — comma-joined metadata list or collection children.
    ///
    /// Cites: `playqueue.py:176-177`, `collection.py:536-537`.
    LibraryDirectory {
        /// URL-decoded path that was encoded into the URI.
        path: String,
    },

    /// `playlist:///<urlencoded(guid)>`.
    ///
    /// Playlist GUID, used for sync.
    ///
    /// Cites: `playlist.py:495`.
    Playlist {
        /// URL-decoded playlist GUID.
        guid: String,
    },

    /// `/playQueues/<id>[?own=0|1&window=N]` — used as `containerKey`.
    ///
    /// Cites: `client.py:520`, `sonos.py:100`.
    PlayQueueContainer {
        /// The play-queue identifier.
        play_queue_id: PlayQueueId,
        /// Whether the client owns the queue (`own=1`).
        own: bool,
        /// Optional window size in items.
        window: Option<u32>,
    },

    /// `https://plex.tv/devices/<clientId>/sync_items[/<id>]`.
    ///
    /// plex.tv sync queue.
    ///
    /// Cites: `sync.py:116-128`, `sync.py:103-105`.
    Device {
        /// Caller's client identifier.
        client_id: ClientIdentifier,
        /// Optional specific sync-item id.
        item: Option<u64>,
    },

    /// Server's `/security/token?type=delegation&scope=all` minted
    /// scoped-delegation token, wrapped so it can be embedded into
    /// player commands without being mistaken for a regular path.
    ///
    /// Cites: `server.py:229-235`.
    SecurityToken(String),
}

// -----------------------------------------------------------------------------
// Parser.
// -----------------------------------------------------------------------------

impl PlexUri {
    /// Parse a Plex URI from a string.
    ///
    /// # Errors
    /// Returns [`Error::Config`] with a descriptive message when the
    /// input does not match any known shape. Failure messages do not
    /// echo the full input verbatim — they include only structural
    /// hints — so logging a parse error cannot leak full keys.
    pub fn parse(input: &str) -> Result<Self, Error> {
        // Order matters: more-specific prefixes must be checked before
        // less-specific ones (e.g. `library:///directory/` before
        // `library://`).
        if let Some(rest) = input.strip_prefix("server://") {
            return parse_server(rest);
        }
        if let Some(rest) = input.strip_prefix("library:///directory/") {
            let decoded = urldecode(rest);
            return Ok(Self::LibraryDirectory { path: decoded });
        }
        if let Some(rest) = input.strip_prefix("library://") {
            return parse_library_anchored(rest);
        }
        if let Some(rest) = input.strip_prefix("playlist:///") {
            let decoded = urldecode(rest);
            return Ok(Self::Playlist { guid: decoded });
        }
        if let Some(rest) = input.strip_prefix("https://plex.tv/devices/") {
            return parse_device(rest);
        }
        if let Some(rest) = input.strip_prefix("/playQueues/") {
            return parse_play_queue_container(rest);
        }
        if let Some(rest) = input.strip_prefix("/security/token") {
            // `?type=delegation&scope=all` or any other query is preserved.
            return Ok(Self::SecurityToken(rest.to_owned()));
        }
        Err(Error::Config(format!(
            "unrecognised Plex URI shape (prefix={:?})",
            head(input, 24)
        )))
    }
}

fn parse_server(rest: &str) -> Result<PlexUri, Error> {
    // Expected: <machineId>/com.plexapp.plugins.library<key>
    let (machine, tail) = rest
        .split_once('/')
        .ok_or_else(|| Error::Config("server:// missing machine identifier".to_owned()))?;
    let key_after_provider = tail
        .strip_prefix("com.plexapp.plugins.library")
        .ok_or_else(|| {
            Error::Config("server:// missing 'com.plexapp.plugins.library'".to_owned())
        })?;
    Ok(PlexUri::Server {
        machine_id: MachineIdentifier::new(machine.to_owned())?,
        key: key_after_provider.to_owned(),
    })
}

fn parse_library_anchored(rest: &str) -> Result<PlexUri, Error> {
    // Expected: <uuid>/item/library/metadata/<ratingKey>
    let (uuid_str, after) = rest
        .split_once('/')
        .ok_or_else(|| Error::Config("library:// missing section UUID".to_owned()))?;
    let suffix = after
        .strip_prefix("item/library/metadata/")
        .ok_or_else(|| {
            Error::Config("library:// missing item/library/metadata/ segment".to_owned())
        })?;
    let rk: u64 = suffix
        .parse()
        .map_err(|e| Error::Config(format!("library:// invalid rating key: {e}")))?;
    let uuid = Uuid::parse_str(uuid_str)
        .map_err(|e| Error::Config(format!("library:// invalid section UUID: {e}")))?;
    Ok(PlexUri::LibraryItem {
        section_uuid: uuid,
        rating_key: RatingKey(rk),
    })
}

fn parse_device(rest: &str) -> Result<PlexUri, Error> {
    // Expected: <clientId>/sync_items[/<id>]
    let (client, tail) = rest
        .split_once('/')
        .ok_or_else(|| Error::Config("device URI missing client identifier".to_owned()))?;
    if let Some(item_str) = tail.strip_prefix("sync_items/") {
        let item: u64 = item_str
            .parse()
            .map_err(|e| Error::Config(format!("device sync_items id not numeric: {e}")))?;
        Ok(PlexUri::Device {
            client_id: ClientIdentifier::new(client.to_owned())?,
            item: Some(item),
        })
    } else if tail == "sync_items" {
        Ok(PlexUri::Device {
            client_id: ClientIdentifier::new(client.to_owned())?,
            item: None,
        })
    } else {
        Err(Error::Config(
            "device URI tail must be 'sync_items' or 'sync_items/<id>'".to_owned(),
        ))
    }
}

fn parse_play_queue_container(rest: &str) -> Result<PlexUri, Error> {
    // Expected: <id>[?own=0|1[&window=N]]
    let (id_str, query) = rest.split_once('?').unwrap_or((rest, ""));
    let play_queue_id = PlayQueueId(
        id_str
            .parse::<u64>()
            .map_err(|e| Error::Config(format!("/playQueues/<id> not numeric: {e}")))?,
    );
    let mut own = false;
    let mut window = None;
    for pair in query.split('&').filter(|s| !s.is_empty()) {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        match k {
            "own" => {
                own = matches!(v, "1" | "true");
            }
            "window" => {
                let n: u32 = v
                    .parse()
                    .map_err(|e| Error::Config(format!("window not numeric: {e}")))?;
                window = Some(n);
            }
            _ => {
                // Forward-compat: ignore unknown keys silently.
            }
        }
    }
    Ok(PlexUri::PlayQueueContainer {
        play_queue_id,
        own,
        window,
    })
}

// -----------------------------------------------------------------------------
// Display.
// -----------------------------------------------------------------------------

impl fmt::Display for PlexUri {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Server { machine_id, key } => {
                write!(f, "server://{machine_id}/com.plexapp.plugins.library{key}")
            }
            Self::LibraryItem {
                section_uuid,
                rating_key,
            } => write!(
                f,
                "library://{section_uuid}/item/library/metadata/{rating_key}",
            ),
            Self::LibraryDirectory { path } => {
                write!(f, "library:///directory/{}", urlencode(path))
            }
            Self::Playlist { guid } => write!(f, "playlist:///{}", urlencode(guid)),
            Self::PlayQueueContainer {
                play_queue_id,
                own,
                window,
            } => {
                let own_v = u8::from(*own);
                write!(f, "/playQueues/{play_queue_id}?own={own_v}")?;
                if let Some(w) = window {
                    write!(f, "&window={w}")?;
                }
                Ok(())
            }
            Self::Device { client_id, item } => {
                write!(f, "https://plex.tv/devices/{client_id}/sync_items")?;
                if let Some(id) = item {
                    write!(f, "/{id}")?;
                }
                Ok(())
            }
            Self::SecurityToken(tail) => write!(f, "/security/token{tail}"),
        }
    }
}

impl FromStr for PlexUri {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

// -----------------------------------------------------------------------------
// Minimal URL-encode / -decode for the handful of unreserved-ASCII
// characters Plex actually emits in `library:///directory/<...>` and
// `playlist:///<...>` paths. The full `url` crate is overkill here and
// would couple us to a percent-encoding charset that isn't quite what
// Plex uses.
// -----------------------------------------------------------------------------

fn urlencode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.bytes() {
        if matches!(byte, b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/')
        {
            out.push(byte as char);
        } else {
            out.push('%');
            // Hex-uppercase, matching python's urllib.parse.quote default.
            out.push(hex_nibble(byte >> 4));
            out.push(hex_nibble(byte & 0x0F));
        }
    }
    out
}

fn urldecode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (hex_value(bytes[i + 1]), hex_value(bytes[i + 2])) {
                out.push((hi << 4) | lo);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    // Best-effort: if the decoded bytes aren't valid UTF-8, fall back
    // to the raw input. Plex paths are ASCII in practice.
    String::from_utf8(out).unwrap_or_else(|_| input.to_owned())
}

const fn hex_nibble(n: u8) -> char {
    let c = match n {
        0..=9 => b'0' + n,
        10..=15 => b'A' + (n - 10),
        _ => b'?', // unreachable for n < 16
    };
    c as char
}

const fn hex_value(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn head(s: &str, n: usize) -> &str {
    let mut end = n.min(s.len());
    while !s.is_char_boundary(end) && end > 0 {
        end -= 1;
    }
    &s[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    const MACHINE: &str = "0123456789abcdef0123456789abcdef01234567";
    const SECTION_UUID: &str = "550e8400-e29b-41d4-a716-446655440000";

    fn round_trip(s: &str) -> PlexUri {
        let parsed = PlexUri::parse(s).unwrap_or_else(|e| panic!("parse {s:?} failed: {e}"));
        assert_eq!(parsed.to_string(), s, "round-trip mismatch for {s:?}");
        parsed
    }

    // ---------- server:// ----------

    #[test]
    fn server_uri_round_trips() {
        let uri = round_trip(&format!(
            "server://{MACHINE}/com.plexapp.plugins.library/library/metadata/12345"
        ));
        match uri {
            PlexUri::Server { machine_id, key } => {
                assert_eq!(machine_id.as_str(), MACHINE);
                assert_eq!(key, "/library/metadata/12345");
            }
            other => panic!("expected Server, got {other:?}"),
        }
    }

    #[test]
    fn server_uri_supports_comma_joined_keys() {
        let s = format!("server://{MACHINE}/com.plexapp.plugins.library/library/metadata/1,2,3");
        round_trip(&s);
    }

    #[test]
    fn server_uri_missing_provider_segment_fails() {
        let err = PlexUri::parse(&format!("server://{MACHINE}/something-else")).unwrap_err();
        assert!(matches!(err, Error::Config(_)));
    }

    // ---------- library:// ----------

    #[test]
    fn library_item_round_trips() {
        let uri = round_trip(&format!(
            "library://{SECTION_UUID}/item/library/metadata/9876"
        ));
        match uri {
            PlexUri::LibraryItem {
                section_uuid,
                rating_key,
            } => {
                assert_eq!(section_uuid.to_string(), SECTION_UUID);
                assert_eq!(rating_key.get(), 9876);
            }
            other => panic!("expected LibraryItem, got {other:?}"),
        }
    }

    #[test]
    fn library_item_rejects_bad_uuid() {
        let err = PlexUri::parse("library://not-a-uuid/item/library/metadata/1").unwrap_err();
        assert!(matches!(err, Error::Config(_)));
    }

    #[test]
    fn library_directory_round_trips_simple_path() {
        let uri = round_trip("library:///directory/some/path");
        match uri {
            PlexUri::LibraryDirectory { path } => assert_eq!(path, "some/path"),
            other => panic!("expected LibraryDirectory, got {other:?}"),
        }
    }

    #[test]
    fn library_directory_decodes_percent_escapes() {
        // %20 -> space, %2C -> ','
        let uri = PlexUri::parse("library:///directory/a%20b%2Cc").unwrap();
        match &uri {
            PlexUri::LibraryDirectory { path } => assert_eq!(path, "a b,c"),
            other => panic!("expected LibraryDirectory, got {other:?}"),
        }
        // And the re-encoded form is canonical (uppercase hex).
        assert_eq!(uri.to_string(), "library:///directory/a%20b%2Cc");
    }

    // ---------- playlist:// ----------

    #[test]
    fn playlist_uri_round_trips() {
        round_trip("playlist:///plex-playlist-guid-001");
    }

    // ---------- /playQueues ----------

    #[test]
    fn play_queue_container_round_trips_own_only() {
        let uri = round_trip("/playQueues/12345?own=1");
        match uri {
            PlexUri::PlayQueueContainer {
                play_queue_id,
                own,
                window,
            } => {
                assert_eq!(play_queue_id.get(), 12345);
                assert!(own);
                assert!(window.is_none());
            }
            other => panic!("expected PlayQueueContainer, got {other:?}"),
        }
    }

    #[test]
    fn play_queue_container_round_trips_with_window() {
        let uri = round_trip("/playQueues/42?own=0&window=20");
        match uri {
            PlexUri::PlayQueueContainer {
                play_queue_id,
                own,
                window,
            } => {
                assert_eq!(play_queue_id.get(), 42);
                assert!(!own);
                assert_eq!(window, Some(20));
            }
            other => panic!("expected PlayQueueContainer, got {other:?}"),
        }
    }

    #[test]
    fn play_queue_container_ignores_unknown_query_keys() {
        // Forward-compat: extra keys parse without error.
        let uri = PlexUri::parse("/playQueues/7?own=1&future=x").unwrap();
        assert!(matches!(uri, PlexUri::PlayQueueContainer { .. }));
    }

    // ---------- device ----------

    #[test]
    fn device_uri_without_item_round_trips() {
        let uri = round_trip("https://plex.tv/devices/my-client-uuid/sync_items");
        match uri {
            PlexUri::Device { client_id, item } => {
                assert_eq!(client_id.as_str(), "my-client-uuid");
                assert!(item.is_none());
            }
            other => panic!("expected Device, got {other:?}"),
        }
    }

    #[test]
    fn device_uri_with_item_round_trips() {
        let uri = round_trip("https://plex.tv/devices/my-client-uuid/sync_items/42");
        match uri {
            PlexUri::Device { client_id: _, item } => assert_eq!(item, Some(42)),
            other => panic!("expected Device, got {other:?}"),
        }
    }

    #[test]
    fn device_uri_rejects_unknown_tail() {
        let err = PlexUri::parse("https://plex.tv/devices/cid/something-else").unwrap_err();
        assert!(matches!(err, Error::Config(_)));
    }

    // ---------- /security/token ----------

    #[test]
    fn security_token_round_trips_with_query() {
        round_trip("/security/token?type=delegation&scope=all");
    }

    #[test]
    fn security_token_round_trips_without_query() {
        round_trip("/security/token");
    }

    // ---------- error paths ----------

    #[test]
    fn parse_rejects_unknown_scheme() {
        let err = PlexUri::parse("imap://nope").unwrap_err();
        assert!(matches!(err, Error::Config(_)));
    }

    #[test]
    fn parse_rejects_garbage() {
        assert!(PlexUri::parse("not a uri at all").is_err());
        assert!(PlexUri::parse("").is_err());
    }

    #[test]
    fn parse_via_from_str() {
        let _: PlexUri = "/playQueues/1?own=1".parse().unwrap();
    }

    // ---------- url encoding ----------

    #[test]
    fn urlencode_passes_through_unreserved() {
        assert_eq!(urlencode("abc-_.~/123"), "abc-_.~/123");
    }

    #[test]
    fn urlencode_percent_escapes_space_and_comma() {
        assert_eq!(urlencode("a b,c"), "a%20b%2Cc");
    }

    #[test]
    fn urldecode_handles_mixed_case_hex() {
        assert_eq!(urldecode("a%20b%2cC"), "a b,C");
        assert_eq!(urldecode("a%20b%2CC"), "a b,C");
    }

    #[test]
    fn urldecode_leaves_dangling_percent() {
        assert_eq!(urldecode("100%"), "100%");
    }
}
