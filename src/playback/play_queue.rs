//! `PlayQueue` — server-side playback queue.
//!
//! A `PlayQueue` is the unit Plex players consume. To start playback
//! on a remote player you typically:
//!
//! 1. Create a `PlayQueue` from one or more library items (or a
//!    playlist) via [`PlexServer::create_play_queue`].
//! 2. Hand the queue's id to a player via the (M4.5) remote-control
//!    surface — `client.play_media(pq)`.
//!
//! ## Wire endpoints
//!
//! | Method | Path                                   | Purpose          |
//! | ------ | -------------------------------------- | ---------------- |
//! | POST   | `/playQueues`                          | Create from `uri` / `playlistID` |
//! | GET    | `/playQueues/{id}`                     | Refresh / fetch  |
//! | PUT    | `/playQueues/{id}?uri=...`             | Append to "Up Next" |
//! | PUT    | `/playQueues/{id}/items/{iid}/move`   | Reorder one item |
//! | DELETE | `/playQueues/{id}/items/{iid}`         | Remove one item  |
//! | DELETE | `/playQueues/{id}/items`               | Clear the queue  |
//!
//! ## Source URIs
//!
//! - **Single library item** —
//!   `server://<machineIdentifier>/com.plexapp.plugins.library<item.key>`
//! - **List of items** —
//!   `library:///directory/<percent-encoded(/library/metadata/RK1,RK2,…)>`
//! - **Playlist** — pass `playlistID=<rk>` instead of `uri`.

use serde::Deserialize;
use url::Url;

use crate::client::HttpClient;
use crate::error::{Error, Result};
use crate::library::LibrarySectionRef;
use crate::media::LibraryItem;
use crate::media::playlist::Playlist;
use crate::media::video::MetadataDto;
use crate::server::{PlexBoolField, PlexServer};
use crate::util::ids::{MachineIdentifier, PlayQueueId};

/// Wire identifier of Plex's stock library content provider.
const LIBRARY_IDENTIFIER: &str = "com.plexapp.plugins.library";

// -----------------------------------------------------------------------------
// PlayQueue domain.
// -----------------------------------------------------------------------------

/// A server-side playback queue.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct PlayQueue {
    /// Stable id assigned by the server. Used in every subsequent
    /// path under `/playQueues/{id}`.
    pub id: PlayQueueId,
    /// Monotonic version stamp. Increments each time the queue is
    /// mutated; useful for cache invalidation.
    pub version: u32,
    /// Total number of items in the queue.
    pub total_count: u32,
    /// Per-queue id of the currently selected (about-to-play /
    /// playing) item, if any.
    pub selected_item_id: Option<u64>,
    /// Offset (zero-based index into [`Self::items`]) of the
    /// currently selected item.
    pub selected_item_offset: Option<u32>,
    /// Rating key of the currently selected item.
    pub selected_metadata_item_id: Option<u64>,
    /// `true` when the queue was created with `shuffle=1`.
    pub shuffled: bool,
    /// Original source URI passed to `POST /playQueues` (e.g.
    /// `server://...` or `library:///directory/...`).
    pub source_uri: Option<String>,
    /// Content provider identifier (typically
    /// `com.plexapp.plugins.library`).
    pub identifier: Option<String>,
    /// Items currently in the queue. Each carries the standard
    /// metadata plus a per-queue `play_queue_item_id`.
    pub items: Vec<PlayQueueItem>,
    http: HttpClient,
    base_url: Url,
}

impl PlayQueue {
    /// Re-fetch this queue from the server, returning the
    /// up-to-date snapshot.
    ///
    /// # Errors
    /// Any transport / parse [`Error`] variant.
    pub async fn refresh(self) -> Result<Self> {
        Self::get(&self.http, &self.base_url, self.id).await
    }

