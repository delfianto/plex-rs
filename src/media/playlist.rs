//! Playlists — server-level (not section-attached) ordered item
//! collections.
//!
//! Plex playlists are kind-typed (`audio` / `video` / `photo`) and
//! live at server scope: a single playlist can contain items from
//! multiple library sections, which is why [`Playlist`] does not
//! hold a [`crate::LibrarySectionRef`] back-link.
//!
//! Smart playlists are persisted as a stored filter URI on the
//! playlist record. The read side preserves the URI; smart-playlist
//! mutation is deferred (analysis/11 §7.4 puts smart-filter
//! round-trip out of scope for v1).
//!
//! Endpoint surface (M4.1 implements *italicised* methods):
//!
//! - `GET /playlists` — *list*
//! - `GET /playlists/<id>` — fetch one (`Playlist::by_rating_key`)
//! - `GET /playlists/<id>/items` — *items*
//! - `DELETE /playlists/<id>` — *delete*
//! - `PUT /playlists/<id>?title=` — defer (rename)
//! - `PUT /playlists/<id>/items?uri=` — defer (add)
//! - `DELETE /playlists/<id>/items/<playlistItemID>` — defer (remove)
//! - `PUT /playlists/<id>/items/<playlistItemID>/move?after=` — defer
//! - `POST /playlists?type=&title=&smart=&uri=` — defer (create)

use std::fmt;

use serde::Deserialize;
use url::Url;

use crate::client::HttpClient;
use crate::error::{Error, Result};
use crate::library::LibrarySectionRef;
use crate::media::LibraryItem;
use crate::media::video::MetadataDto;
use crate::server::join_path;
use crate::util::ids::RatingKey;
use crate::xml::MediaContainer;

// -----------------------------------------------------------------------------
// PlaylistKind.
// -----------------------------------------------------------------------------

/// Playlist content kind, discriminated on the wire by `playlistType`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PlaylistKind {
    /// Music playlist (`playlistType="audio"`).
    Audio,
    /// Video playlist — movies, episodes, clips (`playlistType="video"`).
    Video,
    /// Photo slideshow (`playlistType="photo"`).
    Photo,
    /// Forward-compat for kinds Plex adds later.
    Other(String),
}

impl PlaylistKind {
    /// Map Plex's wire string to a typed variant.
    #[must_use]
    pub fn from_wire(s: &str) -> Self {
        match s {
            "audio" => Self::Audio,
            "video" => Self::Video,
            "photo" => Self::Photo,
            other => Self::Other(other.to_owned()),
        }
    }

    /// Canonical wire string.
    #[must_use]
    pub fn as_wire(&self) -> &str {
        match self {
            Self::Audio => "audio",
            Self::Video => "video",
            Self::Photo => "photo",
            Self::Other(s) => s,
        }
    }
}

impl fmt::Display for PlaylistKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_wire())
    }
}

// -----------------------------------------------------------------------------
// Playlist.
// -----------------------------------------------------------------------------

/// A server-level Plex playlist.
///
/// Carries an [`HttpClient`] handle directly (rather than a
/// [`LibrarySectionRef`]) because playlists are not section-attached;
/// their items can span sections.
#[derive(Clone)]
#[non_exhaustive]
pub struct Playlist {
    /// Playlist's metadata identifier.
    pub rating_key: RatingKey,
    /// Relative key — `/playlists/<rating_key>`.
    pub key: String,
    /// User-visible title.
    pub title: String,
    /// Content kind.
    pub kind: PlaylistKind,
    /// User-provided summary (often empty).
    pub summary: Option<String>,
    /// Whether the playlist is smart (rule-driven) vs. static.
    pub smart: bool,
    /// Plex's redundant secondary playlist-type field (`audio`,
    /// `music`, etc.); when present, may carry a more specific
    /// classification than [`Self::kind`].
    pub playlist_type: Option<String>,
    /// For smart playlists, the stored filter URI Plex uses to
    /// re-derive the item list on every fetch.
    pub content_uri: Option<String>,
    /// Total runtime in milliseconds.
    pub duration_ms: Option<u64>,
    /// Number of items.
    pub leaf_count: Option<u32>,
    /// Number of items the user has played through.
    pub viewed_leaf_count: Option<u32>,
    /// Composite (stitched-collage) thumbnail path.
    pub composite: Option<String>,
    /// Add timestamp (epoch seconds).
    pub added_at: Option<i64>,
    /// Metadata update timestamp (epoch seconds).
    pub updated_at: Option<i64>,
    /// Primary GUID.
    pub guid: Option<String>,

