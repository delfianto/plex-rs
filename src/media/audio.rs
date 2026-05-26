//! Audio-domain leaves: [`Artist`] → [`Album`] → [`Track`].
//!
//! Same shape as the TV hierarchy in [`crate::media::video`]: each
//! type carries the scalars Plex emits on its `<Directory>` /
//! `<Track>` element plus typed parent/grandparent rating-key
//! back-references. The shared `MetadataDto` already models every
//! field the audio path needs, so this module mostly contains
//! conversions and convenience accessors.
//!
//! Listing endpoints:
//! - `LibrarySection::artists()` — `GET /library/sections/<id>/all?type=8`.
//! - [`Artist::albums()`] — `GET /library/metadata/<artist_rk>/children`.
//! - [`Album::tracks()`] — `GET /library/metadata/<album_rk>/children`.

use crate::error::{Error, Result};
use crate::library::LibrarySectionRef;
use crate::media::video::MetadataDto;
use crate::util::ids::RatingKey;

// -----------------------------------------------------------------------------
// Artist.
// -----------------------------------------------------------------------------

/// A musical artist — the top of the music hierarchy.
///
/// Returned by `LibrarySection::artists()` on a music section.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Artist {
    /// Plex metadata identifier.
    pub rating_key: RatingKey,
    /// Relative key.
    pub key: String,
    /// Artist name.
    pub title: String,
    /// Sort title.
    pub title_sort: Option<String>,
    /// Bio / summary.
    pub summary: Option<String>,
    /// Number of albums.
    pub child_count: Option<u32>,
    /// View count at artist level (rarely populated).
    pub view_count: u32,
    /// Add timestamp.
    pub added_at: Option<i64>,
    /// Metadata update timestamp.
    pub updated_at: Option<i64>,
    /// Last listened timestamp.
    pub last_viewed_at: Option<i64>,
    /// Poster path.
    pub thumb: Option<String>,
    /// Background-art path.
    pub art: Option<String>,
    /// Primary GUID.
    pub guid: Option<String>,
    /// Back-reference for edits.
    pub section_ref: LibrarySectionRef,
}

impl Artist {
    /// List this artist's albums via `GET /library/metadata/<rk>/children`.
    ///
    /// # Errors
    /// Any [`Error`] variant.
    pub async fn albums(&self) -> Result<Vec<Album>> {
        super::video::list_children_audio(
            &self.section_ref,
            self.rating_key,
            MetadataDto::into_album,
        )
        .await
    }
}

// -----------------------------------------------------------------------------
// Album.
// -----------------------------------------------------------------------------

/// A music album — one container under an [`Artist`].
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Album {
    /// Plex metadata identifier.
    pub rating_key: RatingKey,
    /// Relative key.
    pub key: String,
    /// Album title.
    pub title: String,
    /// Sort title.
    pub title_sort: Option<String>,
    /// Release year.
    pub year: Option<u16>,
    /// Release date (`YYYY-MM-DD`).
    pub originally_available_at: Option<String>,
    /// Label / studio.
    pub studio: Option<String>,
    /// Summary / review.
    pub summary: Option<String>,
    /// Plex's user rating, 0..=10.
    pub rating: Option<f32>,
    /// Number of tracks.
    pub leaf_count: Option<u32>,
    /// Number of tracks the user has played.
    pub viewed_leaf_count: Option<u32>,
    /// Artist (parent) rating key.
    pub parent_rating_key: RatingKey,
    /// Artist key.
    pub parent_key: Option<String>,
    /// Artist name.
    pub parent_title: Option<String>,
    /// Artist poster.
    pub parent_thumb: Option<String>,
    /// Album-cover path.
    pub thumb: Option<String>,
    /// Background-art path (rarely populated for albums).
    pub art: Option<String>,
    /// Album view count.
    pub view_count: u32,
    /// Last listened timestamp.
    pub last_viewed_at: Option<i64>,
    /// Add timestamp.
    pub added_at: Option<i64>,
    /// Metadata update timestamp.
    pub updated_at: Option<i64>,
    /// Primary GUID.
    pub guid: Option<String>,
    /// Back-reference for edits.
    pub section_ref: LibrarySectionRef,
}

impl Album {
    /// List this album's tracks via
    /// `GET /library/metadata/<rk>/children`.
    ///
    /// # Errors
    /// Any [`Error`] variant.
    pub async fn tracks(&self) -> Result<Vec<Track>> {
        super::video::list_children_audio(
            &self.section_ref,
            self.rating_key,
            MetadataDto::into_track,
        )
        .await
    }
}

// -----------------------------------------------------------------------------
// Track.
// -----------------------------------------------------------------------------