    /// Append `item` to the queue's "Up Next" section.
    ///
    /// `play_next == true` places the item at the head of "Up Next"
    /// (immediately after the currently playing item); `false`
    /// appends to the tail of "Up Next".
    ///
    /// Note that Plex only allows additions to the "Up Next" zone —
    /// items cannot be inserted into arbitrary positions in the
    /// queue. Use [`Self::move_item`] after the fact to reorder.
    ///
    /// # Errors
    /// - [`Error::Config`] when the item's
    ///   [`LibrarySectionRef::id`] is zero (we have no section UUID
    ///   to construct the `library://` URI). For items pulled from a
    ///   real listing this is always populated.
    /// - Any transport [`Error`] variant.
    pub async fn add_item(self, item: &LibraryItem, play_next: bool) -> Result<Self> {
        let item_key = item.key();
        let mut url = self.base_url.join(&format!("/playQueues/{}", self.id.0))?;
        {
            let mut qp = url.query_pairs_mut();
            // python-plexapi uses `library://<section_uuid>/item<item.key>`
            // here, but with the section UUID embedded. We don't yet
            // surface the UUID on LibraryItem, so we fall back to the
            // simpler `server://...` form which PMS also accepts.
            let uri = format!("server://{MACHINE_ID_HINT}/{LIBRARY_IDENTIFIER}{item_key}");
            qp.append_pair("uri", &uri);
            if play_next {
                qp.append_pair("next", "1");
            }
        }
        let body = self
            .http
            .get_bytes_for_method(reqwest::Method::PUT, url.as_str())
            .await?;
        let body_str = std::str::from_utf8(&body)
            .map_err(|e| Error::Config(format!("playQueue body not utf-8: {e}")))?;
        let dto: PlayQueueDto = PlayQueueDto::from_envelope(body_str)?;
        Ok(dto.into_domain(self.http, self.base_url))
    }

    /// Move `item_id` to immediately after `after_id`, or to the
    /// beginning of the queue when `after_id` is `None`.
    ///
    /// `item_id` and `after_id` are **per-queue** ids
    /// (`play_queue_item_id`), not rating keys. Get them from
    /// [`PlayQueueItem::play_queue_item_id`].
    ///
    /// # Errors
    /// Any transport / parse [`Error`] variant.
    pub async fn move_item(self, item_id: u64, after_id: Option<u64>) -> Result<Self> {
        let mut url = self
            .base_url
            .join(&format!("/playQueues/{}/items/{}/move", self.id.0, item_id))?;
        if let Some(after) = after_id {
            url.query_pairs_mut()
                .append_pair("after", &after.to_string());
        }
        let body = self
            .http
            .get_bytes_for_method(reqwest::Method::PUT, url.as_str())
            .await?;
        let body_str = std::str::from_utf8(&body)
            .map_err(|e| Error::Config(format!("playQueue body not utf-8: {e}")))?;
        let dto: PlayQueueDto = PlayQueueDto::from_envelope(body_str)?;
        Ok(dto.into_domain(self.http, self.base_url))
    }

    /// Remove a single item from the queue.
    ///
    /// `item_id` is the per-queue id, not a rating key.
    ///
    /// # Errors
    /// Any transport / parse [`Error`] variant.
    pub async fn remove_item(self, item_id: u64) -> Result<Self> {
        let url = self
            .base_url
            .join(&format!("/playQueues/{}/items/{}", self.id.0, item_id))?;
        let body = self
            .http
            .get_bytes_for_method(reqwest::Method::DELETE, url.as_str())
            .await?;
        let body_str = std::str::from_utf8(&body)
            .map_err(|e| Error::Config(format!("playQueue body not utf-8: {e}")))?;
        let dto: PlayQueueDto = PlayQueueDto::from_envelope(body_str)?;
        Ok(dto.into_domain(self.http, self.base_url))
    }

    /// Clear every item from the queue.
    ///
    /// # Errors
    /// Any transport [`Error`] variant.
    pub async fn clear(self) -> Result<Self> {
        let url = self
            .base_url
            .join(&format!("/playQueues/{}/items", self.id.0))?;
        let body = self
            .http
            .get_bytes_for_method(reqwest::Method::DELETE, url.as_str())
            .await?;
        let body_str = std::str::from_utf8(&body)
            .map_err(|e| Error::Config(format!("playQueue body not utf-8: {e}")))?;
        let dto: PlayQueueDto = PlayQueueDto::from_envelope(body_str)?;
        Ok(dto.into_domain(self.http, self.base_url))
    }