    http: HttpClient,
    base_url: Url,
}

impl fmt::Debug for Playlist {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Playlist")
            .field("rating_key", &self.rating_key)
            .field("title", &self.title)
            .field("kind", &self.kind)
            .field("smart", &self.smart)
            .field("leaf_count", &self.leaf_count)
            .finish_non_exhaustive()
    }
}

impl Playlist {
    /// List the items in this playlist.
    ///
    /// Calls `GET /playlists/<rk>/items` and parses each
    /// `<Video>`/`<Track>`/`<Photo>` into the right
    /// [`LibraryItem`] variant.
    ///
    /// Note: Plex sometimes returns items whose owning library
    /// section is no longer reachable (deleted/disconnected). Each
    /// returned [`LibraryItem`] carries a synthetic
    /// [`LibrarySectionRef`] pointing at section ID 0 in that case —
    /// edit operations on those items will surface `Error::NotFound`
    /// from the server.
    ///
    /// # Errors
    /// Any transport [`Error`] variant.
    pub async fn items(&self) -> Result<Vec<LibraryItem>> {
        let url = join_path(
            &self.base_url,
            &format!("/playlists/{}/items", self.rating_key),
        )?;
        let body = self.http.get_bytes(url.as_str()).await?;
        let body_str = std::str::from_utf8(&body)
            .map_err(|e| Error::Config(format!("playlist items body not utf-8: {e}")))?;
        let mc: MediaContainer<MetadataDto> = MediaContainer::from_json(body_str, "Metadata")?;
        mc.items
            .into_iter()
            .map(|dto| {
                // Each playlist item carries its own librarySectionID
                // on the wire — wire it in so future edit operations
                // route through the right section.
                let section_id = dto.library_section_id_for_playlist().unwrap_or(0);
                let section_ref = LibrarySectionRef {
                    id: section_id,
                    http: self.http.clone(),
                    base_url: self.base_url.clone(),
                };
                dto.into_library_item(section_ref)
            })
            .collect()
    }

    /// Delete this playlist server-side.
    ///
    /// Calls `DELETE /playlists/<rk>`. The local [`Playlist`] value
    /// is consumed (Plex no longer recognises it after deletion).
    ///
    /// # Errors
    /// Any transport [`Error`] variant. [`Error::NotFound`] surfaces
    /// when the playlist was already deleted.
    pub async fn delete(self) -> Result<()> {
        let url = join_path(&self.base_url, &format!("/playlists/{}", self.rating_key))?;
        self.http.delete(url.as_str()).await
    }
}

// -----------------------------------------------------------------------------
// DTO — playlist's `<Playlist>` element (not the shared <Video> shape).
// -----------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PlaylistDto {
    pub(crate) rating_key: String,
    pub(crate) key: String,
    pub(crate) title: String,
    #[serde(default)]
    pub(crate) summary: Option<String>,
    #[serde(rename = "playlistType", default)]
    pub(crate) playlist_type: Option<String>,
    #[serde(default)]
    pub(crate) smart: Option<crate::server::PlexBoolField>,
    #[serde(default)]
    pub(crate) duration: Option<u64>,
    #[serde(default)]
    pub(crate) leaf_count: Option<u32>,
    #[serde(default)]
    pub(crate) viewed_leaf_count: Option<u32>,
    #[serde(default)]
    pub(crate) composite: Option<String>,
    #[serde(default)]
    pub(crate) added_at: Option<i64>,
    #[serde(default)]
    pub(crate) updated_at: Option<i64>,
    #[serde(default)]
    pub(crate) guid: Option<String>,
    #[serde(default)]
    pub(crate) content: Option<String>,
}