/// A music track — the leaf playable in the audio hierarchy.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Track {
    /// Plex metadata identifier.
    pub rating_key: RatingKey,
    /// Relative key.
    pub key: String,
    /// Track title.
    pub title: String,
    /// Sort title.
    pub title_sort: Option<String>,
    /// Original title — the per-track artist when the album is a
    /// compilation (otherwise the album artist).
    pub original_title: Option<String>,
    /// Track number within the disc (`index`).
    pub index: Option<i32>,
    /// Disc number (`parentIndex` — counterintuitive but correct).
    pub disc_number: Option<i32>,
    /// Duration in milliseconds.
    pub duration_ms: Option<u64>,
    /// Album (parent) rating key.
    pub parent_rating_key: RatingKey,
    /// Album key.
    pub parent_key: Option<String>,
    /// Album title.
    pub parent_title: Option<String>,
    /// Album cover.
    pub parent_thumb: Option<String>,
    /// Artist (grandparent) rating key.
    pub grandparent_rating_key: RatingKey,
    /// Artist key.
    pub grandparent_key: Option<String>,
    /// Artist name.
    pub grandparent_title: Option<String>,
    /// Artist poster.
    pub grandparent_thumb: Option<String>,
    /// Artist background-art.
    pub grandparent_art: Option<String>,
    /// View count (plays).
    pub view_count: u32,
    /// Resume offset for partial plays.
    pub view_offset_ms: Option<u64>,
    /// Last listened timestamp.
    pub last_viewed_at: Option<i64>,
    /// Add timestamp.
    pub added_at: Option<i64>,
    /// Metadata update timestamp.
    pub updated_at: Option<i64>,
    /// Plex's user rating, 0..=10.
    pub rating: Option<f32>,
    /// Track-level thumb (rarely populated; usually the album cover).
    pub thumb: Option<String>,
    /// Primary GUID.
    pub guid: Option<String>,
    /// Back-reference for edits.
    pub section_ref: LibrarySectionRef,
}

impl Track {
    /// Has this track ever been played?
    #[must_use]
    pub const fn is_played(&self) -> bool {
        self.view_count > 0
    }
}

// -----------------------------------------------------------------------------
// DTO conversions (on the shared MetadataDto from media::video).
// -----------------------------------------------------------------------------

fn parse_rk(s: &str, field: &str) -> Result<RatingKey> {
    s.parse::<RatingKey>()
        .map_err(|e| Error::Config(format!("metadata.{field} not numeric: {e}")))
}

impl MetadataDto {
    pub(crate) fn into_artist(self, section_ref: LibrarySectionRef) -> Result<Artist> {
        let rating_key = parse_rk(&self.rating_key, "ratingKey")?;
        Ok(Artist {
            rating_key,
            key: self.key,
            title: self.title,
            title_sort: self.title_sort,
            summary: self.summary,
            child_count: self.child_count,
            view_count: self.view_count.unwrap_or(0),
            added_at: self.added_at,
            updated_at: self.updated_at,
            last_viewed_at: self.last_viewed_at,
            thumb: self.thumb,
            art: self.art,
            guid: self.guid,
            section_ref,
        })
    }

    pub(crate) fn into_album(self, section_ref: LibrarySectionRef) -> Result<Album> {
        let rating_key = parse_rk(&self.rating_key, "ratingKey")?;
        let parent_rating_key = self
            .parent_rating_key
            .as_deref()
            .map(|s| parse_rk(s, "parentRatingKey"))
            .transpose()?
            .ok_or_else(|| Error::Config("album missing parentRatingKey".to_owned()))?;
        Ok(Album {
            rating_key,
            key: self.key,
            title: self.title,
            title_sort: self.title_sort,
            year: self.year,
            originally_available_at: self.originally_available_at,
            studio: self.studio,
            summary: self.summary,
            rating: self.rating,
            leaf_count: self.leaf_count,
            viewed_leaf_count: self.viewed_leaf_count,
            parent_rating_key,
            parent_key: self.parent_key,
            parent_title: self.parent_title,
            parent_thumb: self.parent_thumb,
            thumb: self.thumb,
            art: self.art,
            view_count: self.view_count.unwrap_or(0),
            last_viewed_at: self.last_viewed_at,
            added_at: self.added_at,
            updated_at: self.updated_at,
            guid: self.guid,
            section_ref,
        })
    }

    pub(crate) fn into_track(self, section_ref: LibrarySectionRef) -> Result<Track> {
        let rating_key = parse_rk(&self.rating_key, "ratingKey")?;
        let parent_rating_key = self
            .parent_rating_key
            .as_deref()
            .map(|s| parse_rk(s, "parentRatingKey"))
            .transpose()?
            .ok_or_else(|| Error::Config("track missing parentRatingKey".to_owned()))?;
        let grandparent_rating_key = self
            .grandparent_rating_key
            .as_deref()
            .map(|s| parse_rk(s, "grandparentRatingKey"))
            .transpose()?
            .ok_or_else(|| Error::Config("track missing grandparentRatingKey".to_owned()))?;
        Ok(Track {
            rating_key,
            key: self.key,
            title: self.title,
            title_sort: self.title_sort,
            original_title: self.original_title,
            index: self.index,
            disc_number: self.parent_index,
            duration_ms: self.duration,
            parent_rating_key,
            parent_key: self.parent_key,
            parent_title: self.parent_title,
            parent_thumb: self.parent_thumb,
            grandparent_rating_key,
            grandparent_key: self.grandparent_key,
            grandparent_title: self.grandparent_title,
            grandparent_thumb: self.grandparent_thumb,
            grandparent_art: self.grandparent_art,
            view_count: self.view_count.unwrap_or(0),
            view_offset_ms: self.view_offset,
            last_viewed_at: self.last_viewed_at,
            added_at: self.added_at,
            updated_at: self.updated_at,
            rating: self.rating,
            thumb: self.thumb,
            guid: self.guid,
            section_ref,
        })
    }
}