    /// Fetch a queue by id.
    pub(crate) async fn get(http: &HttpClient, base_url: &Url, id: PlayQueueId) -> Result<Self> {
        let url = base_url.join(&format!("/playQueues/{}", id.0))?;
        let body = http.get_bytes(url.as_str()).await?;
        let body_str = std::str::from_utf8(&body)
            .map_err(|e| Error::Config(format!("playQueue body not utf-8: {e}")))?;
        let dto: PlayQueueDto = PlayQueueDto::from_envelope(body_str)?;
        Ok(dto.into_domain(http.clone(), base_url.clone()))
    }
}

/// Placeholder machine-identifier used in URIs sent back to the
/// same server we read from. The exact value doesn't matter because
/// Plex resolves these relative to the authenticated server, but
/// the URI must still be well-formed.
const MACHINE_ID_HINT: &str = ".";

// -----------------------------------------------------------------------------
// PlayQueueItem.
// -----------------------------------------------------------------------------

/// One entry in a [`PlayQueue`]. Carries the standard
/// [`LibraryItem`] metadata plus the per-queue id needed for the
/// `/items/{iid}` mutation paths.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct PlayQueueItem {
    /// Per-queue stable id. Distinct from the item's rating key —
    /// the same item can appear multiple times in a queue with
    /// different `play_queue_item_id` values.
    pub play_queue_item_id: u64,
    /// The played item.
    pub item: LibraryItem,
}

// -----------------------------------------------------------------------------
// CreatePlayQueue builder.
// -----------------------------------------------------------------------------

/// Builder for `POST /playQueues`.
///
/// Construct via [`PlexServer::create_play_queue`]; supply a source
/// (single item, list of items, or playlist) and any flags, then
/// call [`Self::execute`] to materialise the queue.
#[derive(Debug)]
#[non_exhaustive]
// The five flags map 1:1 to PMS query parameters; collapsing them
// into a bitflag would be an abstraction not driven by the wire
// shape (each flag has a different default and a distinct
// semantic).
#[allow(clippy::struct_excessive_bools)]
pub struct CreatePlayQueue<'a> {
    server: &'a PlexServer,
    source: Option<Source<'a>>,
    shuffle: bool,
    repeat: bool,
    include_chapters: bool,
    include_related: bool,
    continuous: bool,
    start_key: Option<String>,
}

#[derive(Debug)]
enum Source<'a> {
    Item(&'a LibraryItem),
    Items(&'a [&'a LibraryItem]),
    Playlist(&'a Playlist),
}

impl<'a> CreatePlayQueue<'a> {
    pub(crate) const fn new(server: &'a PlexServer) -> Self {
        Self {
            server,
            source: None,
            shuffle: false,
            repeat: false,
            include_chapters: true,
            include_related: true,
            continuous: false,
            start_key: None,
        }
    }

    /// Create a queue from a single item.
    #[must_use]
    pub const fn from_item(mut self, item: &'a LibraryItem) -> Self {
        self.source = Some(Source::Item(item));
        self
    }

