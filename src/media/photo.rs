//! Photo-domain leaves: [`Photoalbum`] and [`Photo`].
//!
//! Photoalbums **nest**: a photoalbum's children can be a mix of
//! sub-photoalbums and photos. The `/library/metadata/<rk>/children`
//! response carries both kinds in the same `Metadata[]` array,
//! discriminated by the `type` attribute (`photoalbum` vs `photo`).
//! [`Photoalbum::children`] returns a [`PhotoEntry`] enum so callers
//! can pattern-match cleanly; convenience filters
//! [`Photoalbum::sub_albums`] and [`Photoalbum::photos`] are also
//! provided.

use crate::error::{Error, Result};
use crate::library::LibrarySectionRef;
use crate::media::video::MetadataDto;
use crate::util::ids::RatingKey;
use crate::xml::MediaContainer;

// -----------------------------------------------------------------------------
// Photoalbum.
// -----------------------------------------------------------------------------

/// A Plex photo album / folder.
///
/// Mirrors the Show / Artist top-level container shape but with
/// recursive children — a `Photoalbum` may contain further
/// `Photoalbum`s, [`Photo`]s, or a mix.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Photoalbum {
    /// Plex metadata identifier.
    pub rating_key: RatingKey,
    /// Relative key.
    pub key: String,
    /// User-visible title.
    pub title: String,
    /// Plot / album summary.
    pub summary: Option<String>,
    /// Sort index (manual ordering within parent).
    pub index: Option<i32>,
    /// Total number of direct children (mix of albums + photos).
    pub child_count: Option<u32>,
    /// Direct photo count (when populated by Plex).
    pub leaf_count: Option<u32>,
    /// Add timestamp (epoch seconds).
    pub added_at: Option<i64>,
    /// Update timestamp (epoch seconds).
    pub updated_at: Option<i64>,
    /// Composite-image path (Plex generates a stitched preview).
    pub thumb: Option<String>,
    /// Background-art path.
    pub art: Option<String>,
    /// Parent album rating key (when nested).
    pub parent_rating_key: Option<RatingKey>,
    /// Parent album title (when nested).
    pub parent_title: Option<String>,
    /// Primary GUID.
    pub guid: Option<String>,
    /// Back-reference for edits.
    pub section_ref: LibrarySectionRef,
}

impl Photoalbum {
    /// List every direct child (sub-album or photo) of this album.
    ///
    /// Calls `GET /library/metadata/<rk>/children`.
    ///
    /// # Errors
    /// Any [`Error`] variant.
    pub async fn children(&self) -> Result<Vec<PhotoEntry>> {
        let path = format!("/library/metadata/{}/children", self.rating_key);
        let url = self.section_ref.base_url.join(&path)?;
        let body = self.section_ref.http.get_bytes(url.as_str()).await?;
        let body_str = std::str::from_utf8(&body)
            .map_err(|e| Error::Config(format!("/children body not utf-8: {e}")))?;
        let mc: MediaContainer<MetadataDto> = MediaContainer::from_json(body_str, "Metadata")?;
        mc.items
            .into_iter()
            .map(|dto| dto.into_photo_entry(self.section_ref.clone()))
            .collect()
    }

    /// Convenience: return only the sub-albums of this album.
    ///
    /// # Errors
    /// See [`Photoalbum::children`].
    pub async fn sub_albums(&self) -> Result<Vec<Self>> {
        Ok(self
            .children()
            .await?
            .into_iter()
            .filter_map(|e| match e {
                PhotoEntry::Album(a) => Some(a),
                PhotoEntry::Photo(_) => None,
            })
            .collect())
    }

    /// Convenience: return only the photos directly in this album.
    ///
    /// # Errors
    /// See [`Photoalbum::children`].
    pub async fn photos(&self) -> Result<Vec<Photo>> {
        Ok(self
            .children()
            .await?
            .into_iter()
            .filter_map(|e| match e {
                PhotoEntry::Photo(p) => Some(p),
                PhotoEntry::Album(_) => None,
            })
            .collect())
    }
}

// -----------------------------------------------------------------------------
// Photo.
// -----------------------------------------------------------------------------