impl PlaylistDto {
    pub(crate) fn into_domain(self, http: HttpClient, base_url: Url) -> Result<Playlist> {
        let rating_key: RatingKey = self
            .rating_key
            .parse()
            .map_err(|e: Error| Error::Config(format!("playlist.ratingKey: {e}")))?;
        // Plex emits `playlistType` for the kind. Some endpoints
        // additionally emit a `type` discriminator with finer
        // granularity ("music" / "movie" / etc.); we capture
        // `playlist_type` raw and let the macro-driven kind enum
        // canonicalise the coarse classification.
        let kind = PlaylistKind::from_wire(self.playlist_type.as_deref().unwrap_or(""));
        Ok(Playlist {
            rating_key,
            key: self.key,
            title: self.title,
            kind,
            summary: self.summary,
            smart: self.smart.is_some_and(|b| b.to_bool()),
            playlist_type: self.playlist_type,
            content_uri: self.content,
            duration_ms: self.duration,
            leaf_count: self.leaf_count,
            viewed_leaf_count: self.viewed_leaf_count,
            composite: self.composite,
            added_at: self.added_at,
            updated_at: self.updated_at,
            guid: self.guid,
            http,
            base_url,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn playlist_kind_round_trips_known_values() {
        for k in [
            PlaylistKind::Audio,
            PlaylistKind::Video,
            PlaylistKind::Photo,
        ] {
            assert_eq!(PlaylistKind::from_wire(k.as_wire()), k);
        }
    }

    #[test]
    fn playlist_kind_unknown_preserves_string() {
        let k = PlaylistKind::from_wire("podcast");
        assert_eq!(k.as_wire(), "podcast");
        assert_eq!(k.to_string(), "podcast");
    }

    #[test]
    fn playlist_dto_parses_static_playlist() {
        let body = serde_json::json!({
            "ratingKey": "500",
            "key": "/playlists/500",
            "title": "Workout",
            "summary": "Tunes for the gym",
            "playlistType": "audio",
            "smart": false,
            "duration": 3_600_000,
            "leafCount": 12,
            "viewedLeafCount": 4,
            "addedAt": 1_700_000_000_i64
        });
        let dto: PlaylistDto = serde_json::from_value(body).unwrap();
        let cfg = crate::ClientConfig::builder(crate::ClientIdentifier::new("t").unwrap())
            .build()
            .unwrap();
        let http = HttpClient::new(cfg).unwrap();
        let base = Url::parse("http://plex.local:32400/").unwrap();
        let p = dto.into_domain(http, base).unwrap();
        assert_eq!(p.rating_key.get(), 500);
        assert_eq!(p.title, "Workout");
        assert_eq!(p.kind, PlaylistKind::Audio);
        assert!(!p.smart);
        assert_eq!(p.leaf_count, Some(12));
        assert_eq!(p.duration_ms, Some(3_600_000));
    }

    #[test]
    fn playlist_dto_parses_smart_with_content_uri() {
        let body = serde_json::json!({
            "ratingKey": "501",
            "key": "/playlists/501",
            "title": "Recently Added Action Movies",
            "playlistType": "video",
            "smart": "1",
            "content": "library:///directory/encoded-filter-uri"
        });
        let dto: PlaylistDto = serde_json::from_value(body).unwrap();
        let cfg = crate::ClientConfig::builder(crate::ClientIdentifier::new("t").unwrap())
            .build()
            .unwrap();
        let http = HttpClient::new(cfg).unwrap();
        let base = Url::parse("http://plex.local:32400/").unwrap();
        let p = dto.into_domain(http, base).unwrap();
        assert!(p.smart);
        assert_eq!(p.kind, PlaylistKind::Video);
        assert!(p.content_uri.is_some());
    }
}