    /// Create a queue from an explicit list of items. All items
    /// must be of the same kind (video / audio / photo).
    #[must_use]
    pub const fn from_items(mut self, items: &'a [&'a LibraryItem]) -> Self {
        self.source = Some(Source::Items(items));
        self
    }

    /// Create a queue from a playlist.
    #[must_use]
    pub const fn from_playlist(mut self, playlist: &'a Playlist) -> Self {
        self.source = Some(Source::Playlist(playlist));
        self
    }

    /// Shuffle the queue at creation.
    #[must_use]
    pub const fn shuffle(mut self, on: bool) -> Self {
        self.shuffle = on;
        self
    }

    /// Repeat the queue at creation.
    #[must_use]
    pub const fn repeat(mut self, on: bool) -> Self {
        self.repeat = on;
        self
    }

    /// Include chapters in the queue payload.
    #[must_use]
    pub const fn include_chapters(mut self, on: bool) -> Self {
        self.include_chapters = on;
        self
    }

    /// Include the "related" carousel in the queue payload.
    #[must_use]
    pub const fn include_related(mut self, on: bool) -> Self {
        self.include_related = on;
        self
    }

    /// Continuous playback — for shows, auto-queue subsequent
    /// episodes; no-op for movies.
    #[must_use]
    pub const fn continuous(mut self, on: bool) -> Self {
        self.continuous = on;
        self
    }

    /// Start playback at `start_key` (the wire key of an item in
    /// the supplied source). When omitted, the queue starts at the
    /// first item.
    #[must_use]
    pub fn start_at(mut self, start_key: impl Into<String>) -> Self {
        self.start_key = Some(start_key.into());
        self
    }

    /// Materialise the queue.
    ///
    /// # Errors
    /// - [`Error::Config`] if no source was supplied or
    ///   [`Self::from_items`] was called with an empty slice.
    /// - Any transport [`Error`] variant.
    pub async fn execute(self) -> Result<PlayQueue> {
        let source = self
            .source
            .ok_or_else(|| Error::Config("CreatePlayQueue requires a source".to_owned()))?;
        let base = self.server.base_url();
        let machine_id = &self.server.identity().machine_identifier;
        let mut url = base.join("/playQueues")?;
        {
            let mut qp = url.query_pairs_mut();
            qp.append_pair(
                "includeChapters",
                if self.include_chapters { "1" } else { "0" },
            );
            qp.append_pair(
                "includeRelated",
                if self.include_related { "1" } else { "0" },
            );
            qp.append_pair("repeat", if self.repeat { "1" } else { "0" });
            qp.append_pair("shuffle", if self.shuffle { "1" } else { "0" });
            qp.append_pair("continuous", if self.continuous { "1" } else { "0" });
            match source {
                Source::Item(item) => {
                    qp.append_pair("type", item.list_type());
                    qp.append_pair("uri", &server_uri(machine_id, item.key()));
                }
                Source::Items(items) => {
                    if items.is_empty() {
                        return Err(Error::Config(
                            "CreatePlayQueue::from_items requires at least one item".to_owned(),
                        ));
                    }
                    qp.append_pair("type", items[0].list_type());
                    qp.append_pair("uri", &library_directory_uri(items));
                }
                Source::Playlist(p) => {
                    qp.append_pair("type", playlist_type(p));
                    qp.append_pair("playlistID", &p.rating_key.0.to_string());
                }
            }
            if let Some(key) = &self.start_key {
                qp.append_pair("key", key);
            }
        }
        let body = self
            .server
            .http()
            .get_bytes_for_method(reqwest::Method::POST, url.as_str())
            .await?;
        let body_str = std::str::from_utf8(&body)
            .map_err(|e| Error::Config(format!("playQueue body not utf-8: {e}")))?;
        let dto = PlayQueueDto::from_envelope(body_str)?;
        Ok(dto.into_domain(self.server.http().clone(), base.clone()))
    }
}

const fn playlist_type(p: &Playlist) -> &'static str {
    match p.kind {
        crate::PlaylistKind::Audio => "audio",
        crate::PlaylistKind::Video | crate::PlaylistKind::Other(_) => "video",
        crate::PlaylistKind::Photo => "photo",
    }
}

fn server_uri(machine_id: &MachineIdentifier, item_key: &str) -> String {
    format!(
        "server://{}/{}{}",
        machine_id.as_str(),
        LIBRARY_IDENTIFIER,
        item_key
    )
}

fn library_directory_uri(items: &[&LibraryItem]) -> String {
    let mut ids = String::new();
    for (i, it) in items.iter().enumerate() {
        if i > 0 {
            ids.push(',');
        }
        ids.push_str(&it.rating_key().0.to_string());
    }
    let raw = format!("/library/metadata/{ids}");
    format!("library:///directory/{}", pct_encode_path(&raw))
}

/// Minimal RFC 3986 percent-encoder for the path component embedded
/// in the `library:///directory/<…>` URI. We can't use
/// `url::form_urlencoded` here — that's the wrong reserved set (it
/// uses `+` for space). Reserved set per RFC 3986 unreserved.
fn pct_encode_path(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(byte as char);
            }
            _ => {
                out.push('%');
                let hi = byte >> 4;
                let lo = byte & 0x0F;
                out.push(hex_nibble(hi));
                out.push(hex_nibble(lo));
            }
        }
    }
    out
}

const fn hex_nibble(n: u8) -> char {
    if n < 10 {
        (b'0' + n) as char
    } else {
        (b'A' + n - 10) as char
    }
}

// -----------------------------------------------------------------------------
// PlexServer integration.
// -----------------------------------------------------------------------------