/// A single photo (or video clip from a photo library).
///
/// Width / height live on the associated `<Media>` element which is
/// modelled in M2.5. For now the metadata-level scalars are enough to
/// list and browse.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Photo {
    /// Plex metadata identifier.
    pub rating_key: RatingKey,
    /// Relative key.
    pub key: String,
    /// User-visible title / filename.
    pub title: String,
    /// Sort title.
    pub title_sort: Option<String>,
    /// EXIF caption / description.
    pub summary: Option<String>,
    /// Position within parent album.
    pub index: Option<i32>,
    /// Photo creation date (epoch seconds).
    pub originally_available_at: Option<String>,
    /// Year of capture.
    pub year: Option<u16>,
    /// Photoalbum (parent) rating key.
    pub parent_rating_key: Option<RatingKey>,
    /// Parent album title.
    pub parent_title: Option<String>,
    /// Parent album poster.
    pub parent_thumb: Option<String>,
    /// Add timestamp (epoch seconds).
    pub added_at: Option<i64>,
    /// Update timestamp (epoch seconds).
    pub updated_at: Option<i64>,
    /// Thumb path.
    pub thumb: Option<String>,
    /// View count (Plex tracks photo views too).
    pub view_count: u32,
    /// Primary GUID.
    pub guid: Option<String>,
    /// File / part / stream chain. Empty when not populated by the
    /// originating endpoint.
    pub media: Vec<super::streams::Media>,
    /// Back-reference for edits.
    pub section_ref: LibrarySectionRef,
}

// -----------------------------------------------------------------------------
// PhotoEntry — sum type for mixed children.
// -----------------------------------------------------------------------------

/// One entry returned by [`Photoalbum::children`] — either a
/// sub-album or a photo.
#[derive(Debug, Clone)]
pub enum PhotoEntry {
    /// A nested sub-album.
    Album(Photoalbum),
    /// A photo (or video clip).
    Photo(Photo),
}

// -----------------------------------------------------------------------------
// DTO conversions.
// -----------------------------------------------------------------------------

fn parse_rk_opt(s: Option<&str>, field: &str) -> Result<Option<RatingKey>> {
    s.map(|v| {
        v.parse::<RatingKey>()
            .map_err(|e| Error::Config(format!("metadata.{field} not numeric: {e}")))
    })
    .transpose()
}

fn parse_rk(s: &str, field: &str) -> Result<RatingKey> {
    s.parse::<RatingKey>()
        .map_err(|e| Error::Config(format!("metadata.{field} not numeric: {e}")))
}

impl MetadataDto {
    pub(crate) fn into_photoalbum(self, section_ref: LibrarySectionRef) -> Result<Photoalbum> {
        let rating_key = parse_rk(&self.rating_key, "ratingKey")?;
        let parent_rating_key = parse_rk_opt(self.parent_rating_key.as_deref(), "parentRatingKey")?;
        Ok(Photoalbum {
            rating_key,
            key: self.key,
            title: self.title,
            summary: self.summary,
            index: self.index,
            child_count: self.child_count,
            leaf_count: self.leaf_count,
            added_at: self.added_at,
            updated_at: self.updated_at,
            thumb: self.thumb,
            art: self.art,
            parent_rating_key,
            parent_title: self.parent_title,
            guid: self.guid,
            section_ref,
        })
    }

    pub(crate) fn into_photo(self, section_ref: LibrarySectionRef) -> Result<Photo> {
        let rating_key = parse_rk(&self.rating_key, "ratingKey")?;
        let parent_rating_key = parse_rk_opt(self.parent_rating_key.as_deref(), "parentRatingKey")?;
        let media = self
            .media
            .into_iter()
            .map(super::streams::MediaDto::into_domain)
            .collect();
        Ok(Photo {
            rating_key,
            key: self.key,
            title: self.title,
            title_sort: self.title_sort,
            summary: self.summary,
            index: self.index,
            originally_available_at: self.originally_available_at,
            year: self.year,
            parent_rating_key,
            parent_title: self.parent_title,
            parent_thumb: self.parent_thumb,
            added_at: self.added_at,
            updated_at: self.updated_at,
            thumb: self.thumb,
            view_count: self.view_count.unwrap_or(0),
            guid: self.guid,
            media,
            section_ref,
        })
    }

    /// Dispatch on the `type` discriminator to construct a
    /// [`PhotoEntry`]. Used by [`Photoalbum::children`].
    pub(crate) fn into_photo_entry(self, section_ref: LibrarySectionRef) -> Result<PhotoEntry> {
        match self.metadata_type.as_deref() {
            Some("photoalbum") => Ok(PhotoEntry::Album(self.into_photoalbum(section_ref)?)),
            Some("photo" | "clip") | None => Ok(PhotoEntry::Photo(self.into_photo(section_ref)?)),
            Some(other) => Err(Error::Config(format!("unknown photo entry type {other:?}"))),
        }
    }
}