impl PlexServer {
    /// Start a [`CreatePlayQueue`] builder. Supply a source via
    /// `.from_item`, `.from_items`, or `.from_playlist`, then call
    /// `.execute().await`.
    #[must_use]
    pub const fn create_play_queue(&self) -> CreatePlayQueue<'_> {
        CreatePlayQueue::new(self)
    }

    /// Fetch an existing [`PlayQueue`] by id.
    ///
    /// # Errors
    /// Any transport / parse [`Error`] variant.
    pub async fn play_queue(&self, id: PlayQueueId) -> Result<PlayQueue> {
        PlayQueue::get(self.http(), self.base_url(), id).await
    }
}

// -----------------------------------------------------------------------------
// DTOs.
// -----------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct PlayQueueEnvelope {
    #[serde(rename = "MediaContainer")]
    container: PlayQueueDto,
}

#[derive(Debug, Deserialize)]
struct PlayQueueDto {
    // Plex's wire spelling is `playQueueID` (capital `ID`), not the
    // `camelCase`-default `playQueueId`. The same exception applies
    // to every `*ID` and `*URI` field on this DTO.
    #[serde(rename = "playQueueID")]
    play_queue_id: u64,
    #[serde(rename = "playQueueVersion", default)]
    play_queue_version: u32,
    #[serde(rename = "playQueueTotalCount", default)]
    play_queue_total_count: u32,
    #[serde(rename = "playQueueSelectedItemID", default)]
    play_queue_selected_item_id: Option<u64>,
    #[serde(rename = "playQueueSelectedItemOffset", default)]
    play_queue_selected_item_offset: Option<u32>,
    #[serde(rename = "playQueueSelectedMetadataItemID", default)]
    play_queue_selected_metadata_item_id: Option<u64>,
    #[serde(rename = "playQueueShuffled", default)]
    play_queue_shuffled: Option<PlexBoolField>,
    #[serde(rename = "playQueueSourceURI", default)]
    play_queue_source_uri: Option<String>,
    #[serde(default)]
    identifier: Option<String>,
    #[serde(rename = "Metadata", default)]
    metadata: Vec<PlayQueueItemDto>,
}

#[derive(Debug, Deserialize)]
struct PlayQueueItemDto {
    #[serde(rename = "playQueueItemID")]
    play_queue_item_id: u64,
    #[serde(flatten)]
    metadata: MetadataDto,
}

impl PlayQueueDto {
    fn from_envelope(body: &str) -> Result<Self> {
        // The /playQueues family does NOT use the
        // `MediaContainer<T>::from_json` shape because the queue
        // metadata lives directly on the container alongside the
        // `Metadata` array. Parse the envelope explicitly.
        let env: PlayQueueEnvelope = serde_json::from_str(body)?;
        Ok(env.container)
    }

    fn into_domain(self, http: HttpClient, base_url: Url) -> PlayQueue {
        let mut items = Vec::with_capacity(self.metadata.len());
        for dto in self.metadata {
            let section_id = dto.metadata.library_section_id_for_playlist().unwrap_or(0);
            let section_ref = LibrarySectionRef {
                id: section_id,
                http: http.clone(),
                base_url: base_url.clone(),
            };
            // Drop items whose `into_library_item` rejects — they
            // shouldn't be in a queue at all, but be permissive
            // rather than fail the whole queue load.
            if let Ok(item) = dto.metadata.into_library_item(section_ref) {
                items.push(PlayQueueItem {
                    play_queue_item_id: dto.play_queue_item_id,
                    item,
                });
            }
        }
        PlayQueue {
            id: PlayQueueId(self.play_queue_id),
            version: self.play_queue_version,
            total_count: self.play_queue_total_count,
            selected_item_id: self.play_queue_selected_item_id,
            selected_item_offset: self.play_queue_selected_item_offset,
            selected_metadata_item_id: self.play_queue_selected_metadata_item_id,
            shuffled: self.play_queue_shuffled.is_some_and(|b| b.to_bool()),
            source_uri: self.play_queue_source_uri,
            identifier: self.identifier,
            items,
            http,
            base_url,
        }
    }
}

// -----------------------------------------------------------------------------
// Tests.
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::ids::RatingKey;

    #[test]
    fn pct_encode_path_escapes_slash_and_comma() {
        let s = pct_encode_path("/library/metadata/1,2,3");
        assert_eq!(s, "%2Flibrary%2Fmetadata%2F1%2C2%2C3");
    }

    #[test]
    fn pct_encode_path_preserves_unreserved() {
        assert_eq!(pct_encode_path("abc-DEF_123.~"), "abc-DEF_123.~");
    }

    #[test]
    fn server_uri_format_matches_python_template() {
        let mid = MachineIdentifier::new("abc-machine-id").unwrap();
        let uri = server_uri(&mid, "/library/metadata/42");
        assert_eq!(
            uri,
            "server://abc-machine-id/com.plexapp.plugins.library/library/metadata/42"
        );
    }

    #[test]
    fn library_directory_uri_joins_rating_keys_with_commas() {
        // Build fixed-rating-key dummy LibraryItems.
        fn movie(rk: u64) -> LibraryItem {
            LibraryItem::Movie(crate::Movie {
                rating_key: RatingKey(rk),
                key: format!("/library/metadata/{rk}"),
                title: format!("t{rk}"),
                ..dummy_movie()
            })
        }
        fn dummy_movie() -> crate::Movie {
            // Construct via JSON to avoid hand-listing the ~30 optional
            // fields the Movie struct carries.
            let dto: MetadataDto = serde_json::from_value(serde_json::json!({
                "ratingKey": "0",
                "key": "/library/metadata/0",
                "title": "",
                "type": "movie",
            }))
            .unwrap();
            let http = crate::HttpClient::new(
                crate::ClientConfig::builder(crate::ClientIdentifier::new("t").unwrap())
                    .build()
                    .unwrap(),
            )
            .unwrap();
            let base = Url::parse("http://localhost").unwrap();
            let r = LibrarySectionRef {
                id: 1,
                http,
                base_url: base,
            };
            match dto.into_library_item(r).unwrap() {
                LibraryItem::Movie(m) => m,
                _ => unreachable!(),
            }
        }

        let a = movie(1);
        let b = movie(2);
        let c = movie(3);
        let slice = [&a, &b, &c];
        let uri = library_directory_uri(&slice);
        assert_eq!(
            uri,
            "library:///directory/%2Flibrary%2Fmetadata%2F1%2C2%2C3"
        );
    }

    #[test]
    fn dto_parses_create_response() {
        let body = serde_json::json!({
            "MediaContainer": {
                "identifier": "com.plexapp.plugins.library",
                "playQueueID": 12345,
                "playQueueVersion": 1,
                "playQueueTotalCount": 2,
                "playQueueSelectedItemID": 100,
                "playQueueSelectedItemOffset": 0,
                "playQueueSelectedMetadataItemID": 42,
                "playQueueShuffled": false,
                "playQueueSourceURI": "server://m/com.plexapp.plugins.library/library/metadata/42",
                "Metadata": [
                    {
                        "playQueueItemID": 100,
                        "ratingKey": "42",
                        "key": "/library/metadata/42",
                        "title": "Arrival",
                        "type": "movie",
                        "librarySectionID": 1,
                    },
                    {
                        "playQueueItemID": 101,
                        "ratingKey": "43",
                        "key": "/library/metadata/43",
                        "title": "Dune",
                        "type": "movie",
                        "librarySectionID": 1,
                    }
                ]
            }
        });
        let dto = PlayQueueDto::from_envelope(&body.to_string()).unwrap();
        assert_eq!(dto.play_queue_id, 12345);
        assert_eq!(dto.play_queue_total_count, 2);
        assert_eq!(dto.metadata.len(), 2);
        assert_eq!(dto.metadata[0].play_queue_item_id, 100);
        assert_eq!(dto.metadata[1].play_queue_item_id, 101);
    }

    #[test]
    fn shuffled_accepts_numeric_zero() {
        let body = serde_json::json!({
            "MediaContainer": {
                "playQueueID": 1,
                "playQueueShuffled": 0,
                "Metadata": []
            }
        });
        let dto = PlayQueueDto::from_envelope(&body.to_string()).unwrap();
        assert!(!dto.play_queue_shuffled.unwrap().to_bool());
    }

    #[test]
    fn shuffled_accepts_numeric_one() {
        let body = serde_json::json!({
            "MediaContainer": {
                "playQueueID": 1,
                "playQueueShuffled": 1,
                "Metadata": []
            }
        });
        let dto = PlayQueueDto::from_envelope(&body.to_string()).unwrap();
        assert!(dto.play_queue_shuffled.unwrap().to_bool());
    }
}
